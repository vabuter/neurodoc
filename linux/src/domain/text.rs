//! Поиск подстроки без учёта регистра — общий для поиска по проекту, поиска
//! в файле и замены. Все позиции — байтовые смещения в UTF-8 (так их понимает
//! и GtkTextBuffer через `iter_at_line_index`), в отличие от UTF-16 в C#.

use regex::{Regex, RegexBuilder};

/// Регулярное выражение «эта строка буквально, регистр не важен» — аналог
/// `IndexOf(query, OrdinalIgnoreCase)` в C#. None — запрос так велик, что выражение
/// не компилируется (сотни тысяч символов из буфера обмена); для поиска это «не найдено».
pub fn ci_regex(query: &str) -> Option<Regex> {
    if query.is_empty() {
        return None;
    }
    RegexBuilder::new(&regex::escape(query)).case_insensitive(true).build().ok()
}

/// Первое вхождение, начинающееся не раньше `from` (байтовая граница символа).
pub fn find_ci(text: &str, query: &str, from: usize) -> Option<usize> {
    if from > text.len() || !text.is_char_boundary(from) {
        return None;
    }
    ci_regex(query)?.find_at(text, from).map(|m| m.start())
}

/// Последнее вхождение, целиком лежащее до `end_limit` (аналог `LastIndexOf`).
pub fn rfind_ci(text: &str, query: &str, end_limit: usize) -> Option<usize> {
    ci_regex(query)?.find_iter(text).take_while(|m| m.end() <= end_limit).last().map(|m| m.start())
}

/// Все непересекающиеся вхождения: (начало, конец).
pub fn find_all_ci(text: &str, query: &str) -> Vec<(usize, usize)> {
    ci_regex(query).map(|re| re.find_iter(text).map(|m| (m.start(), m.end())).collect()).unwrap_or_default()
}

/// Начало n-го (с нуля) вхождения.
pub fn nth_occurrence(text: &str, query: &str, n: usize) -> Option<usize> {
    ci_regex(query)?.find_iter(text).nth(n).map(|m| m.start())
}

/// Заменяет все вхождения; строка замены вставляется буквально, без `$`-подстановок.
/// Возвращает новый текст и число замен.
pub fn replace_all_ci(text: &str, query: &str, replacement: &str) -> (String, usize) {
    let Some(re) = ci_regex(query) else { return (text.to_string(), 0) };
    let count = re.find_iter(text).count();
    if count == 0 {
        return (text.to_string(), 0);
    }
    (re.replace_all(text, regex::NoExpand(replacement)).into_owned(), count)
}

/// Начало строки, в которой лежит позиция (байт после ближайшего перевода строки).
pub fn line_start_at(text: &str, pos: usize) -> usize {
    text[..pos].rfind(['\r', '\n']).map(|i| i + 1).unwrap_or(0)
}

/// Конец строки (позиция перевода строки или конец текста), начиная с позиции.
pub fn line_end_at(text: &str, pos: usize) -> usize {
    text[pos..].find(['\r', '\n']).map(|i| pos + i).unwrap_or(text.len())
}

/// Отступает от `from` назад не более чем на `n` символов, но не раньше `min`.
pub fn back_chars(text: &str, from: usize, n: usize, min: usize) -> usize {
    let mut pos = from;
    for (count, (i, _)) in text[min..from].char_indices().rev().enumerate() {
        pos = min + i;
        if count + 1 == n {
            break;
        }
    }
    pos
}

/// Продвигается от `from` вперёд не более чем на `n` символов, но не дальше `max`.
pub fn fwd_chars(text: &str, from: usize, n: usize, max: usize) -> usize {
    let mut pos = from;
    for (count, (i, c)) in text[from..max].char_indices().enumerate() {
        pos = from + i + c.len_utf8();
        if count + 1 == n {
            break;
        }
    }
    pos
}

/// Обрезает текст до `max` символов, добавляя многоточие (как в заголовках панелей).
pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_is_case_insensitive_and_unicode() {
        let text = "Привет, МИР! мир";
        assert_eq!(find_ci(text, "мир", 0), Some("Привет, ".len()));
        assert_eq!(find_ci(text, "мир", "Привет, М".len()), Some("Привет, МИР! ".len()));
        assert_eq!(find_ci(text, "мир", "Привет, ".len() + 1), None, "смещение внутри символа — не ищем");
        assert_eq!(rfind_ci(text, "мир", text.len()), Some("Привет, МИР! ".len()));
        assert_eq!(rfind_ci(text, "мир", "Привет, МИР! ".len()), Some("Привет, ".len()));
        assert_eq!(find_all_ci(text, "МиР").len(), 2);
        assert_eq!(nth_occurrence(text, "мир", 1), Some("Привет, МИР! ".len()));
    }

    #[test]
    fn replace_is_literal() {
        let (out, n) = replace_all_ci("a A b", "a", "$1");
        assert_eq!(out, "$1 $1 b");
        assert_eq!(n, 2);
    }

    #[test]
    fn char_stepping_respects_bounds() {
        let text = "αβγδε";
        assert_eq!(back_chars(text, text.len(), 2, 0), "αβγ".len());
        assert_eq!(back_chars(text, text.len(), 10, "αβ".len()), "αβ".len());
        assert_eq!(fwd_chars(text, 0, 2, text.len()), "αβ".len());
        assert_eq!(fwd_chars(text, 0, 10, "αβγ".len()), "αβγ".len());
        assert_eq!(truncate_chars("абвгд", 3), "абв…");
    }
}
