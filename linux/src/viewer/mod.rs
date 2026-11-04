//! Просмотр содержимого в формате: SVG, подсветка JSON / YAML / Bash / nginx,
//! рендер Markdown и AsciiDoc, HTML как страница, дельты git. Страницы строит
//! `html.rs` — порт `MainWindow.Viewer.cs`; показывает их WebKitGTK (`ui/viewer.rs`).

pub mod html;
pub mod scripts;

/// Режим показа файла. «Как есть» — такой же режим показа, но живёт в настройках
/// (`AppSettings::raw_editing`), а не здесь: он переключается с форматами взаимно.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Text,
    Svg,
    Json,
    Yaml,
    Bash,
    Md,
    Adoc,
    Html,
    Nginx,
}

impl ViewMode {
    /// Кнопки форматов в порядке показа: MD стоит первым — из отрисовываемых форматов
    /// он нужен чаще прочих. (подпись, подсказка)
    pub const FORMATS: [(ViewMode, &'static str, &'static str); 8] = [
        (ViewMode::Md, "MD", "Показать как отрендеренный Markdown"),
        (ViewMode::Svg, "SVG", "Показать содержимое как SVG-рисунок"),
        (ViewMode::Json, "JSON", "Показать с подсветкой синтаксиса JSON"),
        (ViewMode::Yaml, "YAML", "Показать с подсветкой синтаксиса YAML"),
        (ViewMode::Bash, "Bash", "Показать с подсветкой синтаксиса Bash"),
        (ViewMode::Adoc, "AsciiDoc", "Показать как отрендеренный AsciiDoc"),
        (ViewMode::Html, "HTML", "Показать как HTML-страницу"),
        (ViewMode::Nginx, "nginx", "Показать как конфигурацию nginx"),
    ];

    /// Формат файла по расширению — чтобы файл в собственном формате открывался сразу
    /// в подходящем виде. У root и файлов _content расширения нет, для них это Text.
    pub fn detect(id: &str) -> Self {
        let name = id.rsplit('/').next().unwrap_or(id);
        let Some(dot) = name.rfind('.') else { return Self::Text };
        match name[dot..].to_ascii_lowercase().as_str() {
            ".svg" => Self::Svg,
            ".json" => Self::Json,
            ".yaml" | ".yml" => Self::Yaml,
            ".sh" | ".bash" => Self::Bash,
            ".md" | ".markdown" => Self::Md,
            ".adoc" | ".asciidoc" => Self::Adoc,
            ".html" | ".htm" => Self::Html,
            ".conf" | ".nginx" => Self::Nginx,
            _ => Self::Text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mode_by_extension() {
        assert_eq!(ViewMode::detect("contract/auth/start.yaml"), ViewMode::Yaml);
        assert_eq!(ViewMode::detect("README.MD"), ViewMode::Md);
        assert_eq!(ViewMode::detect("a.b/c"), ViewMode::Text);
        assert_eq!(ViewMode::detect("root"), ViewMode::Text);
        assert_eq!(ViewMode::detect("site/nginx.conf"), ViewMode::Nginx);
    }
}
