//! Ссылки в тексте открытого файла — то, что в Windows-версии делает `RecomputeLinks`
//! и помощники вокруг «Новой ссылки» / «Связать». Чистые функции: редактор отдаёт
//! текст и позиции (байтовые), получает диапазоны обратно.

use std::sync::LazyLock;

use regex::Regex;

use super::catalog::{self, NeurodocCatalog, CONTENT_DIR, LINK_REGEX};

/// HTTP/HTTPS-ссылка в тексте.
static WEB_LINK_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s<>"]+"#).unwrap());

/// Ссылка, найденная в тексте редактора. `start..end` — байтовый диапазон всей записи
/// `[якорь](цель)` (у голого адреса — самого адреса).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSpan {
    pub start: usize,
    pub end: usize,
    /// Для neurodoc-ссылки — уже разрешённый id файла, для веб-ссылки — адрес.
    pub target: String,
    pub is_web: bool,
    pub anchor: String,
}

impl LinkSpan {
    /// Позиция символа "]" в ссылке [текст](uuid).
    pub fn anchor_end(&self) -> usize {
        self.start + 1 + self.anchor.len()
    }

    /// Занимает ли ссылка позицию каретки, включая края.
    pub fn contains_caret(&self, caret: usize) -> bool {
        caret >= self.start && caret <= self.end
    }

    /// Пересекается ли диапазон с ссылкой (края не в счёт).
    pub fn overlaps(&self, from: usize, to: usize) -> bool {
        from < self.end && to > self.start
    }
}

/// Все ссылки текста: neurodoc-ссылки с разрешёнными целями и веб-адреса.
/// Цель ссылки считается ОТ ФАЙЛА, в котором она написана.
pub fn scan_links(text: &str, source_id: &str) -> Vec<LinkSpan> {
    let mut links: Vec<LinkSpan> = Vec::new();
    for m in LINK_REGEX.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let raw = m.get(2).map(|g| g.as_str()).unwrap_or("");
        let anchor = m.get(1).map(|g| g.as_str()).unwrap_or("").to_string();
        if catalog::is_web_target(raw) {
            links.push(LinkSpan { start: whole.start(), end: whole.end(), target: raw.to_string(), is_web: true, anchor });
        } else if let Some(target) = catalog::try_resolve_link(source_id, raw) {
            links.push(LinkSpan { start: whole.start(), end: whole.end(), target, is_web: false, anchor });
        }
        // не разобралась — ведёт за пределы каталога; подсветки нет, о ней скажет «Битые ссылки»
    }
    for m in WEB_LINK_REGEX.find_iter(text) {
        // адрес внутри [текста](…) уже учтён выше — вторая подсветка поверх была бы лишней
        if links.iter().any(|l| m.start() >= l.start && m.start() < l.end) {
            continue;
        }
        // завершающая пунктуация предложения — не часть адреса
        let url = m.as_str().trim_end_matches(|c| ".,;:!?)]'\"".contains(c));
        let Some(scheme_end) = url.find("://") else { continue };
        if scheme_end + 3 >= url.len() {
            continue;
        }
        links.push(LinkSpan {
            start: m.start(),
            end: m.start() + url.len(),
            target: url.to_string(),
            is_web: true,
            anchor: String::new(),
        });
    }
    links.sort_by_key(|l| l.start);
    links
}

pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Границы слова, на котором стоит каретка. Каретка у края слова тоже считается:
/// пользователь целится в слово, а не в конкретный промежуток между символами.
/// Пустой диапазон (start == end), если под кареткой не слово.
pub fn word_at(text: &str, position: usize) -> (usize, usize) {
    let caret = position.min(text.len());
    let mut start = caret;
    while let Some(c) = text[..start].chars().next_back() {
        if !is_word_char(c) {
            break;
        }
        start -= c.len_utf8();
    }
    let mut end = caret;
    while let Some(c) = text[end..].chars().next() {
        if !is_word_char(c) {
            break;
        }
        end += c.len_utf8();
    }
    (start, end)
}

/// Приводит текст к пригодному для якоря виду: он однострочный и не содержит "]",
/// иначе ссылка перестанет разбираться.
pub fn sanitize_anchor(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    text.replace(['\r', '\n'], " ").replace(']', "").trim().to_string()
}

/// Сдвигает границы выделения внутрь, отбрасывая пробелы по краям. Именно границы,
/// а не текст: обрезав текст, но заменив весь диапазон, мы бы съели эти пробелы.
pub fn trim_range(text: &str, mut from: usize, mut to: usize) -> (usize, usize) {
    while from < to {
        match text[from..to].chars().next() {
            Some(c) if c.is_whitespace() => from += c.len_utf8(),
            _ => break,
        }
    }
    while to > from {
        match text[from..to].chars().next_back() {
            Some(c) if c.is_whitespace() => to -= c.len_utf8(),
            _ => break,
        }
    }
    (from, to)
}

