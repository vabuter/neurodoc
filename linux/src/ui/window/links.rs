//! Действия со ссылками и файлами: «Новая ссылка», «Связать», снять ссылку, переход
//! по ссылке под кареткой, новый файл, копирование, удаление, режим «как есть».

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::Win;
use crate::domain::catalog::{self, CONTENT_DIR, ROOT_ID};
use crate::domain::links::{check_anchor, parse_clipboard_target, sanitize_anchor, trim_range, word_at};
use crate::ui::util::format_count;

impl Win {
    /// Вставляет новую ссылку в позицию каретки, создаёт файл и переходит к нему.
    pub fn insert_new_link(self: &Rc<Self>) {
        if self.state.borrow().catalog.is_none() || self.state.borrow().current_id.is_none() {
            return;
        }
        let (from, to) = self.editor.selection();
        let (s, e) = (self.editor.iter_at_offset(from), self.editor.iter_at_offset(to));
        // Вложенных ссылок в формате нет: вставка внутрь существующей порвала бы обе.
        // Каретка ровно на краю ссылки — это уже снаружи, там вставлять можно.
        if self.editor.range_overlaps_link(&s, &e) || (from == to && self.editor.link_at(&s).is_some_and(|l| l.span.start < s.line_index() as usize && (s.line_index() as usize) < l.span.end)) {
            self.show_tip("new_link", "Курсор внутри ссылки", "Вложенные ссылки не поддерживаются");
            return;
        }
        let text = self.editor.raw_text();
        let mut from_b = self.editor.byte_of(&text, &s);
        let mut to_b = self.editor.byte_of(&text, &e);
        let anchor;
        if to_b > from_b {
            (from_b, to_b) = trim_range(&text, from_b, to_b);
            anchor = text[from_b..to_b].to_string();
            if let Err(problem) = check_anchor(&anchor) {
                self.show_tip("new_link", problem.title(), problem.subtitle());
                return;
            }
        } else {
            // текстом ссылки становится слово под кареткой; слово, задевающее соседнюю
            // ссылку, оборачивать нельзя — порвём её
            let (ws, we) = word_at(&text, from_b);
            let word_ok = we > ws
                && match (self.editor.iter_at_byte(&text, ws), self.editor.iter_at_byte(&text, we)) {
                    (Some(a), Some(b)) => !self.editor.range_overlaps_link(&a, &b),
                    _ => false,
                };
            if word_ok {
                from_b = ws;
                to_b = we;
                anchor = text[ws..we].to_string();
            } else {
                anchor = String::new();
            }
        }
        let id = {
            let mut st = self.state.borrow_mut();
            let Some(c) = st.catalog.as_mut() else { return };
            match c.create_new_file() {
                Ok(id) => {
                    if !anchor.is_empty() {
                        let _ = c.write_file(&id, &format!("{anchor}\n"));
                    }
                    id
                }
                Err(e) => {
                    drop(st);
                    self.show_error("Не удалось создать файл", &e.to_string());
                    return;
                }
            }
        };
        if let (Some(a), Some(b)) = (self.editor.iter_at_byte(&text, from_b), self.editor.iter_at_byte(&text, to_b)) {
            self.editor.replace_range(&a, &b, &format!("[{anchor}]({id})"));
        }
        self.save_current_file();
        self.open_file(&id, true);
    }

