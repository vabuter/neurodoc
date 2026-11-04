using Microsoft.UI.Xaml.Media;

namespace NeurodocEditor.Services;

/// <summary>
/// Шрифты, поставляемые вместе с приложением (Assets\Fonts), и дополнительные
/// имена, которые всегда предлагаются в настройках.
/// </summary>
public static class BundledFonts
{
    private static readonly Dictionary<string, string> Files = new(StringComparer.OrdinalIgnoreCase)
    {
        ["Inter"] = "Inter-Regular.ttf",
        ["JetBrains Mono"] = "JetBrainsMono-Regular.ttf",
    };

    public static IEnumerable<string> Names => Files.Keys;

    /// <summary>
    /// Имена, добавляемые в список выбора, даже если шрифт не установлен:
    /// проприетарный Anthropic Sans нельзя распространять с приложением,
    /// но если он установлен у пользователя — он заработает.
    /// </summary>
    public static readonly string[] ExtraNames = { "Anthropic Sans" };

    public static bool IsBundled(string name) => Files.ContainsKey(name);

    /// <summary>Путь к файлу встроенного шрифта рядом с приложением, или null.</summary>
    public static string? FilePath(string name) =>
        Files.TryGetValue(name, out var file)
            ? Path.Combine(AppContext.BaseDirectory, "Assets", "Fonts", file)
            : null;

    /// <summary>FontFamily для XAML: для встроенных шрифтов — ms-appx-путь с именем семейства.</summary>
    public static FontFamily CreateFontFamily(string name)
    {
        if (Files.TryGetValue(name, out var file))
        {
            var canonical = Files.Keys.First(k => string.Equals(k, name, StringComparison.OrdinalIgnoreCase));
            return new FontFamily($"ms-appx:///Assets/Fonts/{file}#{canonical}");
        }
        return new FontFamily(name);
    }
}
