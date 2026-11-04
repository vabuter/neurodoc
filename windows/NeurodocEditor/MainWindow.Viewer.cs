using System.Text.Encodings.Web;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.Web.WebView2.Core;
using NeurodocEditor.Services;

namespace NeurodocEditor;

/// <summary>
/// Просмотр содержимого в формате: рендеринг SVG и подсветка синтаксиса
/// JSON / YAML / Bash (WebView2 + highlight.js из Assets\Highlight).
/// </summary>
public sealed partial class MainWindow
{
    private enum ViewMode { Text, Svg, Json, Yaml, Bash, Md, Adoc, Html, Nginx }

    private ViewMode _viewMode = ViewMode.Text;
    private bool _suppressToggle;
    private Task? _viewerInit;
    private string? _hljsScript;
    private string? _hljsCssLight;
    private string? _hljsCssDark;
    private string? _markedScript;
    private string? _asciidoctorScript;
    private string? _nginxScript;

    private (ToggleButton Button, ViewMode Mode)[]? _formatToggles;

    /// <summary>
    /// Кнопка формата и её режим — один список на оба направления. Раньше это
    /// соответствие было записано дважды, в разном виде, и легко расходилось:
    /// добавить формат означало не забыть про обе записи.
    /// </summary>
    /// <remarks>
    /// Собирается при первом обращении: поля XAML появляются только после
    /// InitializeComponent, а инициализатор поля выполнился бы раньше.
    /// </remarks>
    private (ToggleButton Button, ViewMode Mode)[] FormatToggles => _formatToggles ??= new[]
    {
        (MdToggle, ViewMode.Md),
        (SvgToggle, ViewMode.Svg),
        (JsonToggle, ViewMode.Json),
        (YamlToggle, ViewMode.Yaml),
        (BashToggle, ViewMode.Bash),
        (AdocToggle, ViewMode.Adoc),
        (HtmlToggle, ViewMode.Html),
        (NginxToggle, ViewMode.Nginx),
    };

    private void OnFormatToggle(object sender, RoutedEventArgs e)
    {
        if (_suppressToggle) return;
        var toggle = (ToggleButton)sender;
        // снятая кнопка означает возврат к тексту
        var mode = ViewMode.Text;
        if (toggle.IsChecked == true)
            foreach (var (button, candidate) in FormatToggles)
                if (ReferenceEquals(button, toggle))
                {
                    mode = candidate;
                    break;
                }
        _ = SetViewModeAsync(mode);
    }

    private void SyncFormatToggles()
    {
        _suppressToggle = true;
        foreach (var (button, mode) in FormatToggles)
            button.IsChecked = _viewMode == mode;
        _suppressToggle = false;
    }

    /// <summary>
    /// Формат файла по расширению — чтобы файл в собственном формате открывался сразу
    /// в подходящем виде. У root и файлов content расширения нет, для них это Text.
    /// </summary>
    private static ViewMode DetectViewMode(string id) =>
        Path.GetExtension(id).ToLowerInvariant() switch
        {
            ".svg" => ViewMode.Svg,
            ".json" => ViewMode.Json,
            ".yaml" or ".yml" => ViewMode.Yaml,
            ".sh" or ".bash" => ViewMode.Bash,
            ".md" or ".markdown" => ViewMode.Md,
            ".adoc" or ".asciidoc" => ViewMode.Adoc,
            ".html" or ".htm" => ViewMode.Html,
            ".conf" or ".nginx" => ViewMode.Nginx,
            _ => ViewMode.Text,
        };

    /// <summary>Возврат в текстовый режим (при открытии другого файла и т.п.).</summary>
    private void ResetViewMode()
    {
        // дельта показывается при текстовом режиме, ранний выход её бы не снял
        if (_diffActive)
        {
            _diffActive = false;
            _diffKey = null;
            FormatViewer.Visibility = Visibility.Collapsed;
            EditorScroll.Visibility = Visibility.Visible;
        }
        if (_viewMode == ViewMode.Text) return;
        _viewMode = ViewMode.Text;
        SyncFormatToggles();
        FormatViewer.Visibility = Visibility.Collapsed;
        EditorScroll.Visibility = Visibility.Visible;
    }

