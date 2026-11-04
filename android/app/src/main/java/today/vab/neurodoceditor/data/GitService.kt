package today.vab.neurodoceditor.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.eclipse.jgit.api.CreateBranchCommand
import org.eclipse.jgit.api.Git
import org.eclipse.jgit.api.RebaseCommand
import org.eclipse.jgit.api.ResetCommand
import org.eclipse.jgit.api.errors.GitAPIException
import org.eclipse.jgit.diff.DiffEntry
import org.eclipse.jgit.diff.DiffFormatter
import org.eclipse.jgit.diff.RawTextComparator
import org.eclipse.jgit.lib.ConfigConstants
import org.eclipse.jgit.lib.Constants
import org.eclipse.jgit.lib.ObjectId
import org.eclipse.jgit.lib.PersonIdent
import org.eclipse.jgit.lib.Repository
import org.eclipse.jgit.merge.MergeStrategy
import org.eclipse.jgit.revwalk.RevCommit
import org.eclipse.jgit.revwalk.RevWalk
import org.eclipse.jgit.transport.CredentialsProvider
import org.eclipse.jgit.transport.RefSpec
import org.eclipse.jgit.transport.RemoteRefUpdate
import org.eclipse.jgit.transport.URIish
import org.eclipse.jgit.transport.UsernamePasswordCredentialsProvider
import org.eclipse.jgit.treewalk.AbstractTreeIterator
import org.eclipse.jgit.treewalk.CanonicalTreeParser
import org.eclipse.jgit.treewalk.EmptyTreeIterator
import org.eclipse.jgit.treewalk.FileTreeIterator
import org.eclipse.jgit.treewalk.filter.PathFilter
import java.io.ByteArrayOutputStream
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

data class GitCommitInfo(
    val hash: String,
    val shortHash: String,
    val dateMillis: Long,
    val author: String,
    val message: String,
)

/** Изменение файла в коммите или диапазоне; status — буква git (A/M/D/R…). */
data class GitFileChange(val path: String, val status: Char, val oldPath: String?)

enum class PushOutcome { OK, REJECTED_NON_FAST_FORWARD }
enum class PullOutcome { OK, CONFLICT }
enum class SyncOutcome { OK, CONFLICT, PUSH_REJECTED }

/** Git-операция не удалась; сообщение показывается пользователю. */
class GitException(message: String, cause: Throwable? = null) : Exception(message, cause)

/**
 * Все git-операции приложения на встроенном JGit: системного git на Android нет.
 * Повторяет Services/GitService.cs Windows-версии файл в файл по смыслу.
 *
 * Каждый commit сразу уходит push — локальные коммиты сами по себе приложению не нужны.
 * Конфликты не решаются автоматически: методы возвращают исход, а выбор (затереть
 * свои/серверные или отменить) делает пользователь в UI.
 *
 * Все методы ходят в файловую систему и сеть, поэтому выполняются в Dispatchers.IO —
 * сеть в main-потоке на Android запрещена.
 */
