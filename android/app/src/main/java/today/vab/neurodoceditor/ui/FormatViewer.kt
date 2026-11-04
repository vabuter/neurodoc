package today.vab.neurodoceditor.ui

import android.annotation.SuppressLint
import android.webkit.WebView
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import today.vab.neurodoceditor.data.NeurodocCatalog
import today.vab.neurodoceditor.R
import today.vab.neurodoceditor.data.AppSettings
import today.vab.neurodoceditor.ui.theme.LocalNeurodocColors

/** Библиотеки рендеринга, встроенные в приложение (assets/highlight). */
private class ViewerAssets(private val ctx: Context) {
    private fun read(name: String): String =
        runCatching { ctx.assets.open("highlight/" + name).bufferedReader().use { it.readText() } }
            .getOrDefault("")

    /**
     * Встроенный шрифт как data-URI для страницы просмотра.
     *
     * WebView не видит шрифты из res/font по имени, поэтому файл вкладываем в саму
     * страницу. Весит он сотни килобайт — читаем один раз на запуск.
     */
    private val fontCache = HashMap<String, String>()

    fun fontFace(family: String): String = fontCache.getOrPut(family) {
        val resId = when (family) {
            AppSettings.FONT_JETBRAINS -> R.font.jetbrains_mono_regular
            AppSettings.FONT_INTER -> R.font.inter_regular
            else -> return@getOrPut "" // системный шрифт странице и так доступен
        }
        runCatching {
            val bytes = ctx.resources.openRawResource(resId).use { it.readBytes() }
            val data = android.util.Base64.encodeToString(bytes, android.util.Base64.NO_WRAP)
            "@font-face{font-family:'$family';font-style:normal;font-weight:400;" +
                "src:url(data:font/ttf;base64,$data) format('truetype');}"
        }.getOrDefault("")
    }

    val hljs by lazy { read("highlight.min.js") }
    val hljsCssLight by lazy { read("github.min.css") }
    val hljsCssDark by lazy { read("github-dark.min.css") }
    val marked by lazy { read("marked.min.js") }
    val asciidoctor by lazy { read("asciidoctor.min.js") }
    val nginx by lazy { read("nginx.min.js") }
}

/**
 * Просмотр содержимого в выбранном формате — как кнопки форматов в десктопной версии.
 * Рендер идёт в WebView, библиотеки (highlight.js, marked.js, asciidoctor.js) лежат
 * в assets и подключаются локально, без интернета.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun FormatViewer(vm: EditorViewModel, onWebLink: (String) -> Unit) {
    val colors = LocalNeurodocColors.current
    val context = LocalContext.current
    val assets = remember(context) { ViewerAssets(context) }
    val dark = MaterialTheme.colorScheme.background.luminanceIsDark()
    // фон просмотра рисунков — выбор пользователя для проекта, а не тема приложения
    val svgDark = vm.svgBackgroundDark(dark)
    // цель разбирается относительно ТЕКУЩЕГО файла, как и везде в формате
    val sourceId = vm.currentId ?: NeurodocCatalog.ROOT_ID
    val bridge = remember(sourceId) {
        ViewerBridge(
            onLink = { target ->
                if (NeurodocCatalog.isWebTarget(target)) {
                    onWebLink(target)
                } else {
                    // несуществующая цель — не повод никуда уходить: в том же ReadMe.md
                    // ссылки из описания формата ведут на файлы-примеры, которых нет
                    val id = NeurodocCatalog.tryResolveLink(sourceId, target)
                    if (id != null && vm.catalog?.exists(id) == true) vm.openFile(id)
                }
            },
            onSvgBackground = { vm.saveSvgBackground(it) },
        )
    }
    val appSettings = vm.settings
    val html = remember(
        vm.viewMode, vm.text.text, dark, svgDark,
        appSettings.fontFamily, appSettings.fontSize, appSettings.lineSpacing,
    ) {
        buildViewerHtml(
            mode = vm.viewMode,
            text = vm.text.text,
            dark = dark,
            svgDark = svgDark,
            fontFamily = appSettings.fontFamily,
            fontSize = appSettings.fontSize,
            lineSpacing = appSettings.lineSpacing,
            fontFace = assets.fontFace(appSettings.fontFamily),
            hljs = assets.hljs,
            hljsCss = if (dark) assets.hljsCssDark else assets.hljsCssLight,
            marked = assets.marked,
            asciidoctor = assets.asciidoctor,
            nginx = assets.nginx,
        )
    }
    // у просмотра рисунков свой фон — иначе по краям просвечивала бы тема приложения
    val background =
        if (vm.viewMode == ViewMode.SVG) (if (svgDark) 0xFF1A1A1A.toInt() else 0xFFFFFFFF.toInt())
        else colors.editorBackground.toArgb()

    Surface(
        modifier = Modifier.fillMaxSize().padding(horizontal = 8.dp, vertical = 4.dp),
        color = colors.editorBackground,
        shape = RoundedCornerShape(10.dp),
    ) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { ctx ->
                WebView(ctx).apply {
                    settings.javaScriptEnabled = true
                    addJavascriptInterface(bridge, "NeurodocHost")
                    // доступ к файлам не нужен: библиотеки встроены в страницу инлайном,
                    // а сама она грузится из строки, без file://
                    settings.allowFileAccess = false
                    settings.domStorageEnabled = false
                    webChromeClient = object : android.webkit.WebChromeClient() {
                        override fun onConsoleMessage(m: android.webkit.ConsoleMessage): Boolean {
                            android.util.Log.i("NeurodocViewer", "console: " + m.message() + " @" + m.lineNumber())
                            return true
                        }
                    }
                    setBackgroundColor(background)
                }
            },
            update = { webView ->
                webView.setBackgroundColor(background)
                webView.tag = bridge // держим мост живым, пока жив WebView
                webView.loadDataWithBaseURL(null, html, "text/html", "UTF-8", null)
            },
        )
    }
}

/**
 * Мост «страница -> приложение»: клик по ссылке в отрендеренном документе.
 *
 * Ссылку не даём открыть самому WebView — он ушёл бы с отрисованной страницы.
 */
