//! Поиск по текущему файлу (панель Ctrl+F) и «Поиск и замена» (Ctrl+R).

use std::rc::Rc;

use adw::prelude::*;

use super::Win;
use crate::domain::text;

impl Win {
    pub fn set_find_text(&self, text: &str) {
        self.find_entry.set_text(text);
    }

    pub fn find_bar_visible(&self) -> bool {
        self.find_bar.is_visible()
    }

    pub fn show_find_bar(&self) {
        if self.state.borrow().current_id.is_none() {
            return;
        }
        self.reset_view_mode();
        self.find_bar.set_visible(true);
        self.find_entry.grab_focus();
        self.find_entry.select_region(0, -1);
        self.update_find_count(None);
    }

    pub fn close_find_bar(&self) {
        self.find_bar.set_visible(false);
        self.state.borrow_mut().find_matches.clear();
        self.state.borrow_mut().find_current = None;
        self.editor.clear_find_matches();
        self.editor.grab_focus();
    }

    /// Поиск в текущем файле: 1 — вперёд, -1 — назад, 0 — от текущей позиции включительно.
    pub fn find_step(&self, direction: i32) {
        let query = self.find_entry.text().to_string();
        if query.is_empty() {
            self.find_count.set_text("");
            self.update_find_count(None);
            return;
        }
        let text = self.editor.raw_text();
        let (sel_start, sel_end) = self.editor.selection();
        let start_b = self.editor.byte_of(&text, &self.editor.iter_at_offset(sel_start));
        let end_b = self.editor.byte_of(&text, &self.editor.iter_at_offset(sel_end));
        let index = if direction >= 0 {
            let from = if direction > 0 { end_b } else { start_b };
            text::find_ci(&text, &query, from).or_else(|| text::find_ci(&text, &query, 0))
        } else {
            text::rfind_ci(&text, &query, start_b).or_else(|| text::rfind_ci(&text, &query, text.len()))
        };
        let Some(index) = index else {
            self.find_count.set_text("0");
            self.update_find_count(None);
            return;
        };
        let end = text::find_all_ci(&text, &query).into_iter().find(|(s, _)| *s == index).map(|(_, e)| e).unwrap_or(index);
        if let (Some(a), Some(b)) = (self.editor.iter_at_byte(&text, index), self.editor.iter_at_byte(&text, end)) {
            self.editor.select_range(&a, &b);
            self.editor.scroll_to_caret();
        }
        self.update_find_count(Some(index));
    }

    pub fn update_find_count(&self, current: Option<usize>) {
        let query = self.find_entry.text().to_string();
        let text = self.editor.raw_text();
        let matches = if query.is_empty() { Vec::new() } else { text::find_all_ci(&text, &query) };
        let total = matches.len();
        let current_n = current.and_then(|c| matches.iter().position(|(s, _)| *s == c)).map(|i| i + 1);
        self.find_count.set_text(&match (query.is_empty(), current_n) {
            (true, _) => String::new(),
            (false, Some(n)) => format!("{n} из {total}"),
            (false, None) => total.to_string(),
        });
        if self.find_bar.is_visible() {
            self.editor.set_find_matches(&text, &matches, current);
        } else {
            self.editor.clear_find_matches();
        }
        let mut st = self.state.borrow_mut();
        st.find_matches = matches;
        st.find_current = current;
    }

    // ================= Поиск и замена =================

    /// Заменяет одно вхождение (от каретки, с переходом по кругу). true — заменено.
    pub fn replace_one_in_current_file(self: &Rc<Self>, query: &str, replacement: &str) -> bool {
        if self.state.borrow().current_id.is_none() || query.is_empty() {
            return false;
        }
        let text = self.editor.raw_text();
        let (sel_start, _) = self.editor.selection();
        let from = self.editor.byte_of(&text, &self.editor.iter_at_offset(sel_start));
        let Some(index) = text::find_ci(&text, query, from).or_else(|| text::find_ci(&text, query, 0)) else { return false };
        let end = text::find_all_ci(&text, query).into_iter().find(|(s, _)| *s == index).map(|(_, e)| e).unwrap_or(index);
        let (Some(a), Some(b)) = (self.editor.iter_at_byte(&text, index), self.editor.iter_at_byte(&text, end)) else { return false };
        let start = a.offset();
        self.editor.replace_range(&a, &b, replacement);
        let s = self.editor.iter_at_offset(start);
        let e = self.editor.iter_at_offset(start + replacement.chars().count() as i32);
        self.editor.select_range(&s, &e);
        self.editor.scroll_to_caret();
        self.save_current_file();
        true
    }

    /// Заменяет все вхождения в текущем файле одним шагом undo. Возвращает число замен.
    pub fn replace_all_in_current_file(self: &Rc<Self>, query: &str, replacement: &str) -> usize {
        if self.state.borrow().current_id.is_none() || query.is_empty() {
            return 0;
        }
        let text = self.editor.raw_text();
        let matches = text::find_all_ci(&text, query);
        if matches.is_empty() {
            return 0;
        }
        self.editor.buffer.begin_user_action();
        for (s, e) in matches.iter().rev() {
            if let (Some(a), Some(b)) = (self.editor.iter_at_byte(&text, *s), self.editor.iter_at_byte(&text, *e)) {
                let mut a = a;
                let mut b = b;
                self.editor.buffer.delete(&mut a, &mut b);
                self.editor.buffer.insert(&mut a, replacement);
            }
        }
        self.editor.buffer.end_user_action();
        self.editor.place_cursor(&self.editor.buffer.start_iter());
        self.save_current_file();
        matches.len()
    }

    pub fn replace_one_in_project(self: &Rc<Self>, query: &str, replacement: &str) -> bool {
        if query.is_empty() || self.state.borrow().catalog.is_none() {
            return false;
        }
        if self.replace_one_in_current_file(query, replacement) {
            return true;
        }
        let candidates: Vec<String> = {
            let st = self.state.borrow();
            let c = st.catalog.as_ref().unwrap();
            c.all_ids()
                .into_iter()
                .filter(|id| !st.current_id.as_deref().is_some_and(|cur| cur.eq_ignore_ascii_case(id)))
                .filter(|id| text::find_ci(&c.read_file(id), query, 0).is_some())
                .collect()
        };
        let Some(id) = candidates.into_iter().next() else { return false };
        self.open_file(&id, true);
        self.replace_one_in_current_file(query, replacement)
    }

    pub fn replace_all_in_project(self: &Rc<Self>, query: &str, replacement: &str) -> usize {
        if query.is_empty() || self.state.borrow().catalog.is_none() {
            return 0;
        }
        let mut total = 0;
        let ids: Vec<String> = self.state.borrow().catalog.as_ref().unwrap().all_ids();
        for id in ids {
            let is_current = self.state.borrow().current_id.as_deref().is_some_and(|cur| cur.eq_ignore_ascii_case(&id));
            if is_current {
                total += self.replace_all_in_current_file(query, replacement);
                continue;
            }
            let mut st = self.state.borrow_mut();
            let c = st.catalog.as_mut().unwrap();
            let content = c.read_file(&id).into_owned();
            let (replaced, count) = text::replace_all_ci(&content, query, replacement);
            if count == 0 {
                continue;
            }
            let _ = c.write_file(&id, &replaced);
            total += count;
        }
        self.update_panels();
        total
    }
}
