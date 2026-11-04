using System.Text;
using System.Text.RegularExpressions;
using Microsoft.UI;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Text;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using NeurodocEditor.Dialogs;
using NeurodocEditor.Models;
using NeurodocEditor.Services;
using NeurodocEditor.ViewModels;
using Windows.ApplicationModel.DataTransfer;
using Windows.Foundation;
using Windows.UI;

namespace NeurodocEditor;

public sealed partial class MainWindow : Window
{
    public static nint Hwnd { get; private set; }

    private NeurodocCatalog? _catalog;
    private string? _currentId;
    private string _lastSavedText = "";
    private string _lastQuery = "";
    private bool _suppressTextChanged;
    private bool _overlayDirty;
    private bool _overlayScheduled;

    private readonly Stack<string> _back = new();
    private readonly Stack<string> _forward = new();
    private readonly Dictionary<string, ViewState> _viewStates = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Где пользователь был в файле: каретка и прокрутка. Высота контента запоминается
    /// вместе с ними как отпечаток раскладки — по ней видно, актуально ли ещё смещение.
    /// </summary>
    private record ViewState(int Caret, double Offset, double Extent, ViewMode Mode);

    /// <summary>Вид, который ждём возможности восстановить: раскладка ещё не досчитана.</summary>
    private ViewState? _pendingView;
    private int _pendingViewPasses;
    /// <summary>Каретка сразу после возврата в файл; -1 — пользователь уже её двигал.</summary>
    private int _restoredCaret = -1;
    private readonly List<LinkSpan> _links = new();
    private readonly List<LinkChip> _chips = new();
    private readonly List<Controls.ChipView> _chipShapes = new();
    private int _hoverLink = -1;
    private int _pressLink = -1;
    /// <summary>Набор neurodoc-ссылок на момент последнего применения компактного режима.</summary>
    private string? _compactSignature;
    /// <summary>Перевод строки открытого файла — при сохранении возвращаем его же.</summary>
    private string _newline = "\n";
    private Point _pressPoint;

    private readonly DispatcherQueueTimer _saveTimer;
    private readonly DispatcherQueueTimer _watchTimer;
    private FileSystemWatcher? _watcher;
    private readonly DispatcherQueueTimer _tipTimer;
    /// <summary>Пауза перед отметкой о прочтении: промах мышью не должен её ставить.</summary>
    private readonly DispatcherQueueTimer _readTimer;

    private readonly ReadingProgressService _readingService = new();
    private ReadingProgress _reading = new();
    private string? _projectId;

    private record LinkSpan(int Start, int End, string Target, bool IsWeb, string Anchor = "");
    private record LinkChip(Rect Bounds, int LinkIndex, ChipKind Kind, Color Color);

    /// <summary>Как рисуется чип поверх ссылки.</summary>
    private enum ChipKind
    {
        /// <summary>Полупрозрачная «пилюля»: подсвечивает текст, оставляя его читаемым.</summary>
        Pill,

        /// <summary>Непрозрачный кружок: сама ссылка, когда показывать нечего (пустой якорь).</summary>
        Solid,

        /// <summary>
        /// Заплатка цветом фона редактора: прячет служебный символ, который нельзя скрыть
        /// форматированием. Ничего не рисует «от себя» и не реагирует на наведение.
        /// </summary>
        Cover,
    }

    /// <summary>HTTP/HTTPS-ссылка в тексте.</summary>
    private static readonly Regex WebLinkRegex = new(
        @"https?://[^\s<>""]+", RegexOptions.Compiled | RegexOptions.IgnoreCase);

    public MainWindow()
    {
        InitializeComponent();
        Hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        Title = "Neurodoc Editor";
        TitleVersionText.Text = AppVersion.Display;
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        SetWindowIcon();
        SetupWindowSize();

        // события файловой системы приходят пачками (git переписывает десятки файлов) —
        // ждём тишины и только потом перечитываем каталог
        _watchTimer = DispatcherQueue.CreateTimer();
        _watchTimer.Interval = TimeSpan.FromMilliseconds(500);
        _watchTimer.IsRepeating = false;
        _watchTimer.Tick += (_, _) => ReloadFromDisk(manual: false);

        _saveTimer = DispatcherQueue.CreateTimer();
        _saveTimer.Interval = TimeSpan.FromMilliseconds(700);
        _saveTimer.IsRepeating = false;
        _saveTimer.Tick += (_, _) => SaveCurrentFile();

        _tipTimer = DispatcherQueue.CreateTimer();
        _tipTimer.Interval = TimeSpan.FromMilliseconds(1400);
        _tipTimer.IsRepeating = false;
        _tipTimer.Tick += (_, _) => CopyTip.IsOpen = false;

        _readTimer = DispatcherQueue.CreateTimer();
        _readTimer.Interval = TimeSpan.FromSeconds(3);
        _readTimer.IsRepeating = false;
        _readTimer.Tick += (_, _) => MarkCurrentRead(true);

        // Редактор занимает всю прокручиваемую область, поэтому «подтянуть его в видимую
        // область» — это всегда прокрутка к его верху. XAML делает это сам, когда редактор
        // получает фокус, и уже после того, как мы вернули запомненную прокрутку: экран
        // прыгал в начало файла. Видимость каретки мы держим сами (EnsureCaretVisible).
        Editor.BringIntoViewRequested += (_, e) => e.Handled = true;

        Editor.SizeChanged += (_, _) => _overlayDirty = true;
        Editor.LayoutUpdated += (_, _) =>
        {
            if (_pendingView != null) ApplyPendingView();
            if (_overlayDirty)
            {
                _overlayDirty = false;
                RedrawOverlay();
            }
        };

        // ScrollViewer сам помечает колесо обработанным — подписка только через AddHandler
        BreadcrumbsScroll.AddHandler(
            UIElement.PointerWheelChangedEvent, new PointerEventHandler(OnBreadcrumbsWheel), true);
        ToolbarScroll.AddHandler(
            UIElement.PointerWheelChangedEvent, new PointerEventHandler(OnToolbarWheel), true);
        FormatsScroll.AddHandler(
            UIElement.PointerWheelChangedEvent, new PointerEventHandler(OnFormatsWheel), true);

        RootGrid.Loaded += OnRootLoaded;
        RootGrid.ActualThemeChanged += (_, _) =>
        {
            UpdateCaptionColors();
            RedrawOverlay();
            RefreshViewerIfActive();
        };
        Closed += (_, _) => SaveCurrentFile();

        ApplyTheme();
        ApplyEditorFont();
    }

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(nint hwnd);

