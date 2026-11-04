//! Панели: хлебные крошки, обратные ссылки, сироты, битые ссылки, результаты
//! поиска по проекту, прогресс чтения.

use std::rc::Rc;

use adw::prelude::*;

use super::{Win, clear_list, list_row};
use crate::domain::catalog::{self, ROOT_ID};
use crate::domain::text;

impl Win {
    pub(super) fn wire_lists(self: &Rc<Self>) {
        let w = self.clone();
        self.search_list.connect_row_activated(move |_, row| {
            let hit = w.state.borrow().search_rows.get(row.index() as usize).cloned();
            let query = w.state.borrow().last_query.clone();
            if let Some((id, occurrence)) = hit {
                w.open_file(&id, true);
                w.select_occurrence(&query, occurrence);
            }
        });
        let w = self.clone();
        self.backlinks_list.connect_row_activated(move |_, row| {
            let id = w.state.borrow().backlink_rows.get(row.index() as usize).cloned();
            if let Some(id) = id {
                w.open_file(&id, true);
            }
        });
        let w = self.clone();
        self.orphans_list.connect_row_activated(move |_, row| {
            let id = w.state.borrow().orphan_rows.get(row.index() as usize).cloned();
            if let Some(id) = id {
                w.open_file(&id, true);
            }
        });
        let w = self.clone();
        self.broken_list.connect_row_activated(move |_, row| {
            let hit = w.state.borrow().broken_rows.get(row.index() as usize).cloned();
            if let Some((id, position, len)) = hit {
                w.open_file(&id, true);
                // смещение посчитано по файлу на диске; в буфере CRLF свёрнут в LF —
                // вычитаем переводы строк, стоящие до ссылки
                let shift = w
                    .state
                    .borrow()
                    .catalog
                    .as_ref()
                    .map(|c| c.read_file(&id)[..position.min(c.read_file(&id).len())].matches("\r\n").count())
                    .unwrap_or(0);
                let position = position.saturating_sub(shift);
                // выделяем ссылку целиком и ставим на неё каретку
                let text = w.editor.raw_text();
                if let (Some(a), Some(b)) = (w.editor.iter_at_byte(&text, position), w.editor.iter_at_byte(&text, position + len)) {
                    w.editor.select_range(&a, &b);
                    w.editor.scroll_to_caret();
                    w.editor.grab_focus();
                }
            }
        });
        self.wire_git_lists();
    }

    pub fn update_panels(self: &Rc<Self>) {
        self.update_reading_ui();
        let current = {
            let st = self.state.borrow();
            match (&st.catalog, &st.current_id) {
                (Some(_), Some(id)) => id.clone(),
                _ => return,
            }
        };

        let (paths, backlinks, orphans) = {
            let st = self.state.borrow();
            let c = st.catalog.as_ref().unwrap();
            let paths = c.shortest_paths_from_root(&current, 50);
            let titled: Vec<Vec<(String, String)>> = paths
                .iter()
                .map(|p| p.iter().map(|id| (id.clone(), if id == ROOT_ID { "root".to_string() } else { c.title(id, 36) })).collect())
                .collect();
            (titled, c.backlinks(&current), c.orphan_files())
        };
        self.fill_breadcrumbs(&paths);

        clear_list(&self.backlinks_list);
        for b in &backlinks {
            self.backlinks_list.append(&list_row(&b.title, &b.file_id, &["nd-dim", "nd-size-10", "nd-mono"]));
        }
        self.backlinks_empty.set_text("Нет входящих ссылок");
        self.backlinks_empty.set_visible(backlinks.is_empty());

        clear_list(&self.orphans_list);
        for o in &orphans {
            self.orphans_list.append(&list_row(&o.title, &o.file_id, &["nd-dim", "nd-size-10", "nd-mono"]));
        }
        self.orphans_empty.set_text("Нет таких файлов");
        self.orphans_empty.set_visible(orphans.is_empty());
        {
            let mut st = self.state.borrow_mut();
            st.backlink_rows = backlinks.into_iter().map(|b| b.file_id).collect();
            st.orphan_rows = orphans.into_iter().map(|o| o.file_id).collect();
        }
        self.update_broken_links_indicator();
    }

