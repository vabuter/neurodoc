//! Диалоги: проекты, настройки, горячие клавиши, открыть файл, поиск и замена,
//! клонирование, текстовый запрос. Все — libadwaita 1.5 (AdwDialog / AdwAlertDialog).

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::Win;
use crate::domain::catalog::{self, NeurodocCatalog};
use crate::domain::git;
use crate::domain::settings::{ProjectInfo, ThemeMode};
use crate::ui::fonts::font_choices;

/// Таблица горячих клавиш — та же, что в HotkeysDialog Windows-версии.
pub const HOTKEYS: [(&str, &str); 23] = [
    ("Ctrl+S  /  F2", "Сохранить файл (автосохранение тоже работает)"),
    ("Ctrl+F", "Поиск по текущему файлу"),
    ("Ctrl+Shift+F", "Поиск по всему проекту"),
    ("Ctrl+R", "Поиск и замена в текущем файле"),
    ("Ctrl+Shift+R", "Поиск и замена по всему проекту"),
    ("Ctrl+G", "Перейти по ссылке, на которой стоит каретка"),
    ("Ctrl+T", "Новая ссылка: вставить, создать файл и перейти"),
    ("Ctrl+U", "Снять ссылку, оставив её текст"),
    ("Ctrl+L", "Связать выделенный текст со ссылкой из буфера обмена"),
    ("Ctrl+N", "Новый файл, не привязанный к другим"),
    ("Ctrl+H", "Перейти к корневому файлу (root)"),
    ("Ctrl+O", "Открыть файл по UUID или пути"),
    ("Ctrl+B", "Битые ссылки в проекте"),
    ("Alt+←  /  Alt+→", "Назад / вперёд по истории (как в браузере)"),
    ("Tab  /  Shift+Tab", "Отступ 4 пробела / снять отступ (и для выделенных строк)"),
    ("Shift+Alt+↑ / ↓", "Переместить текущую строку вверх / вниз"),
    ("Shift+Delete", "Вырезать строку (или выделение) в буфер"),
    ("Ctrl+Y", "Удалить строку без копирования в буфер"),
    ("Ctrl+Z  /  Ctrl+Shift+Z", "Отменить / повторить"),
    ("Enter", "Новая строка с сохранением отступа"),
    ("Клик по ссылке", "Переход к файлу [текст](цель); http(s)-ссылки — в браузере"),
    ("F5", "Перечитать проект с диска"),
    ("F1", "Эта шпаргалка"),
];

/// Диалог с заголовком и кнопками в шапке.
fn dialog(title: &str, width: i32) -> (adw::Dialog, adw::HeaderBar, gtk::Box) {
    let d = adw::Dialog::new();
    d.set_title(title);
    d.set_content_width(width);
    let header = adw::HeaderBar::new();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_start(20);
    body.set_margin_end(20);
    body.set_margin_top(8);
    body.set_margin_bottom(20);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&body));
    d.set_child(Some(&view));
    (d, header, body)
}

fn labeled_entry(label: &str, placeholder: &str) -> (gtk::Box, gtk::Entry) {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let l = gtk::Label::new(Some(label));
    l.set_xalign(0.0);
    l.add_css_class("nd-size-12");
    b.append(&l);
    let e = gtk::Entry::new();
    e.set_placeholder_text(Some(placeholder));
    b.append(&e);
    (b, e)
}

fn status_label() -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.add_css_class("nd-size-12");
    l.set_visible(false);
    l
}

fn set_status(label: &gtk::Label, text: &str, class: &str) {
    for c in ["nd-error", "nd-success", "nd-dim"] {
        label.remove_css_class(c);
    }
    label.add_css_class(class);
    label.set_text(text);
    label.set_visible(!text.is_empty());
}

fn pick_folder(parent: &adw::ApplicationWindow, entry: &gtk::Entry) {
    let dialog = gtk::FileDialog::new();
    dialog.set_title("Папка каталога");
    let entry = entry.clone();
    dialog.select_folder(Some(parent), None::<&gio::Cancellable>, move |result| {
        if let Ok(folder) = result
            && let Some(path) = folder.path() {
                entry.set_text(&path.display().to_string());
            }
    });
}

impl Win {
    /// Закрывает открытый AdwDialog (для самопроверки).
    pub fn close_dialogs(&self) {
        if let Some(d) = self.window.visible_dialog() {
            d.close();
        }
    }

