//! Главное окно. Разбито на файлы по образцу Windows-версии: здесь — состояние,
//! сборка виджетов, проекты, открытие и сохранение файлов, навигация и слежение
//! за диском; `panels.rs` — крошки и карточки правой колонки; `links.rs` — действия
//! со ссылками; `find.rs` — поиск и замена; `viewer.rs` — просмотр форматов и дельт;
//! `git.rs` — git; `dialogs.rs` — диалоги; `shortcuts.rs` — действия и клавиши.

pub mod dialogs;
pub mod find;
pub mod git;
pub mod links;
pub mod panels;
pub mod shortcuts;
pub mod tip;
pub mod viewer;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::domain::catalog::{self, NeurodocCatalog, ROOT_ID};
use crate::domain::git::{GitCommit, GitService};
use crate::domain::migration;
use crate::domain::reading::{ReadingProgress, ReadingProgressStore};
use crate::domain::settings::{AppSettings, ProjectInfo, SettingsStore, ThemeMode};
use crate::ui::css;
use crate::ui::editor::Editor;
use crate::ui::util::UiTimer;
use crate::viewer::ViewMode;

/// Где пользователь был в файле: каретка (в символах), верхняя видимая позиция и режим.
#[derive(Debug, Clone, Copy)]
pub struct ViewState {
    pub caret: i32,
    pub top: i32,
    pub mode: ViewMode,
}

/// Строка карточки истории git: файл коммита.
#[derive(Debug, Clone)]
pub struct GitFileRow {
    pub path: String,
    pub file_id: Option<String>,
    pub title: String,
    pub detail: String,
    pub is_deleted: bool,
}

#[derive(Default)]
pub struct State {
    pub settings: AppSettings,
    pub catalog: Option<NeurodocCatalog>,
    pub project_id: Option<String>,
    pub current_id: Option<String>,
    pub last_saved_text: String,
    /// Перевод строки открытого файла — при сохранении возвращаем его же.
    pub crlf: bool,
    pub back: Vec<String>,
    pub forward: Vec<String>,
    pub view_states: HashMap<String, ViewState>,
    pub view_mode: ViewMode,
    pub diff_active: bool,
    pub diff_key: Option<String>,
    pub reading: ReadingProgress,
    pub last_query: String,
    pub clipboard_has_link: bool,
    pub broken_hidden_by_user: bool,
    pub find_matches: Vec<(usize, usize)>,
    pub find_current: Option<usize>,
    pub search_rows: Vec<(String, usize)>,
    pub backlink_rows: Vec<String>,
    pub orphan_rows: Vec<String>,
    pub broken_rows: Vec<(String, usize, usize)>,
    pub breadcrumb_rows: Vec<Vec<String>>,
    pub git: Option<GitService>,
    pub git_busy: bool,
    pub git_available: bool,
    pub git_branch: Option<String>,
    pub git_branches: Vec<String>,
    pub git_default_branch: Option<String>,
    pub git_commits: Vec<GitCommit>,
    pub git_selected: Option<GitCommit>,
    pub git_files: Vec<GitFileRow>,
    pub migration_prompt_open: bool,
    pub dialog_open: bool,
}

/// Действия окна: кнопки привязаны к ним по имени, доступность задаётся здесь.
pub struct Actions {
    pub back: gio::SimpleAction,
    pub forward: gio::SimpleAction,
    pub home: gio::SimpleAction,
    pub new_link: gio::SimpleAction,
    pub link_selection: gio::SimpleAction,
    pub new_file: gio::SimpleAction,
    pub open: gio::SimpleAction,
    pub copy_link: gio::SimpleAction,
    pub copy_content: gio::SimpleAction,
    pub broken: gio::SimpleAction,
    pub delete: gio::SimpleAction,
    pub reload: gio::SimpleAction,
    pub replace: gio::SimpleAction,
    pub replace_project: gio::SimpleAction,
    pub search: gio::SimpleAction,
    pub hotkeys: gio::SimpleAction,
    pub settings: gio::SimpleAction,
    pub projects: gio::SimpleAction,
    pub save: gio::SimpleAction,
    pub find: gio::SimpleAction,
}

pub struct Win {
    pub window: adw::ApplicationWindow,
    pub state: RefCell<State>,
    pub store: SettingsStore,
    pub reading_store: ReadingProgressStore,
    pub editor: Rc<Editor>,
    pub actions: Actions,
    css: gtk::CssProvider,

    title_project: gtk::Label,
    title_branch: gtk::Label,
    broken_count: gtk::Label,
    projects_label: gtk::Label,
    pub git_button: gtk::MenuButton,
    pub git_actions: gio::SimpleActionGroup,
    git_spinner: gtk::Spinner,
    search_entry: gtk::SearchEntry,
    pub toolbar_buttons: HashMap<&'static str, gtk::Widget>,

    content_stack: gtk::Stack,
    breadcrumbs_scroll: gtk::ScrolledWindow,
    breadcrumbs_box: gtk::Box,

    file_id_label: gtk::Label,
    raw_toggle: gtk::ToggleButton,
    format_toggles: Vec<(gtk::ToggleButton, ViewMode)>,
    suppress_toggles: Cell<bool>,
    editor_stack: gtk::Stack,
    viewer_box: gtk::Box,
    webview: RefCell<Option<webkit::WebView>>,

    find_bar: gtk::Box,
    find_entry: gtk::Entry,
    find_count: gtk::Label,

    search_card: gtk::Box,
    search_header: gtk::Label,
    search_list: gtk::ListBox,
    git_card: gtk::Box,
    git_back_button: gtk::Button,
    git_header: gtk::Label,
    git_since_toggle: gtk::ToggleButton,
    git_commits_list: gtk::ListBox,
    git_files_list: gtk::ListBox,
    git_empty: gtk::Label,
    backlinks_list: gtk::ListBox,
    backlinks_empty: gtk::Label,
    orphans_list: gtk::ListBox,
    orphans_empty: gtk::Label,
    broken_card: gtk::Box,
    broken_header: gtk::Label,
    broken_list: gtk::ListBox,
    reading_count: gtk::Label,
    reading_bar: gtk::ProgressBar,
    read_next: gtk::Button,
    read_mark: gtk::ToggleButton,
    read_reset: gtk::Button,
    suppress_read_mark: Cell<bool>,

    tip: gtk::Popover,
    tip_title: gtk::Label,
    tip_subtitle: gtk::Label,

    save_timer: UiTimer,
    watch_timer: UiTimer,
    tip_timer: UiTimer,
    read_timer: UiTimer,
    watcher: RefCell<Option<notify::RecommendedWatcher>>,
    /// Сколько раз каталог перечитывался с диска — для самопроверки слежения.
    reload_count: Cell<u32>,
}