    /// Связывает выделенный текст со ссылкой из буфера: выделение становится текстом
    /// ссылки, цель берётся из буфера. Путь пересчитывается от ТЕКУЩЕГО файла.
    pub fn link_selection(self: &Rc<Self>) {
        let current = {
            let st = self.state.borrow();
            match (&st.catalog, &st.current_id) {
                (Some(_), Some(id)) => id.clone(),
                _ => return,
            }
        };
        if !catalog::is_neurodoc_file(&current) {
            self.show_tip("link_selection", "Здесь ссылок не бывает", &format!("Ссылки читаются только из root и файлов {CONTENT_DIR}"));
            return;
        }
        let (mut from, mut to) = self.editor.selection();
        if to <= from {
            (from, to) = self.editor.last_selection.get(); // фокус мог уйти на кнопку
        }
        let max = self.editor.buffer.char_count();
        from = from.clamp(0, max);
        to = to.clamp(0, max);
        if to <= from {
            self.show_tip("link_selection", "Ничего не выделено", "Выделите текст, который станет текстом ссылки");
            return;
        }
        let (s, e) = (self.editor.iter_at_offset(from), self.editor.iter_at_offset(to));
        if self.editor.range_overlaps_link(&s, &e) {
            self.show_tip("link_selection", "Выделение задевает ссылку", "Вложенных ссылок не бывает");
            return;
        }
        let text = self.editor.raw_text();
        let (from_b, to_b) = trim_range(&text, self.editor.byte_of(&text, &s), self.editor.byte_of(&text, &e));
        let anchor = if from_b < to_b { text[from_b..to_b].to_string() } else { String::new() };
        if let Err(problem) = check_anchor(&anchor) {
            self.show_tip("link_selection", problem.title(), problem.subtitle());
            return;
        }
        let w = self.clone();
        glib::spawn_future_local(async move {
            let raw = w.window.clipboard().read_text_future().await.ok().flatten().map(|s| s.to_string()).unwrap_or_default();
            let target_id = {
                let st = w.state.borrow();
                st.catalog.as_ref().and_then(|c| parse_clipboard_target(&raw, c))
            };
            let Some(target_id) = target_id else {
                w.show_tip("link_selection", "В буфере нет ссылки на файл", "Скопируйте ссылку кнопкой «Копировать ссылку»");
                return;
            };
            let target = catalog::make_relative_target(&catalog::dir_of(&current), &target_id);
            let text = w.editor.raw_text();
            if let (Some(a), Some(b)) = (w.editor.iter_at_byte(&text, from_b), w.editor.iter_at_byte(&text, to_b)) {
                w.editor.replace_range(&a, &b, &format!("[{anchor}]({target})"));
            }
            w.save_current_file();
            let title = w.target_tip(&target_id);
            w.show_tip("link_selection", "Связано", &title);
        });
    }

    /// Снимает ссылку под кареткой, оставляя её текст: [текст](цель) превращается в текст.
    ///
    /// Иначе ссылку не убрать: в компактном режиме её служебная часть скрыта, руками до неё
    /// не добраться. Голый http-адрес не трогаем — снимать там нечего, это просто текст.
    pub fn remove_link_at_caret(self: &Rc<Self>) {
        if self.state.borrow().catalog.is_none() {
            return;
        }
        let caret = self.editor.iter_at_offset(self.editor.caret());
        if let Some(link) = self.editor.link_at(&caret)
            && let Some((a, b)) = self.editor.link_iters(&link) {
                // ссылка записана как [текст](цель) — у голого адреса снимать нечего
                let text = self.editor.raw_text();
                let written = text.as_bytes().get(self.editor.byte_of(&text, &a)) == Some(&b'[');
                if written {
                    let anchor = link.span.anchor.clone();
                    let start = a.offset();
                    self.editor.replace_range(&a, &b, &anchor);
                    self.editor.place_cursor(&self.editor.iter_at_offset(start + anchor.chars().count() as i32));
                    return;
                }
            }
        self.show_tip("new_link", "Курсор не на ссылке", "Поставьте каретку внутрь ссылки и повторите");
    }

    /// Переход по ссылке, в границах которой (включая края) находится каретка.
    pub fn navigate_link_at_caret(self: &Rc<Self>) {
        if self.state.borrow().catalog.is_none() {
            return;
        }
        let caret = self.editor.iter_at_offset(self.editor.caret());
        if let Some(link) = self.editor.link_at(&caret) {
            if link.span.is_web {
                self.launch_web_link(&link.span.target);
            } else {
                self.open_file(&link.span.target, true);
            }
        }
    }

    pub fn create_unlinked_file(self: &Rc<Self>) {
        let id = {
            let mut st = self.state.borrow_mut();
            let Some(c) = st.catalog.as_mut() else { return };
            match c.create_new_file() {
                Ok(id) => id,
                Err(e) => {
                    drop(st);
                    self.show_error("Не удалось создать файл", &e.to_string());
                    return;
                }
            }
        };
        self.open_file(&id, true);
    }

    pub fn copy_link(self: &Rc<Self>) {
        let (id, content, title) = {
            let st = self.state.borrow();
            let (Some(c), Some(id)) = (&st.catalog, &st.current_id) else { return };
            if id == ROOT_ID {
                return;
            }
            (id.clone(), c.read_file(id).into_owned(), c.title(id, 60))
        };
        // якорем берём заголовок файла; у пустого файла заголовка нет — оставляем пустой
        let anchor = if content.trim().is_empty() { String::new() } else { sanitize_anchor(&title) };
        // путь считается от файла, куда ссылку вставят; берём вид для _content
        let target = catalog::make_relative_target(CONTENT_DIR, &id);
        let link = format!("[{anchor}]({target})");
        self.window.clipboard().set_text(&link);
        self.show_tip("copy_link", "Ссылка скопирована", &link);
    }

