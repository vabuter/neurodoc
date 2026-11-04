//! Где приложение держит свои файлы на Linux — по XDG, как принято на платформе.
//!
//! Windows-версия складывает всё в %APPDATA%\NeurodocEditor, Android — в приватную
//! папку приложения. Здесь настройки и прогресс чтения — в папке конфигурации,
//! резервные копии миграции — в папке данных, error.log — в папке состояния.
//! `XDG_CONFIG_HOME=/tmp/x` даёт полностью изолированный экземпляр — так и проверяют
//! приложение на тестовом каталоге, не трогая настоящие проекты.

use std::path::{Path, PathBuf};

/// Имя папки приложения внутри XDG-папок.
pub const APP_DIR: &str = "neurodoc-editor";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Значение XDG-переменной, если оно задано и абсолютно (так требует спецификация),
/// иначе путь по умолчанию относительно домашней папки.
fn xdg(var: &str, fallback: &[&str]) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() && Path::new(&v).is_absolute() => PathBuf::from(v),
        _ => fallback.iter().fold(home(), |p, part| p.join(part)),
    }
}

/// `~/.config/neurodoc-editor`: settings.json и прогресс чтения.
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", &[".config"]).join(APP_DIR)
}

/// `~/.local/share` — корень пользовательских данных (шрифты лежат в его `fonts/`).
pub fn xdg_data_home() -> PathBuf {
    xdg("XDG_DATA_HOME", &[".local", "share"])
}

/// `~/.local/share/neurodoc-editor`: резервные копии миграции.
pub fn data_dir() -> PathBuf {
    xdg_data_home().join(APP_DIR)
}

/// `~/.local/state/neurodoc-editor`: error.log.
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", &[".local", "state"]).join(APP_DIR)
}

/// Имя файла из произвольного идентификатора: всё, кроме букв, цифр, точки, дефиса
/// и подчёркивания, заменяется на подчёркивание (как на Android).
pub fn safe_file_name(id: &str) -> String {
    let safe: String = id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' { c } else { '_' })
        .collect();
    if safe.is_empty() { "project".to_string() } else { safe }
}