    /// Диалог с одним текстовым полем; None — пользователь отменил.
    pub async fn prompt_text(&self, title: &str, header: &str, placeholder: &str) -> Option<String> {
        let d = adw::AlertDialog::new(Some(title), None);
        let (b, entry) = labeled_entry(header, placeholder);
        d.set_extra_child(Some(&b));
        d.add_response("cancel", "Отмена");
        d.add_response("ok", "OK");
        d.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        d.set_default_response(Some("ok"));
        d.set_close_response("cancel");
        let d2 = d.clone();
        entry.connect_activate(move |_| d2.emit_by_name::<()>("response", &[&"ok"]));
        let answer = d.choose_future(Some(&self.window)).await;
        if answer == "ok" { Some(entry.text().to_string()) } else { None }
    }

    // ================= Проекты =================

    pub fn show_projects_dialog(self: &Rc<Self>) {
        let (d, header, body) = dialog("Проекты", 520);
        let open_button = gtk::Button::with_label("Открыть");
        open_button.add_css_class("suggested-action");
        header.pack_end(&open_button);

        let stack = gtk::Stack::new();
        body.append(&stack);

        // ---- список ----
        let list_pane = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let hint = gtk::Label::new(Some("Проект — это название и путь к каталогу с neurodoc-документацией."));
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        hint.add_css_class("nd-dim");
        hint.add_css_class("nd-size-12");
        list_pane.append(&hint);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(false);
        list.add_css_class("boxed-list");
        let scroll = gtk::ScrolledWindow::builder().propagate_natural_height(true).max_content_height(300).child(&list).build();
        list_pane.append(&scroll);
        let empty = gtk::Label::new(Some("Список проектов пуст. Добавьте первый проект."));
        empty.set_xalign(0.0);
        empty.add_css_class("nd-dim");
        empty.add_css_class("nd-size-12");
        list_pane.append(&empty);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let add = gtk::Button::with_label("Добавить");
        let edit = gtk::Button::with_label("Изменить");
        let delete = gtk::MenuButton::builder().label("Удалить").build();
        let confirm = gtk::Popover::new();
        let confirm_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let confirm_text = gtk::Label::new(Some("Удалить проект из списка? Файлы на диске не удаляются."));
        confirm_text.set_wrap(true);
        confirm_text.set_max_width_chars(36);
        confirm_box.append(&confirm_text);
        let confirm_button = gtk::Button::with_label("Удалить");
        confirm_button.add_css_class("destructive-action");
        confirm_box.append(&confirm_button);
        confirm.set_child(Some(&confirm_box));
        delete.set_popover(Some(&confirm));
        buttons.append(&add);
        buttons.append(&edit);
        buttons.append(&delete);
        list_pane.append(&buttons);
        stack.add_named(&list_pane, Some("list"));

        // ---- правка ----
        let edit_pane = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let edit_title = gtk::Label::new(None);
        edit_title.set_xalign(0.0);
        edit_title.add_css_class("title-4");
        edit_pane.append(&edit_title);
        let (name_box, name_entry) = labeled_entry("Название", "Например: Fikus");
        edit_pane.append(&name_box);
        let path_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let (path_box, path_entry) = labeled_entry("Путь к каталогу", "/home/…/каталог");
        path_box.set_hexpand(true);
        path_row.append(&path_box);
        let browse = gtk::Button::with_label("Обзор…");
        browse.set_valign(gtk::Align::End);
        path_row.append(&browse);
        edit_pane.append(&path_row);
        let note = gtk::Label::new(Some("Если каталога нет, он будет создан вместе с файлом root, папкой _content и ReadMe.md."));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("nd-dim");
        note.add_css_class("nd-size-12");
        edit_pane.append(&note);
        let error = status_label();
        edit_pane.append(&error);
        let edit_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let save = gtk::Button::with_label("Сохранить");
        save.add_css_class("suggested-action");
        let cancel = gtk::Button::with_label("Отмена");
        edit_buttons.append(&save);
        edit_buttons.append(&cancel);
        edit_pane.append(&edit_buttons);
        stack.add_named(&edit_pane, Some("edit"));
        stack.set_visible_child_name("list");

        let ids: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let editing: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let to_open: Rc<RefCell<Option<ProjectInfo>>> = Rc::new(RefCell::new(None));

        let refresh = {
            let w = self.clone();
            let list = list.clone();
            let ids = ids.clone();
            let empty = empty.clone();
            let edit = edit.clone();
            let delete = delete.clone();
            let open_button = open_button.clone();
            Rc::new(move || {
                super::clear_list(&list);
                let st = w.state.borrow();
                let mut found: Vec<String> = Vec::new();
                let mut select_index = None;
                for (i, p) in st.settings.projects.iter().enumerate() {
                    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                    row.set_margin_top(8);
                    row.set_margin_bottom(8);
                    row.set_margin_start(12);
                    row.set_margin_end(12);
                    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
                    texts.set_hexpand(true);
                    let name = gtk::Label::new(Some(&p.name));
                    name.set_xalign(0.0);
                    name.add_css_class("heading");
                    texts.append(&name);
                    let path = gtk::Label::new(Some(&p.path));
                    path.set_xalign(0.0);
                    path.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    path.add_css_class("nd-dim");
                    path.add_css_class("nd-size-11");
                    texts.append(&path);
                    row.append(&texts);
                    if Some(&p.id) == st.settings.current_project_id.as_ref() {
                        let badge = gtk::Label::new(Some("текущий"));
                        badge.add_css_class("nd-badge");
                        badge.set_valign(gtk::Align::Center);
                        row.append(&badge);
                        select_index = Some(i);
                    }
                    list.append(&gtk::ListBoxRow::builder().child(&row).build());
                    found.push(p.id.clone());
                }
                let has = !found.is_empty();
                empty.set_visible(!has);
                let index = select_index.unwrap_or(0);
                if has {
                    list.select_row(list.row_at_index(index as i32).as_ref());
                }
                edit.set_sensitive(has);
                delete.set_sensitive(has);
                open_button.set_sensitive(has);
                *ids.borrow_mut() = found;
            })
        };
        refresh();

        let selected_id = {
            let list = list.clone();
            let ids = ids.clone();
            move || list.selected_row().and_then(|r| ids.borrow().get(r.index() as usize).cloned())
        };

        // открыть: кнопка или двойной клик
        let commit_open = {
            let w = self.clone();
            let d = d.clone();
            let to_open = to_open.clone();
            let selected_id = selected_id.clone();
            Rc::new(move || {
                let Some(id) = selected_id() else { return };
                let project = {
                    let mut st = w.state.borrow_mut();
                    let Some(p) = st.settings.projects.iter().find(|p| p.id == id).cloned() else { return };
                    st.settings.current_project_id = Some(p.id.clone());
                    p
                };
                w.save_settings();
                *to_open.borrow_mut() = Some(project);
                d.close();
            })
        };
        let c = commit_open.clone();
        open_button.connect_clicked(move |_| c());
        let c = commit_open.clone();
        list.connect_row_activated(move |_, _| c());
        {
            let edit = edit.clone();
            let delete = delete.clone();
            let open_button = open_button.clone();
            list.connect_selected_rows_changed(move |l| {
                let has = l.selected_row().is_some();
                edit.set_sensitive(has);
                delete.set_sensitive(has);
                open_button.set_sensitive(has);
            });
        }

        let show_edit = {
            let stack = stack.clone();
            let edit_title = edit_title.clone();
            let name_entry = name_entry.clone();
            let path_entry = path_entry.clone();
            let error = error.clone();
            let editing = editing.clone();
            let open_button = open_button.clone();
            Rc::new(move |project: Option<ProjectInfo>| {
                edit_title.set_text(if project.is_none() { "Новый проект" } else { "Изменить проект" });
                name_entry.set_text(project.as_ref().map(|p| p.name.as_str()).unwrap_or(""));
                path_entry.set_text(project.as_ref().map(|p| p.path.as_str()).unwrap_or(""));
                set_status(&error, "", "nd-error");
                *editing.borrow_mut() = project.map(|p| p.id);
                stack.set_visible_child_name("edit");
                open_button.set_visible(false);
                name_entry.grab_focus();
            })
        };
        let hide_edit = {
            let stack = stack.clone();
            let open_button = open_button.clone();
            let refresh = refresh.clone();
            Rc::new(move || {
                stack.set_visible_child_name("list");
                open_button.set_visible(true);
                refresh();
            })
        };

        let s = show_edit.clone();
        add.connect_clicked(move |_| s(None));
        {
            let w = self.clone();
            let s = show_edit.clone();
            let selected_id = selected_id.clone();
            edit.connect_clicked(move |_| {
                let Some(id) = selected_id() else { return };
                let project = w.state.borrow().settings.projects.iter().find(|p| p.id == id).cloned();
                s(project);
            });
        }
        let h = hide_edit.clone();
        cancel.connect_clicked(move |_| h());
        {
            let parent = self.window.clone();
            let path_entry = path_entry.clone();
            browse.connect_clicked(move |_| pick_folder(&parent, &path_entry));
        }
        {
            let w = self.clone();
            let name_entry = name_entry.clone();
            let path_entry = path_entry.clone();
            let error = error.clone();
            let editing = editing.clone();
            let h = hide_edit.clone();
            save.connect_clicked(move |_| {
                let name = name_entry.text().trim().to_string();
                let path = path_entry.text().trim().to_string();
                if name.is_empty() {
                    set_status(&error, "Укажите название проекта.", "nd-error");
                    return;
                }
                if path.is_empty() {
                    set_status(&error, "Укажите путь к каталогу проекта.", "nd-error");
                    return;
                }
                let path = expand_path(&path);
                if let Err(e) = NeurodocCatalog::ensure_structure(Path::new(&path)) {
                    set_status(&error, &format!("Не удалось создать структуру каталога: {e}"), "nd-error");
                    return;
                }
                {
                    let mut st = w.state.borrow_mut();
                    match editing.borrow().clone() {
                        None => {
                            let project = ProjectInfo::new(&name, &path);
                            if st.settings.current_project_id.is_none() {
                                st.settings.current_project_id = Some(project.id.clone());
                            }
                            st.settings.projects.push(project);
                        }
                        Some(id) => {
                            if let Some(p) = st.settings.projects.iter_mut().find(|p| p.id == id) {
                                p.name = name;
                                p.path = path;
                            }
                        }
                    }
                }
                w.save_settings();
                h();
            });
        }
        {
            let w = self.clone();
            let confirm = confirm.clone();
            let refresh = refresh.clone();
            let selected_id = selected_id.clone();
            confirm_button.connect_clicked(move |_| {
                confirm.popdown();
                let Some(id) = selected_id() else { return };
                {
                    let mut st = w.state.borrow_mut();
                    st.settings.projects.retain(|p| p.id != id);
                    if st.settings.current_project_id.as_deref() == Some(id.as_str()) {
                        st.settings.current_project_id = st.settings.projects.first().map(|p| p.id.clone());
                    }
                }
                w.save_settings();
                refresh();
            });
        }

        let w = self.clone();
        let to_open2 = to_open.clone();
        d.connect_closed(move |_| {
            let project = to_open2.borrow_mut().take();
            let current = w.state.borrow().settings.current_project().cloned();
            let open_path = w.state.borrow().catalog.as_ref().map(|c| c.root_path().to_path_buf());
            match (project, current) {
                (Some(p), _) => w.open_project(&p),
                (None, None) => {
                    w.state.borrow_mut().catalog = None;
                    w.state.borrow_mut().current_id = None;
                    w.update_project_ui();
                }
                (None, Some(c)) if open_path.as_deref() != Some(Path::new(&c.path)) => w.open_project(&c),
                _ => w.update_project_ui(),
            }
        });
        d.present(Some(&self.window));
    }

