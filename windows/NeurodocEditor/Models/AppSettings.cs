using YamlDotNet.Serialization;

namespace NeurodocEditor.Models;

public class ProjectInfo
{
    public string Id { get; set; } = Guid.NewGuid().ToString("D");
    public string Name { get; set; } = "";
    public string Path { get; set; } = "";

    /// <summary>
    /// Фон области просмотра SVG: "Light", "Dark" или null — «как тема приложения».
    /// </summary>
    /// <remarks>
    /// Настройка проекта, а не приложения: рисунки одного проекта нарисованы под один
    /// фон. Пока пользователь не переключал фон в самом просмотре, тут null и работает
    /// тема; первое же переключение запоминается и дальше побеждает тему.
    /// </remarks>
    public string? SvgBackground { get; set; }
}

public class AppSettings
{
    /// <summary>System | Light | Dark</summary>
    public string Theme { get; set; } = "System";

    public string FontFamily { get; set; } = "Consolas";

    public double FontSize { get; set; } = 14;

    /// <summary>Междустрочный интервал (множитель одинарного интервала).</summary>
    public double LineSpacing { get; set; } = 1.0;

    /// <summary>
    /// Режим «как есть»: файл показывается ровно так, как лежит на диске — без скрытия
    /// служебных символов и без подсветки ссылок. Нужен, чтобы править текст посимвольно.
    /// </summary>
    public bool RawEditing { get; set; }

    public string? CurrentProjectId { get; set; }

    public List<ProjectInfo> Projects { get; set; } = new();

    [YamlIgnore]
    public ProjectInfo? CurrentProject =>
        Projects.FirstOrDefault(p => p.Id == CurrentProjectId);
}