    fn fill_breadcrumbs(self: &Rc<Self>, paths: &[Vec<(String, String)>]) {
        while let Some(child) = self.breadcrumbs_box.first_child() {
            self.breadcrumbs_box.remove(&child);
        }
        for path in paths {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            for (i, (id, title)) in path.iter().enumerate() {
                if i > 0 {
                    let chevron = gtk::Image::from_icon_name("pan-end-symbolic");
                    chevron.set_pixel_size(9);
                    chevron.add_css_class("nd-chevron");
                    row.append(&chevron);
                }
                if i < path.len() - 1 {
                    let b = gtk::Button::with_label(title);
                    b.add_css_class("flat");
                    b.add_css_class("nd-crumb");
                    b.set_focus_on_click(false);
                    let w = self.clone();
                    let id = id.clone();
                    b.connect_clicked(move |_| w.open_file(&id, true));
                    row.append(&b);
                } else {
                    let l = gtk::Label::new(Some(title));
                    l.add_css_class("nd-crumb-last");
                    row.append(&l);
                }
            }
            self.breadcrumbs_box.append(&row);
        }
        self.breadcrumbs_scroll.set_visible(!paths.is_empty());
        self.state.borrow_mut().breadcrumb_rows = paths.iter().map(|p| p.iter().map(|(id, _)| id.clone()).collect()).collect();
    }

    // ================= Битые ссылки =================

    pub fn toggle_broken_links_card(&self) {
        if self.state.borrow().catalog.is_none() {
            return;
        }
        if self.broken_card.is_visible() {
            self.state.borrow_mut().broken_hidden_by_user = true;
            self.broken_card.set_visible(false);
            return;
        }
        self.state.borrow_mut().broken_hidden_by_user = false;
        self.fill_broken_links_list();
        self.broken_card.set_visible(true);
    }

    /// Самопроверка: карточка битых ссылок на экране, видна она уже или нет. В каталоге с
    /// битыми ссылками она появляется сама, и переключатель её бы как раз спрятал.
    pub fn smoke_show_broken_links_card(&self) {
        if !self.broken_card.is_visible() {
            self.toggle_broken_links_card();
        }
    }

    fn fill_broken_links_list(&self) {
        let broken = {
            let st = self.state.borrow();
            let Some(c) = st.catalog.as_ref() else { return };
            c.broken_links()
        };
        clear_list(&self.broken_list);
        for b in &broken {
            let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let t = gtk::Label::new(Some(&b.file_title));
            t.set_xalign(0.0);
            t.set_ellipsize(gtk::pango::EllipsizeMode::End);
            t.add_css_class("nd-size-13");
            row.append(&t);
            let ctx = gtk::Label::new(Some(&b.context));
            ctx.set_xalign(0.0);
            ctx.set_ellipsize(gtk::pango::EllipsizeMode::End);
            ctx.add_css_class("nd-dim");
            ctx.add_css_class("nd-size-11");
            row.append(&ctx);
            let target = gtk::Label::new(Some(&b.target_id));
            target.set_xalign(0.0);
            target.set_ellipsize(gtk::pango::EllipsizeMode::End);
            target.add_css_class("nd-broken-target");
            row.append(&target);
            self.broken_list.append(&gtk::ListBoxRow::builder().child(&row).build());
        }
        self.broken_header.set_text(&if broken.is_empty() { "Битые ссылки: нет".to_string() } else { format!("Битые ссылки: {}", broken.len()) });
        // длина выделения — вся запись ссылки, её концом считаем «)»
        self.state.borrow_mut().broken_rows = broken
            .iter()
            .map(|b| {
                let st = self.state.borrow();
                let len = st
                    .catalog
                    .as_ref()
                    .and_then(|c| c.parsed_links(&b.file_id).iter().find(|l| l.start == b.position).map(|l| l.len))
                    .unwrap_or(b.target_id.len() + 4);
                (b.file_id.clone(), b.position, len)
            })
            .collect();
    }