    // ================= Настройки =================

    pub fn show_settings_dialog(self: &Rc<Self>) {
        let (d, _header, body) = dialog("Настройки", 420);
        let (theme, family, size, spacing) = {
            let s = &self.state.borrow().settings;
            (s.theme, s.font_family.clone(), s.font_size, s.line_spacing)
        };

        let theme_label = gtk::Label::new(Some("Тема оформления"));
        theme_label.set_xalign(0.0);
        theme_label.add_css_class("nd-size-12");
        body.append(&theme_label);
        let theme_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let system = gtk::CheckButton::with_label("Система");
        let light = gtk::CheckButton::with_label("Светлая");
        light.set_group(Some(&system));
        let dark = gtk::CheckButton::with_label("Тёмная");
        dark.set_group(Some(&system));
        match theme {
            ThemeMode::System => system.set_active(true),
            ThemeMode::Light => light.set_active(true),
            ThemeMode::Dark => dark.set_active(true),
        }
        theme_box.append(&system);
        theme_box.append(&light);
        theme_box.append(&dark);
        body.append(&theme_box);
        for (button, mode) in [(&system, ThemeMode::System), (&light, ThemeMode::Light), (&dark, ThemeMode::Dark)] {
            let w = self.clone();
            button.connect_toggled(move |b| {
                if !b.is_active() {
                    return;
                }
                w.state.borrow_mut().settings.theme = mode;
                w.save_settings();
                w.apply_theme();
            });
        }

        let font_label = gtk::Label::new(Some("Шрифт"));
        font_label.set_xalign(0.0);
        font_label.add_css_class("nd-size-12");
        body.append(&font_label);
        let installed: Vec<String> = self.window.pango_context().list_families().iter().map(|f| f.name().to_string()).collect();
        let fonts = font_choices(installed, &family);
        let model = gtk::StringList::new(&fonts.iter().map(String::as_str).collect::<Vec<_>>());
        let dropdown = gtk::DropDown::new(Some(model), None::<gtk::Expression>);
        dropdown.set_enable_search(true);
        if let Some(i) = fonts.iter().position(|f| f.eq_ignore_ascii_case(&family)) {
            dropdown.set_selected(i as u32);
        }
        body.append(&dropdown);
        {
            let w = self.clone();
            let fonts = fonts.clone();
            dropdown.connect_selected_notify(move |dd| {
                if let Some(f) = fonts.get(dd.selected() as usize) {
                    w.state.borrow_mut().settings.font_family = f.clone();
                    w.save_settings();
                    w.apply_editor_font();
                }
            });
        }

        let size_label = gtk::Label::new(Some("Размер шрифта"));
        size_label.set_xalign(0.0);
        size_label.add_css_class("nd-size-12");
        body.append(&size_label);
        let size_spin = gtk::SpinButton::with_range(8.0, 96.0, 1.0);
        size_spin.set_value(size);
        body.append(&size_spin);
        {
            let w = self.clone();
            size_spin.connect_value_changed(move |s| {
                w.state.borrow_mut().settings.font_size = s.value().clamp(8.0, 96.0);
                w.save_settings();
                w.apply_editor_font();
            });
        }

        let spacing_label = gtk::Label::new(Some("Междустрочный интервал"));
        spacing_label.set_xalign(0.0);
        spacing_label.add_css_class("nd-size-12");
        body.append(&spacing_label);
        let spacing_spin = gtk::SpinButton::with_range(0.8, 3.0, 0.1);
        spacing_spin.set_digits(1);
        spacing_spin.set_value(spacing);
        body.append(&spacing_spin);
        {
            let w = self.clone();
            spacing_spin.connect_value_changed(move |s| {
                w.state.borrow_mut().settings.line_spacing = s.value().clamp(0.8, 3.0);
                w.save_settings();
                w.apply_editor_font();
            });
        }

        let note = gtk::Label::new(Some("Изменения применяются сразу и сохраняются в файле настроек (JSON)."));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("nd-dim");
        note.add_css_class("nd-size-12");
        body.append(&note);
        d.present(Some(&self.window));
    }

