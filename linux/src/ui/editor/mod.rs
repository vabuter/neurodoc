//! Редактор: GtkTextView с компактными ссылками на тегах.
//!
//! Из `[текст](цель)` скрывается `(цель)` (тег `invisible`), «[» и «]» остаются в
//! раскладке, но рисуются прозрачными — так у каретки ровно четыре остановки вокруг
//! ссылки (перед «[», начало якоря, конец якоря, после «)») и ни одной внутри цели.
//! Якорь подчёркнут цветом акцента, прочитанный — серым, веб-адрес виден целиком.
//! Ссылка без якоря рисуется кружком (см. `ndview.rs`).
//!
//! Правила, проверенные спайком (examples/spike.rs):
//! - теги ставятся не в обработчиках `insert-text`/`delete-range`, а построчно в idle:
//!   регулярка ссылки не пересекает перевод строки, поэтому хватает затронутых строк;
//! - каретка не стоит перед скрытым и внутри него — страж в `mark-set` переносит её
//!   за ссылку синхронно: отложенный перенос проиграл бы быстрому набору;
//! - копирование, вырезание и сохранение берут текст с `include_hidden_chars = true`,
//!   иначе цели ссылок молча пропадут;
//! - `set_text` необратим: история undo не переживает загрузку файла;
//! - в режиме «как есть» теги снимаются явно, подсветка поиска остаётся.

pub mod ndview;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};

use crate::domain::links::{LinkSpan, scan_links};
use crate::ui::css;

use ndview::NdTextView;

const INDENT_UNIT: &str = "    ";

struct Tags {
    bracket: gtk::TextTag,
    hidden: gtk::TextTag,
    link: gtk::TextTag,
    visited: gtk::TextTag,
    web: gtk::TextTag,
    hover: gtk::TextTag,
    find: gtk::TextTag,
    find_current: gtk::TextTag,
    spacing: gtk::TextTag,
}

/// Ссылка на строке буфера: номер строки и позиции в байтах от начала строки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineLink {
    pub line: i32,
    pub span: LinkSpan,
}

pub struct Editor {
    pub view: NdTextView,
    pub buffer: gtk::TextBuffer,
    pub scroll: gtk::ScrolledWindow,
    tags: Tags,
    source_id: RefCell<String>,
    visited: RefCell<HashSet<String>>,
    raw: Cell<bool>,
    dark: Cell<bool>,
    font_size: Cell<f64>,
    dirty_start: gtk::TextMark,
    dirty_end: gtk::TextMark,
    /// Метка для возврата прокрутки: scroll_to_mark ждёт, пока раскладка досчитается.
    restore_mark: gtk::TextMark,
    dirty: Cell<bool>,
    retag_scheduled: Cell<bool>,
    hover: RefCell<Option<LineLink>>,
    pending_raw_cut: RefCell<Option<String>>,
    suppress: Cell<bool>,
    /// Последнее непустое выделение (в символах): клик по кнопке уводит фокус из редактора.
    pub last_selection: Cell<(i32, i32)>,
    on_link: RefCell<Option<Box<dyn Fn(&LinkSpan)>>>,
    on_changed: RefCell<Option<Box<dyn Fn()>>>,
    on_selection: RefCell<Option<Box<dyn Fn()>>>,
    tooltip_text: RefCell<Option<Box<dyn Fn(&LinkSpan) -> String>>>,
    font_css: gtk::CssProvider,
}

impl Editor {
    pub fn new() -> Rc<Self> {
        let buffer = gtk::TextBuffer::new(None);
        buffer.set_enable_undo(true);
        let tags = Self::make_tags(&buffer);
        let view = NdTextView::new();
        view.set_buffer(Some(&buffer));
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_left_margin(20);
        view.set_right_margin(20);
        view.set_top_margin(6);
        view.set_bottom_margin(16);
        view.set_has_tooltip(true);
        view.set_input_hints(gtk::InputHints::NO_SPELLCHECK);
        view.add_css_class("nd-editor");
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();
        let start = buffer.start_iter();
        let dirty_start = buffer.create_mark(None, &start, true);
        let dirty_end = buffer.create_mark(None, &start, false);
        let restore_mark = buffer.create_mark(None, &start, true);
        let font_css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().expect("дисплей"),
            &font_css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        let editor = Rc::new(Self {
            view,
            buffer,
            scroll,
            tags,
            source_id: RefCell::new("root".to_string()),
            visited: RefCell::new(HashSet::new()),
            raw: Cell::new(false),
            dark: Cell::new(true),
            font_size: Cell::new(14.0),
            dirty_start,
            dirty_end,
            restore_mark,
            dirty: Cell::new(false),
            retag_scheduled: Cell::new(false),
            hover: RefCell::new(None),
            pending_raw_cut: RefCell::new(None),
            suppress: Cell::new(false),
            last_selection: Cell::new((0, 0)),
            on_link: RefCell::new(None),
            on_changed: RefCell::new(None),
            on_selection: RefCell::new(None),
            tooltip_text: RefCell::new(None),
            font_css,
        });
        editor.wire();
        editor.apply_colors(true);
        editor
    }

