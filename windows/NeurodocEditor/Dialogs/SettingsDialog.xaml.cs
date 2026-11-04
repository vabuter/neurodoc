using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using NeurodocEditor.Models;

namespace NeurodocEditor.Dialogs;

public sealed partial class SettingsDialog : ContentDialog
{
    private readonly AppSettings _settings;
    private readonly Action _onChanged;
    private readonly bool _initialized;

    public SettingsDialog(AppSettings settings, Action onChanged)
    {
        _settings = settings;
        _onChanged = onChanged;
        InitializeComponent();

        ThemeRadios.SelectedIndex = settings.Theme switch
        {
            "Light" => 1,
            "Dark" => 2,
            _ => 0,
        };

        var fonts = GetInstalledFonts();
        // встроенные в приложение шрифты + особые имена + текущий выбор
        var extras = Services.BundledFonts.Names
            .Concat(Services.BundledFonts.ExtraNames)
            .Append(settings.FontFamily);
        foreach (var extra in extras)
            if (!fonts.Contains(extra, StringComparer.OrdinalIgnoreCase))
                fonts.Add(extra);
        fonts.Sort(StringComparer.CurrentCultureIgnoreCase);
        FontCombo.ItemsSource = fonts;
        FontCombo.SelectedItem = fonts.FirstOrDefault(
            f => string.Equals(f, settings.FontFamily, StringComparison.OrdinalIgnoreCase));

        SizeBox.Value = settings.FontSize;
        SpacingBox.Value = settings.LineSpacing;
        _initialized = true;
    }

    private static List<string> GetInstalledFonts()
    {
        try
        {
            using var collection = new System.Drawing.Text.InstalledFontCollection();
            return collection.Families
                .Select(f => f.Name)
                .Where(n => !string.IsNullOrWhiteSpace(n))
                .Distinct()
                .OrderBy(n => n, StringComparer.CurrentCultureIgnoreCase)
                .ToList();
        }
        catch
        {
            return new List<string> { "Cascadia Mono", "Consolas", "Courier New", "Segoe UI" };
        }
    }

    private void OnThemeChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_initialized) return;
        _settings.Theme = ThemeRadios.SelectedIndex switch
        {
            1 => "Light",
            2 => "Dark",
            _ => "System",
        };
        Fire();
        // диалог открыт поверх окна — подстраиваем и его тему
        RequestedTheme = _settings.Theme switch
        {
            "Light" => ElementTheme.Light,
            "Dark" => ElementTheme.Dark,
            _ => ElementTheme.Default,
        };
    }

    private void OnFontChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_initialized || FontCombo.SelectedItem is not string font) return;
        _settings.FontFamily = font;
        Fire();
    }

    private void OnSizeChanged(NumberBox sender, NumberBoxValueChangedEventArgs args)
    {
        if (!_initialized || double.IsNaN(args.NewValue)) return;
        _settings.FontSize = Math.Clamp(args.NewValue, 8, 96);
        Fire();
    }

    private void OnSpacingChanged(NumberBox sender, NumberBoxValueChangedEventArgs args)
    {
        if (!_initialized || double.IsNaN(args.NewValue)) return;
        _settings.LineSpacing = Math.Clamp(args.NewValue, 0.8, 3);
        Fire();
    }

    private void Fire() => _onChanged();
}