    // ================= Горячие клавиши =================

    pub fn show_hotkeys_dialog(self: &Rc<Self>) {
        let (d, _header, body) = dialog("Горячие клавиши", 560);
        let grid = gtk::Grid::new();
        grid.set_row_spacing(6);
        grid.set_column_spacing(14);
        for (i, (keys, action)) in HOTKEYS.iter().enumerate() {
            let kbd = gtk::Label::new(Some(keys));
            kbd.add_css_class("nd-kbd");
            kbd.set_halign(gtk::Align::Start);
            kbd.set_width_request(190);
            grid.attach(&kbd, 0, i as i32, 1, 1);
            let desc = gtk::Label::new(Some(action));
            desc.set_xalign(0.0);
            desc.set_wrap(true);
            desc.set_hexpand(true);
            desc.add_css_class("nd-size-13");
            grid.attach(&desc, 1, i as i32, 1, 1);
        }
        let scroll = gtk::ScrolledWindow::builder().propagate_natural_height(true).max_content_height(560).child(&grid).build();
        body.append(&scroll);
        d.present(Some(&self.window));
    }

    // ================= Открыть файл =================

    pub fn show_open_by_id_dialog(self: &Rc<Self>) {
        if self.state.borrow().dialog_open || self.state.borrow().catalog.is_none() {
            return;
        }
        self.state.borrow_mut().dialog_open = true;
        let (d, header, body) = dialog("Открыть файл", 520);
        let open_button = gtk::Button::with_label("Открыть");
        open_button.add_css_class("suggested-action");
        header.pack_end(&open_button);
        let (id_box, id_entry) = labeled_entry("UUID или путь к файлу", "a259b43e-ac17-4167-bf9f-5ff3acf46ba5, root или contract/auth/start.yaml");
        id_entry.add_css_class("nd-mono");
        body.append(&id_box);
        let status = status_label();
        body.append(&status);
        let list_header = gtk::Label::new(Some("Файлы проекта"));
        list_header.set_xalign(0.0);
        list_header.add_css_class("nd-card-title");
        body.append(&list_header);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(false);
        list.add_css_class("nd-list");
        let scroll = gtk::ScrolledWindow::builder().propagate_natural_height(true).max_content_height(240).child(&list).build();
        body.append(&scroll);

        let all: Vec<(String, String)> = {
            let st = self.state.borrow();
            let c = st.catalog.as_ref().unwrap();
            c.all_ids().into_iter().map(|id| (id.clone(), c.title(&id, 60))).collect()
        };
        let all = Rc::new(all);
        let shown: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let suppress = Rc::new(Cell::new(false));

        let apply_filter = {
            let list = list.clone();
            let all = all.clone();
            let shown = shown.clone();
            let list_header = list_header.clone();
            let suppress = suppress.clone();
            Rc::new(move |query: &str| {
                let q = query.trim().trim_matches(|c| c == '<' || c == '>').trim().to_lowercase();
                let items: Vec<&(String, String)> =
                    all.iter().filter(|(id, title)| q.is_empty() || id.to_lowercase().contains(&q) || title.to_lowercase().contains(&q)).collect();
                suppress.set(true);
                super::clear_list(&list);
                for (id, title) in items.iter().take(200) {
                    list.append(&super::list_row(title, id, &["nd-dim", "nd-size-10", "nd-mono"]));
                }
                suppress.set(false);
                list_header.set_text(&if items.len() == all.len() { format!("Файлы проекта: {}", all.len()) } else { format!("Найдено файлов: {}", items.len()) });
                *shown.borrow_mut() = items.iter().take(200).map(|(id, _)| id.clone()).collect();
            })
        };
        let update_status = {
            let w = self.clone();
            let status = status.clone();
            Rc::new(move |input: &str| {
                if input.trim().is_empty() {
                    set_status(&status, "", "nd-dim");
                    return;
                }
                let st = w.state.borrow();
                let Some(c) = st.catalog.as_ref() else { return };
                match c.try_resolve_input(input) {
                    Some(id) => set_status(&status, &c.title(&id, 60), "nd-success"),
                    None => match catalog::try_parse_id(input) {
                        Some(parsed) => set_status(&status, &format!("Файла {parsed} нет в этом проекте."), "nd-error"),
                        None => set_status(&status, "Это не UUID и не путь к файлу проекта.", "nd-dim"),
                    },
                }
            })
        };
        apply_filter("");
        {
            let apply_filter = apply_filter.clone();
            let update_status = update_status.clone();
            let suppress = suppress.clone();
            id_entry.connect_changed(move |e| {
                if suppress.get() {
                    return;
                }
                apply_filter(&e.text());
                update_status(&e.text());
            });
        }
        {
            let id_entry = id_entry.clone();
            let shown = shown.clone();
            let suppress = suppress.clone();
            let update_status = update_status.clone();
            list.connect_selected_rows_changed(move |l| {
                if suppress.get() {
                    return;
                }
                let Some(row) = l.selected_row() else { return };
                let Some(id) = shown.borrow().get(row.index() as usize).cloned() else { return };
                suppress.set(true);
                id_entry.set_text(&id);
                suppress.set(false);
                update_status(&id);
            });
        }
        let chosen: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let try_open = {
            let w = self.clone();
            let d = d.clone();
            let id_entry = id_entry.clone();
            let chosen = chosen.clone();
            let update_status = update_status.clone();
            let status = status.clone();
            Rc::new(move || {
                let input = id_entry.text().to_string();
                let resolved = w.state.borrow().catalog.as_ref().and_then(|c| c.try_resolve_input(&input));
                match resolved {
                    Some(id) => {
                        *chosen.borrow_mut() = Some(id);
                        d.close();
                    }
                    None => {
                        update_status(&input);
                        if input.trim().is_empty() {
                            set_status(&status, "Укажите файл или выберите его в списке.", "nd-dim");
                        }
                    }
                }
            })
        };
        let t = try_open.clone();
        open_button.connect_clicked(move |_| t());
        let t = try_open.clone();
        id_entry.connect_activate(move |_| t());
        {
            let shown = shown.clone();
            let chosen = chosen.clone();
            let d = d.clone();
            list.connect_row_activated(move |_, row| {
                if let Some(id) = shown.borrow().get(row.index() as usize).cloned() {
                    *chosen.borrow_mut() = Some(id);
                    d.close();
                }
            });
        }
        // если в буфере обмена лежит ссылка или UUID — подставляем сразу
        {
            let w = self.clone();
            let id_entry = id_entry.clone();
            self.window.clipboard().read_text_async(None::<&gio::Cancellable>, move |result| {
                let Ok(Some(text)) = result else { return };
                let id = catalog::try_parse_id(&text).filter(|id| w.state.borrow().catalog.as_ref().is_some_and(|c| c.exists(id)));
                if let Some(id) = id {
                    id_entry.set_text(&id);
                    id_entry.select_region(0, -1);
                }
            });
        }
        let w = self.clone();
        d.connect_closed(move |_| {
            w.state.borrow_mut().dialog_open = false;
            if let Some(id) = chosen.borrow_mut().take() {
                w.open_file(&id, true);
            }
        });
        d.present(Some(&self.window));
        id_entry.grab_focus();
    }

