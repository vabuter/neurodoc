//! Мелкие помощники для GTK-кода: одноразовые таймеры, журнал ошибок.

use std::cell::Cell;
use std::fs;
use std::io::Write;
use std::time::Duration;

use gtk::glib;

use crate::domain::paths;

/// Одноразовый таймер с перезапуском — замена DispatcherQueueTimer из Windows-версии:
/// автосохранение, дебаунс слежения за диском, подсказка, отметка о прочтении.
pub struct UiTimer {
    source: Cell<Option<glib::SourceId>>,
    delay: Duration,
}

impl UiTimer {
    pub fn new(delay: Duration) -> Self {
        Self { source: Cell::new(None), delay }
    }

    /// (Пере)запускает отсчёт: прежний ожидающий вызов отменяется.
    pub fn restart<F: FnOnce() + 'static>(&self, f: F) {
        self.stop();
        let id = glib::timeout_add_local_once(self.delay, f);
        self.source.set(Some(id));
    }

    pub fn stop(&self) {
        if let Some(id) = self.source.take() {
            id.remove();
        }
    }

    /// Вызывается самим таймером по срабатыванию: источник уже удалён GLib.
    pub fn forget(&self) {
        self.source.set(None);
    }

    /// Ждёт ли таймер срабатывания. Верно, только если обработчик зовёт [`Self::forget`].
    pub fn is_pending(&self) -> bool {
        let id = self.source.take();
        let pending = id.is_some();
        self.source.set(id);
        pending
    }
}

/// Дописывает запись в error.log (как Windows-версия пишет необработанные исключения).
pub fn log_error(message: &str) {
    let dir = paths::state_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join("error.log")) {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "[{stamp}] {message}");
    }
}

/// Паника в обработчике GTK уронит процесс — хотя бы запишем её в журнал.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_error(&format!("panic: {info}"));
        default(info);
    }));
}

/// Текст для подсказки «N символов» — по-русски, с пробелом-разделителем тысяч.
pub fn format_count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202F}');
        }
        out.push(c);
    }
    out
}

/// Снимок виджета в PNG — для проверки интерфейса без рук: переменная окружения
/// `NEURODOC_SNAPSHOT=/путь.png` сохраняет окно через 2 с после показа.
pub fn snapshot_to_png(widget: &gtk::Widget, path: &str) -> Result<(), String> {
    use gtk::prelude::*;
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let (w, h) = (paintable.intrinsic_width(), paintable.intrinsic_height());
    let snap = gtk::Snapshot::new();
    paintable.snapshot(&snap, w as f64, h as f64);
    let node = snap.to_node().ok_or("пустой снимок")?;
    let native = widget.native().ok_or("нет native")?;
    let renderer = native.renderer().ok_or("нет renderer")?;
    let texture = renderer.render_texture(&node, None);
    texture.save_to_png(path).map_err(|e| e.to_string())
}