    private async Task SetViewModeAsync(ViewMode mode)
    {
        // выбор формата (и возврат к тексту) снимает показанную дельту
        _diffActive = false;
        _diffKey = null;
        // «Как есть» — такой же режим показа, как остальные: включая формат, его снимаем
        if (mode != ViewMode.Text && App.Settings.RawEditing)
        {
            App.Settings.RawEditing = false;
            App.SettingsService.Save(App.Settings);
            SyncRawToggle();
            ApplyRawEditing();
        }

        _viewMode = mode;
        SyncFormatToggles();

        if (mode == ViewMode.Text)
        {
            FormatViewer.Visibility = Visibility.Collapsed;
            EditorScroll.Visibility = Visibility.Visible;
            Editor.Focus(FocusState.Programmatic);
            return;
        }

        SaveCurrentFile();
        try
        {
            await EnsureViewerAsync();
            FormatViewer.NavigateToString(BuildViewerHtml(mode, GetEditorTextForSave()));
            EditorScroll.Visibility = Visibility.Collapsed;
            FormatViewer.Visibility = Visibility.Visible;
        }
        catch (Exception ex)
        {
            _viewMode = ViewMode.Text;
            SyncFormatToggles();
            _ = ShowErrorAsync("Не удалось открыть просмотр", ex.Message);
        }
    }

    /// <summary>Перерисовать просмотр (смена темы, шрифта), если он активен.</summary>
    private void RefreshViewerIfActive()
    {
        if (_viewMode != ViewMode.Text)
            _ = SetViewModeAsync(_viewMode);
    }

    /// <summary>Готовит WebView2 — один раз на всё приложение.</summary>
    /// <remarks>
    /// Кнопки форматов нажимают подряд, а инициализация асинхронная: без общей задачи
    /// второй заход принимался бы создавать WebView2 заново, пока первый не закончил,
    /// и подписывался бы на сообщения повторно — клик по ссылке срабатывал бы дважды.
    /// </remarks>
    private async Task EnsureViewerAsync()
    {
        _viewerInit ??= InitViewerAsync();
        try
        {
            await _viewerInit;
        }
        catch
        {
            // неудачную попытку не запоминаем: следующий заход должен попробовать снова
            _viewerInit = null;
            throw;
        }
    }