    pub fn copy_content(self: &Rc<Self>) {
        if self.state.borrow().current_id.is_none() {
            return;
        }
        let text = self.editor_text_for_save();
        self.window.clipboard().set_text(&text);
        self.show_tip("copy_content", "Содержимое скопировано", &format!("{} символов", format_count(text.chars().count())));
    }

    pub fn delete_current_file(self: &Rc<Self>) {
        let (id, title) = {
            let st = self.state.borrow();
            let (Some(c), Some(id)) = (&st.catalog, &st.current_id) else { return };
            if id == ROOT_ID {
                return;
            }
            (id.clone(), c.title(id, 40))
        };
        let w = self.clone();
        glib::spawn_future_local(async move {
            let body = format!(
                "Файл «{title}» будет удалён безвозвратно.\n\n\
                 «Только файл» — ссылки на него в других файлах останутся, но перестанут работать.\n\
                 «Файл и ссылки» — ссылки на него будут сняты, а их текст останется обычным текстом."
            );
            let answer = w
                .ask(
                    "Удалить файл?",
                    &body,
                    &[("cancel", "Отмена", false), ("file", "Только файл", false), ("links", "Файл и ссылки", false)],
                    "cancel",
                    "cancel",
                )
                .await;
            if answer != "file" && answer != "links" {
                return;
            }
            w.save_timer.stop();
            let result = {
                let mut st = w.state.borrow_mut();
                st.current_id = None;
                st.last_saved_text.clear();
                let Some(c) = st.catalog.as_mut() else { return };
                let r = c.delete_file(&id);
                if r.is_ok() && answer == "links" {
                    let _ = c.remove_links_to(&id);
                }
                st.view_states.remove(&id.to_lowercase());
                r
            };
            if let Err(e) = result {
                w.show_error("Не удалось удалить файл", &e.to_string());
            }
            w.remove_from_history(&id);
            w.open_file(ROOT_ID, false);
        });
    }

    // ================= Буфер обмена и «Связать» =================

    /// Перечитывает буфер обмена: лежит ли там ссылка на файл каталога.
    pub fn refresh_clipboard_state(self: &Rc<Self>) {
        let w = self.clone();
        self.window.clipboard().read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
            let raw = result.ok().flatten().map(|s| s.to_string()).unwrap_or_default();
            let has = {
                let st = w.state.borrow();
                st.catalog.as_ref().is_some_and(|c| parse_clipboard_target(&raw, c).is_some())
            };
            w.state.borrow_mut().clipboard_has_link = has;
            w.update_link_selection_action();
        });
    }

    /// «Связать» работает, только когда есть что связывать: выделенный текст в
    /// neurodoc-файле и ссылка в буфере обмена.
    pub fn update_link_selection_action(&self) {
        let st = self.state.borrow();
        let in_neurodoc = st.catalog.is_some() && st.current_id.as_deref().is_some_and(catalog::is_neurodoc_file);
        let (from, to) = self.editor.selection();
        self.actions.link_selection.set_enabled(in_neurodoc && to != from && st.clipboard_has_link);
    }

    // ================= Режим «как есть» =================

    pub(super) fn on_raw_toggle(self: &Rc<Self>, active: bool) {
        if self.suppress_toggles.get() {
            return;
        }
        {
            let mut st = self.state.borrow_mut();
            st.settings.raw_editing = active;
        }
        self.save_settings();
        // «Как есть» — такой же режим показа, как SVG или MD: включая его, выходим
        // из отрисованного формата
        let mode = self.state.borrow().view_mode;
        if active && mode != crate::viewer::ViewMode::Text {
            self.set_view_mode(crate::viewer::ViewMode::Text);
        }
        self.apply_raw_editing();
    }

    pub fn apply_raw_editing(&self) {
        let raw = self.state.borrow().settings.raw_editing;
        self.editor.set_raw(raw);
    }

    pub fn sync_raw_toggle(&self) {
        let raw = self.state.borrow().settings.raw_editing;
        self.suppress_toggles.set(true);
        self.raw_toggle.set_active(raw);
        self.suppress_toggles.set(false);
        self.editor.set_raw(raw);
    }
}