/// Кнопка панели инструментов: только значок.
fn tool_button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("flat");
    b.add_css_class("nd-tool");
    b.set_tooltip_text(Some(tooltip));
    b.set_focus_on_click(false);
    b
}

/// Кнопка панели инструментов: значок и подпись.
fn tool_button_text(icon: &str, text: &str, tooltip: &str) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::new(Some(text)));
    let b = gtk::Button::builder().child(&content).build();
    b.add_css_class("flat");
    b.add_css_class("nd-tool");
    b.set_tooltip_text(Some(tooltip));
    b.set_focus_on_click(false);
    b
}

fn separator() -> gtk::Box {
    let s = gtk::Box::new(gtk::Orientation::Vertical, 0);
    s.add_css_class("nd-separator");
    s.set_size_request(1, 20);
    s.set_valign(gtk::Align::Center);
    s
}

fn small_label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// Колесо над горизонтальной полосой прокручивает её вбок (как ScrollSidewaysOnWheel).
fn scroll_sideways_on_wheel(scroll: &gtk::ScrolledWindow) {
    let ctl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
    let s = scroll.clone();
    ctl.connect_scroll(move |c, _, dy| {
        let h = s.hadjustment();
        let v = s.vadjustment();
        if h.upper() - h.page_size() <= 0.0 || v.upper() - v.page_size() > 0.5 {
            return glib::Propagation::Proceed;
        }
        let step = if c.unit() == gdk::ScrollUnit::Wheel { 60.0 } else { 1.0 };
        h.set_value(h.value() + dy * step);
        glib::Propagation::Stop
    });
    scroll.add_controller(ctl);
}

/// Карточка правой колонки с заголовком и списком.
fn list_card(title: &str, max_height: i32, closable: bool) -> (gtk::Box, gtk::Label, gtk::ListBox, gtk::Label, Option<gtk::Button>) {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("nd-pane-card");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let title_label = small_label(title, &["nd-card-title"]);
    title_label.set_hexpand(true);
    header.append(&title_label);
    let close = if closable {
        let b = tool_button("window-close-symbolic", "Скрыть");
        b.set_valign(gtk::Align::Center);
        header.append(&b);
        Some(b)
    } else {
        None
    };
    card.append(&header);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_activate_on_single_click(true);
    list.add_css_class("nd-list");
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&list)
        .build();
    if max_height > 0 {
        scroll.set_max_content_height(max_height);
    } else {
        scroll.set_vexpand(true);
    }
    card.append(&scroll);
    let empty = small_label("", &["nd-dim", "nd-size-12"]);
    empty.set_visible(false);
    card.append(&empty);
    (card, title_label, list, empty, close)
}

/// Строка списка: заголовок и вторая строка (id, подпись).
pub fn list_row(title: &str, subtitle: &str, subtitle_classes: &[&str]) -> gtk::ListBoxRow {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
    b.append(&small_label(title, &["nd-size-13"]));
    if !subtitle.is_empty() {
        b.append(&small_label(subtitle, subtitle_classes));
    }
    
    gtk::ListBoxRow::builder().child(&b).build()
}

pub fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

impl Win {
    pub fn build(app: &adw::Application, store: SettingsStore) -> Rc<Self> {
        let settings = store.load();
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Neurodoc Editor")
            .default_width(1440)
            .default_height(860)
            .icon_name("today.vab.NeurodocEditor")
            .build();

        let css_provider = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().expect("дисплей"),
            &css_provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let editor = Editor::new();

        // ---------- заголовок ----------
        let header = adw::HeaderBar::new();
        header.set_show_title(false);
        let title_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        if let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from_static(include_bytes!(
            "../../../../windows/NeurodocEditor/Assets/app-icon-32.png"
        ))) {
            let icon = gtk::Image::from_paintable(Some(&texture));
            icon.set_pixel_size(16);
            title_box.append(&icon);
        }
        title_box.append(&small_label("Neurodoc Editor", &["nd-size-13"]));
        title_box.append(&small_label(crate::VERSION, &["nd-dim", "nd-size-12"]));
        let title_project = small_label("", &["nd-dim", "nd-size-13"]);
        let title_branch = small_label("", &["nd-dim", "nd-size-12"]);
        title_box.append(&title_project);
        title_box.append(&title_branch);
        header.pack_start(&title_box);