    private async Task InitViewerAsync()
    {
        var userDataFolder = System.IO.Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "NeurodocEditor", "WebView2");
        var environment = await CoreWebView2Environment.CreateWithOptionsAsync(
            null, userDataFolder, new CoreWebView2EnvironmentOptions());
        await FormatViewer.EnsureCoreWebView2Async(environment);
        var settings = FormatViewer.CoreWebView2.Settings;
        settings.AreDefaultContextMenusEnabled = false;
        settings.AreDevToolsEnabled = false;
        settings.IsStatusBarEnabled = false;
        settings.AreDefaultScriptDialogsEnabled = false;
        FormatViewer.CoreWebView2.WebMessageReceived += OnViewerLinkClicked;
    }

    /// <summary>
    /// Клик по ссылке в отрендеренном документе: переходим так же, как в редакторе.
    /// </summary>
    /// <remarks>
    /// Ссылку не даём открыть самому WebView2 — он ушёл бы с отрисованной страницы.
    /// Цель разбирается относительно ТЕКУЩЕГО файла, как и везде в формате.
    /// </remarks>
    /// <summary>Префикс сообщения о смене фона — остальные сообщения это цели ссылок.</summary>
    private const string SvgBackgroundMessage = "svgbg:";

    private void OnViewerLinkClicked(CoreWebView2 sender, CoreWebView2WebMessageReceivedEventArgs args)
    {
        string target;
        try { target = args.TryGetWebMessageAsString() ?? ""; }
        catch { return; }
        if (target.Length == 0) return;

        if (target.StartsWith(SvgBackgroundMessage, StringComparison.Ordinal))
        {
            bool dark = target[SvgBackgroundMessage.Length..] == "dark";
            DispatcherQueue.TryEnqueue(() => SaveSvgBackground(dark));
            return;
        }

        DispatcherQueue.TryEnqueue(() =>
        {
            if (NeurodocCatalog.IsWebTarget(target))
            {
                LaunchWebLink(target);
                return;
            }
            if (_catalog == null || _currentId == null) return;
            // несуществующая цель — не повод никуда уходить: в том же ReadMe.md
            // ссылки из описания формата ведут на файлы-примеры, которых нет
            if (NeurodocCatalog.TryResolveLink(_currentId, target, out var id) && _catalog.Exists(id))
                OpenFile(id);
        });
    }

    /// <summary>
    /// Скрипт для отрендеренных документов: делает neurodoc-ссылки рабочими.
    /// </summary>
    /// <remarks>
    /// Markdown превращает [текст](цель) в ссылку сам, а AsciiDoc такого синтаксиса не знает
    /// и оставляет его текстом — поэтому такие места мы размечаем сами. Внутрь кода и уже
    /// готовых ссылок не лезем.
    /// </remarks>
    private const string ViewerLinkScript =
        "<script>(function(){" +
        "var RE=/\\[([^\\]\\n]*)\\]\\(([^()\\s\\\\]+)\\)/g;" +
        "var root=document.getElementById('doc');" +
        "if(root){" +
        "var walker=document.createTreeWalker(root,NodeFilter.SHOW_TEXT,null);" +
        "var nodes=[],n;" +
        "while(n=walker.nextNode()){" +
        "if(n.parentNode&&n.parentNode.closest('a,code,pre'))continue;" +
        "RE.lastIndex=0;if(RE.test(n.nodeValue))nodes.push(n);}" +
        "nodes.forEach(function(node){" +
        "var frag=document.createDocumentFragment(),last=0,m;RE.lastIndex=0;" +
        "while(m=RE.exec(node.nodeValue)){" +
        "frag.appendChild(document.createTextNode(node.nodeValue.slice(last,m.index)));" +
        "var a=document.createElement('a');a.setAttribute('href',m[2]);" +
        "a.textContent=m[1]||m[2];frag.appendChild(a);last=m.index+m[0].length;}" +
        "frag.appendChild(document.createTextNode(node.nodeValue.slice(last)));" +
        "node.parentNode.replaceChild(frag,node);});}" +
        "document.addEventListener('click',function(e){" +
        "var a=e.target&&e.target.closest?e.target.closest('a[href]'):null;" +
        "if(!a)return;e.preventDefault();" +
        "window.chrome.webview.postMessage(a.getAttribute('href'));});" +
        "})();</script>";

    private void LoadHighlightAssets()
    {
        if (_hljsScript != null) return;
        var dir = System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "Highlight");
        _hljsScript = File.ReadAllText(System.IO.Path.Combine(dir, "highlight.min.js"));
        _hljsCssLight = File.ReadAllText(System.IO.Path.Combine(dir, "github.min.css"));
        _hljsCssDark = File.ReadAllText(System.IO.Path.Combine(dir, "github-dark.min.css"));
        _markedScript = File.ReadAllText(System.IO.Path.Combine(dir, "marked.min.js"));
        _asciidoctorScript = File.ReadAllText(System.IO.Path.Combine(dir, "asciidoctor.min.js"));
        _nginxScript = File.ReadAllText(System.IO.Path.Combine(dir, "nginx.min.js"));
    }

    /// <summary>CSS типографики для отрендеренных документов (Markdown / AsciiDoc).</summary>
    /// <summary>
    /// Оформление страницы просмотра SVG: рисунок занимает всё окно и двигается,
    /// в углу — кнопки масштаба.
    /// </summary>
    /// <summary>
    /// Оформление страницы просмотра SVG. Обе палитры отдаём странице сразу: фон
    /// переключается кнопкой прямо в ней, без перезагрузки.
    /// </summary>
    private static string SvgZoomCss() =>
        "html{--bg:#ffffff;--fg:#1b1b1b;--border:#d0d0d0;--panel:rgba(0,0,0,.06);}" +
        "html.dark{--bg:#1a1a1a;--fg:#e8e8e8;--border:#3c3c3c;--panel:rgba(255,255,255,.10);}" +
        "html,body{margin:0;height:100%;overflow:hidden;background:var(--bg);}" +
        "#svgstage{position:fixed;inset:0;overflow:hidden;touch-action:none;cursor:grab;}" +
        "#svgstage:active{cursor:grabbing;}" +
        "#svgbox{position:absolute;left:0;top:0;transform-origin:0 0;}" +
        "#svgbox svg{display:block;}" +
        "#zoombar{position:fixed;right:12px;bottom:12px;display:flex;gap:6px;}" +
        "#zoombar button{width:34px;height:34px;font-size:16px;line-height:1;cursor:pointer;" +
        "border-radius:8px;border:1px solid var(--border);background:var(--panel);color:var(--fg);}";

    /// <summary>
    /// Масштабирование и перетаскивание рисунка: колесо, щипок, перетаскивание,
    /// двойной щелчок и кнопки в углу. Начальный масштаб вписывает рисунок в окно,
    /// но не растягивает его сверх натуральной величины.
    /// </summary>
    /// <remarks>
    /// Точно такой же скрипт лежит в Android-версии (FormatViewer.kt): страницу
    /// просмотра рисуют обе платформы, и расходиться им нельзя.
    /// </remarks>
    private const string SvgZoomScript =
        "<script>" +
        "(function(){" +
        "var box=document.getElementById('svgbox'),stage=document.getElementById('svgstage');" +
        "var scale=1,tx=0,ty=0,natural=1,MIN=0.05,MAX=40;" +
        "function apply(){box.style.transform='translate('+tx+'px,'+ty+'px) scale('+scale+')';}" +
        "function zoomAt(cx,cy,factor){" +
        "var next=Math.min(MAX,Math.max(MIN,scale*factor));" +
        "var k=next/scale;" +
        "tx=cx-k*(cx-tx);ty=cy-k*(cy-ty);scale=next;apply();}" +
        "function sizeSvg(){" +
        "var svg=box.querySelector('svg');if(!svg)return;" +
        "var vb=svg.getAttribute('viewBox'),w=svg.getAttribute('width'),h=svg.getAttribute('height');" +
        "var bad=function(v){return !v||v.indexOf('%')>=0;};" +
        "if(vb&&(bad(w)||bad(h))){var p=vb.trim().split(/[\\s,]+/);" +
        "if(p.length===4){svg.setAttribute('width',p[2]);svg.setAttribute('height',p[3]);}}}" +
        "function fit(){" +
        "sizeSvg();" +
        "scale=1;tx=0;ty=0;apply();" +
        "var r=box.getBoundingClientRect(),w=r.width,h=r.height;" +
        "if(!w||!h){natural=1;return;}" +
        "var k=Math.min((window.innerWidth-24)/w,(window.innerHeight-24)/h,1);" +
        "natural=k;scale=k;" +
        "tx=(window.innerWidth-w*k)/2;ty=(window.innerHeight-h*k)/2;apply();}" +
        "stage.addEventListener('wheel',function(e){" +
        "e.preventDefault();" +
        "zoomAt(e.clientX,e.clientY,e.deltaY<0?1.15:1/1.15);},{passive:false});" +
        "var pts={},pinch=0;" +
        "stage.addEventListener('pointerdown',function(e){" +
        "pts[e.pointerId]={x:e.clientX,y:e.clientY};" +
        "stage.setPointerCapture(e.pointerId);});" +
        "stage.addEventListener('pointermove',function(e){" +
        "if(!pts[e.pointerId])return;" +
        "var ids=Object.keys(pts);" +
        "if(ids.length===1){" +
        "tx+=e.clientX-pts[e.pointerId].x;ty+=e.clientY-pts[e.pointerId].y;apply();}" +
        "else if(ids.length===2){" +
        "pts[e.pointerId]={x:e.clientX,y:e.clientY};" +
        "var a=pts[ids[0]],b=pts[ids[1]];" +
        "var d=Math.hypot(a.x-b.x,a.y-b.y);" +
        "if(pinch)zoomAt((a.x+b.x)/2,(a.y+b.y)/2,d/pinch);" +
        "pinch=d;return;}" +
        "pts[e.pointerId]={x:e.clientX,y:e.clientY};});" +
        "function release(e){delete pts[e.pointerId];if(Object.keys(pts).length<2)pinch=0;}" +
        "stage.addEventListener('pointerup',release);" +
        "stage.addEventListener('pointercancel',release);" +
        "stage.addEventListener('dblclick',function(e){" +
        "e.preventDefault();" +
        "if(Math.abs(scale-natural)<0.01)zoomAt(e.clientX,e.clientY,2.5);else fit();});" +
        "document.getElementById('zin').onclick=function(){zoomAt(window.innerWidth/2,window.innerHeight/2,1.4);};" +
        "document.getElementById('zout').onclick=function(){zoomAt(window.innerWidth/2,window.innerHeight/2,1/1.4);};" +
        "document.getElementById('zfit').onclick=fit;" +
        "function tellHost(dark){" +
        "try{" +
        "if(window.NeurodocHost&&window.NeurodocHost.setSvgBackground)window.NeurodocHost.setSvgBackground(dark);" +
        "else if(window.chrome&&window.chrome.webview)window.chrome.webview.postMessage('svgbg:'+(dark?'dark':'light'));" +
        "}catch(e){}}" +
        "var bgBtn=document.getElementById('zbg');" +
        "function syncBg(){" +
        "var dark=document.documentElement.classList.contains('dark');" +
        "bgBtn.textContent=dark?'☀':'☽';" +
        "bgBtn.title=dark?'Светлый фон':'Тёмный фон';}" +
        "bgBtn.onclick=function(){" +
        "var dark=!document.documentElement.classList.contains('dark');" +
        "document.documentElement.classList.toggle('dark',dark);" +
        "syncBg();tellHost(dark);};" +
        "syncBg();" +
        "window.addEventListener('resize',fit);" +
        "if(document.readyState==='complete')fit();else window.addEventListener('load',fit);" +
        "setTimeout(fit,50);" +
        "})();" +
        "</script>";

    /// <summary>Встроенные шрифты, уже переведённые в data-URI для страницы просмотра.</summary>
    private static readonly Dictionary<string, string> FontFaceCache = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>Правило @font-face со встроенным шрифтом: файл вкладывается в страницу.</summary>
    /// <remarks>
    /// Не полагаемся на то, что шрифт установлен в системе: приложение ставит его для
    /// текущего пользователя ради текстового стека WinUI, а в просмотре другой движок.
    /// Файл весит сотни килобайт, поэтому читается один раз и остаётся в кэше.
    /// </remarks>
    private static string FontFace(string family)
    {
        if (FontFaceCache.TryGetValue(family, out var cached)) return cached;

        var css = "";
        try
        {
            var path = BundledFonts.FilePath(family);
            if (path != null && File.Exists(path))
            {
                var data = Convert.ToBase64String(File.ReadAllBytes(path));
                css = $"@font-face{{font-family:'{family}';font-style:normal;font-weight:400;" +
                      $"src:url(data:font/ttf;base64,{data}) format('truetype');}}";
            }
        }
        catch
        {
            // без встроенного файла страница возьмёт запасной шрифт из стека — не беда
        }
        FontFaceCache[family] = css;
        return css;
    }

    private static string Number(double value) =>
        value.ToString(System.Globalization.CultureInfo.InvariantCulture);

    private static string DocumentCss(bool dark, string background, string foreground)
    {
        string codeBg = dark ? "rgba(255,255,255,.07)" : "rgba(0,0,0,.06)";
        string border = dark ? "#3c3c3c" : "#d0d0d0";
        string link = dark ? "#4da3ff" : "#0b62c4";
        // Шрифт, размер и интервал берём из настроек редактора: отрисованный документ
        // должен читаться так же, как текст, из которого он собран. Моноширинный шрифт
        // прозой читается иначе, но это осознанный выбор пользователя — тот же, что и
        // в редакторе. Код остаётся моноширинным при любом выборе.
        var settings = App.Settings;
        return FontFace(settings.FontFamily) +
            $"body{{margin:0;padding:22px 30px;background:{background};color:{foreground};" +
            $"font-family:'{settings.FontFamily}','Segoe UI',system-ui,sans-serif;" +
            $"font-size:{Number(settings.FontSize)}px;line-height:{Number(settings.LineSpacing)};" +
            "max-width:920px;}" +
            "h1,h2,h3,h4{line-height:1.25;margin:1.1em 0 .45em;}h1{font-size:1.7em}h2{font-size:1.4em}h3{font-size:1.15em}" +
            $"code{{background:{codeBg};padding:1px 5px;border-radius:4px;font-family:Consolas,monospace;font-size:.92em}}" +
            "pre{overflow-x:auto;border-radius:8px;}pre code{display:block;padding:12px 14px;}" +
            $"table{{border-collapse:collapse;margin:.6em 0}}td,th{{border:1px solid {border};padding:4px 10px}}" +
            $"a{{color:{link}}}blockquote{{border-left:3px solid {border};margin:.6em 0;padding:2px 14px;opacity:.92}}" +
            "img{max-width:100%}hr{border:none;border-top:1px solid " + border + "}" +
            "ul,ol{padding-left:1.6em}";
    }

    /// <summary>
    /// Тёмный ли фон у просмотра SVG: выбор пользователя для этого проекта, а пока его
    /// нет — тема приложения.
    /// </summary>
    private bool SvgBackgroundDark(bool themeIsDark) =>
        App.Settings.CurrentProject?.SvgBackground switch
        {
            "Dark" => true,
            "Light" => false,
            _ => themeIsDark,
        };

    /// <summary>Запоминает выбранный в просмотре фон за текущим проектом.</summary>
    private void SaveSvgBackground(bool dark)
    {
        var project = App.Settings.CurrentProject;
        if (project == null) return;
        var value = dark ? "Dark" : "Light";
        if (project.SvgBackground == value) return;
        project.SvgBackground = value;
        App.SettingsService.Save(App.Settings);
    }

    private string BuildViewerHtml(ViewMode mode, string text)
    {
        bool dark = RootGrid.ActualTheme == ElementTheme.Dark;
        string background = dark ? "#1a1a1a" : "#ffffff";
        string foreground = dark ? "#e8e8e8" : "#1b1b1b";

        if (mode == ViewMode.Svg)
        {
            // фон просмотра рисунков — выбор пользователя для проекта, а не тема
            bool svgDark = SvgBackgroundDark(dark);
            // сырое содержимое файла вставляется как разметка — браузер отрисует SVG
            return "<!DOCTYPE html><html" + (svgDark ? " class='dark'" : "") +
                   "><head><meta charset='utf-8'>" +
                   "<meta name='viewport' content='width=device-width,initial-scale=1," +
                   "user-scalable=no'><style>" + SvgZoomCss() + "</style></head><body>" +
                   "<div id='svgstage'><div id='svgbox'>" + text + "</div></div>" +
                   "<div id='zoombar'>" +
                   "<button id='zin' title='Крупнее'>+</button>" +
                   "<button id='zout' title='Мельче'>\u2212</button>" +
                   "<button id='zfit' title='Вписать в окно'>\u21BA</button>" +
                   "<button id='zbg'></button></div>" +
                   SvgZoomScript + "</body></html>";
        }

        if (mode == ViewMode.Html)
        {
            // полный документ показываем как есть; фрагмент — на белом фоне браузера,
            // иначе авторский текст без стилей нечитаем на тёмной теме
            if (text.Contains("<html", StringComparison.OrdinalIgnoreCase))
                return text;
            return "<!DOCTYPE html><html><head><meta charset='utf-8'></head>" +
                   "<body style='background:#ffffff;margin:10px'>" + text + "</body></html>";
        }

        LoadHighlightAssets();

        if (mode is ViewMode.Md or ViewMode.Adoc)
        {
            string hljsCss = dark ? _hljsCssDark! : _hljsCssLight!;
            string source = System.Net.WebUtility.HtmlEncode(text);
            string engine = mode == ViewMode.Md ? _markedScript! : _asciidoctorScript!;
            string render = mode == ViewMode.Md
                ? "document.getElementById('doc').innerHTML=marked.parse(src);" +
                  "document.querySelectorAll('pre code').forEach(function(el){hljs.highlightElement(el);});"
                : "document.getElementById('doc').innerHTML=Asciidoctor().convert(src,{attributes:{showtitle:true}});";
            return "<!DOCTYPE html><html><head><meta charset='utf-8'><style>" +
                   hljsCss + DocumentCss(dark, background, foreground) +
                   "</style><script>" + _hljsScript + "</script><script>" + engine + "</script></head><body>" +
                   "<div id='doc'></div>" +
                   "<textarea id='src' style='display:none'>" + source + "</textarea>" +
                   "<script>var src=document.getElementById('src').value;" + render + "</script>" +
                   ViewerLinkScript +
                   "</body></html>";
        }

        string language = mode switch
        {
            ViewMode.Json => "json",
            ViewMode.Yaml => "yaml",
            ViewMode.Nginx => "nginx",
            _ => "bash",
        };
        if (mode == ViewMode.Json)
            text = TryPrettyPrintJson(text);

        string escaped = System.Net.WebUtility.HtmlEncode(text);
        string css = dark ? _hljsCssDark! : _hljsCssLight!;
        string fontFamily = App.Settings.FontFamily.Replace("'", "");
        double fontSize = App.Settings.FontSize;

        return "<!DOCTYPE html><html><head><meta charset='utf-8'><style>" + css +
               $"body{{margin:0;background:{background};color:{foreground};}}" +
               "pre{margin:0;padding:14px 20px;}" +
               "code.hljs{background:transparent;padding:0;" +
               $"font-family:'{fontFamily}',Consolas,monospace;font-size:{fontSize}px;line-height:1.45;}}" +
               "</style><script>" + _hljsScript + "</script>" +
               (mode == ViewMode.Nginx ? "<script>" + _nginxScript + "</script>" : "") +
               "</head><body>" +
               $"<pre><code class='language-{language}'>" + escaped + "</code></pre>" +
               "<script>hljs.highlightAll();</script></body></html>";
    }

    // ================= Дельта (unified diff из git) =================

    /// <summary>Показана ли сейчас дельта. Отдельно от _viewMode: дельта — не режим файла
    /// и не запоминается в ViewState, это разовый просмотр поверх текстового вида.</summary>
    private bool _diffActive;
    private string? _diffKey;

    /// <summary>
    /// Показывает дельту файла в просмотрщике. Повторный вызов с тем же ключом закрывает —
    /// кнопка «±» работает как переключатель.
    /// </summary>
    private async Task ShowDiffAsync(string key, string title, string unifiedDiff)
    {
        if (_diffActive && _diffKey == key)
        {
            ResetViewMode();
            return;
        }
        await EnsureViewerAsync();
        FormatViewer.NavigateToString(BuildDiffHtml(title, unifiedDiff));
        EditorScroll.Visibility = Visibility.Collapsed;
        FormatViewer.Visibility = Visibility.Visible;
        _diffActive = true;
        _diffKey = key;
    }

    /// <summary>HTML для unified diff: зелёные добавленные строки, красные удалённые.</summary>
    private string BuildDiffHtml(string title, string diff)
    {
        bool dark = RootGrid.ActualTheme == ElementTheme.Dark;
        string background = dark ? "#1a1a1a" : "#ffffff";
        string foreground = dark ? "#e8e8e8" : "#1b1b1b";
        string addBg = dark ? "#1c2f22" : "#e6ffec";
        string delBg = dark ? "#3a2124" : "#ffebe9";
        string dim = dark ? "#8a8a8a" : "#6e6e6e";
        string fontFamily = App.Settings.FontFamily.Replace("'", "");
        double fontSize = App.Settings.FontSize;

        var body = new System.Text.StringBuilder();
        bool empty = true;
        foreach (var raw in diff.Split('\n'))
        {
            var line = raw.TrimEnd('\r');
            if (line.Length == 0 && empty) continue;
            string escaped = System.Net.WebUtility.HtmlEncode(line);
            string cls =
                line.StartsWith("+++") || line.StartsWith("---") ? "meta" :
                line.StartsWith("diff ") || line.StartsWith("index ") ||
                line.StartsWith("new file") || line.StartsWith("deleted file") ||
                line.StartsWith("rename ") || line.StartsWith("similarity ") ? "meta" :
                line.StartsWith("@@") ? "hunk" :
                line.StartsWith('+') ? "add" :
                line.StartsWith('-') ? "del" : "ctx";
            body.Append("<div class='").Append(cls).Append("'>")
                .Append(escaped.Length == 0 ? "&nbsp;" : escaped).Append("</div>");
            empty = false;
        }
        if (empty)
            body.Append("<div class='meta'>Изменений нет</div>");

        return "<!DOCTYPE html><html><head><meta charset='utf-8'><style>" +
               $"body{{margin:0;background:{background};color:{foreground};" +
               $"font-family:'{fontFamily}',Consolas,monospace;font-size:{fontSize}px;line-height:1.45;}}" +
               "#wrap{padding:10px 20px 14px;}" +
               $"#title{{font-weight:600;padding:4px 0 10px;color:{foreground};}}" +
               "#wrap div{white-space:pre-wrap;word-break:break-word;padding:0 6px;}" +
               $".add{{background:{addBg};}}" +
               $".del{{background:{delBg};}}" +
               $".hunk{{color:{dim};padding-top:8px !important;}}" +
               $".meta{{color:{dim};}}" +
               "</style></head><body><div id='wrap'>" +
               "<div id='title'>" + System.Net.WebUtility.HtmlEncode(title) + "</div>" +
               body + "</div></body></html>";
    }

    /// <summary>Форматирует JSON с отступами; при ошибке разбора возвращает исходный текст.</summary>
    private static string TryPrettyPrintJson(string text)
    {
        try
        {
            using var document = JsonDocument.Parse(text, new JsonDocumentOptions
            {
                AllowTrailingCommas = true,
                CommentHandling = JsonCommentHandling.Skip,
            });
            return JsonSerializer.Serialize(document.RootElement, new JsonSerializerOptions
            {
                WriteIndented = true,
                Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
            });
        }
        catch
        {
            return text;
        }
    }
}
