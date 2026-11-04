using System.Diagnostics;
using System.Text;

namespace NeurodocEditor.Services;

/// <summary>Итог одного вызова git.</summary>
public sealed record GitResult(int ExitCode, string StdOut, string StdErr)
{
    public bool Ok => ExitCode == 0;
    /// <summary>Текст для показа пользователю: stderr в приоритете, stdout — добавкой.</summary>
    public string ErrorText =>
        string.Join("\n", new[] { StdErr.Trim(), StdOut.Trim() }.Where(s => s.Length > 0));
}

/// <summary>Git завершился с ошибкой, которую нужно показать пользователю.</summary>
public sealed class GitException(string operation, GitResult result)
    : Exception($"{operation}\n{result.ErrorText}")
{
    public GitResult Result { get; } = result;
}

public sealed record GitCommit(string Hash, string ShortHash, DateTimeOffset Date, string Author, string Message);

/// <summary>Изменение файла в коммите или диапазоне; Status — буква git (A/M/D/R…).</summary>
public sealed record GitFileChange(string Path, char Status, string? OldPath);

public enum PushOutcome { Ok, RejectedNonFastForward }
public enum PullOutcome { Ok, Conflict }
public enum SyncOutcome { Ok, Conflict, PushRejected }

/// <summary>
/// Все git-операции приложения. Работает через системный git.exe: у пользователя уже
/// настроены креды для github.com/gitlab.com, и приложение ими просто пользуется.
/// </summary>
/// <remarks>
/// Каждый commit сразу уходит push — локальные коммиты сами по себе приложению не нужны,
/// поэтому отдельной операции commit здесь нет. Конфликты не решаются автоматически:
/// методы возвращают исход, а выбор (затереть свои/серверные или отменить) делает
/// пользователь в UI.
/// </remarks>
public sealed class GitService(string workDir)
{
    public string WorkDir { get; } = workDir;

    public static bool IsRepository(string path) =>
        Directory.Exists(Path.Combine(path, ".git"));

    private static bool? _gitAvailable;

    /// <summary>Есть ли git в PATH. Ответ кэшируется — за жизнь процесса git не появится.</summary>
    public static async Task<bool> IsGitAvailableAsync()
    {
        if (_gitAvailable is { } known) return known;
        try
        {
            var result = await RunIn(Environment.CurrentDirectory, "--version");
            _gitAvailable = result.Ok;
        }
        catch
        {
            _gitAvailable = false;
        }
        return _gitAvailable.Value;
    }

    // ================= Запуск git =================

    private Task<GitResult> RunAsync(params string[] args) => RunIn(WorkDir, args);

    private async Task<GitResult> RunCheckedAsync(params string[] args)
    {
        var result = await RunAsync(args);
        if (!result.Ok) throw new GitException("git " + string.Join(" ", args), result);
        return result;
    }

    private static async Task<GitResult> RunIn(string workDir, params string[] args)
    {
        var psi = new ProcessStartInfo
        {
            FileName = "git",
            WorkingDirectory = workDir,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8,
        };
        // без кредов git должен упасть с ошибкой, а не молча ждать ввода в невидимой консоли
        psi.EnvironmentVariables["GIT_TERMINAL_PROMPT"] = "0";
        // кириллические имена файлов иначе приходят октальными последовательностями
        psi.ArgumentList.Add("-c");
        psi.ArgumentList.Add("core.quotepath=off");
        foreach (var a in args) psi.ArgumentList.Add(a);

        using var process = Process.Start(psi)
            ?? throw new InvalidOperationException("Не удалось запустить git");
        // сначала дочитываем оба потока, только потом ждём выхода — иначе дедлок на полном пайпе
        var stdout = process.StandardOutput.ReadToEndAsync();
        var stderr = process.StandardError.ReadToEndAsync();
        await Task.WhenAll(stdout, stderr);
        await process.WaitForExitAsync();
        return new GitResult(process.ExitCode, stdout.Result, stderr.Result);
    }

    // ================= Состояние =================

    public async Task<string?> GetCurrentBranchAsync()
    {
        var result = await RunAsync("rev-parse", "--abbrev-ref", "HEAD");
        var name = result.StdOut.Trim();
        return result.Ok && name.Length > 0 ? name : null;
    }

    public async Task<List<string>> GetLocalBranchesAsync()
    {
        var result = await RunCheckedAsync("for-each-ref", "refs/heads", "--format=%(refname:short)");
        return result.StdOut.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .ToList();
    }

