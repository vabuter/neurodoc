using NeurodocEditor.Models;

namespace NeurodocEditor.Services;

/// <summary>
/// Прогресс чтения в YAML: отдельный файл на проект в %APPDATA%\NeurodocEditor\reading.
/// </summary>
/// <remarks>
/// Отдельно от settings.yaml намеренно: список прочитанного растёт и переписывается при
/// каждом переходе, настройкам такое соседство ни к чему. Сброс прогресса — удаление файла.
/// </remarks>
public class ReadingProgressService
{
    private readonly string _dir = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
        "NeurodocEditor", "reading");

    public string FilePathFor(string projectId)
    {
        var safeId = string.Concat((projectId ?? "project").Split(Path.GetInvalidFileNameChars()));
        return Path.Combine(_dir, safeId + ".yaml");
    }

    /// <summary>Прогресс с диска; повреждённый файл не мешает читать — прогресс просто пуст.</summary>
    public ReadingProgress Load(string projectId)
    {
        var progress = YamlStore.Load<ReadingProgress>(FilePathFor(projectId));
        if (progress == null) return new ReadingProgress();
        // файл мог быть без списка посещённого — дальше по нему ходят без проверок
        progress.Visited ??= new Dictionary<string, DateTime>(StringComparer.OrdinalIgnoreCase);
        return progress;
    }

    public void Save(string projectId, ReadingProgress progress) =>
        YamlStore.Save(FilePathFor(projectId), progress);

    public void Reset(string projectId)
    {
        try
        {
            var path = FilePathFor(projectId);
            if (File.Exists(path)) File.Delete(path);
        }
        catch
        {
            // остаётся в памяти — сброс не критичная операция
        }
    }
}
