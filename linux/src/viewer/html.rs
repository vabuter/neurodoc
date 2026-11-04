//! Страницы просмотра: порт `BuildViewerHtml`, `DocumentCss`, `FontFace`,
//! `BuildDiffHtml` и `TryPrettyPrintJson` из `MainWindow.Viewer.cs`. Библиотеки
//! (highlight.js, marked.js, asciidoctor.js) и встроенные шрифты вкладываются
//! в бинарник прямо из `windows/NeurodocEditor/Assets` — третьей копии нет.

use std::sync::LazyLock;

use base64::Engine;

use super::ViewMode;
use super::scripts::{SVG_ZOOM_SCRIPT, VIEWER_LINK_SCRIPT};
use crate::domain::settings::{AppSettings, ProjectInfo};

const HLJS_SCRIPT: &str = include_str!("../../../windows/NeurodocEditor/Assets/Highlight/highlight.min.js");
const HLJS_CSS_LIGHT: &str = include_str!("../../../windows/NeurodocEditor/Assets/Highlight/github.min.css");
const HLJS_CSS_DARK: &str = include_str!("../../../windows/NeurodocEditor/Assets/Highlight/github-dark.min.css");
const MARKED_SCRIPT: &str = include_str!("../../../windows/NeurodocEditor/Assets/Highlight/marked.min.js");
const ASCIIDOCTOR_SCRIPT: &str = include_str!("../../../windows/NeurodocEditor/Assets/Highlight/asciidoctor.min.js");
const NGINX_SCRIPT: &str = include_str!("../../../windows/NeurodocEditor/Assets/Highlight/nginx.min.js");

/// Встроенные шрифты: (семейство, имя файла, содержимое). Те же файлы, что ставит
/// Windows-версия для пользователя и что лежат в `res/font` на Android.
pub const BUNDLED_FONTS: [(&str, &str, &[u8]); 2] = [
    ("Inter", "Inter-Regular.ttf", include_bytes!("../../../windows/NeurodocEditor/Assets/Fonts/Inter-Regular.ttf")),
    (
        "JetBrains Mono",
        "JetBrainsMono-Regular.ttf",
        include_bytes!("../../../windows/NeurodocEditor/Assets/Fonts/JetBrainsMono-Regular.ttf"),
    ),
];

/// Имена, добавляемые в список выбора, даже если шрифт не установлен: проприетарный
/// Anthropic Sans нельзя распространять с приложением, но если он установлен — заработает.
pub const EXTRA_FONT_NAMES: [&str; 1] = ["Anthropic Sans"];

/// Встроенный шрифт по имени семейства (регистр не важен).
pub fn bundled_font(family: &str) -> Option<(&'static str, &'static str, &'static [u8])> {
    BUNDLED_FONTS.iter().copied().find(|(name, _, _)| name.eq_ignore_ascii_case(family))
}

pub fn is_bundled_font(family: &str) -> bool {
    bundled_font(family).is_some()
}

/// Правила @font-face со встроенными шрифтами, уже переведёнными в data-URI.
/// Файл весит сотни килобайт, поэтому кодируется один раз.
static FONT_FACES: LazyLock<Vec<(&'static str, String)>> = LazyLock::new(|| {
    BUNDLED_FONTS
        .iter()
        .map(|(family, _, bytes)| {
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            let css = format!(
                "@font-face{{font-family:'{family}';font-style:normal;font-weight:400;\
                 src:url(data:font/ttf;base64,{data}) format('truetype');}}"
            );
            (*family, css)
        })
        .collect()
});

/// Правило @font-face со встроенным шрифтом: файл вкладывается в страницу.
/// Не полагаемся на то, что шрифт установлен в системе: просмотр рисует отдельный
/// процесс WebKit, и шрифты процесса приложения ему не видны.
fn font_face(family: &str) -> &'static str {
    FONT_FACES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(family))
        .map(|(_, css)| css.as_str())
        .unwrap_or("")
}

/// HTML-экранирование текста (как WebUtility.HtmlEncode).
pub fn html_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn number(value: f64) -> String {
    format!("{value}")
}