class ViewerBridge(
    private val onLink: (String) -> Unit,
    private val onSvgBackground: (Boolean) -> Unit,
) {
    @android.webkit.JavascriptInterface
    fun openLink(target: String) {
        if (target.isBlank()) return
        // JS-интерфейс вызывается не в UI-потоке, а переход трогает состояние Compose
        android.os.Handler(android.os.Looper.getMainLooper()).post { onLink(target) }
    }

    /** Пользователь переключил фон в просмотре рисунка — запоминаем за проектом. */
    @android.webkit.JavascriptInterface
    fun setSvgBackground(dark: Boolean) {
        android.os.Handler(android.os.Looper.getMainLooper()).post { onSvgBackground(dark) }
    }
}

/**
 * Скрипт для отрендеренных документов: делает neurodoc-ссылки рабочими.
 *
 * Markdown превращает `[текст](цель)` в ссылку сам, а AsciiDoc такого синтаксиса не
 * знает и оставляет его текстом — поэтому такие места размечаем сами, обходя текстовые
 * узлы и не трогая содержимое a, code и pre.
 */
private const val VIEWER_LINK_SCRIPT = """<script>(function(){
var RE=/\[([^\]\n]*)\]\(([^()\s\\]+)\)/g;
var root=document.getElementById('doc');
if(root){
var walker=document.createTreeWalker(root,NodeFilter.SHOW_TEXT,null);
var nodes=[],n;
while(n=walker.nextNode()){
if(n.parentNode&&n.parentNode.closest('a,code,pre'))continue;
RE.lastIndex=0;if(RE.test(n.nodeValue))nodes.push(n);}
nodes.forEach(function(node){
var frag=document.createDocumentFragment(),last=0,m;RE.lastIndex=0;
while(m=RE.exec(node.nodeValue)){
frag.appendChild(document.createTextNode(node.nodeValue.slice(last,m.index)));
var a=document.createElement('a');a.setAttribute('href',m[2]);
a.textContent=m[1]||m[2];frag.appendChild(a);last=m.index+m[0].length;}
frag.appendChild(document.createTextNode(node.nodeValue.slice(last)));
node.parentNode.replaceChild(frag,node);});}
document.addEventListener('click',function(e){
var a=e.target&&e.target.closest?e.target.closest('a[href]'):null;
if(!a)return;e.preventDefault();
if(window.NeurodocHost)window.NeurodocHost.openLink(a.getAttribute('href'));});
})();</script>"""

