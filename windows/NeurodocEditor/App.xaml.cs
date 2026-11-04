using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using NeurodocEditor.Models;
using NeurodocEditor.Services;

namespace NeurodocEditor;

public partial class App : Application
{
    public static SettingsService SettingsService { get; } = new();
    public static AppSettings Settings { get; private set; } = new();
    public static MainWindow? Window { get; private set; }

    [DllImport("gdi32.dll", CharSet = CharSet.Unicode)]
    private static extern int AddFontResourceW(string name);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool SendNotifyMessageW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    private static readonly (string File, string DisplayName)[] BundledFontFiles =
    {
        ("Inter-Regular.ttf", "Inter Regular (TrueType)"),
        ("JetBrainsMono-Regular.ttf", "JetBrains Mono Regular (TrueType)"),
    };

    /// <summary>
    /// Устанавливает шрифты из Assets\Fonts для текущего пользователя (без прав
    /// администратора): копия в %LOCALAPPDATA%\Microsoft\Windows\Fonts + запись в HKCU.
    /// Иначе DWrite (WinUI 3 / RichEdit) их не видит. Установка идемпотентна.
    /// </summary>
    private static void RegisterBundledFonts()
    {
        try
        {
            var sourceDir = Path.Combine(AppContext.BaseDirectory, "Assets", "Fonts");
            if (!Directory.Exists(sourceDir)) return;

            var userFontsDir = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Microsoft", "Windows", "Fonts");
            Directory.CreateDirectory(userFontsDir);

            using var key = Microsoft.Win32.Registry.CurrentUser.CreateSubKey(
                @"Software\Microsoft\Windows NT\CurrentVersion\Fonts");
            bool changed = false;

            foreach (var (file, displayName) in BundledFontFiles)
            {
                var source = Path.Combine(sourceDir, file);
                if (!File.Exists(source)) continue;
                var target = Path.Combine(userFontsDir, file);
                if (!File.Exists(target))
                {
                    File.Copy(source, target);
                    changed = true;
                }
                if (key?.GetValue(displayName) as string != target)
                {
                    key?.SetValue(displayName, target);
                    changed = true;
                }
                AddFontResourceW(target);
            }

            if (changed)
            {
                const uint WM_FONTCHANGE = 0x001D;
                SendNotifyMessageW(new IntPtr(0xffff), WM_FONTCHANGE, IntPtr.Zero, IntPtr.Zero);
            }
        }
        catch
        {
            // без встроенных шрифтов приложение всё равно работает
        }
    }

    public App()
    {
        RegisterBundledFonts();
        InitializeComponent();
        UnhandledException += (_, e) =>
        {
            try
            {
                var dir = Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "NeurodocEditor");
                Directory.CreateDirectory(dir);
                File.AppendAllText(Path.Combine(dir, "error.log"),
                    $"[{DateTime.Now:yyyy-MM-dd HH:mm:ss}] {e.Exception}{Environment.NewLine}");
            }
            catch
            {
                // логирование не должно ронять приложение
            }
        };
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        Settings = SettingsService.Load();
        Window = new MainWindow();
        Window.Activate();
    }
}