/// Оформление страницы просмотра SVG. Обе палитры отдаём странице сразу: фон
/// переключается кнопкой прямо в ней, без перезагрузки.
pub const SVG_ZOOM_CSS: &str = concat!(
    "html{--bg:#ffffff;--fg:#1b1b1b;--border:#d0d0d0;--panel:rgba(0,0,0,.06);}",
    "html.dark{--bg:#1a1a1a;--fg:#e8e8e8;--border:#3c3c3c;--panel:rgba(255,255,255,.10);}",
    "html,body{margin:0;height:100%;overflow:hidden;background:var(--bg);}",
    "#svgstage{position:fixed;inset:0;overflow:hidden;touch-action:none;cursor:grab;}",
    "#svgstage:active{cursor:grabbing;}",
    "#svgbox{position:absolute;left:0;top:0;transform-origin:0 0;}",
    "#svgbox svg{display:block;}",
    "#zoombar{position:fixed;right:12px;bottom:12px;display:flex;gap:6px;}",
    "#zoombar button{width:34px;height:34px;font-size:16px;line-height:1;cursor:pointer;",
    "border-radius:8px;border:1px solid var(--border);background:var(--panel);color:var(--fg);}",
);

/// CSS типографики для отрендеренных документов (Markdown / AsciiDoc).
///
/// Шрифт, размер и интервал берём из настроек редактора: отрисованный документ
/// должен читаться так же, как текст, из которого он собран. Моноширинный шрифт
/// прозой читается иначе, но это осознанный выбор пользователя — тот же, что и
/// в редакторе. Код остаётся моноширинным при любом выборе.
fn document_css(dark: bool, background: &str, foreground: &str, settings: &AppSettings) -> String {
    let code_bg = if dark { "rgba(255,255,255,.07)" } else { "rgba(0,0,0,.06)" };
    let border = if dark { "#3c3c3c" } else { "#d0d0d0" };
    let link = if dark { "#4da3ff" } else { "#0b62c4" };
    let family = &settings.font_family;
    format!(
        "{}body{{margin:0;padding:22px 30px;background:{background};color:{foreground};\
         font-family:'{family}','Segoe UI',system-ui,sans-serif;\
         font-size:{}px;line-height:{};max-width:920px;}}\
         h1,h2,h3,h4{{line-height:1.25;margin:1.1em 0 .45em;}}h1{{font-size:1.7em}}h2{{font-size:1.4em}}h3{{font-size:1.15em}}\
         code{{background:{code_bg};padding:1px 5px;border-radius:4px;font-family:Consolas,monospace;font-size:.92em}}\
         pre{{overflow-x:auto;border-radius:8px;}}pre code{{display:block;padding:12px 14px;}}\
         table{{border-collapse:collapse;margin:.6em 0}}td,th{{border:1px solid {border};padding:4px 10px}}\
         a{{color:{link}}}blockquote{{border-left:3px solid {border};margin:.6em 0;padding:2px 14px;opacity:.92}}\
         img{{max-width:100%}}hr{{border:none;border-top:1px solid {border}}}\
         ul,ol{{padding-left:1.6em}}",
        font_face(family),
        number(settings.font_size),
        number(settings.line_spacing),
    )
}

/// Тёмный ли фон у просмотра SVG: выбор пользователя для этого проекта, а пока его
/// нет — тема приложения.
pub fn svg_background_dark(project: Option<&ProjectInfo>, theme_is_dark: bool) -> bool {
    match project.and_then(|p| p.svg_background.as_deref()) {
        Some("Dark") => true,
        Some("Light") => false,
        _ => theme_is_dark,
    }
}

/// Префикс сообщения о смене фона — остальные сообщения это цели ссылок.
pub const SVG_BACKGROUND_MESSAGE: &str = "svgbg:";