    private void SetWindowIcon()
    {
        try
        {
            var icon = System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "app.ico");
            if (File.Exists(icon))
                AppWindow.SetIcon(icon);
        }
        catch
        {
            // без иконки приложение всё равно работает
        }
    }

    private void SetupWindowSize()
    {
        try
        {
            // Resize принимает физические пиксели, поэтому учитываем масштаб экрана
            double scale = Math.Max(1.0, GetDpiForWindow(Hwnd) / 96.0);
            // ширина подобрана так, чтобы верхняя панель действий помещалась целиком
            int width = (int)(1440 * scale);
            int height = (int)(860 * scale);
            var area = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Nearest);
            if (area != null)
            {
                width = Math.Min(width, area.WorkArea.Width);
                height = Math.Min(height, area.WorkArea.Height);
            }
            AppWindow.Resize(new Windows.Graphics.SizeInt32(width, height));
            if (area != null)
            {
                var pos = new Windows.Graphics.PointInt32(
                    area.WorkArea.X + (area.WorkArea.Width - AppWindow.Size.Width) / 2,
                    area.WorkArea.Y + (area.WorkArea.Height - AppWindow.Size.Height) / 2);
                AppWindow.Move(pos);
            }
        }
        catch
        {
            // размер окна — не критично
        }
    }

    private void OnRootLoaded(object sender, RoutedEventArgs e)
    {
        Clipboard.ContentChanged += (_, _) => DispatcherQueue.TryEnqueue(() => _ = RefreshClipboardStateAsync());
        Activated += (_, args) =>
        {
            if (args.WindowActivationState != WindowActivationState.Deactivated)
                _ = RefreshClipboardStateAsync();
        };
        _ = RefreshClipboardStateAsync();
        UpdateCaptionColors();
        var project = App.Settings.CurrentProject;
        if (project != null)
        {
            OpenProject(project);
        }
        else
        {
            UpdateProjectUi();
            _ = ShowProjectsDialogAsync();
        }
    }

    // ================= Тема и шрифт =================

    private void ApplyTheme()
    {
        RootGrid.RequestedTheme = App.Settings.Theme switch
        {
            "Light" => ElementTheme.Light,
            "Dark" => ElementTheme.Dark,
            _ => ElementTheme.Default,
        };
        UpdateCaptionColors();
        RedrawOverlay();
    }

    private void UpdateCaptionColors()
    {
        try
        {
            var tb = AppWindow.TitleBar;
            bool dark = RootGrid.ActualTheme == ElementTheme.Dark;
            tb.ButtonBackgroundColor = Colors.Transparent;
            tb.ButtonInactiveBackgroundColor = Colors.Transparent;
            tb.ButtonForegroundColor = dark ? Colors.White : Colors.Black;
            tb.ButtonInactiveForegroundColor = dark
                ? Color.FromArgb(255, 160, 160, 160)
                : Color.FromArgb(255, 120, 120, 120);
            tb.ButtonHoverBackgroundColor = dark
                ? Color.FromArgb(0x18, 255, 255, 255)
                : Color.FromArgb(0x18, 0, 0, 0);
            tb.ButtonHoverForegroundColor = dark ? Colors.White : Colors.Black;
            tb.ButtonPressedBackgroundColor = dark
                ? Color.FromArgb(0x30, 255, 255, 255)
                : Color.FromArgb(0x30, 0, 0, 0);
            tb.ButtonPressedForegroundColor = dark ? Colors.White : Colors.Black;
        }
        catch
        {
            // на старых сборках Windows цвета кнопок могут быть недоступны
        }
    }

    private void ApplyEditorFont()
    {
        Editor.FontFamily = BundledFonts.CreateFontFamily(App.Settings.FontFamily);
        Editor.FontSize = App.Settings.FontSize;
        ApplyDocumentFont();
        _overlayDirty = true;
        RefreshViewerIfActive();
    }

    private void ApplyDocumentFont()
    {
        var doc = Editor.Document;
        bool prev = _suppressTextChanged;
        _suppressTextChanged = true;
        try
        {
            float sizePt = (float)(App.Settings.FontSize * 72.0 / 96.0); // px -> pt
            float lineSpacing = (float)App.Settings.LineSpacing;
            // Для встроенных (не системных) шрифтов имя в формате документа не задаём:
            // XAML сам применяет ms-appx-шрифт из FontFamily контрола, а незнакомое
            // RichEdit имя приводит к откату на шрифт по умолчанию.
            bool setName = !BundledFonts.IsBundled(App.Settings.FontFamily);

            var fmt = doc.GetDefaultCharacterFormat();
            if (setName) fmt.Name = App.Settings.FontFamily;
            fmt.Size = sizePt;
            doc.SetDefaultCharacterFormat(fmt);

            var para = doc.GetDefaultParagraphFormat();
            para.SetLineSpacing(LineSpacingRule.Multiple, lineSpacing);
            doc.SetDefaultParagraphFormat(para);

            doc.GetText(TextGetOptions.None, out var text);
            var range = doc.GetRange(0, text.Length + 1);
            if (setName) range.CharacterFormat.Name = App.Settings.FontFamily;
            range.CharacterFormat.Size = sizePt;
            range.ParagraphFormat.SetLineSpacing(LineSpacingRule.Multiple, lineSpacing);
        }
        finally
        {
            _suppressTextChanged = prev;
        }
    }

    // ================= Проекты =================

    public void OpenProject(ProjectInfo project)
    {
        SaveCurrentFile();
        _readTimer.Stop();
        _projectId = project.Id;
        _reading = _readingService.Load(project.Id);
        _currentId = null;
        _back.Clear();
        _forward.Clear();
        _viewStates.Clear();
        UpdateNavButtons();
        SearchCard.Visibility = Visibility.Collapsed;
        try
        {
            _catalog = NeurodocCatalog.OpenOrCreate(project.Path);
        }
        catch (Exception ex)
        {
            _catalog = null;
            _ = ShowErrorAsync("Не удалось открыть проект", ex.Message);
        }
        UpdateProjectUi();
        SetupGit(_catalog == null ? null : project.Path);
        StartWatching(_catalog == null ? null : project.Path);
        if (_catalog != null)
        {
            OpenFile(NeurodocCatalog.RootId, push: false);
            // откладываем на следующий тик: во время Loaded показ ContentDialog падает
            DispatcherQueue.TryEnqueue(() => _ = PromptMigrationAsync(project));
        }
    }

    /// <summary>
    /// Следит за каталогом на диске: файлы меняют и другие программы — git, внешний
    /// редактор. Без этого редактор показывал бы устаревшее содержимое.
    /// </summary>
    private void StartWatching(string? path)
    {
        _watchTimer.Stop();
        _watcher?.Dispose();
        _watcher = null;
        if (string.IsNullOrEmpty(path) || !Directory.Exists(path)) return;
        try
        {
            var watcher = new FileSystemWatcher(path)
            {
                IncludeSubdirectories = true,
                // DirectoryName — иначе переименование, перенос или удаление папки целиком не
                // дают ни одного события: файлы внутри не трогаются (inotify на Linux это видит)
                NotifyFilter = NotifyFilters.LastWrite | NotifyFilters.FileName | NotifyFilters.DirectoryName | NotifyFilters.Size,
                // checkout/pull переписывают десятки файлов разом — дефолтные 8 КБ переполняются
                InternalBufferSize = 64 * 1024,
            };
            watcher.Changed += OnCatalogChangedOnDisk;
            watcher.Created += OnCatalogChangedOnDisk;
            watcher.Deleted += OnCatalogChangedOnDisk;
            watcher.Renamed += OnCatalogChangedOnDisk;
            watcher.Error += OnCatalogWatchError;
            watcher.EnableRaisingEvents = true;
            _watcher = watcher;
        }
        catch
        {
            // без слежения приложение работает — остаётся кнопка «Обновить»
        }
    }

    private void OnCatalogChangedOnDisk(object sender, FileSystemEventArgs e)
    {
        // git постоянно пишет в .git (fetch, статус, локи) — к содержимому каталога
        // это отношения не имеет, а событий порождает тысячи
        var rel = e.Name ?? "";
        if (rel == ".git" || rel.StartsWith(@".git\", StringComparison.Ordinal))
            return;
        // при переименовании изменились оба пути: старого больше нет, новый появился
        var paths = e is RenamedEventArgs renamed ? new[] { renamed.OldFullPath, e.FullPath } : new[] { e.FullPath };
        DispatcherQueue.TryEnqueue(() =>
        {
            // во время своей git-операции слежение молчит: перечитаем сами по её концу
            var catalog = _catalog;
            if (_gitBusy || catalog == null) return;
            // Собственная запись поднимает то же событие, что и чужая. Отличаем по
            // содержимому: если для всех путей события диск совпадает с каталогом в памяти,
            // перечитывать нечего. Когда перечитывание уже назначено, сверять незачем —
            // только отодвигаем его.
            if (!_watchTimer.IsRunning && paths.All(p => catalog.MatchesDisk(p) && !CurrentFileChanged(p))) return;
            _watchTimer.Stop();
            _watchTimer.Start();
        });
    }

    /// <summary>
    /// Изменился ли на диске открытый файл, если событие о нём. Каталог может и не держать
    /// его в памяти: файл с точкой в пути или большой, открытый по ссылке, читается с диска
    /// мимо неё. Перечитывание сверяет его с последним сохранённым текстом — так же и здесь.
    /// </summary>
    private bool CurrentFileChanged(string path)
    {
        var (catalog, id) = (_catalog, _currentId);
        if (catalog == null || id == null) return false;
        try
        {
            // System.IO полностью: в этом файле есть и Microsoft.UI.Xaml.Shapes.Path
            var current = System.IO.Path.GetFullPath(catalog.GetFilePath(id));
            if (!string.Equals(System.IO.Path.GetFullPath(path), current, StringComparison.OrdinalIgnoreCase)) return false;
        }
        catch
        {
            return false;
        }
        return catalog.ReadFile(id) != _lastSavedText;
    }

    /// <summary>
    /// Очередь событий переполнилась (большой checkout из терминала): что-то упущено,
    /// поэтому перечитываем каталог целиком.
    /// </summary>
    /// <remarks>
    /// Только переполнение. Прочие ошибки — отвалился сетевой или съёмный диск: слежение
    /// .NET уже выключило, а перечитывание недоступного каталога прочло бы его пустым и
    /// опустошило бы редактор. Остаётся кнопка «Обновить».
    /// </remarks>
    private void OnCatalogWatchError(object sender, System.IO.ErrorEventArgs e)
    {
        if (e.GetException() is not InternalBufferOverflowException) return;
        DispatcherQueue.TryEnqueue(() =>
        {
            if (_gitBusy || _catalog == null) return;
            _watchTimer.Stop();
            _watchTimer.Start();
        });
    }

    private void OnReloadClick(object sender, RoutedEventArgs e) => ReloadFromDisk(manual: true);

    private void OnReloadAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        ReloadFromDisk(manual: true);
        args.Handled = true;
    }

    /// <summary>Перечитывает каталог с диска, подхватывая правки других программ.</summary>
    private void ReloadFromDisk(bool manual)
    {
        if (_catalog == null) return;
        _watchTimer.Stop();
        _catalog.Reload();

        if (_currentId != null)
        {
            var onDisk = _catalog.ReadFile(_currentId);
            // свои несохранённые правки чужим содержимым не затираем: пусть автосохранение
            // сначала доедет, внешнее изменение подхватится следующим событием
            if (GetEditorTextForSave() == _lastSavedText && onDisk != _lastSavedText)
            {
                var selection = Editor.Document.Selection;
                int caret = Math.Min(selection.StartPosition, selection.EndPosition);
                LoadTextIntoEditor(onDisk);
                int clamped = Math.Clamp(caret, 0, GetEditorTextRaw().Length);
                Editor.Document.Selection.SetRange(clamped, clamped);
            }
        }

        UpdatePanels();
        UpdateFileUi();
        _overlayDirty = true;
        RedrawOverlay();
        RefreshViewerIfActive();
        if (manual) ShowCopyTip(ReloadButton, "Проект перечитан", _catalog.RootPath);
    }

    private bool _migrationPromptOpen;

    /// <summary>Что именно предстоит сделать.</summary>
    private static string BuildMigrationPrompt(LegacyScan scan) =>
        $"Ссылок старого вида: {scan.Links} в {scan.Files} файлах — их нужно " +
        "перевести в формат [текст](uuid).\n\n" +
        "Перед изменением будет создана резервная копия всего каталога.";

    /// <summary>
    /// Предлагает перевести каталог на формат [текст](uuid), если в нём остались ссылки
    /// старого вида. Файлы пользователя переписываются только после подтверждения и
    /// только после резервной копии.
    /// </summary>
    private async Task PromptMigrationAsync(ProjectInfo project)
    {
        if (_catalog == null || _migrationPromptOpen) return;
        var scan = CatalogMigration.Scan(_catalog);
        if (!scan.NeedsMigration) return;

        _migrationPromptOpen = true;
        try
        {
            var ask = new ContentDialog
            {
                XamlRoot = RootGrid.XamlRoot,
                RequestedTheme = RootGrid.ActualTheme,
                Title = "Каталог в старом формате",
                Content = BuildMigrationPrompt(scan),
                PrimaryButtonText = "Преобразовать",
                CloseButtonText = "Не сейчас",
                DefaultButton = ContentDialogButton.Primary,
            };
            if (await ask.ShowAsync() != ContentDialogResult.Primary) return;

            MigrationResult result;
            try
            {
                result = CatalogMigration.Migrate(_catalog, project.Id);
            }
            catch (Exception ex)
            {
                await ShowErrorAsync("Не удалось преобразовать каталог", ex.Message);
                return;
            }

            // содержимое файлов изменилось — перечитываем каталог и открываем root заново
            _catalog.Reload();
            _currentId = null;
            _back.Clear();
            _forward.Clear();
            UpdateNavButtons();
            OpenFile(NeurodocCatalog.RootId, push: false);

            var done = new List<string>();
            if (result.Links > 0)
                done.Add($"Ссылок переведено: {result.Links} в {result.Files} файлах.");
            if (result.ReadMeUpdated)
                done.Add("Обновлено описание формата в ReadMe.md.");
            done.Add("");
            done.Add($"Резервная копия:\n{result.BackupPath}");
            await ShowMessageAsync("Каталог преобразован", string.Join("\n", done), "Готово");
        }
        finally
        {
            _migrationPromptOpen = false;
        }
    }

    private void UpdateProjectUi()
    {
        var project = App.Settings.CurrentProject;
        bool has = _catalog != null;
        ContentArea.Visibility = has ? Visibility.Visible : Visibility.Collapsed;
        NoProjectPanel.Visibility = has ? Visibility.Collapsed : Visibility.Visible;
        TitleProjectText.Text = has && project != null ? "·  " + project.Name : "";
        ProjectNameText.Text = has && project != null ? project.Name : "Проекты";
        HomeButton.IsEnabled = has;
        InsertLinkButton.IsEnabled = has;
        NewFileButton.IsEnabled = has;
        SearchBox.IsEnabled = has;
        ReplaceButton.IsEnabled = has;
        BrokenLinksButton.IsEnabled = has;
        OpenByIdButton.IsEnabled = has;
        ReloadButton.IsEnabled = has;
        // git-кнопка живёт и без проекта: клонирование создаёт первый проект
        GitButton.IsEnabled = true;
        UpdateLinkSelectionButton();
        RawToggle.IsEnabled = has;
        SyncRawToggle();
        SvgToggle.IsEnabled = has;
        JsonToggle.IsEnabled = has;
        YamlToggle.IsEnabled = has;
        BashToggle.IsEnabled = has;
        MdToggle.IsEnabled = has;
        AdocToggle.IsEnabled = has;
        HtmlToggle.IsEnabled = has;
        NginxToggle.IsEnabled = has;
        if (!has)
        {
            CopyLinkButton.IsEnabled = false;
            CopyContentButton.IsEnabled = false;
            DeleteButton.IsEnabled = false;
            BackButton.IsEnabled = false;
            ForwardButton.IsEnabled = false;
        }
    }

    private async Task ShowProjectsDialogAsync()
    {
        var dialog = new ProjectsDialog(App.Settings, () => App.SettingsService.Save(App.Settings))
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
        };
        await dialog.ShowAsync();

        var current = App.Settings.CurrentProject;
        if (dialog.ProjectToOpen != null)
        {
            OpenProject(dialog.ProjectToOpen);
        }
        else if (current == null)
        {
            _catalog = null;
            _currentId = null;
            UpdateProjectUi();
        }
        else if (_catalog == null || !string.Equals(_catalog.RootPath, current.Path, StringComparison.OrdinalIgnoreCase))
        {
            OpenProject(current);
        }
        else
        {
            UpdateProjectUi();
        }
    }

    private async void OnProjectsClick(object sender, RoutedEventArgs e) => await ShowProjectsDialogAsync();

    private async void OnSettingsClick(object sender, RoutedEventArgs e)
    {
        var dialog = new SettingsDialog(App.Settings, () =>
        {
            App.SettingsService.Save(App.Settings);
            ApplyTheme();
            ApplyEditorFont();
        })
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
        };
        await dialog.ShowAsync();
    }

    // ================= Открытие и сохранение файлов =================

    private void OpenFile(string id, bool push = true)
    {
        if (_catalog == null) return;
        // в нижний регистр приводим только UUID: у пути регистр — часть имени файла
        if (NeurodocCatalog.IsUuid(id)) id = id.ToLowerInvariant();
        if (string.Equals(_currentId, id, StringComparison.OrdinalIgnoreCase)) return;

        SaveCurrentFile();
        if (_currentId != null)
        {
            var previousSelection = Editor.Document.Selection;
            _viewStates[_currentId] = new ViewState(
                Math.Min(previousSelection.StartPosition, previousSelection.EndPosition),
                EditorScroll.VerticalOffset,
                EditorScroll.ExtentHeight,
                _viewMode);
        }
        if (push && _currentId != null)
        {
            _back.Push(_currentId);
            _forward.Clear(); // как в браузере: новая навигация обнуляет «вперёд»
        }
        _currentId = id;

        LoadTextIntoEditor(_catalog.ReadFile(id));
        // отметку о прочтении ставим не сразу: случайный переход не должен её засчитать
        _readTimer.Stop();
        _readTimer.Start();
        UpdatePanels();
        UpdateFileUi();
        UpdateNavButtons();
        if (FindBar.Visibility == Visibility.Visible)
            UpdateFindCount();

        // файл в собственном формате показываем сразу отрендеренным: так его и читают,
        // а правка требует осознанно выключить режим — контракты труднее задеть случайно
        // Режим, выбранный пользователем для этого файла, важнее расширения: он мог
        // осознанно выключить рендер, чтобы править текст, — и возврат не должен это
        // отменять. Расширение решает только при первом заходе.
        var mode = _viewStates.TryGetValue(id, out var seen) ? seen.Mode : DetectViewMode(id);
        if (mode != ViewMode.Text)
        {
            _ = SetViewModeAsync(mode);
            return;
        }
        Editor.Focus(FocusState.Programmatic);
        RestoreCaret(id);
    }

    /// <summary>Возвращает каретку на её последнее место в файле (в рамках сеанса).</summary>
    private void RestoreCaret(string id)
    {
        if (!_viewStates.TryGetValue(id, out var state)) return;
        if (state.Caret <= 0 && state.Offset <= 0) return;
        int clamped = Math.Clamp(state.Caret, 0, GetEditorTextRaw().Length);

        Editor.Document.Selection.SetRange(clamped, clamped);
        _restoredCaret = clamped;

        // Прокрутку возвращаем не по таймеру, а когда раскладка досчитается: сразу после
        // загрузки ExtentHeight ещё принадлежит предыдущему файлу, и сравнивать не с чем.
        _pendingView = state;
        _pendingViewPasses = 0;
    }

    /// <summary>
    /// Возвращает прокрутку, как только высота контента станет прежней. Высота — отпечаток
    /// раскладки: она меняется и от ширины окна (текст переносится иначе), и от шрифта с
    /// межстрочным интервалом, и от режима «как есть». Совпала — смещение указывает на тот
    /// же текст; не совпала — это был бы прыжок в случайное место, и мы просто оставляем
    /// вид на каретке.
    /// </summary>
    private void ApplyPendingView()
    {
        var state = _pendingView;
        if (state == null) return;

        bool sameLayout = EditorScroll.ExtentHeight > 0
                          && Math.Abs(EditorScroll.ExtentHeight - state.Extent) < 0.5;
        if (sameLayout && state.Offset > 0 && state.Offset <= EditorScroll.ScrollableHeight + 0.5)
        {
            // Смещение просим повторно, пока оно действительно не встанет: сразу после
            // загрузки файла и установки фокуса RichEdit сам подкручивает вид к каретке,
            // а ChangeView в незавершённой раскладке просто возвращает false.
            if (Math.Abs(EditorScroll.VerticalOffset - state.Offset) < 1.0)
            {
                _pendingView = null;
                return;
            }
            EditorScroll.ChangeView(null, state.Offset, null, disableAnimation: true);
        }

        // раскладка считается за несколько проходов; ждём, но не бесконечно —
        // при изменившемся размере окна совпадения не будет никогда
        if (++_pendingViewPasses < 60) return;
        _pendingView = null;
        EnsureCaretVisible();
    }

    private void UpdateNavButtons()
    {
        BackButton.IsEnabled = CanNavigate(_back);
        ForwardButton.IsEnabled = CanNavigate(_forward);
    }

    /// <summary>
    /// Есть ли в истории файл, который ещё существует. Считать записи мало: переход
    /// пропускает исчезнувшие файлы, и стопка из одних таких оставляла бы кнопку
    /// доступной, а нажатие — без всякого действия.
    /// </summary>
    private bool CanNavigate(Stack<string> history)
    {
        var catalog = _catalog;
        return catalog != null && history.Any(catalog.Exists);
    }

    private void LoadTextIntoEditor(string text)
    {
        // RichEdit отдаёт текст только с CRLF, а в каталогах neurodoc обычно LF
        // (так пишет и Android-версия). Запоминаем стиль файла, чтобы сохранение
        // не переписывало его целиком и не ломало diff'ы.
        _newline = text.Contains("\r\n") ? "\r\n" : "\n";
        ResetViewMode();
        _suppressTextChanged = true;
        try
        {
            Editor.Document.SetText(TextSetOptions.None, text);
            ApplyDocumentFont();
            Editor.Document.Selection.SetRange(0, 0);
        }
        finally
        {
            _suppressTextChanged = false;
        }
        _hoverLink = -1;
        RecomputeLinks();
        _compactSignature = null; // документ новый — формат применяем заново
        ApplyCompactLinks();
        // эталон для автосохранения считаем уже после скрытия UUID: GetText отдаёт
        // и скрытый текст, но эталон обязан совпадать с тем, что вернётся при записи
        _lastSavedText = GetEditorTextForSave();
        _pendingView = null; // ожидание от предыдущего файла больше не актуально
        _restoredCaret = -1;
        ClearUndoHistory();
        _overlayDirty = true;
        EditorScroll.ChangeView(null, 0, null, true);
    }

    /// <summary>Сбрасывает историю Undo редактора (обнуление лимита освобождает буфер).</summary>
    /// <remarks>
    /// История принадлежит контролу, а не документу, и переживает SetText. Без сброса
    /// Ctrl+Z в только что открытом файле откатывает правку ПРЕДЫДУЩЕГО — чужой текст
    /// попадает в документ, и автосохранение пишет его на диск.
    /// </remarks>
    private void ClearUndoHistory()
    {
        var doc = Editor.Document;
        int limit = (int)doc.UndoLimit;
        doc.UndoLimit = 0;
        doc.UndoLimit = (uint)(limit > 0 ? limit : 100);
    }

    private string GetEditorTextRaw()
    {
        Editor.Document.GetText(TextGetOptions.None, out var text);
        return text;
    }

    private string GetEditorTextForSave()
    {
        Editor.Document.GetText(TextGetOptions.UseCrlf, out var text);
        return _newline == "\r\n" ? text : text.Replace("\r\n", _newline);
    }

    private void SaveCurrentFile()
    {
        _saveTimer.Stop();
        if (_catalog == null || _currentId == null) return;
        var text = GetEditorTextForSave();
        if (text == _lastSavedText) return;
        try
        {
            _catalog.WriteFile(_currentId, text);
            _lastSavedText = text;
        }
        catch (Exception ex)
        {
            _ = ShowErrorAsync("Не удалось сохранить файл", ex.Message);
            return;
        }
        UpdatePanels();
    }

    private void OnEditorTextChanged(object sender, RoutedEventArgs e)
    {
        if (_suppressTextChanged) return;
        RecomputeLinks();
        ApplyCompactLinks();
        if (FindBar.Visibility == Visibility.Visible)
        {
            UpdateFindCount(); // пересчитать позиции совпадений (сам перерисует оверлей)
            _overlayDirty = true; // и ещё раз после layout, если строки переносятся
        }
        else
        {
            InvalidateOverlay();
        }
        _saveTimer.Stop();
        _saveTimer.Start();
    }

    private void OnSaveAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        SaveCurrentFile();
        args.Handled = true;
    }

    private void OnFindAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        if (SearchBox.IsEnabled)
            SearchBox.Focus(FocusState.Programmatic);
        args.Handled = true;
    }

    private void OnRootPreviewKeyDown(object sender, KeyRoutedEventArgs e)
    {
        // навигация Alt+←/→ как в браузере — для любого фокуса внутри окна
        if (!IsKeyDown(Windows.System.VirtualKey.Menu) ||
            IsKeyDown(Windows.System.VirtualKey.Shift) ||
            IsKeyDown(Windows.System.VirtualKey.Control))
        {
            return;
        }
        if (e.Key == Windows.System.VirtualKey.Left)
        {
            GoBack();
            e.Handled = true;
        }
        else if (e.Key == Windows.System.VirtualKey.Right)
        {
            GoForward();
            e.Handled = true;
        }
    }

    // ================= Автоотступ и вставка =================

    private static bool IsKeyDown(Windows.System.VirtualKey key) =>
        Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(key)
            .HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);

    private void OnEditorPreviewKeyDown(object sender, KeyRoutedEventArgs e)
    {
        bool shift = IsKeyDown(Windows.System.VirtualKey.Shift);
        bool alt = IsKeyDown(Windows.System.VirtualKey.Menu);
        bool ctrl = IsKeyDown(Windows.System.VirtualKey.Control);

        // Shift+Alt+Вверх/Вниз — переместить текущую строку
        if (shift && alt &&
            e.Key is Windows.System.VirtualKey.Up or Windows.System.VirtualKey.Down)
        {
            MoveCurrentLine(e.Key == Windows.System.VirtualKey.Down ? 1 : -1);
            e.Handled = true;
            return;
        }

        // Shift+Delete — вырезать: выделение вырезает сам RichEdit,
        // без выделения (как в IntelliJ IDEA) вырезаем текущую строку
        if (shift && !alt && !ctrl && e.Key == Windows.System.VirtualKey.Delete)
        {
            var sel = Editor.Document.Selection;
            if (sel.StartPosition == sel.EndPosition)
            {
                DeleteCurrentLine(copyToClipboard: true);
                e.Handled = true;
            }
            return;
        }

        // Ctrl+Y — удалить текущую строку без копирования (как в IntelliJ IDEA)
        if (ctrl && !alt && !shift && e.Key == Windows.System.VirtualKey.Y)
        {
            DeleteCurrentLine(copyToClipboard: false);
            e.Handled = true;
            return;
        }

        // Ctrl+G — переход по ссылке, на которой стоит каретка
        if (ctrl && !alt && !shift && e.Key == Windows.System.VirtualKey.G)
        {
            NavigateLinkAtCaret();
            e.Handled = true;
            return;
        }

        // Ctrl+B — «Битые ссылки»: RichEdit съедает его (полужирный) раньше акселератора
        if (ctrl && !alt && !shift && e.Key == Windows.System.VirtualKey.B)
        {
            ToggleBrokenLinksCard();
            e.Handled = true;
            return;
        }

        // Ctrl+L — связать выделение со ссылкой из буфера. RichEdit съедает его
        // (выравнивание влево) раньше глобального акселератора
        if (ctrl && !alt && !shift && e.Key == Windows.System.VirtualKey.L)
        {
            _ = LinkSelectionAsync();
            e.Handled = true;
            return;
        }

        // Ctrl+U — снять ссылку, оставив текст. RichEdit съедает его (подчёркивание)
        if (ctrl && !alt && !shift && e.Key == Windows.System.VirtualKey.U)
        {
            RemoveLinkAtCaret();
            e.Handled = true;
            return;
        }

        // Ctrl+R / Ctrl+Shift+R — «Поиск и замена» (файл / проект): RichEdit съедает
        // Ctrl+R (выравнивание вправо) раньше глобального акселератора
        if (ctrl && !alt && e.Key == Windows.System.VirtualKey.R)
        {
            _ = ShowReplaceDialogAsync(wholeProjectScope: shift);
            e.Handled = true;
            return;
        }

        // Tab / Shift+Tab — отступ 4 пробелами (символ табуляции не вставляется)
        if (!ctrl && !alt && e.Key == Windows.System.VirtualKey.Tab)
        {
            HandleTabIndent(unindent: shift);
            e.Handled = true;
            return;
        }

        // Ctrl+Z — отмена, Ctrl+Shift+Z — повтор (Ctrl+Y занят удалением строки).
        // Оба идут через UndoSkippingFormatting: скрытие UUID в компактном режиме
        // тоже попадает в стек RichEdit, и без пропуска Ctrl+Z разворачивал бы ссылку.
        if (ctrl && !alt && e.Key == Windows.System.VirtualKey.Z)
        {
            UndoSkippingFormatting(redo: shift);
            e.Handled = true;
            return;
        }

        if (e.Key != Windows.System.VirtualKey.Enter) return;

        var selection = Editor.Document.Selection;
        int selStart = Math.Min(selection.StartPosition, selection.EndPosition);
        var text = GetEditorTextRaw();
        selStart = Math.Clamp(selStart, 0, text.Length);

        int lineStart = selStart == 0 ? 0 : text.LastIndexOf('\r', selStart - 1) + 1;
        var indent = new StringBuilder();
        for (int i = lineStart; i < selStart && i < text.Length; i++)
        {
            char c = text[i];
            if (c is ' ' or '\t') indent.Append(c);
            else break;
        }

        e.Handled = true;
        selection.TypeText("\r" + indent);
    }

    private const string IndentUnit = "    "; // 4 пробела

    /// <summary>Tab/Shift+Tab: отступ выделенных строк (или вставка/снятие отступа у каретки).</summary>
    private void HandleTabIndent(bool unindent)
    {
        var doc = Editor.Document;
        var selection = doc.Selection;
        int selStart = Math.Min(selection.StartPosition, selection.EndPosition);
        int selEnd = Math.Max(selection.StartPosition, selection.EndPosition);
        var text = GetEditorTextRaw();
        selStart = Math.Clamp(selStart, 0, text.Length);
        selEnd = Math.Clamp(selEnd, 0, text.Length);
        bool hasSelection = selEnd > selStart;

        if (!unindent && !hasSelection)
        {
            selection.TypeText(IndentUnit);
            return;
        }

        int blockStart = selStart == 0 ? 0 : text.LastIndexOf('\r', selStart - 1) + 1;
        // если конец выделения стоит в начале строки, эту строку не трогаем
        int lastPosition = hasSelection && selEnd > 0 && text.Length > 0 && text[selEnd - 1] == '\r'
            ? selEnd - 1
            : selEnd;
        int blockEnd = lastPosition < text.Length ? text.IndexOf('\r', lastPosition) : text.Length;
        if (blockEnd < 0) blockEnd = text.Length;

        var block = text[blockStart..blockEnd];
        var lines = block.Split('\r');
        int removedFromFirst = 0;
        for (int i = 0; i < lines.Length; i++)
        {
            if (unindent)
            {
                int removed;
                if (lines[i].StartsWith('\t'))
                {
                    lines[i] = lines[i][1..];
                    removed = 1;
                }
                else
                {
                    removed = 0;
                    while (removed < IndentUnit.Length && removed < lines[i].Length && lines[i][removed] == ' ')
                        removed++;
                    lines[i] = lines[i][removed..];
                }
                if (i == 0) removedFromFirst = removed;
            }
            else if (lines[i].Length > 0)
            {
                lines[i] = IndentUnit + lines[i];
            }
        }

        var replaced = string.Join('\r', lines);
        if (replaced == block) return;
        doc.GetRange(blockStart, blockEnd).SetText(TextSetOptions.None, replaced);

        if (!hasSelection)
        {
            int caret = Math.Max(blockStart, selStart - removedFromFirst);
            selection.SetRange(caret, caret);
        }
        else
        {
            selection.SetRange(blockStart, blockStart + replaced.Length);
        }
    }

    /// <summary>Границы строки, в которой находится позиция: [start, endNoCr) без символа '\r'.</summary>
    private static (int Start, int End) GetLineBounds(string text, int position)
    {
        position = Math.Clamp(position, 0, text.Length);
        int start = position == 0 ? 0 : text.LastIndexOf('\r', position - 1) + 1;
        int end = position < text.Length ? text.IndexOf('\r', position) : -1;
        if (end < 0) end = text.Length;
        return (start, end);
    }

    private void MoveCurrentLine(int direction)
    {
        var doc = Editor.Document;
        var selection = doc.Selection;
        int caret = Math.Min(selection.StartPosition, selection.EndPosition);
        var text = GetEditorTextRaw();
        var (curStart, curEnd) = GetLineBounds(text, caret);
        int caretOffset = Math.Max(0, caret - curStart);
        string current = text[curStart..curEnd];

        if (direction < 0)
        {
            if (curStart == 0) return;
            int prevStart = curStart >= 2 ? text.LastIndexOf('\r', curStart - 2) + 1 : 0;
            string previous = text[prevStart..(curStart - 1)];
            doc.GetRange(prevStart, curEnd).SetText(TextSetOptions.None, current + "\r" + previous);
            int newCaret = prevStart + Math.Min(caretOffset, current.Length);
            selection.SetRange(newCaret, newCaret);
        }
        else
        {
            int nextStart = curEnd + 1;
            if (curEnd >= text.Length || nextStart >= text.Length) return; // последняя строка
            int nextEnd = text.IndexOf('\r', nextStart);
            if (nextEnd < 0) nextEnd = text.Length;
            string next = text[nextStart..nextEnd];
            doc.GetRange(curStart, nextEnd).SetText(TextSetOptions.None, next + "\r" + current);
            int newCaret = curStart + next.Length + 1 + Math.Min(caretOffset, current.Length);
            selection.SetRange(newCaret, newCaret);
        }
    }

    private void DeleteCurrentLine(bool copyToClipboard)
    {
        var doc = Editor.Document;
        var selection = doc.Selection;
        int caret = Math.Min(selection.StartPosition, selection.EndPosition);
        var text = GetEditorTextRaw();
        if (text.Length == 0) return;
        var (curStart, curEnd) = GetLineBounds(text, caret);

        if (copyToClipboard)
        {
            try
            {
                var package = new DataPackage();
                package.SetText(text[curStart..curEnd] + Environment.NewLine);
                Clipboard.SetContent(package);
            }
            catch
            {
                // буфер обмена может быть занят другим процессом
            }
        }

        int deleteFrom = curStart;
        int deleteTo = curEnd < text.Length ? curEnd + 1 : curEnd; // включая '\r'
        if (curEnd >= text.Length && curStart > 0)
            deleteFrom = curStart - 1; // последняя строка: забираем и предыдущий '\r'
        if (deleteTo <= deleteFrom) return;

        doc.GetRange(deleteFrom, deleteTo).SetText(TextSetOptions.None, "");
        int newCaret = Math.Min(deleteFrom, Math.Max(0, GetEditorTextRaw().Length - 1));
        selection.SetRange(newCaret, newCaret);
    }

    private async void OnEditorPaste(object sender, TextControlPasteEventArgs e)
    {
        e.Handled = true;
        try
        {
            var data = Clipboard.GetContent();
            if (data.Contains(StandardDataFormats.Text))
            {
                var text = await data.GetTextAsync();
                text = text.Replace("\r\n", "\r").Replace("\n", "\r");
                Editor.Document.Selection.TypeText(text);
            }
        }
        catch
        {
            // буфер обмена может быть занят другим процессом
        }
    }

    // ================= Ссылки: разбор, отрисовка, наведение, клик =================

    private void RecomputeLinks()
    {
        _links.Clear();
        var text = GetEditorTextRaw();
        // цель ссылки считается ОТ ФАЙЛА, в котором она написана
        var source = _currentId ?? NeurodocCatalog.RootId;
        foreach (Match m in NeurodocCatalog.LinkRegex.Matches(text))
        {
            var raw = m.Groups[2].Value;
            var anchor = m.Groups[1].Value;
            if (NeurodocCatalog.IsWebTarget(raw))
            {
                _links.Add(new LinkSpan(m.Index, m.Index + m.Length, raw, IsWeb: true, Anchor: anchor));
            }
            else if (NeurodocCatalog.TryResolveLink(source, raw, out var target))
            {
                _links.Add(new LinkSpan(m.Index, m.Index + m.Length, target, IsWeb: false, Anchor: anchor));
            }
            // не разобралась — ведёт за пределы каталога; чипа нет, о ней скажет «Битые ссылки»
        }
        foreach (Match m in WebLinkRegex.Matches(text))
        {
            // адрес внутри [текста](…) уже учтён выше — второй чип поверх был бы лишним
            if (_links.Any(l => m.Index >= l.Start && m.Index < l.End)) continue;
            // завершающая пунктуация предложения — не часть адреса
            var url = m.Value.TrimEnd('.', ',', ';', ':', '!', '?', ')', ']', '\'', '"');
            int schemeEnd = url.IndexOf("://", StringComparison.Ordinal);
            if (schemeEnd < 0 || schemeEnd + 3 >= url.Length) continue;
            _links.Add(new LinkSpan(m.Index, m.Index + url.Length, url, IsWeb: true));
        }
        _links.Sort((a, b) => a.Start.CompareTo(b.Start));
    }

    /// <summary>Держит каретку в видимой области внешнего ScrollViewer.</summary>
    private void EnsureCaretVisible()
    {
        try
        {
            if (EditorScroll.ViewportHeight <= 0) return;
            Editor.Document.Selection.GetRect(PointOptions.AllowOffClient, out Rect rect, out _);
            double top = rect.Y + Editor.Padding.Top;
            double bottom = top + Math.Max(rect.Height, 16);
            double viewTop = EditorScroll.VerticalOffset;
            double viewBottom = viewTop + EditorScroll.ViewportHeight;
            const double margin = 12;
            if (top < viewTop + margin)
                EditorScroll.ChangeView(null, Math.Max(0, top - margin), null);
            else if (bottom > viewBottom - margin)
                EditorScroll.ChangeView(null, bottom - EditorScroll.ViewportHeight + margin, null);
        }
        catch
        {
            // прокрутка к каретке — не критичная операция
        }
    }

    private void OnEditorSelectionChanged(object sender, RoutedEventArgs e)
    {
        if (_suppressTextChanged) return;
        var selection = Editor.Document.Selection;
        int from = Math.Min(selection.StartPosition, selection.EndPosition);
        int to = Math.Max(selection.StartPosition, selection.EndPosition);
        if (to > from) _lastSelection = (from, to);
        UpdateLinkSelectionButton();
        // Пока возвращаем запомненный вид — каретку под него не подкручиваем. Пользователь
        // мог уйти прокруткой далеко от каретки, и это его позиция; иначе EnsureCaretVisible
        // тут же утащит экран обратно к каретке, и возврат «назад» снова прыгнет.
        // То же и после восстановления: RichEdit поднимает SelectionChanged задним числом,
        // уже обработав фокус, — такое событие каретку не двигало и вид менять не должно.
        if (from == _restoredCaret && to == _restoredCaret) return;
        _restoredCaret = -1;
        if (_pendingView == null) EnsureCaretVisible();
    }

    /// <summary>
    /// «Связать» работает, только когда есть что связывать: выделенный текст в
    /// neurodoc-файле и ссылка в буфере обмена.
    /// </summary>
    private void UpdateLinkSelectionButton()
    {
        var selection = Editor.Document.Selection;
        bool inNeurodocFile = _catalog != null && _currentId != null
                              && NeurodocCatalog.IsNeurodocFile(_currentId);
        bool hasSelection = selection.EndPosition != selection.StartPosition;
        LinkSelectionButton.IsEnabled = inNeurodocFile && hasSelection && _clipboardHasLink;
    }

    /// <summary>
    /// Перечитывает буфер обмена. Событие ContentChanged не приходит, пока приложение
    /// неактивно, поэтому перепроверяем ещё и при возврате фокуса в окно.
    /// </summary>
    private async Task RefreshClipboardStateAsync()
    {
        _clipboardHasLink = await ReadClipboardTargetAsync() != null;
        UpdateLinkSelectionButton();
    }

    /// <summary>
    /// Достаёт из буфера обмена цель ссылки и приводит её к id файла каталога.
    /// Принимает и «[текст](цель)», и голую цель — UUID или путь.
    /// </summary>
    /// <remarks>
    /// Ссылкой считается только то, что ведёт на СУЩЕСТВУЮЩИЙ файл каталога. Иначе за путь
    /// сходит любой текст: <see cref="NeurodocCatalog.TryResolveLink"/> отвергает лишь
    /// веб-адреса, абсолютные пути и выход за корень, поэтому обычная скопированная фраза
    /// проходила как относительный путь и уезжала в файл ссылкой (проверено на практике).
    /// </remarks>
    private async Task<string?> ReadClipboardTargetAsync()
    {
        if (_catalog == null) return null;
        try
        {
            var content = Clipboard.GetContent();
            if (!content.Contains(StandardDataFormats.Text)) return null;
            var raw = (await content.GetTextAsync() ?? "").Trim();
            if (raw.Length == 0 || raw.Length > 500) return null;

            var match = NeurodocCatalog.LinkRegex.Match(raw);
            var target = match.Success ? match.Groups[2].Value : raw;
            // в цели не бывает пробелов — сразу отсекаем произвольный текст
            if (target.Length == 0 || target.Any(char.IsWhiteSpace)) return null;
            // «Скопировать ссылку» кладёт путь в виде для content — оттуда и разбираем
            if (!NeurodocCatalog.TryResolveLink(NeurodocCatalog.ContentDir + "/x", target, out var id))
                return null;
            return _catalog.Exists(id) ? id : null;
        }
        catch
        {
            // буфер может быть занят другим приложением — молча считаем, что ссылки нет
            return null;
        }
    }

    private void OnLinkSelectionClick(object sender, RoutedEventArgs e) => _ = LinkSelectionAsync();

    /// <summary>
    /// Связывает выделенный текст со ссылкой из буфера: выделение становится текстом
    /// ссылки, цель берётся из буфера.
    /// </summary>
    /// <remarks>
    /// Путь до цели пересчитывается от ТЕКУЩЕГО файла: в буфере он записан в виде для
    /// content, и вставленный как есть в root он бы увёл за пределы каталога.
    /// </remarks>
    private async Task LinkSelectionAsync()
    {
        if (_catalog == null || _currentId == null) return;

        if (!NeurodocCatalog.IsNeurodocFile(_currentId))
        {
            ShowCopyTip(LinkSelectionButton, "Здесь ссылок не бывает",
                        $"Ссылки читаются только из root и файлов {NeurodocCatalog.ContentDir}");
            return;
        }

        var selection = Editor.Document.Selection;
        int from = Math.Min(selection.StartPosition, selection.EndPosition);
        int to = Math.Max(selection.StartPosition, selection.EndPosition);
        if (to <= from) (from, to) = _lastSelection; // фокус мог уйти на кнопку

        var text = GetEditorTextRaw();
        from = Math.Clamp(from, 0, text.Length);
        to = Math.Clamp(to, 0, text.Length);
        if (to <= from)
        {
            ShowCopyTip(LinkSelectionButton, "Ничего не выделено",
                        "Выделите текст, который станет текстом ссылки");
            return;
        }

        if (_links.Any(l => from < l.End && to > l.Start))
        {
            ShowCopyTip(LinkSelectionButton, "Выделение задевает ссылку",
                        "Вложенных ссылок не бывает");
            return;
        }

        (from, to) = TrimRange(text, from, to);
        var anchor = from < to ? text[from..to] : "";
        if (!IsAnchorUsable(LinkSelectionButton, anchor)) return;

        var targetId = await ReadClipboardTargetAsync();
        if (targetId == null)
        {
            ShowCopyTip(LinkSelectionButton, "В буфере нет ссылки на файл",
                        "Скопируйте ссылку кнопкой «Копировать ссылку»");
            return;
        }

        var target = NeurodocCatalog.MakeRelativeTarget(NeurodocCatalog.DirOf(_currentId), targetId);
        selection.SetRange(from, to);
        selection.TypeText("[" + anchor + "](" + target + ")");
        SaveCurrentFile();
        ShowCopyTip(LinkSelectionButton, "Связано", _catalog.GetTitle(targetId));
    }

    /// <summary>
    /// Компактный режим ссылок: тело UUID скрывается, на виду остаются только угловые
    /// скобки — их целиком закрывает сплошной кружок на оверлее.
    /// </summary>
    /// <remarks>
    /// Скобки специально оставлены видимыми: текст, набранный вплотную к ссылке,
    /// наследует формат соседнего символа, и от скрытого «&gt;» он стал бы невидимым.
    /// Формат применяется, только когда изменился сам набор ссылок: при обычном наборе
    /// текста RichEdit сдвигает формат вместе с символами сам, а лишние обращения к
    /// CharacterFormat засоряли бы историю Undo (иначе документ мы не форматируем).
    /// </remarks>
    /// <param name="force">
    /// Перепроверить видимость, даже если набор ссылок не менялся: нужно после отмены,
    /// которая могла откатить наше форматирование.
    /// </param>
    private void ApplyCompactLinks(bool force = false)
    {
        // в режиме «как есть» файл показывается ровно как на диске — ничего не скрываем
        // прячем всегда, кроме режима «как есть»: отдельной настройки для этого нет —
        // развёрнутый вид даёт тот же тумблер
        bool compact = !App.Settings.RawEditing;
        // В подписи намеренно только цели, без позиций: при вставке текста перед ссылкой
        // её смещение меняется, но RichEdit двигает формат вместе с символами сам.
        // Позиции в подписи заставляли бы переформатировать документ на каждой правке —
        // это и лишние шаги Undo, и сброшенный стек Redo.
        var signature = compact
            ? string.Join(",", _links.Where(l => !l.IsWeb).Select(l => l.Target))
            : null;
        bool setChanged = signature != _compactSignature;
        if (!setChanged && !force) return;

        var doc = Editor.Document;
        bool prev = _suppressTextChanged;
        _suppressTextChanged = true;
        try
        {
            // Снимаем скрытие со всего документа только когда набор ссылок изменился:
            // ссылку могли разобрать правкой, и её «хвост» остался бы невидимым куском
            // текста. При обычной перепроверке это лишняя запись в стеке Undo.
            if (setChanged)
            {
                doc.GetText(TextGetOptions.None, out var text);
                doc.GetRange(0, text.Length + 1).CharacterFormat.Hidden = FormatEffect.Off;
            }

            if (compact)
                foreach (var link in _links.Where(l => !l.IsWeb))
                {
                    // Из [текст](uuid) прячем "]", "(" и UUID. Видимыми обязаны остаться
                    // ровно два символа — "[" перед якорем и ")" в конце: текст, набранный
                    // вплотную к ним (в начале якоря или сразу после ссылки), наследует
                    // формат соседа и от скрытого стал бы невидимым. Эти двое закрашиваются
                    // на оверлее цветом фона редактора, поэтому глазу их всё равно не видно.
                    var body = doc.GetRange(AnchorEnd(link), link.End - 1);
                    // трогаем формат, только если он и правда другой: каждое присваивание
                    // CharacterFormat — отдельный шаг Undo и сброс стека Redo
                    if (body.CharacterFormat.Hidden != FormatEffect.On)
                        body.CharacterFormat.Hidden = FormatEffect.On;
                }
        }
        finally
        {
            _suppressTextChanged = prev;
        }
        _compactSignature = signature;
    }

    /// <summary>Позиция символа "]" в ссылке [текст](uuid).</summary>
    private static int AnchorEnd(LinkSpan link) => link.Start + 1 + link.Anchor.Length;

    /// <summary>
    /// Цвет фона редактора — им закрашиваются служебные символы ссылки. Берём из карточки,
    /// а не константой, чтобы не разъезжаться с темой из App.xaml.
    /// </summary>
    private Color EditorBackgroundColor() =>
        EditorCard.Background is SolidColorBrush brush
            ? brush.Color
            : (RootGrid.ActualTheme == ElementTheme.Dark
                ? Color.FromArgb(0xFF, 0x1A, 0x1A, 0x1A)
                : Colors.White);

    /// <summary>Чем подписать ссылку в подсказке: заголовком цели, а для битой — её id.</summary>
    private string TargetTip(string id) => _catalog?.GetTitle(id) ?? id;

    /// <summary>Отмена/повтор с пропуском шагов, не изменивших текст.</summary>
    /// <remarks>
    /// Компактный режим прячет UUID через CharacterFormat, а RichEdit кладёт такое
    /// форматирование в стек Undo наравне с правками. Без пропуска первый Ctrl+Z
    /// просто разворачивал бы ссылку вместо отмены того, что набрал пользователь.
    /// </remarks>
    private void UndoSkippingFormatting(bool redo)
    {
        var doc = Editor.Document;
        var before = GetEditorTextRaw();
        // потолок шагов — страховка от зацикливания, если текст перестал меняться
        for (int step = 0; step < 64; step++)
        {
            if (redo ? !doc.CanRedo() : !doc.CanUndo()) break;
            if (redo) doc.Redo(); else doc.Undo();
            if (GetEditorTextRaw() != before) break;
        }
        // откат мог развернуть ссылку, не тронув текст, — проверяем видимость явно
        RecomputeLinks();
        ApplyCompactLinks(force: true);
        _overlayDirty = true;
        RedrawOverlay();
    }

    /// <summary>Просит перерисовать оверлей — ровно один раз.</summary>
    /// <remarks>
    /// Правка внутри строки не меняет высоту контента, и LayoutUpdated может не сработать
    /// вовсе — тогда чипы «зависли» бы на старых местах. Раньше поэтому рисовали и сразу,
    /// и ещё раз после layout: две полные перерисовки на каждое нажатие клавиши. Теперь
    /// отложенный вызов сверяется с флагом — если layout уже прошёл и оверлей перерисован,
    /// делать нечего. Порядок безразличен: координаты TOM не ждут раскладки XAML.
    /// </remarks>
    private void InvalidateOverlay()
    {
        _overlayDirty = true;
        if (_overlayScheduled) return;
        _overlayScheduled = true;
        DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
        {
            _overlayScheduled = false;
            if (!_overlayDirty) return;
            _overlayDirty = false;
            RedrawOverlay();
        });
    }

    private void RedrawOverlay()
    {
        _chips.Clear();
        _chipShapes.Clear();
        LinkOverlay.Children.Clear();
        if (_catalog == null) return;
        DrawFindHighlights();
        // «как есть»: подсветку ссылок не рисуем совсем, но совпадения поиска оставляем —
        // без них найденное не видно, нативное выделение RichEdit без фокуса невидимо
        if (App.Settings.RawEditing) return;
        if (_links.Count == 0) return;

        bool dark = RootGrid.ActualTheme == ElementTheme.Dark;
        Color accent = GetAccentColor(dark);
        Color webColor = GetWebLinkColor(dark);
        Color visitedColor = GetVisitedColor(dark);

        // GetRect возвращает координаты относительно текстовой области (без Padding контрола)
        double offsetX = Editor.Padding.Left;
        double offsetY = Editor.Padding.Top;

        // Рисует чипы поверх диапазона документа — по одному на строку. Порядок вызовов =
        // порядок слоёв: заплатки добавляются раньше подчёркивания, чтобы то легло поверх.
        void AddChip(int from, int to, int linkIndex, Color color, ChipKind kind, string? tip)
        {
            foreach (var rect in LineRects(from, to))
            {
                // Вырожденный прямоугольник TOM отдаёт, например, на краю переносимой
                // строки. Чип с нулевой или отрицательной высотой построить нельзя:
                // ChipView бросает «Non-negative number required» и роняет приложение
                // прямо из обработчика LayoutUpdated (поймано в error.log).
                if (rect.Width <= 0 || rect.Height <= 0) continue;

                // фон чуть ниже строки текста; при увеличенном межстрочном интервале
                // лишнее пространство строки (снизу) не включаем
                double chipHeight = Math.Min(rect.Height - 2, App.Settings.FontSize * 1.25);
                if (chipHeight <= 0) chipHeight = rect.Height; // строка тоньше отступа
                Rect bounds;
                if (kind == ChipKind.Cover)
                {
                    // заплатка закрывает символ целиком, по всей высоте строки и без скруглений:
                    // от скруглённых углов у глифа остаются торчащие уголки
                    bounds = new Rect(rect.X + offsetX - 1, rect.Y + offsetY, rect.Width + 2, rect.Height);
                }
                else if (kind == ChipKind.Solid)
                {
                    // закрываемые символы узкие — кружок обязан перекрыть их целиком
                    double width = Math.Max(rect.Width + 4, chipHeight);
                    bounds = new Rect(rect.X + offsetX + rect.Width / 2 - width / 2,
                                      rect.Y + offsetY + 1, width, chipHeight);
                }
                else
                {
                    // ровно по тексту: поля по бокам выглядели бы обрамляющими пробелами
                    bounds = new Rect(rect.X + offsetX, rect.Y + offsetY + 1, rect.Width, chipHeight);
                }
                _chips.Add(new LinkChip(bounds, linkIndex, kind, color));

                var chip = new Controls.ChipView
                {
                    Width = bounds.Width,
                    Height = bounds.Height,
                    CornerRadius = new CornerRadius(kind == ChipKind.Solid ? bounds.Height / 2 : 0),
                };
                if (tip != null) ToolTipService.SetToolTip(chip, tip);
                ApplyChipStyle(chip, color, linkIndex == _hoverLink, kind);
                Canvas.SetLeft(chip, bounds.X);
                Canvas.SetTop(chip, bounds.Y);
                _chipShapes.Add(chip);
                LinkOverlay.Children.Add(chip);
            }
        }

        for (int i = 0; i < _links.Count; i++)
        {
            var link = _links[i];
            // пройденные ссылки гасим, как ":visited" в браузере: по виду ссылки читатель
            // должен помнить, был он там или нет
            var color = link.IsWeb
                ? webColor
                : _reading.IsRead(link.Target) ? visitedColor : accent;

            if (link.IsWeb)
            {
                // веб-адрес виден целиком, одна «пилюля»
                AddChip(link.Start, link.End, i, color,
                        ChipKind.Pill, tip: link.IsWeb ? link.Target : TargetTip(link.Target));
                continue;
            }

            // Компактный режим: скрыто всё, кроме "[", якоря и ")".
            if (link.Anchor.Length == 0)
            {
                // якоря нет — показывать нечего, вся ссылка уходит под кружок
                AddChip(link.Start, link.End, i, color, ChipKind.Solid, tip: TargetTip(link.Target));
                continue;
            }

            // Есть якорь — он и есть кликабельный текст, отдельный значок был бы лишним.
            // Скобки закрашиваем фоном редактора, сверху кладём обычную «пилюлю».
            var cover = EditorBackgroundColor();
            AddChip(link.Start, link.Start + 1, i, cover, ChipKind.Cover, tip: null);
            AddChip(link.End - 1, link.End, i, cover, ChipKind.Cover, tip: null);
            // подчёркиваем ровно якорь: закрашенные "[" и ")" под черту попадать не должны,
            // иначе она торчит по бокам и читается как лишние пробелы. Кликабельность от
            // этого не страдает — заплатки тоже попадают в _chips с тем же индексом ссылки.
            AddChip(link.Start + 1, AnchorEnd(link), i, color, ChipKind.Pill, tip: TargetTip(link.Target));
        }
    }

    /// <summary>Прямоугольник диапазона документа в координатах текстовой области.</summary>
    private Rect RangeRect(int from, int to)
    {
        Editor.Document.GetRange(from, to).GetRect(PointOptions.AllowOffClient, out Rect r, out _);
        return r;
    }

    /// <summary>Прямоугольники диапазона по строкам — по одному на строку.</summary>
    /// <remarks>
    /// GetRect на многострочном диапазоне отдаёт общую обрамляющую рамку: для якоря,
    /// перенесённого на другую строку, это огромный блок поверх чужого текста. Поэтому
    /// такой диапазон разбираем посимвольно и группируем по строке. Пока диапазон
    /// умещается в одну строку, посимвольный разбор не запускается — оверлей
    /// перерисовывается на каждую правку, и лишние обращения к TOM тут дороги.
    /// </remarks>
    private List<Rect> LineRects(int from, int to)
    {
        var whole = RangeRect(from, to);
        if (whole.Width <= 0 || whole.Height <= 0) return new List<Rect>();

        var first = RangeRect(from, from + 1);
        double lineHeight = first.Height > 0 ? first.Height : whole.Height;
        if (whole.Height <= lineHeight * 1.5) return new List<Rect> { whole }; // одна строка

        var lines = new List<Rect>();
        Rect current = default;
        bool has = false;
        for (int i = from; i < to; i++)
        {
            var r = RangeRect(i, i + 1);
            if (r.Width <= 0 && r.Height <= 0) continue;
            if (has && Math.Abs(r.Y - current.Y) < 1)
            {
                current = new Rect(current.X, current.Y,
                    Math.Max(current.Width, r.X + r.Width - current.X),
                    Math.Max(current.Height, r.Height));
            }
            else
            {
                if (has) lines.Add(current);
                current = r;
                has = true;
            }
        }
        if (has) lines.Add(current);
        return lines;
    }

    private static Color GetAccentColor(bool dark)
    {
        var key = dark ? "SystemAccentColorLight2" : "SystemAccentColorDark1";
        if (Application.Current.Resources.TryGetValue(key, out var value) && value is Color color)
            return color;
        return dark ? Color.FromArgb(255, 99, 176, 255) : Color.FromArgb(255, 0, 95, 184);
    }

    private static Color GetWebLinkColor(bool dark) =>
        dark ? Color.FromArgb(255, 77, 163, 255) : Color.FromArgb(255, 11, 98, 196);

    /// <summary>Цвет уже прочитанной ссылки — намеренно нейтральный, а не акцентный.</summary>
    private static Color GetVisitedColor(bool dark) =>
        dark ? Color.FromArgb(255, 134, 134, 142) : Color.FromArgb(255, 118, 118, 128);

    /// <summary>
    /// Кисти чипов. Цветов наперечёт (акцент, веб, пройденная ссылка, фон), а сами чипы
    /// пересоздаются на каждой перерисовке — заводить кисть заново каждый раз незачем.
    /// </summary>
    private static readonly Dictionary<uint, SolidColorBrush> ChipBrushes = new();

    private static SolidColorBrush ChipBrush(Color color)
    {
        uint key = ((uint)color.A << 24) | ((uint)color.R << 16) | ((uint)color.G << 8) | color.B;
        if (!ChipBrushes.TryGetValue(key, out var brush))
            ChipBrushes[key] = brush = new SolidColorBrush(color);
        return brush;
    }

    private static void ApplyChipStyle(Controls.ChipView chip, Color color, bool hovered, ChipKind kind)
    {
        // Заплатка и кружок закрывают собой символы, поэтому они ВСЕГДА полностью
        // непрозрачны: при малейшей прозрачности скобки просвечивают сквозь заливку.
        // Наведение показываем яркостью самого цвета, а не альфой.
        switch (kind)
        {
            case ChipKind.Cover:
                chip.Background = ChipBrush(color);
                chip.BorderBrush = ChipBrush(color);
                chip.BorderThickness = new Thickness(0);
                break;
            case ChipKind.Solid:
                var fill = hovered ? color : Dim(color, 0.72);
                chip.Background = ChipBrush(fill);
                chip.BorderBrush = ChipBrush(fill);
                chip.BorderThickness = new Thickness(1);
                break;
            default:
                // Текст ссылки подчёркиваем, а не заливаем: при переносе строки якорь
                // разрывается на несколько кусков, и заливка смотрелась бы рвано.
                // Фон прозрачный, но НЕ null — иначе чип перестанет ловить клики.
                chip.Background = ChipBrush(hovered
                    ? Color.FromArgb(0x22, color.R, color.G, color.B)
                    : Colors.Transparent);
                chip.BorderBrush = ChipBrush(
                    Color.FromArgb(hovered ? (byte)0xFF : (byte)0xDC, color.R, color.G, color.B));
                chip.BorderThickness = new Thickness(0, 0, 0, hovered ? 2 : 1);
                break;
        }
    }

    /// <summary>Приглушённый вариант цвета — непрозрачный, в отличие от снижения альфы.</summary>
    private static Color Dim(Color color, double factor) => Color.FromArgb(
        0xFF, (byte)(color.R * factor), (byte)(color.G * factor), (byte)(color.B * factor));

    private int HitLink(Point p)
    {
        foreach (var chip in _chips)
            if (chip.Bounds.Contains(p))
                return chip.LinkIndex;
        return -1;
    }

    private void UpdateHover(int newHover)
    {
        if (newHover == _hoverLink) return;
        int previous = _hoverLink;
        _hoverLink = newHover;
        // мышь ходит по тексту постоянно; перекрашиваем только те чипы, у которых
        // наведение действительно изменилось — покинутую ссылку и новую
        for (int c = 0; c < _chips.Count && c < _chipShapes.Count; c++)
        {
            int index = _chips[c].LinkIndex;
            if (index != previous && index != newHover) continue;
            ApplyChipStyle(_chipShapes[c], _chips[c].Color, index == newHover, _chips[c].Kind);
        }
    }

    private void OnOverlayPointerMoved(object sender, PointerRoutedEventArgs e) =>
        UpdateHover(HitLink(e.GetCurrentPoint(LinkOverlay).Position));

    private void OnOverlayPointerExited(object sender, PointerRoutedEventArgs e) => UpdateHover(-1);

    private void OnOverlayPointerPressed(object sender, PointerRoutedEventArgs e)
    {
        var point = e.GetCurrentPoint(LinkOverlay);
        if (!point.Properties.IsLeftButtonPressed)
        {
            _pressLink = -1;
            return;
        }
        _pressPoint = point.Position;
        _pressLink = HitLink(_pressPoint);
        if (_pressLink >= 0)
            LinkOverlay.CapturePointer(e.Pointer);
    }

    private void OnOverlayPointerReleased(object sender, PointerRoutedEventArgs e)
    {
        LinkOverlay.ReleasePointerCaptures();
        if (_pressLink < 0) return;
        var pos = e.GetCurrentPoint(LinkOverlay).Position;
        int pressed = _pressLink;
        _pressLink = -1;
        if (Math.Abs(pos.X - _pressPoint.X) > 4 || Math.Abs(pos.Y - _pressPoint.Y) > 4) return;
        if (HitLink(pos) != pressed) return;
        if (pressed >= _links.Count) return;

        var link = _links[pressed];
        if (link.IsWeb)
        {
            LaunchWebLink(link.Target);
        }
        else
        {
            DispatcherQueue.TryEnqueue(() => OpenFile(link.Target));
        }
    }

    /// <summary>Открывает URL в браузере по умолчанию.</summary>
    private static void LaunchWebLink(string url)
    {
        try
        {
            _ = Windows.System.Launcher.LaunchUriAsync(new Uri(url));
        }
        catch
        {
            // некорректный URL — молча игнорируем
        }
    }


    // ================= Кнопки панели инструментов =================

    /// <summary>
    /// Шаг по истории: снимаем файл с одной стопки, текущий кладём на другую.
    /// </summary>
    /// <remarks>
    /// Исчезнувшие файлы пропускаем — их могли удалить или переименовать другой
    /// программой, а история про это не знает. Проверяем через Exists: он смотрит
    /// сначала в память, а не на диск, и негодный id для него — просто «нет файла»
    /// (GetFilePath на таком бросает исключение).
    /// </remarks>
    private void GoHistory(Stack<string> from, Stack<string> to)
    {
        if (_catalog == null) return;
        while (from.Count > 0)
        {
            var id = from.Pop();
            if (!_catalog.Exists(id)) continue;
            if (_currentId != null) to.Push(_currentId);
            OpenFile(id, push: false);
            break;
        }
        UpdateNavButtons();
    }

    private void GoBack() => GoHistory(_back, _forward);

    private void GoForward() => GoHistory(_forward, _back);

    private void OnBackClick(object sender, RoutedEventArgs e) => GoBack();

    private void OnForwardClick(object sender, RoutedEventArgs e) => GoForward();

    private void OnHomeClick(object sender, RoutedEventArgs e) => OpenFile(NeurodocCatalog.RootId);

    private void OnHomeAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        if (_catalog != null)
            OpenFile(NeurodocCatalog.RootId);
        args.Handled = true;
    }

    private void OnInsertLinkClick(object sender, RoutedEventArgs e) => InsertNewLink();

    private void OnInsertLinkAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        InsertNewLink();
        args.Handled = true;
    }

    private void InsertNewLink()
    {
        if (_catalog == null || _currentId == null) return;

        var selection = Editor.Document.Selection;
        int from = Math.Min(selection.StartPosition, selection.EndPosition);
        int to = Math.Max(selection.StartPosition, selection.EndPosition);

        // Вложенных ссылок в формате нет: вставка внутрь существующей порвала бы обе.
        // Каретка ровно на краю ссылки — это уже снаружи, там вставлять можно.
        if (_links.Any(l => from < l.End && to > l.Start))
        {
            ShowCopyTip(InsertLinkButton, "Курсор внутри ссылки",
                        "Вложенные ссылки не поддерживаются");
            return;
        }

        // Текстом ссылки становится выделение, а без выделения — слово под кареткой.
        // Если и слова нет (каретка в пустом месте), якорь пустой — однокнопочный
        // сценарий не меняется.
        var text = GetEditorTextRaw();
        string anchor;
        if (to > from)
        {
            (from, to) = TrimRange(text, from, to);
            anchor = from < to ? text[from..to] : "";
            if (!IsAnchorUsable(InsertLinkButton, anchor)) return;
        }
        else
        {
            var (wordStart, wordEnd) = WordAt(text, from);
            // слово, задевающее соседнюю ссылку, оборачивать нельзя — порвём её
            if (wordEnd > wordStart && !_links.Any(l => wordStart < l.End && wordEnd > l.Start))
            {
                from = wordStart;
                to = wordEnd;
                anchor = text[wordStart..wordEnd];
            }
            else
            {
                anchor = "";
            }
        }

        var id = _catalog.CreateNewFile();
        if (anchor.Length > 0) _catalog.WriteFile(id, anchor + "\n");
        // TypeText заменяет содержимое диапазона, поэтому слово превращается в ссылку
        selection.SetRange(from, to);
        selection.TypeText("[" + anchor + "](" + id + ")");
        SaveCurrentFile();
        OpenFile(id);
    }

    /// <summary>
    /// Границы слова, на котором стоит каретка. Каретка у края слова тоже считается:
    /// пользователь целится в слово, а не в конкретный промежуток между символами.
    /// </summary>
    /// <returns>Пустой диапазон (start == end), если под кареткой не слово.</returns>
    private static (int Start, int End) WordAt(string text, int position)
    {
        int caret = Math.Clamp(position, 0, text.Length);
        int start = caret;
        while (start > 0 && IsWordChar(text[start - 1])) start--;
        int end = caret;
        while (end < text.Length && IsWordChar(text[end])) end++;
        return (start, end);
    }

    private static bool IsWordChar(char c) => char.IsLetterOrDigit(c) || c == '_';

    /// <summary>
    /// Приводит текст к пригодному для якоря виду: он однострочный и не содержит "]",
    /// иначе ссылка перестанет разбираться.
    /// </summary>
    private static string SanitizeAnchor(string? text) =>
        string.IsNullOrWhiteSpace(text)
            ? ""
            : text.Replace('\r', ' ').Replace('\n', ' ').Replace("]", "").Trim();

    /// <summary>
    /// Сдвигает границы выделения внутрь, отбрасывая пробелы по краям. Именно границы,
    /// а не текст: обрезав текст, но заменив весь диапазон, мы бы съели эти пробелы.
    /// </summary>
    private static (int Start, int End) TrimRange(string text, int from, int to)
    {
        while (from < to && char.IsWhiteSpace(text[from])) from++;
        while (to > from && char.IsWhiteSpace(text[to - 1])) to--;
        return (from, to);
    }

    /// <summary>
    /// Годится ли выделенный текст в текст ссылки; если нет — объясняет почему.
    /// </summary>
    /// <remarks>
    /// Непригодное выделение намеренно НЕ «чинится» молча: вычистить перевод строки
    /// значило бы склеить абзацы файла, а "]" — удалить символ из документа. Обе правки
    /// пользователь не заказывал и не увидел бы, пока не открыл файл чем-то ещё.
    /// Проверка идёт по настоящим разделителям строк; перенос по ширине окна — только
    /// раскладка, в тексте его нет.
    /// </remarks>
    private bool IsAnchorUsable(FrameworkElement tipTarget, string anchor)
    {
        if (anchor.Contains('\r') || anchor.Contains('\n'))
        {
            ShowCopyTip(tipTarget, "Выделено несколько строк",
                        "Текст ссылки должен быть в одной строке");
            return false;
        }
        if (anchor.Contains(']'))
        {
            ShowCopyTip(tipTarget, "В выделении есть «]»",
                        "Этот символ недопустим в тексте ссылки");
            return false;
        }
        if (anchor.Length == 0)
        {
            ShowCopyTip(tipTarget, "Выделены только пробелы",
                        "Выделите текст, который станет текстом ссылки");
            return false;
        }
        return true;
    }

    /// <summary>
    /// Снимает ссылку под кареткой, оставляя её текст: [текст](цель) превращается в текст.
    /// </summary>
    /// <remarks>
    /// Иначе ссылку не убрать: в компактном режиме её служебная часть скрыта, руками до неё
    /// не добраться. Голый http-адрес не трогаем — снимать там нечего, это просто текст.
    /// </remarks>
    private void RemoveLinkAtCaret()
    {
        if (_catalog == null) return;
        var text = GetEditorTextRaw();
        var selection = Editor.Document.Selection;
        int caret = Math.Min(selection.StartPosition, selection.EndPosition);

        foreach (var link in _links)
        {
            if (caret < link.Start || caret > link.End) continue;
            // ссылка записана как [текст](цель) — у голого адреса снимать нечего
            if (link.Start >= text.Length || text[link.Start] != '[') continue;

            var range = Editor.Document.GetRange(link.Start, link.End);
            range.SetText(TextSetOptions.None, link.Anchor);
            // TextChanged пересоберёт формат: набор ссылок изменился, скрытие снимется со
            // всего документа и ляжет заново, так что унаследовать скрытость текст не может
            var caretAfter = link.Start + link.Anchor.Length;
            Editor.Document.Selection.SetRange(caretAfter, caretAfter);
            return;
        }

        ShowCopyTip(InsertLinkButton, "Курсор не на ссылке",
                    "Поставьте каретку внутрь ссылки и повторите");
    }

    /// <summary>Переход по ссылке, в границах которой (включая края) находится каретка.</summary>
    private void NavigateLinkAtCaret()
    {
        if (_catalog == null) return;
        var selection = Editor.Document.Selection;
        int caret = Math.Min(selection.StartPosition, selection.EndPosition);
        foreach (var link in _links)
        {
            if (caret >= link.Start && caret <= link.End)
            {
                if (link.IsWeb)
                    LaunchWebLink(link.Target);
                else
                    OpenFile(link.Target);
                return;
            }
        }
    }

    private void OnNewFileClick(object sender, RoutedEventArgs e) => CreateUnlinkedFile();

    private void OnNewFileAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        CreateUnlinkedFile();
        args.Handled = true;
    }

    private void CreateUnlinkedFile()
    {
        if (_catalog == null) return;
        var id = _catalog.CreateNewFile();
        OpenFile(id);
    }

    private void OnOpenByIdClick(object sender, RoutedEventArgs e) => _ = ShowOpenByIdDialogAsync();

    private void OnOpenByIdAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        _ = ShowOpenByIdDialogAsync();
        args.Handled = true;
    }

    private bool _openByIdDialogOpen;

    /// <summary>Диалог перехода к файлу по UUID или пути.</summary>
    private async Task ShowOpenByIdDialogAsync()
    {
        if (_openByIdDialogOpen || _catalog == null) return;
        _openByIdDialogOpen = true;
        try
        {
            var dialog = new OpenByIdDialog(_catalog)
            {
                XamlRoot = RootGrid.XamlRoot,
                RequestedTheme = RootGrid.ActualTheme,
            };
            await dialog.ShowAsync();
            if (dialog.SelectedId != null)
                OpenFile(dialog.SelectedId);
        }
        catch
        {
            // второй открытый диалог бросает исключение — просто игнорируем
        }
        finally
        {
            _openByIdDialogOpen = false;
        }
    }

    private void OnRawToggle(object sender, RoutedEventArgs e)
    {
        if (_suppressRawToggle) return;
        App.Settings.RawEditing = RawToggle.IsChecked == true;
        App.SettingsService.Save(App.Settings);
        // «Как есть» — такой же режим показа, как SVG или MD: включая его, выходим
        // из отрисованного формата, иначе два режима были бы включены разом
        if (App.Settings.RawEditing && _viewMode != ViewMode.Text)
        {
            _ = SetViewModeAsync(ViewMode.Text);
            return; // ApplyRawEditing вызовет он сам, когда вернёт текстовый вид
        }
        ApplyRawEditing();
    }

    /// <summary>Применяет режим «как есть»: снимает скрытие и перерисовывает оверлей.</summary>
    /// <remarks>
    /// Скрытие снимаем ЯВНО. Само по себе <see cref="ApplyCompactLinks"/> этого не сделает:
    /// при выключенном компактном режиме подпись набора ссылок — null, она совпадает с уже
    /// сохранённой, и ветка «снять скрытие со всего документа» не выполняется.
    /// </remarks>
    private void ApplyRawEditing()
    {
        _compactSignature = null; // формат надо пересобрать целиком, а не по изменению набора
        ClearHiddenFormatting();
        ApplyCompactLinks(force: true);
        _overlayDirty = true;
        RedrawOverlay();
    }

    /// <summary>Снимает скрытие со всего документа: текст показывается ровно как на диске.</summary>
    private void ClearHiddenFormatting()
    {
        var doc = Editor.Document;
        bool prev = _suppressTextChanged;
        _suppressTextChanged = true;
        try
        {
            doc.GetText(TextGetOptions.None, out var text);
            doc.GetRange(0, text.Length + 1).CharacterFormat.Hidden = FormatEffect.Off;
        }
        finally
        {
            _suppressTextChanged = prev;
        }
    }

    private bool _suppressRawToggle;

    /// <summary>Лежит ли в буфере обмена ссылка на файл каталога (для кнопки «Связать»).</summary>
    private bool _clipboardHasLink;
    /// <summary>Последнее непустое выделение: клик по кнопке уводит фокус из редактора.</summary>
    private (int Start, int End) _lastSelection;

    private void SyncRawToggle()
    {
        _suppressRawToggle = true;
        RawToggle.IsChecked = App.Settings.RawEditing;
        _suppressRawToggle = false;
    }

    private void OnBrokenLinksClick(object sender, RoutedEventArgs e) => ToggleBrokenLinksCard();

    private void OnBrokenLinksAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        ToggleBrokenLinksCard();
        args.Handled = true;
    }

    private bool _brokenLinksHiddenByUser;

    private void OnBrokenLinksCloseClick(object sender, RoutedEventArgs e)
    {
        _brokenLinksHiddenByUser = true;
        BrokenLinksCard.Visibility = Visibility.Collapsed;
    }

    /// <summary>Показывает/скрывает карточку со списком битых ссылок проекта.</summary>
    private void ToggleBrokenLinksCard()
    {
        if (_catalog == null) return;
        if (BrokenLinksCard.Visibility == Visibility.Visible)
        {
            _brokenLinksHiddenByUser = true;
            BrokenLinksCard.Visibility = Visibility.Collapsed;
            return;
        }
        _brokenLinksHiddenByUser = false;
        FillBrokenLinksList();
        BrokenLinksCard.Visibility = Visibility.Visible;
    }

    private void FillBrokenLinksList()
    {
        if (_catalog == null) return;
        var items = _catalog.GetBrokenLinks()
            .Select(b => new BrokenLinkVM
            {
                FileId = b.FileId,
                FileTitle = b.FileTitle,
                TargetId = b.TargetId,
                Position = b.Position,
                Context = b.Context,
            })
            .ToList();
        BrokenLinksList.ItemsSource = items;
        BrokenLinksHeader.Text = items.Count == 0
            ? "Битые ссылки: нет"
            : $"Битые ссылки: {items.Count}";
    }

    /// <summary>Переход к месту битой ссылки: открыть файл и выделить её.</summary>
    private void OnBrokenLinkClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is not BrokenLinkVM vm) return;
        OpenFile(vm.FileId);
        int start = vm.Position;
        int length = vm.TargetId.Length + 2; // вместе с угловыми скобками
        DispatcherQueue.TryEnqueue(() =>
        {
            var text = GetEditorTextRaw();
            if (start < 0 || start + length > text.Length) return;
            Editor.Document.Selection.SetRange(start, start + length);
            EnsureCaretVisible();
            Editor.Focus(FocusState.Programmatic);
        });
    }

    private void UpdateBrokenLinksIndicator()
    {
        if (_catalog == null)
        {
            BrokenLinksCountText.Text = "";
            return;
        }
        int count = _catalog.GetBrokenLinks().Count;
        BrokenLinksCountText.Text = count == 0 ? "" : count.ToString();
        // карточка видна, только когда есть что показывать
        if (count == 0)
        {
            BrokenLinksCard.Visibility = Visibility.Collapsed;
            return;
        }
        FillBrokenLinksList();
        if (!_brokenLinksHiddenByUser) BrokenLinksCard.Visibility = Visibility.Visible;
    }

    private void OnCopyLinkClick(object sender, RoutedEventArgs e)
    {
        if (_currentId == null || _currentId == NeurodocCatalog.RootId) return;
        // якорем берём заголовок файла; у пустого файла заголовка нет — оставляем пустой
        var content = _catalog?.ReadFile(_currentId) ?? "";
        var anchor = string.IsNullOrWhiteSpace(content)
            ? ""
            : SanitizeAnchor(_catalog!.GetTitle(_currentId));
        // Путь считается от файла, куда ссылку вставят, а этого приложение знать не может.
        // Берём вид для content: там пишут ссылки в подавляющем большинстве случаев.
        var target = NeurodocCatalog.MakeRelativeTarget(NeurodocCatalog.ContentDir, _currentId);
        var link = "[" + anchor + "](" + target + ")";
        var package = new DataPackage();
        package.SetText(link);
        Clipboard.SetContent(package);
        ShowCopyTip(CopyLinkButton, "Ссылка скопирована", link);
    }

    private void OnCopyContentClick(object sender, RoutedEventArgs e)
    {
        if (_catalog == null || _currentId == null) return;
        var text = GetEditorTextForSave();
        var package = new DataPackage();
        package.SetText(text);
        Clipboard.SetContent(package);
        ShowCopyTip(CopyContentButton, "Содержимое скопировано",
            $"{text.Length:N0} символов".Replace(' ', ' '));
    }

    private void ShowCopyTip(FrameworkElement target, string title, string subtitle)
    {
        CopyTip.Target = target;
        CopyTip.Title = title;
        CopyTip.Subtitle = subtitle;
        CopyTip.IsOpen = true;
        _tipTimer.Stop();
        _tipTimer.Start();
    }

    private async void OnDeleteClick(object sender, RoutedEventArgs e)
    {
        if (_catalog == null || _currentId == null || _currentId == NeurodocCatalog.RootId) return;

        var title = _catalog.GetTitle(_currentId, 40);
        var confirm = new ContentDialog
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
            Title = "Удалить файл?",
            Content = $"Файл «{title}» будет удалён безвозвратно.\n\n" +
                      "«Только файл» — ссылки на него в других файлах останутся, но перестанут работать.\n" +
                      "«Файл и ссылки» — ссылки на него будут сняты, а их текст останется обычным текстом.",
            PrimaryButtonText = "Только файл",
            SecondaryButtonText = "Файл и ссылки",
            CloseButtonText = "Отмена",
            DefaultButton = ContentDialogButton.Close,
        };
        var result = await confirm.ShowAsync();
        if (result != ContentDialogResult.Primary && result != ContentDialogResult.Secondary) return;

        var id = _currentId;
        _saveTimer.Stop();
        _currentId = null;
        _lastSavedText = "";
        try
        {
            _catalog.DeleteFile(id);
            if (result == ContentDialogResult.Secondary)
                _catalog.RemoveLinksTo(id);
        }
        catch (Exception ex)
        {
            _ = ShowErrorAsync("Не удалось удалить файл", ex.Message);
        }
        _viewStates.Remove(id);
        RemoveFromHistory(id);
        OpenFile(NeurodocCatalog.RootId, push: false);
    }

    private void RemoveFromHistory(string id)
    {
        var keptBack = _back.Where(x => x != id).Reverse().ToList();
        _back.Clear();
        foreach (var item in keptBack)
            _back.Push(item);

        var keptForward = _forward.Where(x => x != id).Reverse().ToList();
        _forward.Clear();
        foreach (var item in keptForward)
            _forward.Push(item);

        UpdateNavButtons();
    }

    // ================= Панели: крошки, обратные ссылки =================

    private void UpdateFileUi()
    {
        bool isRoot = _currentId == NeurodocCatalog.RootId;
        FileIdText.Text = _currentId ?? "";
        CopyLinkButton.IsEnabled = _catalog != null && _currentId != null && !isRoot;
        CopyContentButton.IsEnabled = _catalog != null && _currentId != null;
        DeleteButton.IsEnabled = _catalog != null && _currentId != null && !isRoot;
    }

    private void UpdatePanels()
    {
        UpdateReadingUi();
        if (_catalog == null || _currentId == null) return;

        var paths = _catalog.FindShortestPathsFromRoot(_currentId);
        var pathVms = new List<BreadcrumbPath>();
        foreach (var path in paths)
        {
            var segments = new List<BreadcrumbSegment>();
            for (int i = 0; i < path.Count; i++)
            {
                var id = path[i];
                segments.Add(new BreadcrumbSegment
                {
                    Id = id,
                    Title = id == NeurodocCatalog.RootId ? "root" : _catalog.GetTitle(id, 36),
                    IsClickable = i < path.Count - 1,
                    ChevronVisibility = i == 0 ? Visibility.Collapsed : Visibility.Visible,
                });
            }
            pathVms.Add(new BreadcrumbPath { Segments = segments });
        }
        BreadcrumbsList.ItemsSource = pathVms;
        BreadcrumbsScroll.Visibility = pathVms.Count > 0 ? Visibility.Visible : Visibility.Collapsed;

        var backlinks = _catalog.GetBacklinks(_currentId)
            .Select(b => new BacklinkVM { Id = b.FileId, Title = b.Title })
            .ToList();
        BacklinksList.ItemsSource = backlinks;
        NoBacklinksText.Visibility = backlinks.Count == 0 ? Visibility.Visible : Visibility.Collapsed;

        var orphans = _catalog.GetOrphanFiles()
            .Select(o => new BacklinkVM { Id = o.FileId, Title = o.Title })
            .ToList();
        OrphansList.ItemsSource = orphans;
        NoOrphansText.Visibility = orphans.Count == 0 ? Visibility.Visible : Visibility.Collapsed;

        UpdateBrokenLinksIndicator();
    }

    // ================= Прогресс чтения =================

    /// <summary>Ставит или снимает отметку о прочтении текущего файла.</summary>
    private void MarkCurrentRead(bool read)
    {
        if (_projectId == null || _currentId == null) return;
        // файлы в собственных форматах в обход чтения не входят — и отмечать их нечего
        if (!NeurodocCatalog.IsNeurodocFile(_currentId)) return;
        if (read == _reading.IsRead(_currentId)) return;

        if (read) _reading.Visited[_currentId] = DateTime.Now;
        else _reading.Visited.Remove(_currentId);

        _readingService.Save(_projectId, _reading);
        UpdateReadingUi();
        // цвет ссылок на этот файл в других файлах изменится при следующем открытии,
        // а вот ссылки в текущем файле надо перекрасить сразу
        RedrawOverlay();
    }

    private void UpdateReadingUi()
    {
        if (_catalog == null)
        {
            ReadingCountText.Text = "Проект не открыт";
            ReadingBar.Value = 0;
            ReadNextButton.IsEnabled = false;
            ReadMarkToggle.IsEnabled = false;
            return;
        }

        var order = _catalog.BuildReadingOrder();
        // файлы могли быть удалены — в счётчике учитываем только существующие
        int read = order.Count(_reading.IsRead);
        ReadingCountText.Text = order.Count == 0
            ? "Файлов нет"
            : $"Прочитано {read} из {order.Count}";
        ReadingBar.Maximum = Math.Max(1, order.Count);
        ReadingBar.Value = read;

        bool markable = _currentId != null && NeurodocCatalog.IsNeurodocFile(_currentId);
        ReadMarkToggle.IsEnabled = markable;
        ReadMarkToggle.IsChecked = markable && _reading.IsRead(_currentId!);
        // Кнопка активна, когда клик действительно куда-то приведёт И текущий файл
        // дочитан: «Далее» — это шаг вперёд по порядку чтения, а уходить из
        // неотмеченного файла значит оставлять дырку, к которой придётся возвращаться.
        // Файлы в собственных форматах в обход чтения не входят и ничего не задерживают.
        bool currentDone = !markable || _reading.IsRead(_currentId!);
        ReadNextButton.IsEnabled = currentDone && NextUnreadFile(order) != null;
    }

    /// <summary>
    /// Следующий непрочитанный файл в порядке обхода, или null — идти некуда.
    /// </summary>
    /// <remarks>
    /// Текущий файл не в счёт, даже если он ещё не отмечен прочитанным: переход к самому
    /// себе <see cref="OpenFile"/> отсекает, и кнопка выглядела бы рабочей впустую. Такое
    /// бывает постоянно — отметка ставится не сразу, а через паузу после открытия.
    /// </remarks>
    private string? NextUnreadFile(IReadOnlyList<string>? order = null)
    {
        if (_catalog == null) return null;
        foreach (var id in order ?? _catalog.BuildReadingOrder())
        {
            if (_reading.IsRead(id)) continue;
            if (string.Equals(id, _currentId, StringComparison.OrdinalIgnoreCase)) continue;
            return id;
        }
        return null;
    }

    /// <summary>Открывает следующий непрочитанный файл в порядке обхода.</summary>
    private void OnReadNextClick(object sender, RoutedEventArgs e)
    {
        // страховка: при выключенной кнопке клик и так не придёт
        if (NextUnreadFile() is { } next) OpenFile(next);
    }

    private void OnReadMarkToggled(object sender, RoutedEventArgs e)
    {
        // таймер паузы больше не нужен: пользователь решил сам
        _readTimer.Stop();
        MarkCurrentRead(ReadMarkToggle.IsChecked == true);
    }

    private async void OnReadResetClick(object sender, RoutedEventArgs e)
    {
        if (_projectId == null) return;
        var dialog = new ContentDialog
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
            Title = "Сбросить прогресс чтения?",
            Content = "Отметки о прочитанных файлах в этом проекте будут забыты. "
                    + "Сама документация не изменится.",
            PrimaryButtonText = "Сбросить",
            CloseButtonText = "Отмена",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;

        _reading = new ReadingProgress();
        _readingService.Reset(_projectId);
        UpdateReadingUi();
        RedrawOverlay();
    }

    private void OnBreadcrumbsWheel(object sender, PointerRoutedEventArgs e) =>
        ScrollSidewaysOnWheel(BreadcrumbsScroll, e);

    private void OnToolbarWheel(object sender, PointerRoutedEventArgs e) =>
        ScrollSidewaysOnWheel(ToolbarScroll, e);

    private void OnFormatsWheel(object sender, PointerRoutedEventArgs e) =>
        ScrollSidewaysOnWheel(FormatsScroll, e);

    /// <summary>Колесо над горизонтальной полосой прокручивает её вбок.</summary>
    /// <remarks>
    /// Вертикальное колесо `ScrollViewer` в горизонтальную прокрутку сам не превращает —
    /// панель просто стоит на месте. Подписка обязана идти через
    /// <c>AddHandler(..., handledEventsToo: true)</c>: обычную XAML-подписку ScrollViewer
    /// не вызывает вовсе, он помечает событие обработанным раньше.
    /// </remarks>
    private static void ScrollSidewaysOnWheel(ScrollViewer scroll, PointerRoutedEventArgs e)
    {
        if (scroll.ScrollableWidth <= 0 || scroll.ScrollableHeight > 0) return;
        int delta = e.GetCurrentPoint(scroll).Properties.MouseWheelDelta;
        scroll.ChangeView(scroll.HorizontalOffset - delta, null, null);
        e.Handled = true;
    }

    private void OnBreadcrumbClick(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is BreadcrumbSegment segment && segment.IsClickable)
            OpenFile(segment.Id);
    }

    private void OnBacklinkClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is BacklinkVM vm)
            OpenFile(vm.Id);
    }

    // ================= Поиск =================

    private void OnSearchSubmitted(AutoSuggestBox sender, AutoSuggestBoxQuerySubmittedEventArgs args)
    {
        if (_catalog == null) return;
        var query = args.QueryText?.Trim() ?? "";
        if (query.Length == 0)
        {
            SearchCard.Visibility = Visibility.Collapsed;
            return;
        }
        _lastQuery = query;
        var results = _catalog.Search(query);
        SearchList.ItemsSource = results.Select(r => new SearchResultVM
        {
            FileId = r.FileId,
            Title = r.Title,
            Prefix = r.Prefix,
            Match = r.Match,
            Suffix = r.Suffix,
            Occurrence = r.Occurrence,
        }).ToList();
        SearchHeaderText.Text = results.Count == 0
            ? "Ничего не найдено"
            : $"Результаты поиска · {results.Count}";
        SearchCard.Visibility = Visibility.Visible;
    }

    private void OnSearchCloseClick(object sender, RoutedEventArgs e) =>
        SearchCard.Visibility = Visibility.Collapsed;

    private void OnSearchResultClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is not SearchResultVM vm) return;
        OpenFile(vm.FileId);
        SelectOccurrence(_lastQuery, vm.Occurrence);
    }

    private void SelectOccurrence(string query, int occurrence)
    {
        if (string.IsNullOrEmpty(query)) return;
        var text = GetEditorTextRaw();
        int index = -1, from = 0;
        for (int i = 0; i <= occurrence; i++)
        {
            index = text.IndexOf(query, from, StringComparison.OrdinalIgnoreCase);
            if (index < 0) return;
            from = index + Math.Max(1, query.Length);
        }
        int start = index;
        DispatcherQueue.TryEnqueue(() =>
        {
            var selection = Editor.Document.Selection;
            selection.SetRange(start, start + query.Length);
            EnsureCaretVisible();
            Editor.Focus(FocusState.Programmatic);
        });
    }

    // ================= Прочее =================

    private async void OnHotkeysClick(object sender, RoutedEventArgs e) => await ShowHotkeysDialogAsync();

    private void OnHotkeysAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        _ = ShowHotkeysDialogAsync();
        args.Handled = true;
    }

    private async Task ShowHotkeysDialogAsync()
    {
        var dialog = new HotkeysDialog
        {
            XamlRoot = RootGrid.XamlRoot,
            RequestedTheme = RootGrid.ActualTheme,
        };
        try { await dialog.ShowAsync(); } catch { }
    }

    private Task ShowErrorAsync(string title, string message) =>
        ShowMessageAsync(title, message, "Закрыть");

    private async Task ShowMessageAsync(string title, string message, string closeText)
    {
        try
        {
            var dialog = new ContentDialog
            {
                XamlRoot = RootGrid.XamlRoot,
                RequestedTheme = RootGrid.ActualTheme,
                Title = title,
                Content = message,
                CloseButtonText = closeText,
            };
            await dialog.ShowAsync();
        }
        catch
        {
            // второй открытый диалог бросает исключение — просто игнорируем
        }
    }

}