    fn make_tags(buffer: &gtk::TextBuffer) -> Tags {
        let table = buffer.tag_table();
        // скобки красятся цветом фона редактора: прозрачный foreground GTK рисует серым
        let bracket = gtk::TextTag::builder().name("nd-bracket").build();
        let hidden = gtk::TextTag::builder().name("nd-hidden").invisible(true).build();
        let link = gtk::TextTag::builder().name("nd-link").underline(pango::Underline::Single).build();
        let visited = gtk::TextTag::builder().name("nd-link-visited").underline(pango::Underline::Single).build();
        let web = gtk::TextTag::builder().name("nd-link-web").underline(pango::Underline::Single).build();
        let find = gtk::TextTag::builder().name("nd-find").build();
        let find_current = gtk::TextTag::builder().name("nd-find-current").build();
        let spacing = gtk::TextTag::builder().name("nd-spacing").build();
        let hover = gtk::TextTag::builder().name("nd-link-hover").underline(pango::Underline::Double).build();
        // порядок добавления = приоритет: наведение и текущее совпадение — сверху
        for t in [&spacing, &bracket, &hidden, &link, &visited, &web, &find, &find_current, &hover] {
            table.add(t);
        }
        Tags { bracket, hidden, link, visited, web, hover, find, find_current, spacing }
    }

    // ================= Обратные вызовы приложения =================

    pub fn set_on_link_activated(&self, f: impl Fn(&LinkSpan) + 'static) {
        *self.on_link.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_changed(&self, f: impl Fn() + 'static) {
        *self.on_changed.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_selection_changed(&self, f: impl Fn() + 'static) {
        *self.on_selection.borrow_mut() = Some(Box::new(f));
    }

    // ================= Подключение сигналов =================

    fn wire(self: &Rc<Self>) {
        let ed = self.clone();
        self.buffer.connect_insert_text(move |buf, iter, _| ed.extend_dirty(buf, iter, iter));
        let ed = self.clone();
        self.buffer.connect_delete_range(move |buf, start, end| ed.extend_dirty(buf, start, end));
        let ed = self.clone();
        self.buffer.connect_changed(move |_| {
            ed.schedule_retag();
            if !ed.suppress.get()
                && let Some(f) = &*ed.on_changed.borrow() {
                    f();
                }
        });

        let ed = self.clone();
        let guarding = Rc::new(Cell::new(false));
        self.buffer.connect_mark_set(move |buf, iter, mark| {
            let insert = buf.get_insert();
            let bound = buf.selection_bound();
            if mark != &insert && mark != &bound {
                return;
            }
            if mark == &insert && !ed.raw.get() && !guarding.get() && !buf.has_selection() && iter.has_tag(&ed.tags.hidden) {
                let mut moved = *iter;
                if moved.forward_to_tag_toggle(Some(&ed.tags.hidden)) {
                    guarding.set(true);
                    buf.place_cursor(&moved);
                    guarding.set(false);
                }
            }
            if let Some((s, e)) = buf.selection_bounds() {
                ed.last_selection.set((s.offset(), e.offset()));
            }
            if !ed.suppress.get()
                && let Some(f) = &*ed.on_selection.borrow() {
                    f();
                }
        });

        // копирование и вырезание кладут сырой текст: GTK сериализует только видимое
        let ed = self.clone();
        self.view.connect_local("copy-clipboard", true, move |_| {
            if let Some((s, e)) = ed.buffer.selection_bounds() {
                ed.view.clipboard().set_text(&ed.buffer.text(&s, &e, true));
            }
            None
        });
        let ed = self.clone();
        self.view.connect_cut_clipboard(move |_| {
            if let Some((s, e)) = ed.buffer.selection_bounds() {
                *ed.pending_raw_cut.borrow_mut() = Some(ed.buffer.text(&s, &e, true).to_string());
            }
        });
        let ed = self.clone();
        self.view.connect_local("cut-clipboard", true, move |_| {
            if let Some(raw) = ed.pending_raw_cut.borrow_mut().take() {
                ed.view.clipboard().set_text(&raw);
            }
            None
        });
        // вставка — только текст, с нормализованными переводами строк (как OnEditorPaste)
        let ed = self.clone();
        self.view.connect_paste_clipboard(move |view| {
            view.stop_signal_emission_by_name("paste-clipboard");
            let ed = ed.clone();
            view.clipboard().read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
                let Ok(Some(text)) = result else { return };
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                ed.buffer.begin_user_action();
                ed.buffer.delete_selection(true, true);
                ed.buffer.insert_at_cursor(&text);
                ed.buffer.end_user_action();
                ed.scroll_to_caret();
            });
        });

        // наведение: рука и подчёркивание по прямоугольнику глифов ссылки
        let motion = gtk::EventControllerMotion::new();
        let ed = self.clone();
        motion.connect_motion(move |_, x, y| {
            let hit = ed.hit_link(x, y);
            ed.set_hover(hit);
        });
        let ed = self.clone();
        motion.connect_leave(move |_| ed.set_hover(None));
        self.view.add_controller(motion);
        // GTK сам ставит «text» — при показе и когда возвращает спрятанный на время набора
        // указатель, причём уже после нашего обработчика движения: возвращаем руку сразу
        let ed = self.clone();
        self.view.connect_cursor_notify(move |_| ed.sync_cursor());

        // клик по ссылке — в фазе захвата, чтобы TextView не ставил каретку; допуск 4 px
        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        // Нажатие внутри выделения тоже забираем себе: иначе GTK начал бы перетаскивание
        // выделенного текста, а оно переносит только видимые символы — цели ссылок пропали бы.
        // Короткий клик без движения ставит каретку, как и в GTK.
        let pressed: Rc<RefCell<Option<(f64, f64, Option<LineLink>)>>> = Rc::new(RefCell::new(None));
        let ed = self.clone();
        let p = pressed.clone();
        click.connect_pressed(move |g, n, x, y| {
            if n != 1 {
                return;
            }
            if g.current_event_state().intersects(gdk::ModifierType::SHIFT_MASK | gdk::ModifierType::CONTROL_MASK) {
                return;
            }
            if let Some(link) = ed.hit_link(x, y) {
                *p.borrow_mut() = Some((x, y, Some(link)));
                g.set_state(gtk::EventSequenceState::Claimed);
                return;
            }
            if let Some((s, e)) = ed.buffer.selection_bounds() {
                let (bx, by) = ed.view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
                if let Some((iter, _)) = ed.view.iter_at_position(bx, by)
                    && iter.in_range(&s, &e)
                {
                    *p.borrow_mut() = Some((x, y, None));
                    g.set_state(gtk::EventSequenceState::Claimed);
                }
            }
        });
        let ed = self.clone();
        let p = pressed.clone();
        click.connect_released(move |_, _, x, y| {
            let Some((px, py, link)) = p.borrow_mut().take() else { return };
            if (x - px).abs() > 4.0 || (y - py).abs() > 4.0 {
                return;
            }
            let Some(link) = link else {
                // клик внутри выделения без движения — поставить каретку
                let (bx, by) = ed.view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
                if let Some((iter, _)) = ed.view.iter_at_position(bx, by) {
                    ed.buffer.place_cursor(&iter);
                }
                return;
            };
            if ed.hit_link(x, y).as_ref() != Some(&link) {
                return;
            }
            let span = link.span.clone();
            let ed2 = ed.clone();
            glib::idle_add_local_once(move || {
                if let Some(f) = &*ed2.on_link.borrow() {
                    f(&span);
                }
            });
        });
        self.view.add_controller(click);

        let ed = self.clone();
        self.view.connect_query_tooltip(move |_, x, y, _, tooltip| {
            let Some(link) = ed.hit_link(x as f64, y as f64) else { return false };
            let text = ed.tooltip_text.borrow().as_ref().map(|f| f(&link.span));
            let Some(text) = text else { return false };
            tooltip.set_text(Some(&text));
            true
        });

        // кружки ссылок без якоря
        let ed = self.clone();
        self.view.set_draw_extra(Box::new(move |view, snapshot| ed.draw_circles(view, snapshot)));
    }