    fn update_broken_links_indicator(&self) {
        let count = {
            let st = self.state.borrow();
            match st.catalog.as_ref() {
                Some(c) => c.broken_links().len(),
                None => {
                    drop(st);
                    self.broken_count.set_text("");
                    return;
                }
            }
        };
        self.broken_count.set_text(&if count == 0 { String::new() } else { count.to_string() });
        // карточка видна, только когда есть что показывать
        if count == 0 {
            self.broken_card.set_visible(false);
            return;
        }
        self.fill_broken_links_list();
        if !self.state.borrow().broken_hidden_by_user {
            self.broken_card.set_visible(true);
        }
    }

    // ================= Поиск по проекту =================

    pub fn run_project_search(self: &Rc<Self>, query: &str) {
        let query = query.trim().to_string();
        if query.is_empty() {
            self.search_card.set_visible(false);
            return;
        }
        let results = {
            let st = self.state.borrow();
            let Some(c) = st.catalog.as_ref() else { return };
            c.search(&query, 200)
        };
        self.state.borrow_mut().last_query = query.clone();
        clear_list(&self.search_list);
        for r in &results {
            let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let t = gtk::Label::new(Some(&r.title));
            t.set_xalign(0.0);
            t.set_ellipsize(gtk::pango::EllipsizeMode::End);
            t.add_css_class("nd-dim");
            t.add_css_class("nd-size-11");
            row.append(&t);
            let accent = format!("#{:06X}", crate::ui::css::accent(self.is_dark()));
            let markup = format!(
                "{}<b><span foreground=\"{accent}\">{}</span></b>{}",
                glib::markup_escape_text(&r.prefix),
                glib::markup_escape_text(&r.match_text),
                glib::markup_escape_text(&r.suffix)
            );
            let snippet = gtk::Label::new(None);
            snippet.set_markup(&markup);
            snippet.set_xalign(0.0);
            snippet.set_wrap(true);
            snippet.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            snippet.set_lines(3);
            snippet.set_ellipsize(gtk::pango::EllipsizeMode::End);
            snippet.add_css_class("nd-size-12");
            row.append(&snippet);
            self.search_list.append(&gtk::ListBoxRow::builder().child(&row).build());
        }
        self.search_header.set_text(&if results.is_empty() { "Ничего не найдено".to_string() } else { format!("Результаты поиска · {}", results.len()) });
        self.state.borrow_mut().search_rows = results.iter().map(|r| (r.file_id.clone(), r.occurrence)).collect();
        self.search_card.set_visible(true);
    }

    pub fn select_occurrence(&self, query: &str, occurrence: usize) {
        if query.is_empty() {
            return;
        }
        let text = self.editor.raw_text();
        let Some(start) = text::nth_occurrence(&text, query, occurrence) else { return };
        let end = text::find_all_ci(&text, query).iter().find(|(s, _)| *s == start).map(|(_, e)| *e).unwrap_or(start);
        if let (Some(a), Some(b)) = (self.editor.iter_at_byte(&text, start), self.editor.iter_at_byte(&text, end)) {
            self.editor.select_range(&a, &b);
            self.editor.scroll_to_caret();
            self.editor.grab_focus();
        }
    }

    pub(super) fn search_entry_sensitive(&self) -> bool {
        self.search_entry.is_sensitive()
    }

    pub(super) fn search_entry_grab_focus(&self) {
        self.search_entry.grab_focus();
    }

    // ================= Прогресс чтения =================