/// Почему выделенный текст не годится в текст ссылки.
///
/// Непригодное выделение намеренно НЕ «чинится» молча: вычистить перевод строки
/// значило бы склеить абзацы файла, а "]" — удалить символ из документа. Обе правки
/// пользователь не заказывал и не увидел бы, пока не открыл файл чем-то ещё.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorProblem {
    MultiLine,
    Bracket,
    Empty,
}

impl AnchorProblem {
    pub fn title(self) -> &'static str {
        match self {
            Self::MultiLine => "Выделено несколько строк",
            Self::Bracket => "В выделении есть «]»",
            Self::Empty => "Выделены только пробелы",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::MultiLine => "Текст ссылки должен быть в одной строке",
            Self::Bracket => "Этот символ недопустим в тексте ссылки",
            Self::Empty => "Выделите текст, который станет текстом ссылки",
        }
    }
}

/// Годится ли текст в якорь ссылки. Проверка идёт по настоящим разделителям строк;
/// перенос по ширине окна — только раскладка, в тексте его нет.
pub fn check_anchor(anchor: &str) -> Result<(), AnchorProblem> {
    if anchor.contains(['\r', '\n']) {
        return Err(AnchorProblem::MultiLine);
    }
    if anchor.contains(']') {
        return Err(AnchorProblem::Bracket);
    }
    if anchor.is_empty() {
        return Err(AnchorProblem::Empty);
    }
    Ok(())
}

/// Достаёт из текста буфера обмена цель ссылки и приводит её к id файла каталога.
/// Принимает и «[текст](цель)», и голую цель — UUID или путь.
///
/// Ссылкой считается только то, что ведёт на СУЩЕСТВУЮЩИЙ файл каталога. Иначе за путь
/// сходит любой текст: `try_resolve_link` отвергает лишь веб-адреса, абсолютные пути и
/// выход за корень, поэтому обычная скопированная фраза проходила как относительный
/// путь и уезжала в файл ссылкой (проверено на практике).
pub fn parse_clipboard_target(raw: &str, catalog: &NeurodocCatalog) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().count() > 500 {
        return None;
    }
    let target = match LINK_REGEX.captures(raw) {
        Some(m) => m.get(2).map(|g| g.as_str()).unwrap_or(""),
        None => raw,
    };
    // в цели не бывает пробелов — сразу отсекаем произвольный текст
    if target.is_empty() || target.chars().any(char::is_whitespace) {
        return None;
    }
    // «Скопировать ссылку» кладёт путь в виде для _content — оттуда и разбираем
    let id = catalog::try_resolve_link(&format!("{CONTENT_DIR}/x"), target)?;
    if catalog.exists(&id) { Some(id) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_neurodoc_and_web_links() {
        let uuid = "5c673c53-8ad9-49eb-9fb6-ffb3954848f0";
        let text = format!("см. [подход]({uuid}) и [сайт](https://a.b/c), а также https://x.y/z. [наружу](../../x)");
        let links = scan_links(&text, "root");
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, uuid);
        assert_eq!(links[0].anchor, "подход");
        assert!(!links[0].is_web);
        assert_eq!(links[0].anchor_end(), links[0].start + 1 + "подход".len());
        assert!(links[1].is_web);
        assert_eq!(links[1].target, "https://a.b/c");
        assert_eq!(links[2].target, "https://x.y/z");
        assert_eq!(&text[links[2].start..links[2].end], "https://x.y/z");
    }

    #[test]
    fn word_and_trim_helpers_are_char_aware() {
        let text = "первое слово_два, три";
        let pos = "первое сл".len();
        assert_eq!(word_at(text, pos), ("первое ".len(), "первое слово_два".len()));
        assert_eq!(word_at(text, "первое слово_два".len()), ("первое ".len(), "первое слово_два".len()));
        assert_eq!(word_at(text, "первое слово_два,".len()), ("первое слово_два,".len(), "первое слово_два,".len()));
        let padded = "  аб  ";
        assert_eq!(trim_range(padded, 0, padded.len()), (2, 2 + "аб".len()));
        assert_eq!(sanitize_anchor(" a\nb] "), "a b");
        assert_eq!(check_anchor("a\nb"), Err(AnchorProblem::MultiLine));
        assert_eq!(check_anchor("a]b"), Err(AnchorProblem::Bracket));
        assert_eq!(check_anchor(""), Err(AnchorProblem::Empty));
        assert_eq!(check_anchor("ok"), Ok(()));
    }
}