private fun androidx.compose.ui.graphics.Color.luminanceIsDark(): Boolean {
    val r = red; val g = green; val b = blue
    return (0.299 * r + 0.587 * g + 0.114 * b) < 0.5
}

/** Собирает HTML-страницу для выбранного формата (порт логики из MainWindow.Viewer.cs). */
fun buildViewerHtml(
    mode: ViewMode,
    text: String,
    dark: Boolean,
    svgDark: Boolean,
    fontFamily: String,
    fontSize: Float,
    lineSpacing: Float,
    fontFace: String,
    hljs: String,
    hljsCss: String,
    marked: String,
    asciidoctor: String,
    nginx: String,
): String {
    val background = if (dark) "#1a1a1a" else "#ffffff"
    val foreground = if (dark) "#e8e8e8" else "#1b1b1b"

    if (mode == ViewMode.SVG) {
        return "<!DOCTYPE html><html" + (if (svgDark) " class='dark'" else "") +
            "><head><meta charset='utf-8'>" +
            "<meta name='viewport' content='width=device-width,initial-scale=1," +
            "user-scalable=no'><style>" + svgZoomCss() + "</style></head><body>" +
            "<div id='svgstage'><div id='svgbox'>$text</div></div>" +
            "<div id='zoombar'>" +
            "<button id='zin' title='Крупнее'>+</button>" +
            "<button id='zout' title='Мельче'>\u2212</button>" +
            "<button id='zfit' title='Вписать в окно'>\u21BA</button>" +
            "<button id='zbg'></button></div>" +
            SVG_ZOOM_SCRIPT + "</body></html>"
    }

    if (mode == ViewMode.HTML) {
        if (text.contains("<html", ignoreCase = true)) return text
        return "<!DOCTYPE html><html><head><meta charset='utf-8'>" +
            "<meta name='viewport' content='width=device-width, initial-scale=1'></head>" +
            "<body style='background:#ffffff;margin:10px'>$text</body></html>"
    }

    val head = buildString {
        append("<!DOCTYPE html><html><head><meta charset='utf-8'>")
        append("<meta name='viewport' content='width=device-width, initial-scale=1'>")
        append("<style>")
        append(hljsCss)
        append("body{margin:0;background:$background;color:$foreground;}")
        append("pre{margin:0;padding:12px 14px;}")
        append("code.hljs{background:transparent;padding:0;")
        append("font-family:monospace;font-size:${fontSize}px;line-height:1.45;}")
        if (mode == ViewMode.MD || mode == ViewMode.ADOC) {
            append(fontFace)
            append(documentCss(dark, background, foreground, fontFamily, fontSize, lineSpacing))
        }
        append("</style>")
        append("<script>").append(hljs).append("</script>")
        if (mode == ViewMode.MD) append("<script>").append(marked).append("</script>")
        if (mode == ViewMode.ADOC) append("<script>").append(asciidoctor).append("</script>")
        // грамматика nginx в highlight.js не встроена — подключаем отдельным модулем
        if (mode == ViewMode.NGINX) append("<script>").append(nginx).append("</script>")
        append("</head>")
    }

    // содержимое передаём через JSON-строку — безопасно для любых кавычек и переводов строк
    val payload = JSONArray().put(text).toString()

    return when (mode) {
        ViewMode.MD -> head + "<body><div id='doc'></div><script>" +
            "var src=$payload[0];" +
            "document.getElementById('doc').innerHTML=marked.parse(src);" +
            "document.querySelectorAll('pre code').forEach(function(el){hljs.highlightElement(el);});" +
            "</script>" + VIEWER_LINK_SCRIPT + "</body></html>"

        ViewMode.ADOC -> head + "<body><div id='doc'></div><script>" +
            "var src=$payload[0];" +
            "document.getElementById('doc').innerHTML=Asciidoctor().convert(src,{attributes:{showtitle:true}});" +
            "</script>" + VIEWER_LINK_SCRIPT + "</body></html>"

        else -> {
            val language = when (mode) {
                ViewMode.JSON -> "json"
                ViewMode.YAML -> "yaml"
                ViewMode.NGINX -> "nginx"
                else -> "bash"
            }
            val prepared = if (mode == ViewMode.JSON) prettyJson(text) else text
            val preparedPayload = JSONArray().put(prepared).toString()
            head + "<body><pre><code id='code' class='language-$language'></code></pre><script>" +
                "document.getElementById('code').textContent=$preparedPayload[0];" +
                "hljs.highlightAll();" +
                "</script></body></html>"
        }
    }
}