    /// Ставит или снимает отметку о прочтении текущего файла.
    pub fn mark_current_read(&self, read: bool) {
        let (project_id, current) = {
            let st = self.state.borrow();
            match (&st.project_id, &st.current_id) {
                (Some(p), Some(c)) => (p.clone(), c.clone()),
                _ => return,
            }
        };
        // файлы в собственных форматах в обход чтения не входят — и отмечать их нечего
        if !catalog::is_neurodoc_file(&current) {
            return;
        }
        {
            let mut st = self.state.borrow_mut();
            if read == st.reading.is_read(&current) {
                return;
            }
            st.reading.mark(&current, read);
            let _ = self.reading_store.save(&project_id, &st.reading);
        }
        self.update_reading_ui();
        // ссылки в текущем файле перекрашиваем сразу
        self.editor.set_visited(self.visited_set());
    }

    pub fn update_reading_ui(&self) {
        let st = self.state.borrow();
        let Some(c) = st.catalog.as_ref() else {
            drop(st);
            self.reading_count.set_text("Проект не открыт");
            self.reading_bar.set_fraction(0.0);
            self.read_next.set_sensitive(false);
            self.read_mark.set_sensitive(false);
            return;
        };
        let order = c.build_reading_order();
        // файлы могли быть удалены — в счётчике учитываем только существующие
        let read = order.iter().filter(|id| st.reading.is_read(id)).count();
        let markable = st.current_id.as_deref().is_some_and(catalog::is_neurodoc_file);
        let current_read = st.current_id.as_deref().is_some_and(|id| st.reading.is_read(id));
        // «Далее» — шаг вперёд по порядку чтения; уходить из неотмеченного файла значит
        // оставлять дырку, к которой придётся возвращаться
        let current_done = !markable || current_read;
        let next = order.iter().find(|id| !st.reading.is_read(id) && !st.current_id.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(id)));
        let count_text = if order.is_empty() { "Файлов нет".to_string() } else { format!("Прочитано {read} из {}", order.len()) };
        let fraction = if order.is_empty() { 0.0 } else { read as f64 / order.len() as f64 };
        let next_enabled = current_done && next.is_some();
        drop(st);
        self.reading_count.set_text(&count_text);
        self.reading_bar.set_fraction(fraction);
        self.read_mark.set_sensitive(markable);
        self.suppress_read_mark.set(true);
        self.read_mark.set_active(markable && current_read);
        self.suppress_read_mark.set(false);
        self.read_next.set_sensitive(next_enabled);
    }

    /// Следующий непрочитанный файл в порядке обхода (текущий не в счёт), или None.
    fn next_unread_file(&self) -> Option<String> {
        let st = self.state.borrow();
        let c = st.catalog.as_ref()?;
        c.build_reading_order()
            .into_iter()
            .find(|id| !st.reading.is_read(id) && !st.current_id.as_deref().is_some_and(|cur| cur.eq_ignore_ascii_case(id)))
    }

    pub(super) fn on_read_next(self: &Rc<Self>) {
        if let Some(next) = self.next_unread_file() {
            self.open_file(&next, true);
        }
    }

    pub(super) fn on_read_mark_toggled(&self, active: bool) {
        if self.suppress_read_mark.get() {
            return;
        }
        self.read_timer.stop(); // пользователь решил сам
        self.mark_current_read(active);
    }

    pub(super) fn on_read_reset(self: &Rc<Self>) {
        let Some(project_id) = self.state.borrow().project_id.clone() else { return };
        let w = self.clone();
        glib::spawn_future_local(async move {
            let answer = w
                .ask(
                    "Сбросить прогресс чтения?",
                    "Отметки о прочитанных файлах в этом проекте будут забыты. Сама документация не изменится.",
                    &[("cancel", "Отмена", false), ("reset", "Сбросить", true)],
                    "cancel",
                    "cancel",
                )
                .await;
            if answer != "reset" {
                return;
            }
            w.state.borrow_mut().reading = Default::default();
            w.reading_store.reset(&project_id);
            w.update_reading_ui();
            w.editor.set_visited(w.visited_set());
        });
    }
}

use gtk::glib;