    // ================= Поиск и замена =================

    pub fn show_replace_dialog(self: &Rc<Self>, whole_project: bool) {
        if self.state.borrow().dialog_open || self.state.borrow().current_id.is_none() {
            return;
        }
        self.state.borrow_mut().dialog_open = true;
        self.reset_view_mode();
        let (d, header, body) = dialog("Поиск и замена", 460);
        let replace_all = gtk::Button::with_label("Заменить всё");
        replace_all.add_css_class("suggested-action");
        let replace_one = gtk::Button::with_label("Заменить");
        header.pack_end(&replace_all);
        header.pack_end(&replace_one);
        let (find_box, find_entry) = labeled_entry("Найти", "Строка поиска");
        body.append(&find_box);
        let (repl_box, repl_entry) = labeled_entry("Заменить на", "Строка замены");
        body.append(&repl_box);
        let scope_label = gtk::Label::new(Some("Область поиска"));
        scope_label.set_xalign(0.0);
        scope_label.add_css_class("nd-size-12");
        body.append(&scope_label);
        let scope_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let file_scope = gtk::CheckButton::with_label("Текущий файл");
        let project_scope = gtk::CheckButton::with_label("Весь проект");
        project_scope.set_group(Some(&file_scope));
        if whole_project { project_scope.set_active(true) } else { file_scope.set_active(true) }
        scope_box.append(&file_scope);
        scope_box.append(&project_scope);
        body.append(&scope_box);
        let status = status_label();
        body.append(&status);

        let do_replace = {
            let w = self.clone();
            let find_entry = find_entry.clone();
            let repl_entry = repl_entry.clone();
            let project_scope = project_scope.clone();
            let status = status.clone();
            Rc::new(move |all: bool| {
                let query = find_entry.text().to_string();
                if query.is_empty() {
                    set_status(&status, "Укажите строку поиска.", "nd-dim");
                    return;
                }
                let replacement = repl_entry.text().to_string();
                let whole = project_scope.is_active();
                if all {
                    let count = if whole { w.replace_all_in_project(&query, &replacement) } else { w.replace_all_in_current_file(&query, &replacement) };
                    set_status(&status, &if count > 0 { format!("Заменено вхождений: {count}.") } else { "Совпадений не найдено.".to_string() }, "nd-success");
                } else {
                    let replaced = if whole { w.replace_one_in_project(&query, &replacement) } else { w.replace_one_in_current_file(&query, &replacement) };
                    set_status(&status, if replaced { "Заменено одно вхождение." } else { "Совпадений не найдено." }, "nd-success");
                }
            })
        };
        let r = do_replace.clone();
        replace_all.connect_clicked(move |_| r(true));
        let r = do_replace.clone();
        replace_one.connect_clicked(move |_| r(false));
        let r = do_replace.clone();
        find_entry.connect_activate(move |_| r(false));
        let w = self.clone();
        d.connect_closed(move |_| w.state.borrow_mut().dialog_open = false);
        d.present(Some(&self.window));
        find_entry.grab_focus();
    }