    /// <summary>
    /// Default-ветка проекта. origin/HEAD ставится клоном; после «Опубликовать» его нет,
    /// и тогда одна сетевая операция прописывает его навсегда. Без remote — main/master/первая.
    /// </summary>
    public async Task<string> GetDefaultBranchAsync()
    {
        for (int attempt = 0; attempt < 2; attempt++)
        {
            var head = await RunAsync("symbolic-ref", "--short", "refs/remotes/origin/HEAD");
            if (head.Ok)
            {
                var name = head.StdOut.Trim();
                int slash = name.IndexOf('/');
                if (slash > 0) return name[(slash + 1)..];
            }
            if (attempt == 0) await RunAsync("remote", "set-head", "origin", "--auto");
        }
        var branches = await GetLocalBranchesAsync();
        return branches.FirstOrDefault(b => b == "main")
            ?? branches.FirstOrDefault(b => b == "master")
            ?? branches.FirstOrDefault()
            ?? "main";
    }

    public async Task<bool> IsDirtyAsync()
    {
        var result = await RunCheckedAsync("status", "--porcelain");
        return result.StdOut.Trim().Length > 0;
    }

    // ================= История 1: опубликовать проект =================

    /// <summary>Делает каталог репозиторием и отправляет на remote. Каталог не должен быть репозиторием.</summary>
    public async Task InitAndPublishAsync(string remoteUrl)
    {
        if (IsRepository(WorkDir))
            throw new InvalidOperationException("Каталог уже является git-репозиторием.");

        // git commit без identity падает с невнятным советом — проверяем заранее
        var name = await RunAsync("config", "--get", "user.name");
        var email = await RunAsync("config", "--get", "user.email");
        if (name.StdOut.Trim().Length == 0 || email.StdOut.Trim().Length == 0)
            throw new InvalidOperationException(
                "В git не настроено имя автора. Выполните:\n" +
                "git config --global user.name \"Имя\"\n" +
                "git config --global user.email \"адрес@почты\"");

        await RunCheckedAsync("init", "-b", "main");
        // каталоги neurodoc — LF; autocrlf пользователя не должен их молча переписывать
        await RunCheckedAsync("config", "core.autocrlf", "false");
        await RunCheckedAsync("add", ".");
        await RunCheckedAsync("commit", "-m", "Initial commit");
        await RunCheckedAsync("remote", "add", "origin", remoteUrl);
        await RunCheckedAsync("push", "-u", "origin", "main");
    }

    // ================= История 2: клонировать =================

    /// <summary>Клонирует в папку. Непустую папку сначала очищает — подтверждение спрашивает UI.</summary>
    public static async Task CloneAsync(string url, string targetDir)
    {
        if (Directory.Exists(targetDir) && Directory.EnumerateFileSystemEntries(targetDir).Any())
            ClearDirectory(targetDir);
        Directory.CreateDirectory(targetDir);
        var result = await RunIn(targetDir, "clone", url, ".");
        if (!result.Ok) throw new GitException("git clone " + url, result);
    }

    /// <summary>Очищает содержимое папки. Объекты в .git — read-only, атрибут снимается перед удалением.</summary>
    private static void ClearDirectory(string dir)
    {
        var info = new DirectoryInfo(dir);
        foreach (var file in info.EnumerateFiles("*", SearchOption.AllDirectories))
            file.Attributes = FileAttributes.Normal;
        foreach (var entry in info.EnumerateFileSystemInfos())
        {
            if (entry is DirectoryInfo sub) sub.Delete(recursive: true);
            else entry.Delete();
        }
    }

    // ================= История 3: переключение веток =================

    private const string StashPrefix = "neurodoc-auto:";

    /// <summary>
    /// Переключается на ветку. Правки текущей ветки прячутся в stash с её меткой,
    /// при возврате на ветку её stash возвращается.
    /// </summary>
    public async Task SwitchBranchAsync(string branch)
    {
        var current = await GetCurrentBranchAsync();
        if (current == branch) return;

        if (current != null && await IsDirtyAsync())
            await RunCheckedAsync("stash", "push", "--include-untracked", "-m", StashPrefix + current);

        await RunCheckedAsync("switch", branch);

        if (await FindStashAsync(branch) is { } stashRef)
            await RunCheckedAsync("stash", "pop", stashRef);
    }