/**
 * Оформление страницы просмотра SVG: рисунок занимает всё окно и двигается,
 * в углу — кнопки масштаба.
 */
private fun svgZoomCss(): String =
    "html{--bg:#ffffff;--fg:#1b1b1b;--border:#d0d0d0;--panel:rgba(0,0,0,.06);}" +
        "html.dark{--bg:#1a1a1a;--fg:#e8e8e8;--border:#3c3c3c;--panel:rgba(255,255,255,.10);}" +
        "html,body{margin:0;height:100%;overflow:hidden;background:var(--bg);}" +
        "#svgstage{position:fixed;inset:0;overflow:hidden;touch-action:none;cursor:grab;}" +
        "#svgstage:active{cursor:grabbing;}" +
        "#svgbox{position:absolute;left:0;top:0;transform-origin:0 0;}" +
        "#svgbox svg{display:block;}" +
        "#zoombar{position:fixed;right:12px;bottom:12px;display:flex;gap:6px;}" +
        "#zoombar button{width:38px;height:38px;font-size:17px;line-height:1;cursor:pointer;" +
        "border-radius:8px;border:1px solid var(--border);background:var(--panel);color:var(--fg);}"

/**
 * Масштабирование и перетаскивание рисунка: щипок, перетаскивание, двойное касание
 * и кнопки в углу. Начальный масштаб вписывает рисунок в экран, но не растягивает
 * его сверх натуральной величины.
 *
 * Точно такой же скрипт лежит в Windows-версии (MainWindow.Viewer.cs): страницу
 * просмотра рисуют обе платформы, и расходиться им нельзя.
 */
private const val SVG_ZOOM_SCRIPT =
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
        "</script>"

private fun documentCss(
    dark: Boolean,
    background: String,
    foreground: String,
    fontFamily: String,
    fontSize: Float,
    lineSpacing: Float,
): String {
    val codeBg = if (dark) "rgba(255,255,255,.07)" else "rgba(0,0,0,.06)"
    val border = if (dark) "#3c3c3c" else "#d0d0d0"
    val link = if (dark) "#4da3ff" else "#0b62c4"
    // Шрифт, размер и интервал берём из настроек редактора: отрисованный документ
    // должен читаться так же, как текст, из которого он собран. Код остаётся
    // моноширинным при любом выборе.
    val size = fontSize.toString().removeSuffix(".0")
    val height = lineSpacing.toString().removeSuffix(".0")
    return "body{padding:16px 18px;font-family:'$fontFamily',system-ui,sans-serif;" +
        "font-size:${size}px;line-height:$height;}" +
        "h1,h2,h3,h4{line-height:1.25;margin:1.1em 0 .45em;}h1{font-size:1.6em}h2{font-size:1.35em}" +
        "code{background:$codeBg;padding:1px 5px;border-radius:4px;font-family:monospace;font-size:.92em}" +
        "pre{overflow-x:auto;border-radius:8px;}pre code{display:block;padding:10px 12px;}" +
        "table{border-collapse:collapse;margin:.6em 0}td,th{border:1px solid $border;padding:4px 10px}" +
        "a{color:$link}blockquote{border-left:3px solid $border;margin:.6em 0;padding:2px 12px;opacity:.92}" +
        "img{max-width:100%}hr{border:none;border-top:1px solid $border}ul,ol{padding-left:1.4em}"
}

/** Форматирует JSON с отступами; при ошибке разбора возвращает исходный текст. */
private fun prettyJson(text: String): String = runCatching {
    val trimmed = text.trim()
    when {
        trimmed.startsWith("{") -> JSONObject(trimmed).toString(2)
        trimmed.startsWith("[") -> JSONArray(trimmed).toString(2)
        else -> text
    }
}.getOrDefault(text)