pub fn build_viewer_html(mode: ViewMode, text: &str, dark: bool, svg_dark: bool, settings: &AppSettings) -> String {
    let background = if dark { "#1a1a1a" } else { "#ffffff" };
    let foreground = if dark { "#e8e8e8" } else { "#1b1b1b" };

    match mode {
        ViewMode::Text => String::new(),
        ViewMode::Svg => {
            // сырое содержимое файла вставляется как разметка — браузер отрисует SVG
            format!(
                "<!DOCTYPE html><html{}><head><meta charset='utf-8'>\
                 <meta name='viewport' content='width=device-width,initial-scale=1,\
                 user-scalable=no'><style>{SVG_ZOOM_CSS}</style></head><body>\
                 <div id='svgstage'><div id='svgbox'>{text}</div></div>\
                 <div id='zoombar'>\
                 <button id='zin' title='Крупнее'>+</button>\
                 <button id='zout' title='Мельче'>\u{2212}</button>\
                 <button id='zfit' title='Вписать в окно'>\u{21BA}</button>\
                 <button id='zbg'></button></div>\
                 {SVG_ZOOM_SCRIPT}</body></html>",
                if svg_dark { " class='dark'" } else { "" }
            )
        }
        ViewMode::Html => {
            // полный документ показываем как есть; фрагмент — на белом фоне браузера,
            // иначе авторский текст без стилей нечитаем на тёмной теме
            if text.to_ascii_lowercase().contains("<html") {
                text.to_string()
            } else {
                format!(
                    "<!DOCTYPE html><html><head><meta charset='utf-8'></head>\
                     <body style='background:#ffffff;margin:10px'>{text}</body></html>"
                )
            }
        }
        ViewMode::Md | ViewMode::Adoc => {
            let hljs_css = if dark { HLJS_CSS_DARK } else { HLJS_CSS_LIGHT };
            let source = html_encode(text);
            let engine = if mode == ViewMode::Md { MARKED_SCRIPT } else { ASCIIDOCTOR_SCRIPT };
            let render = if mode == ViewMode::Md {
                "document.getElementById('doc').innerHTML=marked.parse(src);\
                 document.querySelectorAll('pre code').forEach(function(el){hljs.highlightElement(el);});"
            } else {
                "document.getElementById('doc').innerHTML=Asciidoctor().convert(src,{attributes:{showtitle:true}});"
            };
            format!(
                "<!DOCTYPE html><html><head><meta charset='utf-8'><style>{hljs_css}{}</style>\
                 <script>{HLJS_SCRIPT}</script><script>{engine}</script></head><body>\
                 <div id='doc'></div>\
                 <textarea id='src' style='display:none'>{source}</textarea>\
                 <script>var src=document.getElementById('src').value;{render}</script>\
                 {VIEWER_LINK_SCRIPT}</body></html>",
                document_css(dark, background, foreground, settings)
            )
        }
        ViewMode::Json | ViewMode::Yaml | ViewMode::Bash | ViewMode::Nginx => {
            let language = match mode {
                ViewMode::Json => "json",
                ViewMode::Yaml => "yaml",
                ViewMode::Nginx => "nginx",
                _ => "bash",
            };
            let pretty;
            let text = if mode == ViewMode::Json {
                pretty = try_pretty_print_json(text);
                pretty.as_str()
            } else {
                text
            };
            let escaped = html_encode(text);
            let css = if dark { HLJS_CSS_DARK } else { HLJS_CSS_LIGHT };
            let font_family = settings.font_family.replace('\'', "");
            format!(
                "<!DOCTYPE html><html><head><meta charset='utf-8'><style>{css}\
                 body{{margin:0;background:{background};color:{foreground};}}\
                 pre{{margin:0;padding:14px 20px;}}\
                 code.hljs{{background:transparent;padding:0;\
                 font-family:'{font_family}',Consolas,monospace;font-size:{}px;line-height:1.45;}}\
                 </style><script>{HLJS_SCRIPT}</script>{}\
                 </head><body>\
                 <pre><code class='language-{language}'>{escaped}</code></pre>\
                 <script>hljs.highlightAll();</script></body></html>",
                number(settings.font_size),
                if mode == ViewMode::Nginx { format!("<script>{NGINX_SCRIPT}</script>") } else { String::new() },
            )
        }
    }
}

/// HTML для unified diff: зелёные добавленные строки, красные удалённые.
/// Тот же вид, что `BuildDiffHtml` (Windows) и `buildDiffHtml` (Android).
pub fn build_diff_html(title: &str, diff: &str, dark: bool, settings: &AppSettings) -> String {
    let background = if dark { "#1a1a1a" } else { "#ffffff" };
    let foreground = if dark { "#e8e8e8" } else { "#1b1b1b" };
    let add_bg = if dark { "#1c2f22" } else { "#e6ffec" };
    let del_bg = if dark { "#3a2124" } else { "#ffebe9" };
    let dim = if dark { "#8a8a8a" } else { "#6e6e6e" };
    let font_family = settings.font_family.replace('\'', "");

    let mut body = String::new();
    let mut empty = true;
    for raw in diff.split('\n') {
        let line = raw.trim_end_matches('\r');
        if line.is_empty() && empty {
            continue;
        }
        let escaped = html_encode(line);
        let class = if line.starts_with("+++")
            || line.starts_with("---")
            || line.starts_with("diff ")
            || line.starts_with("index ")
            || line.starts_with("new file")
            || line.starts_with("deleted file")
            || line.starts_with("rename ")
            || line.starts_with("similarity ")
        {
            "meta"
        } else if line.starts_with("@@") {
            "hunk"
        } else if line.starts_with('+') {
            "add"
        } else if line.starts_with('-') {
            "del"
        } else {
            "ctx"
        };
        body.push_str(&format!("<div class='{class}'>{}</div>", if escaped.is_empty() { "&nbsp;" } else { &escaped }));
        empty = false;
    }
    if empty {
        body.push_str("<div class='meta'>Изменений нет</div>");
    }

    format!(
        "<!DOCTYPE html><html><head><meta charset='utf-8'><style>\
         body{{margin:0;background:{background};color:{foreground};\
         font-family:'{font_family}',Consolas,monospace;font-size:{}px;line-height:1.45;}}\
         #wrap{{padding:10px 20px 14px;}}\
         #title{{font-weight:600;padding:4px 0 10px;color:{foreground};}}\
         #wrap div{{white-space:pre-wrap;word-break:break-word;padding:0 6px;}}\
         .add{{background:{add_bg};}}\
         .del{{background:{del_bg};}}\
         .hunk{{color:{dim};padding-top:8px !important;}}\
         .meta{{color:{dim};}}\
         </style></head><body><div id='wrap'>\
         <div id='title'>{}</div>{body}</div></body></html>",
        number(settings.font_size),
        html_encode(title),
    )
}