    // ================= Контекст: файл, прочитанное, тема, шрифт =================

    pub fn set_source_id(&self, id: &str) {
        *self.source_id.borrow_mut() = id.to_string();
    }

    pub fn source_id(&self) -> String {
        self.source_id.borrow().clone()
    }

    /// Множество прочитанных файлов (в нижнем регистре) — ими красятся пройденные ссылки.
    pub fn set_visited(&self, visited: HashSet<String>) {
        *self.visited.borrow_mut() = visited;
        self.retag_all();
    }

    pub fn is_raw(&self) -> bool {
        self.raw.get()
    }

    /// Режим «как есть»: теги снимаются явно, перетэговка не идёт.
    pub fn set_raw(&self, raw: bool) {
        self.raw.set(raw);
        self.retag_all();
        self.set_hover(None);
    }

    pub fn apply_colors(&self, dark: bool) {
        self.dark.set(dark);
        let accent = css::accent(dark);
        self.tags.link.set_foreground_rgba(Some(&css::rgba(accent, 1.0)));
        self.tags.link.set_underline_rgba(Some(&css::rgba(accent, 0.86)));
        let visited = if dark { css::VISITED_DARK } else { css::VISITED_LIGHT };
        self.tags.visited.set_foreground_rgba(Some(&css::rgba(visited, 1.0)));
        self.tags.visited.set_underline_rgba(Some(&css::rgba(visited, 0.86)));
        let web = if dark { css::WEB_DARK } else { css::WEB_LIGHT };
        self.tags.web.set_foreground_rgba(Some(&css::rgba(web, 1.0)));
        self.tags.web.set_underline_rgba(Some(&css::rgba(web, 0.86)));
        self.tags.hover.set_background_rgba(Some(&css::rgba(accent, 0.13)));
        self.tags.bracket.set_foreground_rgba(Some(&css::rgba(if dark { css::EDITOR_BG_DARK } else { css::EDITOR_BG_LIGHT }, 1.0)));
        self.tags.find.set_background_rgba(Some(&css::rgba(0xFFC107, if dark { 0.27 } else { 0.35 })));
        self.tags.find_current.set_background_rgba(Some(&css::rgba(0xFF9800, if dark { 0.55 } else { 0.5 })));
        self.view.queue_draw();
    }