    // ================= Клонировать =================

    /// Клонирование репозитория с сервера в папку на диске. Непустую папку диалог
    /// предупреждением предлагает очистить: клонировать можно только «как есть».
    pub fn show_clone_dialog(self: &Rc<Self>, on_done: impl Fn(ProjectInfo) + 'static) {
        let (d, _header, body) = dialog("Клонировать с сервера", 520);
        let stack = gtk::Stack::new();
        body.append(&stack);
        let form = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let (url_box, url_entry) = labeled_entry("Адрес репозитория (HTTPS или SSH)", "git@gitlab.com:you/project.git");
        form.append(&url_box);
        let (name_box, name_entry) = labeled_entry("Название проекта", "подставится из адреса");
        form.append(&name_box);
        let path_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let (path_box, path_entry) = labeled_entry("Папка на диске", "/home/…/каталог");
        path_box.set_hexpand(true);
        path_row.append(&path_box);
        let browse = gtk::Button::with_label("Обзор…");
        browse.set_valign(gtk::Align::End);
        path_row.append(&browse);
        form.append(&path_row);
        let error = status_label();
        form.append(&error);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let clone_button = gtk::Button::with_label("Клонировать");
        clone_button.add_css_class("suggested-action");
        let busy = gtk::Spinner::new();
        buttons.append(&clone_button);
        buttons.append(&busy);
        form.append(&buttons);
        stack.add_named(&form, Some("form"));
        let confirm = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let confirm_title = gtk::Label::new(Some("Папка не пуста"));
        confirm_title.set_xalign(0.0);
        confirm_title.add_css_class("title-4");
        confirm.append(&confirm_title);
        let confirm_text = gtk::Label::new(None);
        confirm_text.set_xalign(0.0);
        confirm_text.set_wrap(true);
        confirm.append(&confirm_text);
        let confirm_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let confirm_button = gtk::Button::with_label("Удалить и клонировать");
        confirm_button.add_css_class("destructive-action");
        let back = gtk::Button::with_label("Назад");
        confirm_buttons.append(&confirm_button);
        confirm_buttons.append(&back);
        confirm.append(&confirm_buttons);
        stack.add_named(&confirm, Some("confirm"));
        stack.set_visible_child_name("form");

        // название проекта подставляется из адреса, пока пользователь его не правил
        let name_edited = Rc::new(Cell::new(false));
        {
            let name_edited = name_edited.clone();
            let name_entry2 = name_entry.clone();
            name_entry.connect_changed(move |e| {
                if e.has_focus() {
                    name_edited.set(true);
                }
                let _ = &name_entry2;
            });
        }
        {
            let name_entry = name_entry.clone();
            let name_edited = name_edited.clone();
            url_entry.connect_changed(move |e| {
                if name_edited.get() {
                    return;
                }
                name_entry.set_text(&git::project_name_from_url(&e.text()));
            });
        }
        {
            let parent = self.window.clone();
            let path_entry = path_entry.clone();
            browse.connect_clicked(move |_| pick_folder(&parent, &path_entry));
        }
        let on_done = Rc::new(on_done);
        let run_clone = {
            let d = d.clone();
            let name_entry = name_entry.clone();
            let error = error.clone();
            let clone_button = clone_button.clone();
            let busy = busy.clone();
            let on_done = on_done.clone();
            Rc::new(move |url: String, path: String| {
                clone_button.set_sensitive(false);
                busy.start();
                let d = d.clone();
                let name_entry = name_entry.clone();
                let error = error.clone();
                let clone_button = clone_button.clone();
                let busy = busy.clone();
                let on_done = on_done.clone();
                glib::spawn_future_local(async move {
                    let p = path.clone();
                    let result = super::git::blocking(move || git::clone(&url, Path::new(&p))).await;
                    busy.stop();
                    clone_button.set_sensitive(true);
                    match result {
                        Ok(Ok(())) => {
                            let project = ProjectInfo::new(name_entry.text().trim(), &path);
                            d.close();
                            on_done(project);
                        }
                        Ok(Err(e)) | Err(e) => set_status(&error, &e.to_string(), "nd-error"),
                    }
                });
            })
        };
        {
            let url_entry = url_entry.clone();
            let name_entry = name_entry.clone();
            let path_entry = path_entry.clone();
            let error = error.clone();
            let stack = stack.clone();
            let confirm_text = confirm_text.clone();
            let run_clone = run_clone.clone();
            clone_button.connect_clicked(move |_| {
                set_status(&error, "", "nd-error");
                let url = url_entry.text().trim().to_string();
                let path = path_entry.text().trim().to_string();
                if url.is_empty() {
                    set_status(&error, "Укажите адрес репозитория.", "nd-error");
                    return;
                }
                if name_entry.text().trim().is_empty() {
                    set_status(&error, "Укажите название проекта.", "nd-error");
                    return;
                }
                if path.is_empty() {
                    set_status(&error, "Укажите папку на диске.", "nd-error");
                    return;
                }
                let path = expand_path(&path);
                path_entry.set_text(&path);
                let non_empty = Path::new(&path).is_dir() && std::fs::read_dir(&path).map(|mut d| d.next().is_some()).unwrap_or(false);
                if non_empty {
                    confirm_text.set_text(&format!("В папке «{path}» уже есть файлы. Они будут удалены, и репозиторий будет клонирован с сервера как есть."));
                    stack.set_visible_child_name("confirm");
                    return;
                }
                run_clone(url, path);
            });
        }
        {
            let stack = stack.clone();
            back.connect_clicked(move |_| stack.set_visible_child_name("form"));
        }
        {
            let stack = stack.clone();
            let url_entry = url_entry.clone();
            let path_entry = path_entry.clone();
            confirm_button.connect_clicked(move |_| {
                stack.set_visible_child_name("form");
                run_clone(url_entry.text().trim().to_string(), path_entry.text().trim().to_string());
            });
        }
        d.present(Some(&self.window));
    }
}

/// `~` в начале пути — домашняя папка; остальное как есть (абсолютный путь).
fn expand_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest).display().to_string();
        }
    match std::path::absolute(path) {
        Ok(p) => p.display().to_string(),
        Err(_) => path.to_string(),
    }
}
