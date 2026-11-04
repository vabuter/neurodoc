using NeurodocEditor.Models;

namespace NeurodocEditor.Services;

/// <summary>Хранение настроек приложения в YAML-файле в %APPDATA%\NeurodocEditor.</summary>
public class SettingsService
{
    private readonly string _dir = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "NeurodocEditor");

    public string SettingsFilePath => Path.Combine(_dir, "settings.yaml");

    /// <summary>Настройки с диска; повреждённый файл не мешает запуску — берутся значения по умолчанию.</summary>
    public AppSettings Load()
    {
        var settings = YamlStore.Load<AppSettings>(SettingsFilePath);
        return settings == null ? new AppSettings() : Normalize(settings);
    }

    public void Save(AppSettings settings) => YamlStore.Save(SettingsFilePath, settings);

    private static AppSettings Normalize(AppSettings s)
    {
        s.Projects ??= new List<ProjectInfo>();
        s.Projects.RemoveAll(p => string.IsNullOrWhiteSpace(p.Path));
        foreach (var p in s.Projects)
            if (string.IsNullOrWhiteSpace(p.Id)) p.Id = Guid.NewGuid().ToString("D");
        if (s.Theme is not ("System" or "Light" or "Dark")) s.Theme = "System";
        if (string.IsNullOrWhiteSpace(s.FontFamily)) s.FontFamily = "Consolas";
        if (double.IsNaN(s.FontSize) || s.FontSize < 8 || s.FontSize > 96) s.FontSize = 14;
        if (double.IsNaN(s.LineSpacing) || s.LineSpacing < 0.8 || s.LineSpacing > 3) s.LineSpacing = 1.0;
        if (s.CurrentProjectId != null && s.Projects.All(p => p.Id != s.CurrentProjectId))
            s.CurrentProjectId = s.Projects.FirstOrDefault()?.Id;
        return s;
    }
}