    pub fn apply_font(&self, family: &str, size: f64, line_spacing: f64) {
        self.font_size.set(size);
        let family = family.replace('"', "");
        self.font_css.load_from_string(&format!(".nd-editor {{ font-family: \"{family}\"; font-size: {size}px; }}"));
        self.tags.spacing.set_line_height(line_spacing as f32);
        self.retag_all();
    }

    // ================= Текст =================

    /// Загружает текст необратимо: история undo прежнего файла не должна тянуться в новый.
    pub fn load_text(&self, text: &str) {
        self.suppress.set(true);
        self.buffer.begin_irreversible_action();
        self.buffer.set_text(text);
        self.buffer.end_irreversible_action();
        self.dirty.set(false);
        self.retag_all();
        self.buffer.place_cursor(&self.buffer.start_iter());
        self.last_selection.set((0, 0)); // выделение прежнего файла к новому не относится
        self.suppress.set(false);
        self.set_hover(None);
        self.scroll.vadjustment().set_value(0.0);
    }

    /// Текст буфера со скрытыми символами.
    pub fn raw_text(&self) -> String {
        let (s, e) = (self.buffer.start_iter(), self.buffer.end_iter());
        self.buffer.text(&s, &e, true).to_string()
    }

    /// Позиция каретки в символах.
    pub fn caret(&self) -> i32 {
        self.buffer.iter_at_mark(&self.buffer.get_insert()).offset()
    }

    /// Выделение в символах (start <= end); без выделения — каретка дважды.
    pub fn selection(&self) -> (i32, i32) {
        match self.buffer.selection_bounds() {
            Some((s, e)) => (s.offset(), e.offset()),
            None => {
                let c = self.caret();
                (c, c)
            }
        }
    }

    pub fn iter_at_offset(&self, offset: i32) -> gtk::TextIter {
        self.buffer.iter_at_offset(offset)
    }