    /// <summary>Stash-запись ветки, или null. Сверка всей метки: «dev» не должен находить «dev2».</summary>
    private async Task<string?> FindStashAsync(string branch)
    {
        var result = await RunCheckedAsync("stash", "list", "--format=%gd%x1f%gs");
        foreach (var line in result.StdOut.Split('\n', StringSplitOptions.RemoveEmptyEntries))
        {
            var parts = line.Split('\u001f');
            if (parts.Length != 2) continue;
            // git сам приписывает к сообщению стэша «On <ветка>: » — отрезаем до нашей метки
            var subject = parts[1].Trim();
            int colon = subject.IndexOf(": ", StringComparison.Ordinal);
            if (colon >= 0) subject = subject[(colon + 2)..];
            if (subject == StashPrefix + branch)
                return parts[0].Trim();
        }
        return null;
    }

    // ================= История 4: создание и удаление веток =================

    /// <summary>Создаёт ветку от текущей, переключается на неё и сразу отправляет на сервер.</summary>
    public async Task CreateBranchAsync(string name)
    {
        await RunCheckedAsync("switch", "-c", name);
        await RunCheckedAsync("push", "-u", "origin", name);
    }

    /// <summary>
    /// Удаляет ветку локально и на сервере. Текущую ветку UI сначала должен покинуть
    /// (SwitchBranchAsync на default), затем звать этот метод.
    /// </summary>
    public async Task DeleteBranchAsync(string name)
    {
        // работа удаляемой ветки уходит вместе с ней — мёртвый stash копить незачем
        if (await FindStashAsync(name) is { } stashRef)
            await RunCheckedAsync("stash", "drop", stashRef);

        await RunCheckedAsync("branch", "-D", name);

        var remote = await RunAsync("push", "origin", "--delete", name);
        // ветки может не быть на сервере (создана до «Опубликовать») — это не ошибка
        if (!remote.Ok && !remote.StdErr.Contains("remote ref does not exist"))
            throw new GitException("git push origin --delete " + name, remote);
    }

    // ================= История 5: сохранить на сервере =================

    /// <summary>Шаблонное сообщение коммита: дата-время и устройство.</summary>
    public static string BuildCommitMessage() =>
        $"{DateTime.Now:yyyy-MM-dd HH:mm} · {Environment.MachineName}";

    /// <summary>
    /// Commit всех правок и push. RejectedNonFastForward — на сервере чужие коммиты;
    /// выбор (затереть их или отступить) остаётся за пользователем.
    /// </summary>
    public async Task<PushOutcome> CommitAndPushAsync(string message)
    {
        await CommitAllAsync(message);
        return await PushAsync();
    }

    /// <summary>Затирает серверную ветку своей. Осознанный выбор пользователя в диалоге конфликта.</summary>
    public Task ForcePushAsync() => RunCheckedAsync("push", "--force");

    private async Task CommitAllAsync(string message)
    {
        await RunCheckedAsync("add", "-A");
        var staged = await RunAsync("diff", "--cached", "--quiet");
        if (staged.ExitCode == 1)
            await RunCheckedAsync("commit", "-m", message);
    }

    private async Task<PushOutcome> PushAsync()
    {
        var push = await RunAsync("push");
        if (push.Ok) return PushOutcome.Ok;

        // у свежесозданной ветки нет upstream — прописываем и повторяем
        if (push.StdErr.Contains("no upstream", StringComparison.OrdinalIgnoreCase))
        {
            var branch = await GetCurrentBranchAsync()
                ?? throw new GitException("git push", push);
            await RunCheckedAsync("push", "-u", "origin", branch);
            return PushOutcome.Ok;
        }

        if (push.StdErr.Contains("non-fast-forward") ||
            push.StdErr.Contains("[rejected]") ||
            push.StdErr.Contains("fetch first"))
            return PushOutcome.RejectedNonFastForward;

        throw new GitException("git push", push);
    }

    // ================= История 6: получить с сервера =================

    /// <summary>Pull. Conflict — слияние не прошло; репозиторий уже возвращён в исходное состояние.</summary>
    public async Task<PullOutcome> PullAsync()
    {
        var pull = await RunAsync("pull", "--no-rebase");
        if (pull.Ok) return PullOutcome.Ok;

        // состояние определяем по файлам git, а не по тексту ошибки: он локализуем и меняется
        if (File.Exists(Path.Combine(WorkDir, ".git", "MERGE_HEAD")))
        {
            await RunCheckedAsync("merge", "--abort");
            return PullOutcome.Conflict;
        }
        // «local changes would be overwritten»: слияние не начиналось, откатывать нечего
        if (pull.StdErr.Contains("would be overwritten", StringComparison.OrdinalIgnoreCase))
            return PullOutcome.Conflict;

        throw new GitException("git pull", pull);
    }

