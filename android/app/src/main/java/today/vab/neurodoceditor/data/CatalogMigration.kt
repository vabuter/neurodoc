package today.vab.neurodoceditor.data

import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Что в каталоге осталось от прежнего формата. */
data class LegacyScan(val files: Int, val links: Int) {
    val needsMigration: Boolean get() = links > 0
}

/** Что сделала миграция и куда положена резервная копия. */
data class MigrationResult(
    val files: Int,
    val links: Int,
    val backupPath: String,
    val readMeUpdated: Boolean,
)

/**
 * Перевод каталога с прежнего формата: ссылки `<uuid>` в `[](uuid)`.
 *
 * Переход полный: старая форма ссылкой больше не считается, и до миграции каталог
 * прежнего формата выглядит пустым, без единой ссылки. Файлы пользователя меняются
 * только после явного подтверждения и только после создания резервной копии всего
 * каталога.
 */
object CatalogMigration {

    /** Фразы из старого описания формата — по ним узнаём несвежий ReadMe.md. */
    private val LEGACY_READ_ME_MARKERS = listOf(
        "обрамленный символами", // описание ссылок прежнего формата
        "обрамлённый < и >",     // он же в редакции Android-версии
    )

    /** Сколько ссылок старого формата осталось в каталоге. */
    fun scan(catalog: NeurodocCatalog): LegacyScan {
        var files = 0
        var links = 0
        for (id in catalog.allIds()) {
            val count = NeurodocCatalog.LEGACY_LINK_REGEX.findAll(catalog.readFile(id)).count()
            if (count == 0) continue
            files++
            links += count
        }
        return LegacyScan(files, links)
    }

    /** Куда ляжет резервная копия каталога перед миграцией. */
    fun buildBackupPath(backupsDir: File, projectId: String?): File {
        val safeId = (projectId ?: "project").replace(Regex("""[^\w.-]"""), "_")
        val stamp = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        return File(backupsDir, "$safeId-$stamp")
    }

    /**
     * Переводит каталог на новый формат. Сначала копирует каталог целиком, затем заменяет
     * каждую ссылку `<uuid>` на `[](uuid)` — пустой якорь означает «показать заголовок цели».
     *
     * @throws java.io.IOException не удалось создать резервную копию — миграция не начата.
     */
    fun migrate(catalog: NeurodocCatalog, backupsDir: File, projectId: String?): MigrationResult {
        val backup = buildBackupPath(backupsDir, projectId)
        copyDirectory(File(catalog.rootPath), backup)

        var files = 0
        var links = 0
        for (id in catalog.allIds().toList()) {
            val text = catalog.readFile(id)
            val count = NeurodocCatalog.LEGACY_LINK_REGEX.findAll(text).count()
            if (count == 0) continue

            // заменяем только сами ссылки: остальной текст, включая переводы строк,
            // остаётся байт в байт прежним
            val updated = NeurodocCatalog.LEGACY_LINK_REGEX.replace(text) { m ->
                "[](" + m.groupValues[1].lowercase() + ")"
            }
            catalog.writeFile(id, updated)
            files++
            links += count
        }

        return MigrationResult(files, links, backup.path, updateReadMe(catalog.rootPath))
    }

    /**
     * Обновляет ReadMe.md, если он всё ещё описывает старый формат. Изменённый
     * пользователем файл не трогаем — маркером служит фраза из нашего же прежнего шаблона.
     */
    private fun updateReadMe(rootPath: String): Boolean = runCatching {
        val readme = File(rootPath).listFiles()
            ?.firstOrNull { it.name.equals("readme.md", ignoreCase = true) }
            ?: return false
        val text = readme.readText()
        if (LEGACY_READ_ME_MARKERS.none { text.contains(it) }) return false
        readme.writeText(NeurodocCatalog.READ_ME_TEMPLATE)
        true
    }.getOrDefault(false) // описание формата — не критичная часть, сбой не срывает миграцию

    private fun copyDirectory(source: File, destination: File) {
        destination.mkdirs()
        source.listFiles()?.forEach { child ->
            val target = File(destination, child.name)
            if (child.isDirectory) copyDirectory(child, target) else child.copyTo(target, overwrite = true)
        }
    }
}
