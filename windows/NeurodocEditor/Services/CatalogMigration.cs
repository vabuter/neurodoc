using System.Text.RegularExpressions;

namespace NeurodocEditor.Services;

/// <summary>Что в каталоге осталось от прежнего формата.</summary>
public record LegacyScan(int Files, int Links)
{
    public bool NeedsMigration => Links > 0;
}

/// <summary>Что сделала миграция и куда положена резервная копия.</summary>
public record MigrationResult(int Files, int Links, string BackupPath, bool ReadMeUpdated);

/// <summary>Перевод каталога с прежнего формата: ссылки &lt;uuid&gt; в [](uuid).</summary>
/// <remarks>
/// Переход полный: старая форма ссылкой больше не считается, и до миграции каталог
/// прежнего формата выглядит пустым, без единой ссылки. Файлы пользователя меняются
/// только после явного подтверждения и только после создания резервной копии всего
/// каталога.
/// </remarks>
public static class CatalogMigration
{
    /// <summary>Фраза из старого описания формата — по ней узнаём несвежий ReadMe.md.</summary>
    private static readonly string[] LegacyReadMeMarkers =
    {
        "обрамленный символами",  // описание ссылок прежнего формата
    };

    /// <summary>Сколько ссылок старого формата осталось в каталоге.</summary>
    public static LegacyScan Scan(NeurodocCatalog catalog)
    {
        int files = 0, links = 0;
        foreach (var id in catalog.AllIds())
        {
            int count = NeurodocCatalog.LegacyLinkRegex.Matches(catalog.ReadFile(id)).Count;
            if (count == 0) continue;
            files++;
            links += count;
        }
        return new LegacyScan(files, links);
    }

    /// <summary>Куда ляжет резервная копия каталога перед миграцией.</summary>
    public static string BuildBackupPath(string projectId)
    {
        var root = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "NeurodocEditor", "backups");
        var safeId = string.Concat((projectId ?? "project").Split(Path.GetInvalidFileNameChars()));
        return Path.Combine(root, $"{safeId}-{DateTime.Now:yyyyMMdd-HHmmss}");
    }

    /// <summary>
    /// Переводит каталог на новый формат. Сначала копирует каталог целиком, затем заменяет
    /// каждую ссылку &lt;uuid&gt; на [](uuid) — пустой якорь означает «показать заголовок цели».
    /// </summary>
    /// <exception cref="IOException">Не удалось создать резервную копию — миграция не начата.</exception>
    public static MigrationResult Migrate(NeurodocCatalog catalog, string projectId)
    {
        var backupPath = BuildBackupPath(projectId);
        CopyDirectory(catalog.RootPath, backupPath);

        int files = 0, links = 0;
        foreach (var id in catalog.AllIds().ToList())
        {
            var text = catalog.ReadFile(id);
            int count = NeurodocCatalog.LegacyLinkRegex.Matches(text).Count;
            if (count == 0) continue;

            // заменяем только сами ссылки: остальной текст, включая переводы строк,
            // остаётся байт в байт прежним
            var updated = NeurodocCatalog.LegacyLinkRegex.Replace(
                text, m => "[](" + m.Groups[1].Value.ToLowerInvariant() + ")");
            catalog.WriteFile(id, updated);
            files++;
            links += count;
        }

        return new MigrationResult(files, links, backupPath, UpdateReadMe(catalog.RootPath));
    }

    /// <summary>
    /// Обновляет ReadMe.md, если он всё ещё описывает старый формат. Изменённый пользователем
    /// файл не трогаем — маркером служит фраза из нашего же прежнего шаблона.
    /// </summary>
    private static bool UpdateReadMe(string rootPath)
    {
        try
        {
            var readme = Directory.EnumerateFiles(rootPath).FirstOrDefault(f =>
                string.Equals(Path.GetFileName(f), "readme.md", StringComparison.OrdinalIgnoreCase));
            if (readme == null) return false;
            var text = File.ReadAllText(readme);
            if (!LegacyReadMeMarkers.Any(m => text.Contains(m, StringComparison.Ordinal))) return false;

            File.WriteAllText(readme, NeurodocCatalog.ReadMeTemplate, new System.Text.UTF8Encoding(false));
            return true;
        }
        catch
        {
            // описание формата — не критичная часть каталога, сбой не срывает миграцию
            return false;
        }
    }

    private static void CopyDirectory(string source, string destination)
    {
        Directory.CreateDirectory(destination);
        foreach (var file in Directory.EnumerateFiles(source))
            File.Copy(file, Path.Combine(destination, Path.GetFileName(file)), overwrite: true);
        foreach (var dir in Directory.EnumerateDirectories(source))
            CopyDirectory(dir, Path.Combine(destination, Path.GetFileName(dir)));
    }
}