    /// <summary>Затирает локальное состояние серверным. Осознанный выбор пользователя в диалоге конфликта.</summary>
    public async Task ResetToRemoteAsync()
    {
        var branch = await GetCurrentBranchAsync()
            ?? throw new InvalidOperationException("Не удалось определить текущую ветку.");
        await RunCheckedAsync("fetch", "origin");
        await RunCheckedAsync("reset", "--hard", "origin/" + branch);
        await RunCheckedAsync("clean", "-fd");
    }

    // ================= История 7: синхронизировать =================

    /// <summary>
    /// «Получить, затем сохранить» одной операцией: commit → pull --rebase → push.
    /// К pull дерево уже чистое (всё закоммичено), autostash — страховка.
    /// </summary>
    public async Task<SyncOutcome> SyncAsync(string message)
    {
        await CommitAllAsync(message);

        var pull = await RunAsync("pull", "--rebase", "--autostash");
        if (!pull.Ok)
        {
            bool rebaseInProgress =
                Directory.Exists(Path.Combine(WorkDir, ".git", "rebase-merge")) ||
                Directory.Exists(Path.Combine(WorkDir, ".git", "rebase-apply"));
            if (rebaseInProgress)
            {
                await RunCheckedAsync("rebase", "--abort");
                return SyncOutcome.Conflict;
            }
            throw new GitException("git pull --rebase", pull);
        }

        return await PushAsync() == PushOutcome.Ok ? SyncOutcome.Ok : SyncOutcome.PushRejected;
    }

    // ================= История 8: история коммитов =================

    /// <summary>Коммиты текущей ветки, новые сверху.</summary>
    public async Task<List<GitCommit>> GetHistoryAsync(int limit = 200)
    {
        // разделители US/RS: разбор не зависит ни от локали, ни от текста сообщений
        var result = await RunAsync(
            "log", $"--max-count={limit}", "--format=%H%x1f%h%x1f%aI%x1f%an%x1f%s%x1e");
        if (!result.Ok) return new();  // пустой репозиторий без коммитов

        var commits = new List<GitCommit>();
        foreach (var record in result.StdOut.Split('\u001e', StringSplitOptions.RemoveEmptyEntries))
        {
            var f = record.Trim('\n', '\r', ' ').Split('\u001f');
            if (f.Length != 5) continue;
            commits.Add(new GitCommit(f[0], f[1], DateTimeOffset.Parse(f[2]), f[3], f[4]));
        }
        return commits;
    }

    // ================= История 9: изменённые файлы и дифф =================

    /// <summary>Файлы, изменённые одним коммитом.</summary>
    public async Task<List<GitFileChange>> GetCommitFilesAsync(string hash)
    {
        var result = await RunCheckedAsync("show", hash, "--name-status", "--format=", "-M");
        return ParseNameStatus(result.StdOut);
    }

    /// <summary>Файлы, изменённые от коммита до текущего состояния (включая незакоммиченное).</summary>
    public async Task<List<GitFileChange>> GetChangesSinceAsync(string hash)
    {
        var result = await RunCheckedAsync("diff", "--name-status", "-M", hash);
        return ParseNameStatus(result.StdOut);
    }

    private static List<GitFileChange> ParseNameStatus(string output)
    {
        var changes = new List<GitFileChange>();
        foreach (var line in output.Split('\n', StringSplitOptions.RemoveEmptyEntries))
        {
            var parts = line.TrimEnd('\r').Split('\t');
            if (parts.Length < 2 || parts[0].Length == 0) continue;
            char status = parts[0][0];
            // у переименования два пути: старый и новый
            changes.Add(status == 'R' && parts.Length >= 3
                ? new GitFileChange(parts[2], status, parts[1])
                : new GitFileChange(parts[1], status, null));
        }
        return changes;
    }

    /// <summary>Unified diff файла: внутри одного коммита либо от коммита до текущего состояния.</summary>
    public async Task<string> GetFileDiffAsync(string hash, string path, bool sinceMode)
    {
        var result = sinceMode
            ? await RunCheckedAsync("diff", hash, "--", path)
            : await RunCheckedAsync("show", "--format=", hash, "--", path);
        return result.StdOut;
    }

    /// <summary>Содержимое файла в ревизии — заголовок для уже удалённого файла берётся отсюда.</summary>
    public async Task<string?> ReadBlobAsync(string rev, string path)
    {
        var result = await RunAsync("show", $"{rev}:{path}");
        return result.Ok ? result.StdOut : null;
    }
}