    /// Итератор по байтовому смещению в тексте `text` (тексте буфера).
    pub fn iter_at_byte(&self, text: &str, byte: usize) -> Option<gtk::TextIter> {
        let byte = byte.min(text.len());
        if !text.is_char_boundary(byte) {
            return None; // устаревшее смещение попало внутрь символа
        }
        let line = text[..byte].matches('\n').count() as i32;
        let line_start = text[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
        self.buffer.iter_at_line_index(line, (byte - line_start) as i32)
    }

    /// Байтовое смещение итератора в тексте буфера.
    pub fn byte_of(&self, text: &str, iter: &gtk::TextIter) -> usize {
        let line = iter.line() as usize;
        let line_start: usize = text.split_inclusive('\n').take(line).map(str::len).sum();
        line_start + iter.line_index() as usize
    }

    pub fn select_range(&self, start: &gtk::TextIter, end: &gtk::TextIter) {
        self.buffer.select_range(start, end);
    }

    pub fn place_cursor(&self, iter: &gtk::TextIter) {
        self.buffer.place_cursor(iter);
    }

    /// Ставит позицию (в символах) в верх видимой области — возврат прокрутки при
    /// переходе «назад». Через метку, а не координаты: сразу после загрузки раскладка
    /// ещё не досчитана, и scroll_to_mark сам дождётся её.
    pub fn scroll_offset_to_top(&self, offset: i32) {
        let max = self.buffer.char_count();
        let iter = self.buffer.iter_at_offset(offset.clamp(0, max));
        self.buffer.move_mark(&self.restore_mark, &iter);
        self.view.scroll_to_mark(&self.restore_mark, 0.0, true, 0.0, 0.0);
    }

    pub fn scroll_to_caret(&self) {
        self.view.scroll_to_mark(&self.buffer.get_insert(), 0.1, false, 0.0, 0.0);
    }

    pub fn grab_focus(&self) {
        self.view.grab_focus();
    }

    /// Заменяет диапазон одним шагом undo.
    pub fn replace_range(&self, start: &gtk::TextIter, end: &gtk::TextIter, text: &str) {
        let mut s = *start;
        let mut e = *end;
        self.buffer.begin_user_action();
        self.buffer.delete(&mut s, &mut e);
        self.buffer.insert(&mut s, text);
        self.buffer.end_user_action();
    }

    /// Заменяет выделение (или вставляет у каретки) одним шагом undo и ставит каретку после.
    pub fn replace_selection(&self, text: &str) {
        self.buffer.begin_user_action();
        self.buffer.delete_selection(true, true);
        self.buffer.insert_at_cursor(text);
        self.buffer.end_user_action();
    }

    // ================= Ссылки =================

    /// Ссылки на строке буфера; позиции — байты от начала строки.
    pub fn links_on_line(&self, line: i32) -> Vec<LinkSpan> {
        let Some(start) = self.buffer.iter_at_line(line) else { return Vec::new() };
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        let text = self.buffer.text(&start, &end, true);
        scan_links(&text, &self.source_id.borrow())
    }

    /// Ссылка, в границах которой (включая края) стоит итератор.
    pub fn link_at(&self, iter: &gtk::TextIter) -> Option<LineLink> {
        let idx = iter.line_index() as usize;
        self.links_on_line(iter.line())
            .into_iter()
            .find(|l| l.contains_caret(idx))
            .map(|span| LineLink { line: iter.line(), span })
    }

    /// Пересекает ли диапазон итераторов какую-нибудь ссылку (края не в счёт).
    pub fn range_overlaps_link(&self, start: &gtk::TextIter, end: &gtk::TextIter) -> bool {
        for line in start.line()..=end.line() {
            let Some(ls) = self.buffer.iter_at_line(line) else { break };
            let from = if line == start.line() { start.line_index() as usize } else { 0 };
            let to = if line == end.line() {
                end.line_index() as usize
            } else {
                let mut le = ls;
                le.forward_to_line_end();
                le.line_index() as usize + 1
            };
            if self.links_on_line(line).iter().any(|l| l.overlaps(from, to)) {
                return true;
            }
        }
        false
    }

    /// Итераторы границ ссылки на строке.
    pub fn link_iters(&self, link: &LineLink) -> Option<(gtk::TextIter, gtk::TextIter)> {
        Some((
            self.buffer.iter_at_line_index(link.line, link.span.start as i32)?,
            self.buffer.iter_at_line_index(link.line, link.span.end as i32)?,
        ))
    }

    /// Попадание по ссылке — по прямоугольнику глифа, а не по ближайшему символу:
    /// пустое место справа от строки иначе считалось бы ссылкой.
    fn hit_link(&self, x: f64, y: f64) -> Option<LineLink> {
        if self.raw.get() {
            return None;
        }
        let (bx, by) = self.view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let (iter, _) = self.view.iter_at_position(bx, by)?;
        let rect = self.glyph_rect(&iter);
        if bx < rect.x() || bx >= rect.x() + rect.width() || by < rect.y() || by >= rect.y() + rect.height() {
            return None;
        }
        let idx = iter.line_index() as usize;
        self.links_on_line(iter.line())
            .into_iter()
            .find(|l| idx >= l.start && idx < l.end)
            .map(|span| LineLink { line: iter.line(), span })
    }

    /// Прямоугольник глифа под итератором, в координатах буфера. `iter_location` здесь не
    /// годится: GTK (проверено на 4.22) ищет позицию в раскладке по полному байтовому
    /// индексу в строке, а невидимый текст — скрытая цель ссылки — в раскладку не попадает.
    /// После первой ссылки строки он отдавал конец строки нулевого размера: вторая и
    /// следующие ссылки не ловили ни клик, ни наведение, а их кружки не рисовались.
    /// Каретку GTK ставит по видимому индексу — из её позиций прямоугольник и собран:
    /// левый край — перед символом, правый — перед следующим видимым.
    fn glyph_rect(&self, iter: &gtk::TextIter) -> gdk::Rectangle {
        let (left, _) = self.view.cursor_locations(Some(iter));
        let mut next = *iter;
        next.forward_visible_cursor_position();
        let (mut right, _) = self.view.cursor_locations(Some(&next));
        if next == *iter || right.y() != left.y() {
            // символ последний на экранной строке: правый край — её конец
            let mut end = *iter;
            self.view.forward_display_line_end(&mut end);
            right = self.view.cursor_locations(Some(&end)).0;
        }
        let (x0, x1) = (left.x().min(right.x()), left.x().max(right.x()));
        // конец экранной строки при переносе GTK ставит перед последним символом — тогда
        // ширину неоткуда взять, и глиф считается шириной в полстроки
        let width = if x1 > x0 && right.y() == left.y() { x1 - x0 } else { (left.height() / 2).max(1) };
        gdk::Rectangle::new(x0, left.y(), width, left.height())
    }

    /// Курсор-«рука» над ссылкой. Ставится самому TextView: дочернего виджета у текстовой
    /// области в GTK4 нет, курсор «text» стоит на нём же. Сверяется на каждом движении:
    /// на время набора GTK прячет указатель («none») и при движении мыши сам возвращает
    /// ему «text», не зная о ссылке. Спрятанный указатель не трогаем — его вернёт GTK.
    fn sync_cursor(&self) {
        let want = if self.hover.borrow().is_some() { "pointer" } else { "text" };
        let current = self.view.cursor().and_then(|c| c.name());
        if current.as_deref() != Some(want) && current.as_deref() != Some("none") {
            self.view.set_cursor_from_name(Some(want));
        }
    }

    fn set_hover(&self, link: Option<LineLink>) {
        if *self.hover.borrow() == link {
            self.sync_cursor();
            return;
        }
        self.clear_hover_tag();
        if let Some(l) = &link {
            let (a, b) = self.anchor_range(&l.span);
            if let (Some(a), Some(b)) = (
                self.buffer.iter_at_line_index(l.line, a as i32),
                self.buffer.iter_at_line_index(l.line, b as i32),
            )
                && (!l.span.anchor.is_empty() || l.span.is_web) {
                    self.buffer.apply_tag(&self.tags.hover, &a, &b);
                }
        }
        *self.hover.borrow_mut() = link;
        self.sync_cursor();
        self.view.queue_draw();
    }

    /// Снимает подсветку наведения только там, где она стоит. Снятие тега со всего буфера
    /// заставляло GTK заново раскладывать весь документ на каждое пересечение ссылки.
    /// Диапазон ищется по границам тега, а не по номеру строки прежней ссылки: пока мышь
    /// над ссылкой, набор выше по тексту сдвигает строки.
    fn clear_hover_tag(&self) {
        let tag = &self.tags.hover;
        let mut s = self.buffer.start_iter();
        while s.starts_tag(Some(tag)) || s.forward_to_tag_toggle(Some(tag)) {
            let mut e = s;
            if !e.forward_to_tag_toggle(Some(tag)) {
                e = self.buffer.end_iter();
            }
            let next = e.offset();
            self.buffer.remove_tag(tag, &s, &e);
            // смена тегов делает итераторы недействительными — берём заново по позиции
            s = self.buffer.iter_at_offset(next);
            if s.is_end() {
                break;
            }
        }
    }

    /// Самопроверка (NEURODOC_SMOKE): навести «мышь» на первую ссылку в видимой части
    /// или увести с неё — тот же путь, что у движения указателя.
    /// Самопроверка: «мышь» проходит строку буфера (все её экранные строки) с шагом 2 px,
    /// и для каждой ссылки строки считается, сколько раз её поймало попадание.
    pub fn smoke_link_hits(&self, line: i32) -> Vec<u32> {
        let links = self.links_on_line(line);
        let mut hits = vec![0u32; links.len()];
        let Some(start) = self.buffer.iter_at_line(line) else { return hits };
        let (top, height) = self.view.line_yrange(&start);
        let width = self.view.width();
        for y in (top + 2..top + height).step_by(4) {
            for x in (0..width).step_by(2) {
                let (wx, wy) = self.view.buffer_to_window_coords(gtk::TextWindowType::Widget, x, y);
                if let Some(hit) = self.hit_link(wx as f64, wy as f64)
                    && hit.line == line
                    && let Some(i) = links.iter().position(|l| l.start == hit.span.start)
                {
                    hits[i] += 1;
                }
            }
        }
        hits
    }

    pub fn smoke_hover_visible_link(&self, on: bool) {
        if !on {
            self.set_hover(None);
            return;
        }
        let visible = self.view.visible_rect();
        let (mut iter, _) = self.view.line_at_y(visible.y());
        for _ in 0..50 {
            let line = iter.line();
            if let Some(span) = self.links_on_line(line).into_iter().next() {
                self.set_hover(Some(LineLink { line, span }));
                return;
            }
            if !iter.forward_line() {
                return;
            }
        }
    }

    /// Диапазон подсветки: якорь, у веб-ссылки — весь адрес, у пустого якоря — скобки.
    fn anchor_range(&self, l: &LinkSpan) -> (usize, usize) {
        if l.is_web {
            (l.start, l.end)
        } else if l.anchor.is_empty() {
            (l.start, l.start + 2)
        } else {
            (l.start + 1, l.anchor_end())
        }
    }

    // ================= Перетэговка =================

    fn extend_dirty(&self, buf: &gtk::TextBuffer, start: &gtk::TextIter, end: &gtk::TextIter) {
        if !self.dirty.get() {
            buf.move_mark(&self.dirty_start, start);
            buf.move_mark(&self.dirty_end, end);
            self.dirty.set(true);
            return;
        }
        if start.offset() < buf.iter_at_mark(&self.dirty_start).offset() {
            buf.move_mark(&self.dirty_start, start);
        }
        if end.offset() > buf.iter_at_mark(&self.dirty_end).offset() {
            buf.move_mark(&self.dirty_end, end);
        }
    }

    fn schedule_retag(self: &Rc<Self>) {
        if self.retag_scheduled.get() {
            return;
        }
        self.retag_scheduled.set(true);
        let ed = self.clone();
        // раньше перерисовки GTK — сырой текст не мелькнёт ни на кадр
        glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
            ed.retag_scheduled.set(false);
            if ed.dirty.get() {
                ed.dirty.set(false);
                let a = ed.buffer.iter_at_mark(&ed.dirty_start).line();
                let b = ed.buffer.iter_at_mark(&ed.dirty_end).line();
                ed.retag_lines(a, b);
            }
            glib::ControlFlow::Break
        });
    }

    pub fn retag_all(&self) {
        let last = self.buffer.end_iter().line();
        self.retag_lines(0, last);
    }

    fn retag_lines(&self, from: i32, to: i32) {
        let raw = self.raw.get();
        let visited = self.visited.borrow();
        let source = self.source_id.borrow();
        for line in from..=to {
            let Some(start) = self.buffer.iter_at_line(line) else { break };
            let mut end = start;
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            for tag in [&self.tags.bracket, &self.tags.hidden, &self.tags.link, &self.tags.visited, &self.tags.web] {
                self.buffer.remove_tag(tag, &start, &end);
            }
            // интервал строк — тегом на всю строку вместе с переводом строки
            let mut line_end_full = end;
            line_end_full.forward_line();
            self.buffer.apply_tag(&self.tags.spacing, &start, &line_end_full);
            if raw {
                continue;
            }
            let text = self.buffer.text(&start, &end, true);
            for l in scan_links(&text, &source) {
                let at = |idx: usize| self.buffer.iter_at_line_index(line, idx as i32);
                if l.is_web {
                    if let (Some(a), Some(b)) = (at(l.start), at(l.end)) {
                        self.buffer.apply_tag(&self.tags.web, &a, &b);
                    }
                    continue;
                }
                let anchor_end = l.anchor_end();
                let is_visited = visited.contains(&l.target.to_lowercase());
                if let (Some(a), Some(b)) = (at(l.start), at(l.start + 1)) {
                    self.buffer.apply_tag(&self.tags.bracket, &a, &b);
                }
                if !l.anchor.is_empty()
                    && let (Some(a), Some(b)) = (at(l.start + 1), at(anchor_end)) {
                        self.buffer.apply_tag(if is_visited { &self.tags.visited } else { &self.tags.link }, &a, &b);
                    }
                if let (Some(a), Some(b)) = (at(anchor_end), at(anchor_end + 1)) {
                    self.buffer.apply_tag(&self.tags.bracket, &a, &b);
                }
                if let (Some(a), Some(b)) = (at(anchor_end + 1), at(l.end)) {
                    self.buffer.apply_tag(&self.tags.hidden, &a, &b);
                }
            }
        }
        self.view.queue_draw();
    }

    // ================= Кружки ссылок без якоря =================

    fn draw_circles(&self, view: &gtk::TextView, snapshot: &gtk::Snapshot) {
        if self.raw.get() {
            return;
        }
        let visible = view.visible_rect();
        let (mut iter, _) = view.line_at_y(visible.y());
        let hover = self.hover.borrow();
        let visited = self.visited.borrow();
        let accent = css::accent(self.dark.get());
        let font_size = self.font_size.get();
        loop {
            let line = iter.line();
            let (y_top, _) = view.line_yrange(&iter);
            if y_top > visible.y() + visible.height() {
                break;
            }
            for l in self.links_on_line(line) {
                if l.is_web || !l.anchor.is_empty() {
                    continue;
                }
                let (Some(a), Some(b)) = (
                    self.buffer.iter_at_line_index(line, l.start as i32),
                    self.buffer.iter_at_line_index(line, (l.start + 1) as i32),
                ) else {
                    continue;
                };
                let ra = self.glyph_rect(&a);
                let rb = self.glyph_rect(&b);
                if ra.y() != rb.y() {
                    continue; // скобки разнесло по строкам — рисовать нечего
                }
                let width_glyphs = (rb.x() + rb.width() - ra.x()).max(1) as f64;
                let line_h = ra.height() as f64;
                let mut d = (line_h - 2.0).min(font_size * 1.25);
                if d <= 0.0 {
                    d = line_h;
                }
                let w = (width_glyphs + 4.0).max(d);
                let (wx, wy) = view.buffer_to_window_coords(gtk::TextWindowType::Widget, ra.x(), ra.y());
                let x = wx as f64 + width_glyphs / 2.0 - w / 2.0;
                let y = wy as f64 + 1.0;
                let is_hover = hover.as_ref().is_some_and(|h| h.line == line && h.span.start == l.start);
                let is_visited = visited.contains(&l.target.to_lowercase());
                let base = if is_visited { if self.dark.get() { css::VISITED_DARK } else { css::VISITED_LIGHT } } else { accent };
                let color = if is_hover { css::rgba(base, 1.0) } else { css::dim(base, 0.72) };
                let rect = graphene::Rect::new(x as f32, y as f32, w as f32, d as f32);
                snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(rect, (d / 2.0) as f32));
                snapshot.append_color(&color, &rect);
                snapshot.pop();
            }
            if !iter.forward_line() {
                break;
            }
        }
    }

    // ================= Подсказки =================

    pub fn set_tooltip_provider(&self, f: impl Fn(&LinkSpan) -> String + 'static) {
        *self.tooltip_text.borrow_mut() = Some(Box::new(f));
    }

    // ================= Поиск =================

    /// Подсвечивает совпадения (байтовые диапазоны в тексте `text`); текущее — ярче.
    pub fn set_find_matches(&self, text: &str, matches: &[(usize, usize)], current: Option<usize>) {
        let (s, e) = (self.buffer.start_iter(), self.buffer.end_iter());
        self.buffer.remove_tag(&self.tags.find, &s, &e);
        self.buffer.remove_tag(&self.tags.find_current, &s, &e);
        for &(from, to) in matches {
            let (Some(a), Some(b)) = (self.iter_at_byte(text, from), self.iter_at_byte(text, to)) else { continue };
            let tag = if Some(from) == current { &self.tags.find_current } else { &self.tags.find };
            self.buffer.apply_tag(tag, &a, &b);
        }
    }

    pub fn clear_find_matches(&self) {
        let (s, e) = (self.buffer.start_iter(), self.buffer.end_iter());
        self.buffer.remove_tag(&self.tags.find, &s, &e);
        self.buffer.remove_tag(&self.tags.find_current, &s, &e);
    }

    // ================= Правка строк (как в IntelliJ IDEA) =================

    fn current_line_bounds(&self) -> (gtk::TextIter, gtk::TextIter) {
        let caret = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let mut start = caret;
        start.set_line_offset(0);
        let mut end = caret;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        (start, end)
    }

    /// Enter: новая строка с отступом текущей.
    pub fn insert_newline_with_indent(&self) {
        // отступ берём со строки начала выделения, как на Windows
        let caret = match self.buffer.selection_bounds() {
            Some((s, _)) => s,
            None => self.buffer.iter_at_mark(&self.buffer.get_insert()),
        };
        let mut line_start = caret;
        line_start.set_line_offset(0);
        let prefix = self.buffer.text(&line_start, &caret, true);
        let indent: String = prefix.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        self.replace_selection(&format!("\n{indent}"));
        self.scroll_to_caret();
    }

    /// Tab / Shift+Tab: отступ 4 пробелами (или снятие) у каретки либо у выделенных строк.
    pub fn indent(&self, unindent: bool) {
        let (sel_start, sel_end) = match self.buffer.selection_bounds() {
            Some((s, e)) => (s, e),
            None => {
                let c = self.buffer.iter_at_mark(&self.buffer.get_insert());
                (c, c)
            }
        };
        let has_selection = sel_end.offset() > sel_start.offset();
        if !unindent && !has_selection {
            self.replace_selection(INDENT_UNIT);
            return;
        }
        let mut block_start = sel_start;
        block_start.set_line_offset(0);
        // если конец выделения стоит в начале строки, эту строку не трогаем
        let mut last = sel_end;
        if has_selection && last.line_offset() == 0 && last.offset() > 0 {
            last.backward_char();
        }
        let mut block_end = last;
        if !block_end.ends_line() {
            block_end.forward_to_line_end();
        }
        let block = self.buffer.text(&block_start, &block_end, true).to_string();
        let mut removed_from_first = 0;
        let lines: Vec<String> = block
            .split('\n')
            .enumerate()
            .map(|(i, line)| {
                if unindent {
                    let removed = if line.starts_with('\t') {
                        1
                    } else {
                        line.chars().take(INDENT_UNIT.len()).take_while(|c| *c == ' ').count()
                    };
                    if i == 0 {
                        removed_from_first = removed;
                    }
                    line[removed..].to_string()
                } else if line.is_empty() {
                    String::new()
                } else {
                    format!("{INDENT_UNIT}{line}")
                }
            })
            .collect();
        let replaced = lines.join("\n");
        if replaced == block {
            return;
        }
        let caret_offset = sel_start.offset();
        let block_offset = block_start.offset();
        self.replace_range(&block_start, &block_end, &replaced);
        if !has_selection {
            let caret = (caret_offset - removed_from_first as i32).max(block_offset);
            self.buffer.place_cursor(&self.buffer.iter_at_offset(caret));
        } else {
            let s = self.buffer.iter_at_offset(block_offset);
            let e = self.buffer.iter_at_offset(block_offset + replaced.chars().count() as i32);
            self.buffer.select_range(&s, &e);
        }
    }

    /// Shift+Alt+↑/↓: переместить текущую строку.
    pub fn move_line(&self, down: bool) {
        let caret = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let (cur_start, cur_end) = self.current_line_bounds();
        let caret_offset = caret.offset() - cur_start.offset();
        let current = self.buffer.text(&cur_start, &cur_end, true).to_string();
        if !down {
            if cur_start.offset() == 0 {
                return;
            }
            let mut prev_start = cur_start;
            prev_start.backward_line();
            let mut prev_end = prev_start;
            if !prev_end.ends_line() {
                prev_end.forward_to_line_end();
            }
            let previous = self.buffer.text(&prev_start, &prev_end, true).to_string();
            let base = prev_start.offset();
            self.replace_range(&prev_start, &cur_end, &format!("{current}\n{previous}"));
            let new_caret = base + caret_offset.min(current.chars().count() as i32);
            self.buffer.place_cursor(&self.buffer.iter_at_offset(new_caret));
        } else {
            let mut next_start = cur_end;
            if !next_start.forward_line() {
                return; // последняя строка
            }
            if next_start.offset() == cur_end.offset() {
                return;
            }
            let mut next_end = next_start;
            if !next_end.ends_line() {
                next_end.forward_to_line_end();
            }
            let next = self.buffer.text(&next_start, &next_end, true).to_string();
            let base = cur_start.offset();
            self.replace_range(&cur_start, &next_end, &format!("{next}\n{current}"));
            let new_caret = base + next.chars().count() as i32 + 1 + caret_offset.min(current.chars().count() as i32);
            self.buffer.place_cursor(&self.buffer.iter_at_offset(new_caret));
        }
        self.scroll_to_caret();
    }

    /// Shift+Delete без выделения / Ctrl+Y: удалить текущую строку (при желании — в буфер).
    pub fn delete_line(&self, copy_to_clipboard: bool) {
        if self.buffer.char_count() == 0 {
            return;
        }
        let (cur_start, cur_end) = self.current_line_bounds();
        if copy_to_clipboard {
            let line = self.buffer.text(&cur_start, &cur_end, true);
            self.view.clipboard().set_text(&format!("{line}\n"));
        }
        let mut delete_from = cur_start;
        let mut delete_to = cur_end;
        if !delete_to.is_end() {
            delete_to.forward_line(); // вместе с переводом строки
        } else if delete_from.offset() > 0 {
            delete_from.backward_char(); // последняя строка: забираем и предыдущий перевод
        }
        if delete_to.offset() <= delete_from.offset() {
            return;
        }
        let from = delete_from.offset();
        self.buffer.begin_user_action();
        self.buffer.delete(&mut delete_from, &mut delete_to);
        self.buffer.end_user_action();
        let max = (self.buffer.char_count() - 1).max(0);
        self.buffer.place_cursor(&self.buffer.iter_at_offset(from.min(max)));
    }
}