        // ---------- панель инструментов ----------
        let mut toolbar_buttons: HashMap<&'static str, gtk::Widget> = HashMap::new();
        let left = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let mut add = |key: &'static str, b: gtk::Button, action: &str| {
            b.set_action_name(Some(action));
            left.append(&b);
            toolbar_buttons.insert(key, b.upcast());
        };
        add("back", tool_button("go-previous-symbolic", "Назад (Alt+←)"), "win.back");
        add("forward", tool_button("go-next-symbolic", "Вперёд (Alt+→)"), "win.forward");
        add("home", tool_button("go-home-symbolic", "Перейти к корневому файлу (Ctrl+H)"), "win.home");
        left.append(&separator());
        add(
            "new_link",
            tool_button_text("insert-link-symbolic", "Новая ссылка", "Вставить новую ссылку в позицию курсора и перейти к новому файлу (Ctrl+T)"),
            "win.new-link",
        );
        add(
            "link_selection",
            tool_button_text("edit-paste-symbolic", "Связать", "Связать выделенный текст со ссылкой из буфера обмена (Ctrl+L)"),
            "win.link-selection",
        );
        add("new_file", tool_button_text("document-new-symbolic", "Новый файл", "Создать новый файл, не привязанный к другим (Ctrl+N)"), "win.new-file");
        add("open", tool_button("document-open-symbolic", "Открыть файл по UUID или пути (Ctrl+O)"), "win.open");
        add("copy_link", tool_button("edit-copy-symbolic", "Копировать ссылку на текущий файл в буфер обмена"), "win.copy-link");
        add("copy_content", tool_button("edit-select-all-symbolic", "Копировать содержимое файла в буфер обмена"), "win.copy-content");
        left.append(&separator());
        let broken_count = gtk::Label::new(None);
        broken_count.add_css_class("nd-size-12");
        let broken_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        broken_content.append(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        broken_content.append(&broken_count);
        let broken_button = gtk::Button::builder().child(&broken_content).build();
        broken_button.add_css_class("flat");
        broken_button.add_css_class("nd-tool");
        broken_button.set_tooltip_text(Some("Битые ссылки в проекте (Ctrl+B)"));
        broken_button.set_focus_on_click(false);
        add("broken", broken_button, "win.broken-links");
        add("delete", tool_button("user-trash-symbolic", "Удалить текущий файл"), "win.delete");
        add("reload", tool_button("view-refresh-symbolic", "Перечитать проект с диска (F5)"), "win.reload");
        left.append(&separator());
        let git_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        git_content.append(&small_label("Git", &["nd-size-13"]));
        git_content.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
        let git_button = gtk::MenuButton::builder().child(&git_content).build();
        git_button.add_css_class("flat");
        git_button.add_css_class("nd-tool");
        git_button.set_tooltip_text(Some("Git: сохранение и получение изменений, ветки, история"));
        left.append(&git_button);
        let git_spinner = gtk::Spinner::new();
        git_spinner.set_margin_start(4);
        git_spinner.set_visible(false);
        left.append(&git_spinner);

        let right = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let replace_button = tool_button("edit-find-replace-symbolic", "Поиск и замена (Ctrl+R)");
        replace_button.set_action_name(Some("win.replace"));
        right.append(&replace_button);
        toolbar_buttons.insert("replace", replace_button.upcast());
        let search_entry = gtk::SearchEntry::new();
        search_entry.set_placeholder_text(Some("Поиск по проекту (Ctrl+Shift+F)"));
        search_entry.set_width_request(280);
        search_entry.set_valign(gtk::Align::Center);
        right.append(&search_entry);
        let hotkeys_button = tool_button("help-about-symbolic", "Горячие клавиши (F1)");
        hotkeys_button.set_action_name(Some("win.hotkeys"));
        right.append(&hotkeys_button);
        let settings_button = tool_button("emblem-system-symbolic", "Настройки");
        settings_button.set_action_name(Some("win.settings"));
        right.append(&settings_button);
        let projects_label = gtk::Label::new(Some("Проекты"));
        let projects_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        projects_content.append(&gtk::Image::from_icon_name("folder-symbolic"));
        projects_content.append(&projects_label);
        let projects_button = gtk::Button::builder().child(&projects_content).build();
        projects_button.add_css_class("flat");
        projects_button.add_css_class("nd-tool");
        projects_button.set_tooltip_text(Some("Проекты"));
        projects_button.set_action_name(Some("win.projects"));
        projects_button.set_focus_on_click(false);
        right.append(&projects_button);
        toolbar_buttons.insert("projects", projects_button.upcast());
        toolbar_buttons.insert("copy_link_target", toolbar_buttons["copy_link"].clone());

        let toolbar = gtk::CenterBox::new();
        toolbar.set_start_widget(Some(&left));
        toolbar.set_end_widget(Some(&right));
        toolbar.set_margin_start(12);
        toolbar.set_margin_end(12);
        toolbar.set_margin_top(2);
        toolbar.set_margin_bottom(8);
        let toolbar_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(&toolbar)
            .build();
        scroll_sideways_on_wheel(&toolbar_scroll);
        let toolbar_handle = gtk::WindowHandle::builder().child(&toolbar_scroll).build();

        // ---------- крошки ----------
        let breadcrumbs_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let breadcrumbs_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::External)
            .propagate_natural_height(true)
            .max_content_height(108)
            .child(&breadcrumbs_box)
            .build();
        breadcrumbs_scroll.set_visible(false);
        scroll_sideways_on_wheel(&breadcrumbs_scroll);

        // ---------- карточка редактора ----------
        let editor_card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        editor_card.add_css_class("nd-editor-card");
        editor_card.set_hexpand(true);
        editor_card.set_vexpand(true);
        let editor_header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        editor_header.set_margin_start(20);
        editor_header.set_margin_end(12);
        editor_header.set_margin_top(8);
        editor_header.set_margin_bottom(10);
        let file_id_label = small_label("", &["nd-dim", "nd-size-11", "nd-mono"]);
        file_id_label.set_max_width_chars(34);
        file_id_label.set_valign(gtk::Align::Center);
        editor_header.append(&file_id_label);
        let formats_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        formats_box.set_halign(gtk::Align::End);
        let raw_toggle = gtk::ToggleButton::with_label("Как есть");
        raw_toggle.add_css_class("nd-format-toggle");
        raw_toggle.set_focus_on_click(false);
        raw_toggle.set_tooltip_text(Some(
            "Показать файл без форматирования: ссылки не сворачиваются и не подсвечиваются, текст правится посимвольно",
        ));
        formats_box.append(&raw_toggle);
        let fsep = separator();
        fsep.set_size_request(1, 16);
        formats_box.append(&fsep);
        let mut format_toggles = Vec::new();
        for (mode, label, tip) in ViewMode::FORMATS {
            let t = gtk::ToggleButton::with_label(label);
            t.add_css_class("nd-format-toggle");
            t.set_tooltip_text(Some(tip));
            t.set_focus_on_click(false);
            formats_box.append(&t);
            format_toggles.push((t, mode));
        }
        let formats_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .child(&formats_box)
            .build();
        scroll_sideways_on_wheel(&formats_scroll);
        editor_header.append(&formats_scroll);
        editor_card.append(&editor_header);

        let editor_stack = gtk::Stack::new();
        editor_stack.set_vexpand(true);
        editor_stack.add_named(&editor.scroll, Some("editor"));
        let viewer_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        viewer_box.set_vexpand(true);
        editor_stack.add_named(&viewer_box, Some("viewer"));
        editor_stack.set_visible_child_name("editor");

        let find_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        find_bar.add_css_class("nd-findbar");
        find_bar.set_halign(gtk::Align::End);
        find_bar.set_valign(gtk::Align::Start);
        find_bar.set_margin_top(2);
        find_bar.set_margin_end(26);
        find_bar.set_visible(false);
        let find_entry = gtk::Entry::new();
        find_entry.set_placeholder_text(Some("Найти в файле"));
        find_entry.set_width_request(210);
        find_bar.append(&find_entry);
        let find_count = gtk::Label::new(None);
        find_count.add_css_class("nd-dim");
        find_count.add_css_class("nd-size-12");
        find_count.set_width_request(44);
        find_bar.append(&find_count);
        let find_prev = tool_button("go-up-symbolic", "Предыдущее (Shift+Enter)");
        let find_next = tool_button("go-down-symbolic", "Следующее (Enter)");
        let find_close = tool_button("window-close-symbolic", "Закрыть (Esc)");
        find_bar.append(&find_prev);
        find_bar.append(&find_next);
        find_bar.append(&find_close);

        let editor_overlay = gtk::Overlay::new();
        editor_overlay.set_child(Some(&editor_stack));
        editor_overlay.add_overlay(&find_bar);
        editor_card.append(&editor_overlay);

        // ---------- правая колонка ----------
        let right_column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        right_column.set_width_request(300);
        right_column.set_hexpand(false);
        let (search_card, search_header, search_list, _search_empty, search_close) = list_card("Результаты поиска", 290, true);
        search_card.set_visible(false);
        right_column.append(&search_card);

        let (git_card, git_header, git_commits_list, git_empty, git_close) = list_card("История", 290, true);
        git_card.set_visible(false);
        let git_back_button = tool_button("go-previous-symbolic", "Назад к списку коммитов");
        git_back_button.set_visible(false);
        git_header.parent().and_downcast::<gtk::Box>().inspect(|h| {
            h.prepend(&git_back_button);
        });
        let git_since_toggle = gtk::ToggleButton::with_label("Все изменения с этого коммита");
        git_since_toggle.add_css_class("nd-small-button");
        git_since_toggle.set_visible(false);
        git_since_toggle.set_halign(gtk::Align::Start);
        git_card.insert_child_after(&git_since_toggle, git_card.first_child().as_ref());
        let git_files_list = gtk::ListBox::new();
        git_files_list.set_selection_mode(gtk::SelectionMode::None);
        git_files_list.set_activate_on_single_click(true);
        git_files_list.add_css_class("nd-list");
        git_files_list.set_visible(false);
        // ScrolledWindow оборачивает список в Viewport — окно прокрутки ищем выше по дереву
        let mut ancestor = git_commits_list.parent();
        while let Some(a) = ancestor.clone() {
            if let Ok(scroll) = a.clone().downcast::<gtk::ScrolledWindow>() {
                let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
                scroll.set_child(None::<&gtk::Widget>);
                holder.append(&git_commits_list);
                holder.append(&git_files_list);
                scroll.set_child(Some(&holder));
                break;
            }
            ancestor = a.parent();
        }
        right_column.append(&git_card);

        let (backlinks_card, _, backlinks_list, backlinks_empty, _) = list_card("Ссылаются на этот файл", 0, false);
        backlinks_card.set_vexpand(true);
        right_column.append(&backlinks_card);
        let (orphans_card, _, orphans_list, orphans_empty, _) = list_card("Файлы без входящих ссылок", 250, false);
        right_column.append(&orphans_card);
        let (broken_card, broken_header, broken_list, _, broken_close) = list_card("Битые ссылки", 210, true);
        broken_card.set_visible(false);
        right_column.append(&broken_card);

        let reading_card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        reading_card.add_css_class("nd-pane-card");
        reading_card.append(&small_label("Чтение", &["nd-card-title"]));
        let reading_count = small_label("", &["nd-dim", "nd-size-12"]);
        reading_card.append(&reading_count);
        let reading_bar = gtk::ProgressBar::new();
        reading_bar.add_css_class("nd-progress");
        reading_card.append(&reading_bar);
        let reading_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let read_next = gtk::Button::with_label("Далее");
        read_next.add_css_class("nd-small-button");
        read_next.set_tooltip_text(Some("Открыть следующий непрочитанный файл"));
        let read_mark = gtk::ToggleButton::with_label("Прочитано");
        read_mark.add_css_class("nd-small-button");
        read_mark.set_tooltip_text(Some("Отметка о прочтении текущего файла"));
        let read_reset = gtk::Button::with_label("Сбросить");
        read_reset.add_css_class("nd-small-button");
        read_reset.set_tooltip_text(Some("Забыть прогресс чтения этого проекта"));
        for b in [&read_next, &read_reset] {
            b.set_focus_on_click(false);
        }
        read_mark.set_focus_on_click(false);
        reading_buttons.append(&read_next);
        reading_buttons.append(&read_mark);
        reading_buttons.append(&read_reset);
        reading_card.append(&reading_buttons);
        right_column.append(&reading_card);

        // ---------- содержимое ----------
        let right_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&right_column)
            .build();
        let columns = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        columns.set_vexpand(true);
        columns.append(&editor_card);
        columns.append(&right_scroll);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.set_margin_bottom(12);
        content.append(&breadcrumbs_scroll);
        content.append(&columns);

        let no_project = adw::StatusPage::builder()
            .icon_name("folder-open-symbolic")
            .title("Проект не выбран")
            .description("Добавьте проект — название и путь к каталогу neurodoc.")
            .build();
        let open_projects = gtk::Button::with_label("Открыть список проектов");
        open_projects.add_css_class("suggested-action");
        open_projects.add_css_class("pill");
        open_projects.set_halign(gtk::Align::Center);
        open_projects.set_action_name(Some("win.projects"));
        no_project.set_child(Some(&open_projects));

        let content_stack = gtk::Stack::new();
        content_stack.add_named(&no_project, Some("noproject"));
        content_stack.add_named(&content, Some("content"));
        content_stack.set_visible_child_name("noproject");

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.add_top_bar(&toolbar_handle);
        toolbar_view.set_top_bar_style(adw::ToolbarStyle::Flat);
        toolbar_view.set_content(Some(&content_stack));
        window.set_content(Some(&toolbar_view));

        // ---------- подсказка ----------
        let tip = gtk::Popover::new();
        tip.set_autohide(false);
        tip.set_position(gtk::PositionType::Bottom);
        let tip_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let tip_title = small_label("", &["nd-tip-title"]);
        let tip_subtitle = small_label("", &["nd-dim", "nd-size-12"]);
        tip_subtitle.set_wrap(true);
        tip_subtitle.set_max_width_chars(48);
        tip_box.append(&tip_title);
        tip_box.append(&tip_subtitle);
        tip.set_child(Some(&tip_box));

        let actions = shortcuts::make_actions(&window);

        let win = Rc::new(Self {
            window,
            state: RefCell::new(State { settings, git_available: true, ..Default::default() }),
            store,
            reading_store: ReadingProgressStore::new(),
            editor,
            actions,
            css: css_provider,
            title_project,
            title_branch,
            broken_count,
            projects_label,
            git_button,
            git_actions: gio::SimpleActionGroup::new(),
            git_spinner,
            search_entry,
            toolbar_buttons,
            content_stack,
            breadcrumbs_scroll,
            breadcrumbs_box,
            file_id_label,
            raw_toggle,
            format_toggles,
            suppress_toggles: Cell::new(false),
            editor_stack,
            viewer_box,
            webview: RefCell::new(None),
            find_bar,
            find_entry,
            find_count,
            search_card,
            search_header,
            search_list,
            git_card,
            git_back_button,
            git_header,
            git_since_toggle,
            git_commits_list,
            git_files_list,
            git_empty,
            backlinks_list,
            backlinks_empty,
            orphans_list,
            orphans_empty,
            broken_card,
            broken_header,
            broken_list,
            reading_count,
            reading_bar,
            read_next,
            read_mark,
            read_reset,
            suppress_read_mark: Cell::new(false),
            tip,
            tip_title,
            tip_subtitle,
            save_timer: UiTimer::new(Duration::from_millis(700)),
            watch_timer: UiTimer::new(Duration::from_millis(500)),
            tip_timer: UiTimer::new(Duration::from_millis(1400)),
            read_timer: UiTimer::new(Duration::from_secs(3)),
            watcher: RefCell::new(None),
            reload_count: Cell::new(0),
        });

        // ---------- связи ----------
        shortcuts::wire(&win, &toolbar_view);
        if let Some(b) = search_close {
            let w = win.clone();
            b.connect_clicked(move |_| w.search_card.set_visible(false));
        }
        if let Some(b) = git_close {
            let w = win.clone();
            b.connect_clicked(move |_| w.git_card.set_visible(false));
        }
        if let Some(b) = broken_close {
            let w = win.clone();
            b.connect_clicked(move |_| {
                w.state.borrow_mut().broken_hidden_by_user = true;
                w.broken_card.set_visible(false);
            });
        }
        let w = win.clone();
        find_prev.connect_clicked(move |_| w.find_step(-1));
        let w = win.clone();
        find_next.connect_clicked(move |_| w.find_step(1));
        let w = win.clone();
        find_close.connect_clicked(move |_| w.close_find_bar());
        let w = win.clone();
        win.find_entry.connect_changed(move |_| w.find_step(0));
        let w = win.clone();
        win.find_entry.connect_activate(move |_| w.find_step(1));
        let key = gtk::EventControllerKey::new();
        let w = win.clone();
        key.connect_key_pressed(move |_, keyval, _, state| {
            if keyval == gdk::Key::Escape {
                w.close_find_bar();
                return glib::Propagation::Stop;
            }
            if (keyval == gdk::Key::Return || keyval == gdk::Key::KP_Enter) && state.contains(gdk::ModifierType::SHIFT_MASK) {
                w.find_step(-1);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        win.find_entry.add_controller(key);

        let w = win.clone();
        win.search_entry.connect_activate(move |e| w.run_project_search(&e.text()));
        let w = win.clone();
        win.search_entry.connect_stop_search(move |_| w.editor.grab_focus());

        let w = win.clone();
        win.raw_toggle.connect_toggled(move |t| w.on_raw_toggle(t.is_active()));
        for (toggle, mode) in &win.format_toggles {
            let w = win.clone();
            let mode = *mode;
            toggle.connect_toggled(move |t| w.on_format_toggle(mode, t.is_active()));
        }

        let w = win.clone();
        win.read_next.connect_clicked(move |_| w.on_read_next());
        let w = win.clone();
        win.read_mark.connect_toggled(move |t| w.on_read_mark_toggled(t.is_active()));
        let w = win.clone();
        win.read_reset.connect_clicked(move |_| w.on_read_reset());
        let w = win.clone();
        win.git_back_button.connect_clicked(move |_| w.show_git_commits_pane());
        let w = win.clone();
        win.git_since_toggle.connect_toggled(move |_| w.load_git_files());

        win.wire_lists();
        win.wire_editor();
        win.wire_git_menu();

        let w = win.clone();
        win.window.connect_close_request(move |_| {
            w.save_current_file();
            glib::Propagation::Proceed
        });
        let w = win.clone();
        win.window.connect_is_active_notify(move |window| {
            if window.is_active() {
                w.refresh_clipboard_state();
            }
        });
        let w = win.clone();
        win.window.clipboard().connect_changed(move |_| w.refresh_clipboard_state());
        let w = win.clone();
        adw::StyleManager::default().connect_dark_notify(move |_| w.on_theme_changed());

        win.apply_theme();
        win.apply_editor_font();
        win.update_project_ui();
        win.refresh_clipboard_state();
        win
    }

    /// Показывает окно и открывает текущий проект (или список проектов).
    pub fn present(self: &Rc<Self>) {
        self.window.present();
        let project = self.state.borrow().settings.current_project().cloned();
        match project {
            Some(p) => self.open_project(&p),
            None => {
                let w = self.clone();
                glib::idle_add_local_once(move || w.show_projects_dialog());
            }
        }
    }

    fn wire_editor(self: &Rc<Self>) {
        let w = self.clone();
        self.editor.set_on_changed(move || w.on_editor_text_changed());
        let w = self.clone();
        self.editor.set_on_selection_changed(move || w.update_link_selection_action());
        let w = self.clone();
        self.editor.set_on_link_activated(move |link| {
            if link.is_web {
                w.launch_web_link(&link.target);
            } else {
                w.open_file(&link.target, true);
            }
        });
        let w = self.clone();
        self.editor.set_tooltip_provider(move |link| if link.is_web { link.target.clone() } else { w.target_tip(&link.target) });
    }

    // ================= Тема и шрифт =================

    pub fn is_dark(&self) -> bool {
        adw::StyleManager::default().is_dark()
    }

    /// Шаг самопроверки: переключить тему.
    pub fn smoke_set_theme(&self, theme: ThemeMode) {
        self.state.borrow_mut().settings.theme = theme;
        self.apply_theme();
    }

    /// Самопроверка слежения за диском: число перечитываний, корень каталога и текст
    /// файла в памяти каталога.
    pub fn smoke_reload_count(&self) -> u32 {
        self.reload_count.get()
    }

    pub fn smoke_catalog_root(&self) -> Option<PathBuf> {
        self.state.borrow().catalog.as_ref().map(|c| c.root_path().to_path_buf())
    }

    pub fn smoke_catalog_text(&self, id: &str) -> String {
        self.state.borrow().catalog.as_ref().map(|c| c.read_file(id).into_owned()).unwrap_or_default()
    }

    /// Первый по порядку id файл `_content`, который начинается с `<svg`, — схема для шага
    /// самопроверки просмотра SVG (у таких файлов нет расширения, режим включают кнопкой).
    pub fn smoke_first_svg(&self) -> Option<String> {
        let state = self.state.borrow();
        let catalog = state.catalog.as_ref()?;
        catalog
            .all_ids()
            .into_iter()
            .find(|id| catalog::is_uuid(id) && catalog.read_file(id).trim_start().starts_with("<svg"))
    }

    /// Шаг самопроверки: тумблер «Как есть», как если бы его нажали.
    pub fn smoke_set_raw(&self, on: bool) {
        self.raw_toggle.set_active(on);
    }

    pub fn apply_theme(&self) {
        let theme = self.state.borrow().settings.theme;
        adw::StyleManager::default().set_color_scheme(match theme {
            ThemeMode::Light => adw::ColorScheme::ForceLight,
            ThemeMode::Dark => adw::ColorScheme::ForceDark,
            ThemeMode::System => adw::ColorScheme::Default,
        });
        self.on_theme_changed();
    }

    fn on_theme_changed(&self) {
        let dark = self.is_dark();
        self.css.load_from_string(&css::stylesheet(dark));
        self.editor.apply_colors(dark);
        self.refresh_viewer_if_active();
    }

    pub fn apply_editor_font(&self) {
        let (family, size, spacing) = {
            let s = &self.state.borrow().settings;
            (s.font_family.clone(), s.font_size, s.line_spacing)
        };
        self.editor.apply_font(&family, size, spacing);
        self.refresh_viewer_if_active();
    }

    pub fn save_settings(&self) {
        let settings = self.state.borrow().settings.clone();
        if let Err(e) = self.store.save(&settings) {
            crate::ui::util::log_error(&format!("не удалось сохранить настройки: {e}"));
        }
    }

    // ================= Проекты =================

    pub fn open_project(self: &Rc<Self>, project: &ProjectInfo) {
        self.save_current_file();
        self.read_timer.stop();
        let opened = NeurodocCatalog::open_or_create(Path::new(&project.path));
        {
            let mut st = self.state.borrow_mut();
            st.project_id = Some(project.id.clone());
            st.reading = self.reading_store.load(&project.id);
            st.current_id = None;
            st.back.clear();
            st.forward.clear();
            st.view_states.clear();
            st.catalog = None;
        }
        self.editor.set_visited(self.visited_set());
        self.update_nav_actions();
        self.search_card.set_visible(false);
        let path = match opened {
            Ok(catalog) => {
                self.state.borrow_mut().catalog = Some(catalog);
                // прочитанные ссылки красятся серым с первого же файла, а не после первой отметки
                self.editor.set_visited(self.visited_set());
                Some(PathBuf::from(&project.path))
            }
            Err(e) => {
                self.show_error("Не удалось открыть проект", &e.to_string());
                None
            }
        };
        self.update_project_ui();
        self.setup_git(path.as_deref());
        self.start_watching(path.as_deref());
        if path.is_some() {
            self.open_file(ROOT_ID, false);
            let w = self.clone();
            let project = project.clone();
            glib::idle_add_local_once(move || w.prompt_migration(&project));
        }
    }

    /// Следит за каталогом на диске: файлы меняют и другие программы — git, внешний
    /// редактор. Без этого редактор показывал бы устаревшее содержимое.
    fn start_watching(self: &Rc<Self>, path: Option<&Path>) {
        use notify::Watcher;
        self.watch_timer.stop();
        *self.watcher.borrow_mut() = None;
        let Some(path) = path else { return };
        if !path.is_dir() {
            return;
        }
        let (tx, rx) = async_channel::unbounded::<notify::Event>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.send_blocking(event);
            }
        });
        let Ok(mut watcher) = watcher else { return };
        if watcher.watch(path, notify::RecursiveMode::Recursive).is_err() {
            return; // без слежения приложение работает — остаётся кнопка «Обновить»
        }
        *self.watcher.borrow_mut() = Some(watcher);
        let w = self.clone();
        let root = path.to_path_buf();
        glib::spawn_future_local(async move {
            while let Ok(event) = rx.recv().await {
                if w.watcher.borrow().is_none() {
                    break;
                }
                w.on_catalog_changed_on_disk(&root, &event);
            }
        });
    }

    fn on_catalog_changed_on_disk(self: &Rc<Self>, root: &Path, event: &notify::Event) {
        // inotify сообщает и об открытии/закрытии файла на чтение, а само перечитывание
        // открывает каждый файл каталога: без этого фильтра оно запускало себя по кругу
        // раз в полсекунды, и открытый просмотр (MD, SVG) каждый раз сбрасывался в начало.
        // На Windows FileSystemWatcher чтения не видит вовсе (NotifyFilter — запись и имена).
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return;
        }
        // git постоянно пишет в .git (fetch, статус, локи) — к содержимому каталога
        // это отношения не имеет, а событий порождает тысячи
        // переполнение очереди inotify приходит без путей, но значит «что-то упущено»
        let relevant = event.need_rescan() || event.paths.iter().any(|p| {
            let rel = p.strip_prefix(root).unwrap_or(p);
            let first = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned());
            first.as_deref() != Some(".git")
        });
        if !relevant {
            return;
        }
        let st = self.state.borrow();
        // во время своей git-операции слежение молчит: перечитаем сами по её концу
        if st.git_busy {
            return;
        }
        // Собственная запись поднимает то же событие, что и чужая. Отличаем по содержимому:
        // если для всех путей события диск совпадает с каталогом в памяти, перечитывать
        // нечего. Когда перечитывание уже назначено, сверять незачем — только отодвигаем его.
        // Открытый файл каталог может и не держать в памяти: файл с точкой в пути или
        // большой, открытый по ссылке, читается с диска мимо неё. Перечитывание сверяет его
        // с последним сохранённым текстом — так же и здесь.
        let unchanged = !self.watch_timer.is_pending()
            && !event.need_rescan()
            && st.catalog.as_ref().is_some_and(|c| {
                let current = st.current_id.as_ref().and_then(|id| Some((id, c.file_path(id).ok()?)));
                event.paths.iter().all(|p| {
                    c.matches_disk(p)
                        && !current.as_ref().is_some_and(|(id, cur)| cur == p && c.read_file(id).as_ref() != st.last_saved_text)
                })
            });
        drop(st);
        if unchanged {
            return;
        }
        let w = self.clone();
        self.watch_timer.restart(move || {
            w.watch_timer.forget();
            w.reload_from_disk(false);
        });
    }

    /// Перечитывает каталог с диска, подхватывая правки других программ.
    pub fn reload_from_disk(self: &Rc<Self>, manual: bool) {
        self.watch_timer.stop();
        self.reload_count.set(self.reload_count.get() + 1);
        let has = self.state.borrow().catalog.is_some();
        if !has {
            return;
        }
        {
            let mut st = self.state.borrow_mut();
            if let Some(c) = st.catalog.as_mut() {
                c.reload();
            }
        }
        let (current, on_disk, last_saved) = {
            let st = self.state.borrow();
            let current = st.current_id.clone();
            let on_disk = current.as_ref().and_then(|id| st.catalog.as_ref().map(|c| c.read_file(id).into_owned()));
            (current, on_disk, st.last_saved_text.clone())
        };
        if let (Some(_), Some(on_disk)) = (current, on_disk) {
            // свои несохранённые правки чужим содержимым не затираем: пусть автосохранение
            // сначала доедет, внешнее изменение подхватится следующим событием
            if self.editor_text_for_save() == last_saved && on_disk != last_saved {
                let caret = self.editor.caret();
                self.load_text_into_editor(&on_disk);
                let max = self.editor.buffer.char_count();
                self.editor.place_cursor(&self.editor.iter_at_offset(caret.clamp(0, max)));
            }
        }
        self.update_panels();
        self.update_file_ui();
        // Просмотр не перерисовываем: он строится только из текста открытого файла.
        // Изменившийся текст загружен заново и вернул режим к тексту (так и на Windows),
        // а неизменный перерисовка лишь отбросила бы страницу к началу.
        if manual {
            let path = self.state.borrow().catalog.as_ref().map(|c| c.root_path().display().to_string()).unwrap_or_default();
            self.show_tip("reload", "Проект перечитан", &path);
        }
    }

    /// Предлагает перевести каталог на формат [текст](uuid), если в нём остались ссылки
    /// старого вида. Файлы пользователя переписываются только после подтверждения и
    /// только после резервной копии.
    fn prompt_migration(self: &Rc<Self>, project: &ProjectInfo) {
        let scan = {
            let st = self.state.borrow();
            if st.migration_prompt_open {
                return;
            }
            let Some(c) = st.catalog.as_ref() else { return };
            migration::scan(c)
        };
        if !scan.needs_migration() {
            return;
        }
        self.state.borrow_mut().migration_prompt_open = true;
        let w = self.clone();
        let project = project.clone();
        glib::spawn_future_local(async move {
            let body = format!(
                "Ссылок старого вида: {} в {} файлах — их нужно перевести в формат [текст](uuid).\n\n\
                 Перед изменением будет создана резервная копия всего каталога.",
                scan.links, scan.files
            );
            let answer = w
                .ask("Каталог в старом формате", &body, &[("later", "Не сейчас", false), ("migrate", "Преобразовать", true)], "migrate", "later")
                .await;
            if answer == "migrate" {
                let result = {
                    let mut st = w.state.borrow_mut();
                    match st.catalog.as_mut() {
                        Some(c) => migration::migrate(c, &project.id),
                        None => return,
                    }
                };
                match result {
                    Ok(result) => {
                        // содержимое файлов изменилось — перечитываем каталог и открываем root заново
                        {
                            let mut st = w.state.borrow_mut();
                            if let Some(c) = st.catalog.as_mut() {
                                c.reload();
                            }
                            st.current_id = None;
                            st.back.clear();
                            st.forward.clear();
                        }
                        w.update_nav_actions();
                        w.open_file(ROOT_ID, false);
                        let mut done = Vec::new();
                        if result.links > 0 {
                            done.push(format!("Ссылок переведено: {} в {} файлах.", result.links, result.files));
                        }
                        if result.readme_updated {
                            done.push("Обновлено описание формата в ReadMe.md.".to_string());
                        }
                        done.push(String::new());
                        done.push(format!("Резервная копия:\n{}", result.backup_path.display()));
                        w.show_message("Каталог преобразован", &done.join("\n"), "Готово").await;
                    }
                    Err(e) => w.show_error("Не удалось преобразовать каталог", &e.to_string()),
                }
            }
            w.state.borrow_mut().migration_prompt_open = false;
        });
    }

    pub fn update_project_ui(&self) {
        let (has, name) = {
            let st = self.state.borrow();
            (st.catalog.is_some(), st.settings.current_project().map(|p| p.name.clone()))
        };
        self.content_stack.set_visible_child_name(if has { "content" } else { "noproject" });
        self.title_project.set_text(&match (&name, has) {
            (Some(n), true) => format!("·  {n}"),
            _ => String::new(),
        });
        self.projects_label.set_text(name.as_deref().filter(|_| has).unwrap_or("Проекты"));
        for a in [
            &self.actions.home,
            &self.actions.new_link,
            &self.actions.new_file,
            &self.actions.search,
            &self.actions.replace,
            &self.actions.replace_project,
            &self.actions.broken,
            &self.actions.open,
            &self.actions.reload,
            &self.actions.save,
            &self.actions.find,
        ] {
            a.set_enabled(has);
        }
        self.search_entry.set_sensitive(has);
        self.update_link_selection_action();
        self.raw_toggle.set_sensitive(has);
        self.sync_raw_toggle();
        for (t, _) in &self.format_toggles {
            t.set_sensitive(has);
        }
        if !has {
            self.actions.copy_link.set_enabled(false);
            self.actions.copy_content.set_enabled(false);
            self.actions.delete.set_enabled(false);
            self.actions.back.set_enabled(false);
            self.actions.forward.set_enabled(false);
        }
    }

    // ================= Открытие и сохранение файлов =================

    pub fn open_file(self: &Rc<Self>, id: &str, push: bool) {
        // в нижний регистр приводим только UUID: у пути регистр — часть имени файла
        let id = if catalog::is_uuid(id) { id.to_lowercase() } else { id.to_string() };
        {
            let st = self.state.borrow();
            if st.catalog.is_none() {
                return;
            }
            if st.current_id.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&id)) {
                return;
            }
        }
        self.save_current_file();
        let text = {
            let mut st = self.state.borrow_mut();
            if let Some(cur) = st.current_id.clone() {
                let vs = ViewState { caret: self.editor.caret(), top: self.top_visible_offset(), mode: st.view_mode };
                st.view_states.insert(cur.to_lowercase(), vs);
                if push {
                    st.back.push(cur);
                    st.forward.clear(); // как в браузере: новая навигация обнуляет «вперёд»
                }
            }
            st.current_id = Some(id.clone());
            st.catalog.as_ref().map(|c| c.read_file(&id).into_owned()).unwrap_or_default()
        };
        self.editor.set_source_id(&id);
        self.load_text_into_editor(&text);
        // отметку о прочтении ставим не сразу: случайный переход не должен её засчитать
        let w = self.clone();
        self.read_timer.restart(move || {
            w.read_timer.forget();
            w.mark_current_read(true);
        });
        self.update_panels();
        self.update_file_ui();
        self.update_nav_actions();
        if self.find_bar.is_visible() {
            self.update_find_count(None);
        }
        // файл в собственном формате показываем сразу отрендеренным; режим, выбранный
        // пользователем для этого файла, важнее расширения
        let mode = self
            .state
            .borrow()
            .view_states
            .get(&id.to_lowercase())
            .map(|v| v.mode)
            .unwrap_or_else(|| ViewMode::detect(&id));
        if mode != ViewMode::Text {
            self.set_view_mode(mode);
            return;
        }
        self.editor.grab_focus();
        self.restore_view(&id);
    }

    /// Символ в верхней видимой строке редактора — отпечаток прокрутки для ViewState.
    fn top_visible_offset(&self) -> i32 {
        let rect = self.editor.view.visible_rect();
        let (iter, _) = self.editor.view.line_at_y(rect.y());
        iter.offset()
    }

    /// Возвращает каретку и прокрутку на последнее место в файле (в рамках сеанса).
    fn restore_view(&self, id: &str) {
        let Some(state) = self.state.borrow().view_states.get(&id.to_lowercase()).copied() else { return };
        if state.caret <= 0 && state.top <= 0 {
            return;
        }
        let max = self.editor.buffer.char_count();
        self.editor.place_cursor(&self.editor.iter_at_offset(state.caret.clamp(0, max)));
        self.editor.scroll_offset_to_top(state.top);
    }

    pub fn load_text_into_editor(&self, text: &str) {
        // стиль перевода строки запоминаем, чтобы сохранение не переписывало файл целиком
        self.state.borrow_mut().crlf = text.contains("\r\n");
        self.reset_view_mode();
        // одиночный \r GtkTextBuffer тоже считает переводом строки, а наши построчные
        // смещения — нет; нормализуем его сразу (RichEdit на Windows делает то же)
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.editor.load_text(&normalized);
        self.state.borrow_mut().last_saved_text = self.editor_text_for_save();
        if self.find_bar.is_visible() {
            self.update_find_count(None);
        }
    }

    /// Текст редактора в стиле перевода строк файла.
    pub fn editor_text_for_save(&self) -> String {
        let text = self.editor.raw_text().replace("\r\n", "\n").replace('\r', "\n");
        if self.state.borrow().crlf { text.replace('\n', "\r\n") } else { text }
    }

    pub fn save_current_file(self: &Rc<Self>) {
        self.save_timer.stop();
        let current = {
            let st = self.state.borrow();
            if st.catalog.is_none() {
                return;
            }
            st.current_id.clone()
        };
        let Some(id) = current else { return };
        let text = self.editor_text_for_save();
        if text == self.state.borrow().last_saved_text {
            return;
        }
        let result = {
            let mut st = self.state.borrow_mut();
            let r = st.catalog.as_mut().map(|c| c.write_file(&id, &text));
            if r.as_ref().is_some_and(|r| r.is_ok()) {
                st.last_saved_text = text;
            }
            r
        };
        if let Some(Err(e)) = result {
            self.show_error("Не удалось сохранить файл", &e.to_string());
            return;
        }
        self.update_panels();
    }

    fn on_editor_text_changed(self: &Rc<Self>) {
        if self.find_bar.is_visible() {
            // текущее совпадение после правки не сохраняем — как UpdateFindCount() на Windows
            self.update_find_count(None);
        }
        let w = self.clone();
        self.save_timer.restart(move || {
            w.save_timer.forget();
            w.save_current_file();
        });
    }

    // ================= Навигация =================

    pub fn go_history(self: &Rc<Self>, back: bool) {
        loop {
            let next = {
                let mut st = self.state.borrow_mut();
                let Some(id) = (if back { st.back.pop() } else { st.forward.pop() }) else { break };
                let exists = st.catalog.as_ref().is_some_and(|c| c.exists(&id));
                if !exists {
                    continue; // файл исчез — история про это не знает
                }
                if let Some(cur) = st.current_id.clone() {
                    if back { st.forward.push(cur) } else { st.back.push(cur) }
                }
                id
            };
            self.open_file(&next, false);
            break;
        }
        self.update_nav_actions();
    }

    pub fn remove_from_history(&self, id: &str) {
        let mut st = self.state.borrow_mut();
        st.back.retain(|x| x != id);
        st.forward.retain(|x| x != id);
        drop(st);
        self.update_nav_actions();
    }

    pub fn update_nav_actions(&self) {
        let st = self.state.borrow();
        let can = |h: &Vec<String>| st.catalog.as_ref().is_some_and(|c| h.iter().any(|id| c.exists(id)));
        self.actions.back.set_enabled(can(&st.back));
        self.actions.forward.set_enabled(can(&st.forward));
    }

    pub fn update_file_ui(&self) {
        let st = self.state.borrow();
        let has = st.catalog.is_some() && st.current_id.is_some();
        let is_root = st.current_id.as_deref() == Some(ROOT_ID);
        self.file_id_label.set_text(st.current_id.as_deref().unwrap_or(""));
        self.actions.copy_link.set_enabled(has && !is_root);
        self.actions.copy_content.set_enabled(has);
        self.actions.delete.set_enabled(has && !is_root);
    }

    pub fn visited_set(&self) -> HashSet<String> {
        let st = self.state.borrow();
        st.catalog
            .as_ref()
            .map(|c| c.all_ids().into_iter().filter(|id| st.reading.is_read(id)).map(|id| id.to_lowercase()).collect())
            .unwrap_or_default()
    }

    /// Чем подписать ссылку в подсказке: заголовком цели, а для битой — её id.
    pub fn target_tip(&self, id: &str) -> String {
        let st = self.state.borrow();
        st.catalog.as_ref().map(|c| c.title(id, 60)).unwrap_or_else(|| id.to_string())
    }

    pub fn launch_web_link(&self, url: &str) {
        let launcher = gtk::UriLauncher::new(url);
        launcher.launch(Some(&self.window), None::<&gio::Cancellable>, |_| {});
    }

    // ================= Диалоги-сообщения =================

    /// Вопрос с кнопками: (id, подпись, suggested). Возвращает id ответа.
    pub async fn ask(&self, title: &str, body: &str, buttons: &[(&str, &str, bool)], default: &str, close: &str) -> String {
        let dialog = adw::AlertDialog::new(Some(title), Some(body));
        for (id, label, suggested) in buttons {
            dialog.add_response(id, label);
            if *suggested {
                dialog.set_response_appearance(id, adw::ResponseAppearance::Suggested);
            }
        }
        dialog.set_default_response(Some(default));
        dialog.set_close_response(close);
        dialog.choose_future(Some(&self.window)).await.to_string()
    }

    pub async fn show_message(&self, title: &str, body: &str, close_text: &str) {
        let dialog = adw::AlertDialog::new(Some(title), Some(body));
        dialog.add_response("close", close_text);
        dialog.set_default_response(Some("close"));
        dialog.set_close_response("close");
        let _ = dialog.choose_future(Some(&self.window)).await;
    }

    pub fn show_error(&self, title: &str, body: &str) {
        let dialog = adw::AlertDialog::new(Some(title), Some(body));
        dialog.add_response("close", "Закрыть");
        dialog.set_default_response(Some("close"));
        dialog.set_close_response("close");
        dialog.present(Some(&self.window));
    }
}