class GitService(
    private val dir: File,
    /** Креды по URL репозитория; null — анонимный доступ (file://, публичный https). */
    private val credentials: (String) -> CredentialsProvider?,
    /** Автор коммитов из настроек приложения: git config на Android нет. */
    private val identity: () -> PersonIdent,
) {
    companion object {
        private const val STASH_PREFIX = "neurodoc-auto:"
        private const val NETWORK_TIMEOUT_SEC = 30

        fun isRepository(dir: File): Boolean = File(dir, ".git").isDirectory

        /** Креды для хоста: username подставляется по правилам github/gitlab, если не задан. */
        fun credentialsProvider(url: String, accounts: List<GitAccount>): CredentialsProvider? {
            val host = runCatching { URIish(url).host }.getOrNull() ?: return null
            val account = accounts.firstOrNull { it.host.equals(host, ignoreCase = true) }
                ?: return null
            if (account.token.isBlank()) return null
            val username = account.username.ifBlank {
                // gitlab принимает токен как пароль пользователя oauth2; github — с любым именем
                if (host.equals("gitlab.com", ignoreCase = true)) "oauth2" else "token"
            }
            return UsernamePasswordCredentialsProvider(username, account.token)
        }

        /** Шаблонное сообщение коммита: дата-время и устройство. */
        fun buildCommitMessage(): String {
            val stamp = SimpleDateFormat("yyyy-MM-dd HH:mm", Locale.US).format(Date())
            return "$stamp · ${android.os.Build.MODEL}"
        }

        /** История 2: клонирование. Непустую папку очищает — подтверждение спрашивает UI. */
        suspend fun clone(url: String, target: File, cp: CredentialsProvider?): Unit =
            withContext(Dispatchers.IO) {
                if (target.isDirectory && target.listFiles()?.isNotEmpty() == true) {
                    target.listFiles()?.forEach { it.deleteRecursively() }
                }
                target.mkdirs()
                wrap("Клонирование") {
                    Git.cloneRepository()
                        .setURI(url)
                        .setDirectory(target)
                        .setCredentialsProvider(cp)
                        .setTimeout(NETWORK_TIMEOUT_SEC)
                        .call()
                        .close()
                }
            }

        private inline fun <T> wrap(operation: String, block: () -> T): T =
            try {
                block()
            } catch (e: GitAPIException) {
                throw GitException("$operation: ${e.message}", e)
            } catch (e: org.eclipse.jgit.errors.TransportException) {
                throw GitException("$operation: ${e.message}", e)
            }
    }

    private fun <T> withGit(block: (Git) -> T): T =
        Git.open(dir).use(block)

    private fun cp(git: Git): CredentialsProvider? {
        val url = git.repository.config.getString("remote", "origin", "url") ?: return null
        return credentials(url)
    }

    // ================= Состояние =================

    suspend fun currentBranch(): String? = withContext(Dispatchers.IO) {
        withGit { git -> git.repository.branch }
    }

    suspend fun localBranches(): List<String> = withContext(Dispatchers.IO) {
        withGit { git ->
            git.branchList().call().map { Repository.shortenRefName(it.name) }
        }
    }

    /**
     * Default-ветка проекта: origin/HEAD (ставится клоном), иначе main/master/первая.
     * В отличие от Windows-версии в сеть не ходим: lsRemote с мобильного дорог.
     */
    suspend fun defaultBranch(): String = withContext(Dispatchers.IO) {
        withGit { git ->
            val head = git.repository.exactRef("refs/remotes/origin/HEAD")
            val target = head?.target?.name
            if (target != null && target.startsWith("refs/remotes/origin/")) {
                return@withGit target.removePrefix("refs/remotes/origin/")
            }
            val branches = git.branchList().call().map { Repository.shortenRefName(it.name) }
            branches.firstOrNull { it == "main" }
                ?: branches.firstOrNull { it == "master" }
                ?: branches.firstOrNull()
                ?: "main"
        }
    }

    suspend fun isDirty(): Boolean = withContext(Dispatchers.IO) {
        withGit { git -> !git.status().call().isClean }
    }

    // ================= История 1: опубликовать =================

    suspend fun initAndPublish(remoteUrl: String): Unit = withContext(Dispatchers.IO) {
        if (isRepository(dir)) throw GitException("Каталог уже является git-репозиторием.")
        wrap("Публикация") {
            Git.init().setDirectory(dir).setInitialBranch("main").call().use { git ->
                git.add().addFilepattern(".").call()
                val ident = identity()
                git.commit().setMessage("Initial commit")
                    .setAuthor(ident).setCommitter(ident).call()
                git.remoteAdd().setName("origin").setUri(URIish(remoteUrl)).call()
                git.push()
                    .setRemote("origin")
                    .setRefSpecs(RefSpec("refs/heads/main:refs/heads/main"))
                    .setCredentialsProvider(credentials(remoteUrl))
                    .setTimeout(NETWORK_TIMEOUT_SEC)
                    .call()
                setUpstream(git, "main")
            }
        }
    }

    /** У JGit нет push -u: upstream прописывается в конфиг руками. */
    private fun setUpstream(git: Git, branch: String) {
        val config = git.repository.config
        config.setString(ConfigConstants.CONFIG_BRANCH_SECTION, branch,
            ConfigConstants.CONFIG_KEY_REMOTE, "origin")
        config.setString(ConfigConstants.CONFIG_BRANCH_SECTION, branch,
            ConfigConstants.CONFIG_KEY_MERGE, "refs/heads/$branch")
        config.save()
    }

    // ================= История 3: переключение веток =================

    suspend fun switchBranch(branch: String): Unit = withContext(Dispatchers.IO) {
        wrap("Переключение ветки") {
            withGit { git ->
                val current = git.repository.branch
                if (current == branch) return@withGit

                if (!git.status().call().isClean) {
                    git.stashCreate()
                        .setIncludeUntracked(true)
                        .setWorkingDirectoryMessage(STASH_PREFIX + current)
                        .setIndexMessage(STASH_PREFIX + current)
                        .call()
                }

                git.checkout().setName(branch).call()

                findStash(git, branch)?.let { index ->
                    git.stashApply().setStashRef("stash@{$index}").call()
                    git.stashDrop().setStashRef(index).call()
                }
            }
        }
    }

    /** Номер stash-записи ветки. Сверка всей метки: «dev» не должен находить «dev2». */
    private fun findStash(git: Git, branch: String): Int? {
        git.stashList().call().forEachIndexed { index, stash ->
            // JGit добавляет к заданному сообщению префикс "On <ветка>: " — ищем по хвосту
            val message = stash.fullMessage.substringAfter(": ", stash.fullMessage)
            if (message == STASH_PREFIX + branch) return index
        }
        return null
    }

    // ================= История 4: создание и удаление веток =================

    suspend fun createBranch(name: String): Unit = withContext(Dispatchers.IO) {
        wrap("Создание ветки") {
            withGit { git ->
                git.checkout().setCreateBranch(true).setName(name)
                    .setUpstreamMode(CreateBranchCommand.SetupUpstreamMode.NOTRACK)
                    .call()
                git.push()
                    .setRemote("origin")
                    .setRefSpecs(RefSpec("refs/heads/$name:refs/heads/$name"))
                    .setCredentialsProvider(cp(git))
                    .setTimeout(NETWORK_TIMEOUT_SEC)
                    .call()
                setUpstream(git, name)
            }
        }
    }

    /** Текущую ветку UI сначала должен покинуть (switchBranch на default), затем звать это. */
    suspend fun deleteBranch(name: String): Unit = withContext(Dispatchers.IO) {
        wrap("Удаление ветки") {
            withGit { git ->
                // работа удаляемой ветки уходит вместе с ней — мёртвый stash копить незачем
                findStash(git, name)?.let { git.stashDrop().setStashRef(it).call() }

                git.branchDelete().setBranchNames(name).setForce(true).call()

                // на сервере ветки может не быть — пустой результат не ошибка
                git.push()
                    .setRemote("origin")
                    .setRefSpecs(RefSpec(":refs/heads/$name"))
                    .setCredentialsProvider(cp(git))
                    .setTimeout(NETWORK_TIMEOUT_SEC)
                    .call()
            }
        }
    }

    // ================= История 5: сохранить на сервере =================

    suspend fun commitAndPush(message: String): PushOutcome = withContext(Dispatchers.IO) {
        wrap("Сохранение на сервере") {
            withGit { git ->
                commitAll(git, message)
                push(git, force = false)
            }
        }
    }

    /** Затирает серверную ветку своей. Осознанный выбор пользователя в диалоге конфликта. */
    suspend fun forcePush(): Unit = withContext(Dispatchers.IO) {
        wrap("Отправка на сервер") {
            withGit { git ->
                if (push(git, force = true) != PushOutcome.OK)
                    throw GitException("Сервер отклонил принудительную отправку.")
            }
        }
    }

    private fun commitAll(git: Git, message: String) {
        // два прохода add: без setUpdate(true) JGit не стейджит удаления
        git.add().addFilepattern(".").call()
        git.add().addFilepattern(".").setUpdate(true).call()
        val status = git.status().call()
        if (status.hasUncommittedChanges()) {
            val ident = identity()
            git.commit().setMessage(message).setAuthor(ident).setCommitter(ident).call()
        }
    }

    private fun push(git: Git, force: Boolean): PushOutcome {
        val branch = git.repository.branch
        val results = git.push()
            .setRemote("origin")
            .setRefSpecs(RefSpec("refs/heads/$branch:refs/heads/$branch").setForceUpdate(force))
            .setCredentialsProvider(cp(git))
            .setTimeout(NETWORK_TIMEOUT_SEC)
            .call()
        for (result in results) {
            for (update in result.remoteUpdates) {
                when (update.status) {
                    RemoteRefUpdate.Status.OK,
                    RemoteRefUpdate.Status.UP_TO_DATE -> {}
                    RemoteRefUpdate.Status.REJECTED_NONFASTFORWARD ->
                        return PushOutcome.REJECTED_NON_FAST_FORWARD
                    else -> throw GitException(
                        "Сервер отклонил отправку: ${update.status} ${update.message ?: ""}")
                }
            }
        }
        return PushOutcome.OK
    }

    // ================= История 6: получить с сервера =================

    suspend fun pull(): PullOutcome = withContext(Dispatchers.IO) {
        wrap("Получение с сервера") {
            withGit { git ->
                val result = git.pull()
                    .setCredentialsProvider(cp(git))
                    .setTimeout(NETWORK_TIMEOUT_SEC)
                    .setStrategy(MergeStrategy.RECURSIVE)
                    .call()
                val merge = result.mergeResult
                if (result.isSuccessful) return@withGit PullOutcome.OK
                // слияние не прошло — откатываем до исходного состояния и отдаём выбор пользователю
                git.repository.writeMergeCommitMsg(null)
                git.repository.writeMergeHeads(null)
                git.reset().setMode(ResetCommand.ResetType.HARD).call()
                if (merge != null || result.rebaseResult != null) PullOutcome.CONFLICT
                else throw GitException("Получение с сервера не удалось.")
            }
        }
    }

    /** Затирает локальное состояние серверным. Осознанный выбор пользователя в диалоге конфликта. */
    suspend fun resetToRemote(): Unit = withContext(Dispatchers.IO) {
        wrap("Получение с сервера") {
            withGit { git ->
                val branch = git.repository.branch
                git.fetch()
                    .setRemote("origin")
                    .setCredentialsProvider(cp(git))
                    .setTimeout(NETWORK_TIMEOUT_SEC)
                    .call()
                git.reset().setMode(ResetCommand.ResetType.HARD)
                    .setRef("refs/remotes/origin/$branch").call()
                git.clean().setCleanDirectories(true).setForce(true).call()
            }
        }
    }

    // ================= История 7: синхронизировать =================

    /** «Получить, затем сохранить» одной операцией: commit → pull --rebase → push. */
    suspend fun sync(message: String): SyncOutcome = withContext(Dispatchers.IO) {
        wrap("Синхронизация") {
            withGit { git ->
                commitAll(git, message)

                val result = git.pull()
                    .setRebase(true)
                    .setCredentialsProvider(cp(git))
                    .setTimeout(NETWORK_TIMEOUT_SEC)
                    .call()
                if (!result.isSuccessful) {
                    val rebase = result.rebaseResult
                    if (rebase != null && !rebase.status.isSuccessful) {
                        git.rebase().setOperation(RebaseCommand.Operation.ABORT).call()
                        return@withGit SyncOutcome.CONFLICT
                    }
                    throw GitException("Синхронизация не удалась.")
                }

                if (push(git, force = false) == PushOutcome.OK) SyncOutcome.OK
                else SyncOutcome.PUSH_REJECTED
            }
        }
    }

    // ================= История 8: история коммитов =================

    suspend fun history(limit: Int = 200): List<GitCommitInfo> = withContext(Dispatchers.IO) {
        withGit { git ->
            if (git.repository.resolve(Constants.HEAD) == null) return@withGit emptyList()
            git.log().setMaxCount(limit).call().map { commit ->
                GitCommitInfo(
                    hash = commit.name,
                    shortHash = commit.name.substring(0, 7),
                    dateMillis = commit.commitTime * 1000L,
                    author = commit.authorIdent.name,
                    message = commit.shortMessage,
                )
            }
        }
    }

    // ================= История 9: изменённые файлы и дифф =================

    /** Файлы, изменённые одним коммитом. */
    suspend fun commitFiles(hash: String): List<GitFileChange> = withContext(Dispatchers.IO) {
        withGit { git ->
            val repo = git.repository
            val commit = parseCommit(repo, hash)
            val parentTree = if (commit.parentCount > 0)
                treeIterator(repo, commit.getParent(0)) else EmptyTreeIterator()
            diffEntries(repo, parentTree, treeIterator(repo, commit))
        }
    }

    /** Файлы, изменённые от коммита до текущего состояния (включая незакоммиченное). */
    suspend fun changesSince(hash: String): List<GitFileChange> = withContext(Dispatchers.IO) {
        withGit { git ->
            val repo = git.repository
            diffEntries(repo, treeIterator(repo, parseCommit(repo, hash)), FileTreeIterator(repo))
        }
    }

    private fun diffEntries(
        repo: Repository,
        oldTree: AbstractTreeIterator,
        newTree: AbstractTreeIterator,
    ): List<GitFileChange> {
        DiffFormatter(ByteArrayOutputStream()).use { formatter ->
            formatter.setRepository(repo)
            formatter.setDetectRenames(true)
            return formatter.scan(oldTree, newTree).map { entry ->
                when (entry.changeType) {
                    DiffEntry.ChangeType.ADD -> GitFileChange(entry.newPath, 'A', null)
                    DiffEntry.ChangeType.DELETE -> GitFileChange(entry.oldPath, 'D', null)
                    DiffEntry.ChangeType.RENAME -> GitFileChange(entry.newPath, 'R', entry.oldPath)
                    DiffEntry.ChangeType.COPY -> GitFileChange(entry.newPath, 'A', entry.oldPath)
                    else -> GitFileChange(entry.newPath, 'M', null)
                }
            }
        }
    }

    /** Unified diff файла: внутри одного коммита либо от коммита до текущего состояния. */
    suspend fun fileDiff(hash: String, path: String, sinceMode: Boolean): String =
        withContext(Dispatchers.IO) {
            withGit { git ->
                val repo = git.repository
                val commit = parseCommit(repo, hash)
                val out = ByteArrayOutputStream()
                DiffFormatter(out).use { formatter ->
                    formatter.setRepository(repo)
                    formatter.setDetectRenames(true)
                    formatter.setDiffComparator(RawTextComparator.DEFAULT)
                    formatter.setPathFilter(PathFilter.create(path))
                    if (sinceMode) {
                        formatter.format(treeIterator(repo, commit), FileTreeIterator(repo))
                    } else {
                        val parentTree = if (commit.parentCount > 0)
                            treeIterator(repo, commit.getParent(0)) else EmptyTreeIterator()
                        formatter.format(parentTree, treeIterator(repo, commit))
                    }
                }
                out.toString("UTF-8")
            }
        }

    /** Содержимое файла в ревизии — заголовок для уже удалённого файла берётся отсюда. */
    suspend fun readBlob(hash: String, path: String, fromParent: Boolean): String? =
        withContext(Dispatchers.IO) {
            withGit { git ->
                val repo = git.repository
                runCatching {
                    var commit = parseCommit(repo, hash)
                    if (fromParent) {
                        if (commit.parentCount == 0) return@runCatching null
                        commit = parseCommit(repo, commit.getParent(0).name)
                    }
                    val treeWalk = org.eclipse.jgit.treewalk.TreeWalk.forPath(repo, path, commit.tree)
                        ?: return@runCatching null
                    val blob = repo.open(treeWalk.getObjectId(0))
                    String(blob.bytes, Charsets.UTF_8)
                }.getOrNull()
            }
        }

    private fun parseCommit(repo: Repository, hash: String): RevCommit =
        RevWalk(repo).use { walk -> walk.parseCommit(ObjectId.fromString(hash)) }

    private fun treeIterator(repo: Repository, commit: RevCommit): CanonicalTreeParser {
        val parsed = RevWalk(repo).use { walk -> walk.parseCommit(commit.id) }
        return CanonicalTreeParser().apply {
            repo.newObjectReader().use { reader -> reset(reader, parsed.tree.id) }
        }
    }
}