/// Форматирует JSON с отступами; при ошибке разбора возвращает исходный текст.
/// Кириллица не экранируется, порядок ключей и запись чисел сохраняются. Комментарии и
/// висячие запятые допускаются, как у System.Text.Json на Windows.
pub fn try_pretty_print_json(text: &str) -> String {
    let cleaned = strip_json_extras(text);
    match serde_json::from_str::<serde_json::Value>(&cleaned) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.to_string()),
        Err(_) => text.to_string(),
    }
}

/// Убирает комментарии `//…` и `/*…*/` и запятые перед `}`/`]` вне строк.
fn strip_json_extras(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            ',' => {
                // висячая запятая: до следующего значимого символа только пробелы и комментарии
                let mut j = i + 1;
                loop {
                    while j < chars.len() && chars[j].is_whitespace() {
                        j += 1;
                    }
                    if chars.get(j) == Some(&'/') && chars.get(j + 1) == Some(&'/') {
                        while j < chars.len() && chars[j] != '\n' {
                            j += 1;
                        }
                        continue;
                    }
                    if chars.get(j) == Some(&'/') && chars.get(j + 1) == Some(&'*') {
                        j += 2;
                        while j + 1 < chars.len() && !(chars[j] == '*' && chars[j + 1] == '/') {
                            j += 1;
                        }
                        j += 2;
                        continue;
                    }
                    break;
                }
                if !matches!(chars.get(j), Some('}') | Some(']')) {
                    out.push(c);
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_pretty_print_keeps_cyrillic_and_order() {
        let out = try_pretty_print_json(r#"{"б":1,"а":"привет"}"#);
        assert_eq!(out, "{\n  \"б\": 1,\n  \"а\": \"привет\"\n}");
        assert_eq!(try_pretty_print_json("not json"), "not json");
        // комментарии, висячие запятые и запись чисел — как у System.Text.Json
        let lax = "{\n  // комментарий\n  \"a\": [1.50, 123456789012345678901,], /* ещё */\n  \"s\": \"http://x/y, // не комментарий\",\n}";
        assert_eq!(
            try_pretty_print_json(lax),
            "{\n  \"a\": [\n    1.50,\n    123456789012345678901\n  ],\n  \"s\": \"http://x/y, // не комментарий\"\n}"
        );
    }

    #[test]
    fn builds_pages_for_every_mode() {
        let settings = AppSettings::default();
        let svg = build_viewer_html(ViewMode::Svg, "<svg viewBox='0 0 1 1'/>", true, true, &settings);
        assert!(svg.contains("class='dark'") && svg.contains("id='svgstage'") && svg.contains("tellHost"));
        let md = build_viewer_html(ViewMode::Md, "# Заголовок <b>", true, false, &settings);
        assert!(md.contains("&lt;b&gt;") && md.contains("@font-face{font-family:'JetBrains Mono'"));
        assert!(md.contains("window.chrome.webview.postMessage"));
        let json = build_viewer_html(ViewMode::Json, "{\"a\":1}", false, false, &settings);
        assert!(json.contains("language-json") && json.contains("&quot;a&quot;: 1"));
        let html = build_viewer_html(ViewMode::Html, "<p>x</p>", true, false, &settings);
        assert!(html.contains("background:#ffffff"));
        let diff = build_diff_html("t", "@@ -1 +1 @@\n-a\n+b\n", true, &settings);
        assert!(diff.contains("class='del'>-a") && diff.contains("class='add'>+b"));
        assert!(build_diff_html("t", "\n\n", true, &settings).contains("Изменений нет"));
    }
}
