namespace NeurodocEditor.Models;

/// <summary>
/// Что читатель уже прошёл в этом проекте. Состояние читателя, а не документации,
/// поэтому лежит рядом с настройками, а не в каталоге neurodoc.
/// </summary>
public class ReadingProgress
{
    /// <summary>Файл -> когда он был отмечен прочитанным.</summary>
    public Dictionary<string, DateTime> Visited { get; set; } = new(StringComparer.OrdinalIgnoreCase);

    public bool IsRead(string id) => Visited.ContainsKey(id);
}
