//! Оконный интерфейс: GTK4 / libadwaita.

pub mod css;
pub mod editor;
pub mod fonts;
pub mod smoke;
pub mod util;
pub mod window;

use adw::prelude::*;
use gtk::glib;

use crate::domain::settings::SettingsStore;

pub const APP_ID: &str = "today.vab.NeurodocEditor";

/// Точка входа приложения. Экземпляров может быть несколько, как на Windows: иначе
/// запуск на тестовом каталоге с XDG_CONFIG_HOME молча будил бы уже открытое окно.
/// Шрифты ставятся до создания GTK.
pub fn run() -> glib::ExitCode {
    util::install_panic_hook();
    fonts::install_bundled_fonts();
    let app = adw::Application::builder().application_id(APP_ID).flags(gtk::gio::ApplicationFlags::NON_UNIQUE).build();
    app.connect_activate(|app| {
        let win = window::Win::build(app, SettingsStore::new());
        win.present();
        if let Ok(path) = std::env::var("NEURODOC_SNAPSHOT") {
            let window = win.window.clone();
            let quit = std::env::var("NEURODOC_SNAPSHOT_QUIT").is_ok();
            glib::timeout_add_local_once(std::time::Duration::from_millis(2500), move || {
                match util::snapshot_to_png(window.upcast_ref(), &path) {
                    Ok(()) => eprintln!("снимок: {path}"),
                    Err(e) => eprintln!("снимок не удался: {e}"),
                }
                if quit {
                    window.close();
                }
            });
        }
        if let Ok(dir) = std::env::var("NEURODOC_SMOKE") {
            smoke::run(win.clone(), dir);
        }
        // окно держится приложением; Rc живёт столько же, сколько окно
        std::mem::forget(win);
    });
    app.run()
}
