//! Действия окна и клавиши. Глобальные сочетания висят на содержимом окна в фазе
//! захвата: они срабатывают раньше привязок редактора и WebKit, но не внутри
//! диалогов — те живут вне этого поддерева (как акселераторы RootGrid на Windows).
//! Редакторные сочетания — на самом TextView, тоже в захвате: GTK отдаёт Ctrl+Y
//! под redo, а Alt со стрелками — под перемещение по словам.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{Actions, Win};

pub fn make_actions(window: &adw::ApplicationWindow) -> Actions {
    let mk = |name: &str| {
        let a = gio::SimpleAction::new(name, None);
        window.add_action(&a);
        a
    };
    Actions {
        back: mk("back"),
        forward: mk("forward"),
        home: mk("home"),
        new_link: mk("new-link"),
        link_selection: mk("link-selection"),
        new_file: mk("new-file"),
        open: mk("open"),
        copy_link: mk("copy-link"),
        copy_content: mk("copy-content"),
        broken: mk("broken-links"),
        delete: mk("delete"),
        reload: mk("reload"),
        replace: mk("replace"),
        replace_project: mk("replace-project"),
        search: mk("project-search"),
        hotkeys: mk("hotkeys"),
        settings: mk("settings"),
        projects: mk("projects"),
        save: mk("save"),
        find: mk("find"),
    }
}

pub fn wire(win: &Rc<Win>, root: &adw::ToolbarView) {
    macro_rules! on {
        ($action:expr, |$w:ident| $body:expr) => {{
            let $w = win.clone();
            $action.connect_activate(move |_, _| {
                $body;
            });
        }};
    }
    on!(win.actions.back, |w| w.go_history(true));
    on!(win.actions.forward, |w| w.go_history(false));
    on!(win.actions.home, |w| w.open_file(crate::domain::catalog::ROOT_ID, true));
    on!(win.actions.new_link, |w| w.insert_new_link());
    on!(win.actions.link_selection, |w| w.link_selection());
    on!(win.actions.new_file, |w| w.create_unlinked_file());
    on!(win.actions.open, |w| w.show_open_by_id_dialog());
    on!(win.actions.copy_link, |w| w.copy_link());
    on!(win.actions.copy_content, |w| w.copy_content());
    on!(win.actions.broken, |w| w.toggle_broken_links_card());
    on!(win.actions.delete, |w| w.delete_current_file());
    on!(win.actions.reload, |w| w.reload_from_disk(true));
    on!(win.actions.replace, |w| w.show_replace_dialog(false));
    on!(win.actions.replace_project, |w| w.show_replace_dialog(true));
    on!(win.actions.search, |w| w.focus_project_search());
    on!(win.actions.hotkeys, |w| w.show_hotkeys_dialog());
    on!(win.actions.settings, |w| w.show_settings_dialog());
    on!(win.actions.projects, |w| w.show_projects_dialog());
    on!(win.actions.save, |w| w.save_current_file());
    on!(win.actions.find, |w| w.show_find_bar());

    let global = gtk::ShortcutController::new();
    global.set_propagation_phase(gtk::PropagationPhase::Capture);
    for (trigger, action) in [
        ("<Control>s|F2", "win.save"),
        ("<Control>f", "win.find"),
        ("<Control><Shift>f", "win.project-search"),
        ("<Control>r", "win.replace"),
        ("<Control><Shift>r", "win.replace-project"),
        ("<Control>n", "win.new-file"),
        ("<Control>t", "win.new-link"),
        ("F5", "win.reload"),
        ("<Control>h", "win.home"),
        ("<Control>o", "win.open"),
        ("<Control>b", "win.broken-links"),
        ("F1", "win.hotkeys"),
        ("<Alt>Left", "win.back"),
        ("<Alt>Right", "win.forward"),
    ] {
        global.add_shortcut(gtk::Shortcut::new(gtk::ShortcutTrigger::parse_string(trigger), Some(gtk::NamedAction::new(action))));
    }
    root.add_controller(global);

    let editor = gtk::ShortcutController::new();
    editor.set_propagation_phase(gtk::PropagationPhase::Capture);
    let add = |trigger: &str, f: Box<dyn Fn(&Rc<Win>) -> bool>| {
        let w = win.clone();
        let action = gtk::CallbackAction::new(move |_, _| if f(&w) { glib::Propagation::Stop } else { glib::Propagation::Proceed });
        editor.add_shortcut(gtk::Shortcut::new(gtk::ShortcutTrigger::parse_string(trigger), Some(action)));
    };
    add("Return|KP_Enter", Box::new(|w| {
        w.editor.insert_newline_with_indent();
        true
    }));
    add("Tab", Box::new(|w| {
        w.editor.indent(false);
        true
    }));
    add("<Shift>ISO_Left_Tab|<Shift>Tab", Box::new(|w| {
        w.editor.indent(true);
        true
    }));
    add("<Shift><Alt>Up", Box::new(|w| {
        w.editor.move_line(false);
        true
    }));
    add("<Shift><Alt>Down", Box::new(|w| {
        w.editor.move_line(true);
        true
    }));
    add("<Shift>Delete", Box::new(|w| {
        // выделение вырезает сам GTK (сырым текстом), без выделения — вырезаем строку
        if w.editor.buffer.has_selection() {
            return false;
        }
        w.editor.delete_line(true);
        true
    }));
    add("<Control>y", Box::new(|w| {
        w.editor.delete_line(false);
        true
    }));
    add("<Control>g", Box::new(|w| {
        w.navigate_link_at_caret();
        true
    }));
    add("<Control>l", Box::new(|w| {
        w.link_selection();
        true
    }));
    add("<Control>u", Box::new(|w| {
        w.remove_link_at_caret();
        true
    }));
    add("Escape", Box::new(|w| {
        if w.find_bar_visible() {
            w.close_find_bar();
            return true;
        }
        false
    }));
    win.editor.view.add_controller(editor);
}

impl Win {
    pub fn focus_project_search(&self) {
        if self.search_entry_sensitive() {
            self.search_entry_grab_focus();
        }
    }
}
