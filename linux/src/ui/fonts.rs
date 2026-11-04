//! Встроенные шрифты Inter и JetBrains Mono ставятся пользователю в
//! ~/.local/share/fonts/neurodoc-editor при первом запуске — то же, что делает
//! Windows-версия с папкой шрифтов профиля. Так их видят и Pango в редакторе,
//! и отдельный процесс WebKit для просмотра (ему шрифты процесса приложения
//! недоступны, страница вкладывает их ещё и через @font-face).
//!
//! Делается ДО создания GTK: fontconfig читает папки при первом обращении,
//! и новые файлы попадают в список сразу, без перезапуска.

use std::fs;
use std::path::PathBuf;

use crate::domain::paths;
use crate::viewer::html::BUNDLED_FONTS;

fn user_fonts_dir() -> PathBuf {
    paths::xdg_data_home().join("fonts").join(paths::APP_DIR)
}

/// Копирует встроенные шрифты в папку пользователя, если их там нет или они другие.
/// Ошибки глотаются: без встроенных шрифтов приложение всё равно работает.
pub fn install_bundled_fonts() {
    let dir = user_fonts_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (_, file, bytes) in BUNDLED_FONTS {
        let target = dir.join(file);
        let same = fs::metadata(&target).map(|m| m.len() == bytes.len() as u64).unwrap_or(false);
        if !same {
            let _ = fs::write(&target, bytes);
        }
    }
}

/// Семейства шрифтов для списка в настройках: установленные + встроенные + особые
/// имена + текущий выбор, без дублей, по алфавиту (как в SettingsDialog на Windows).
pub fn font_choices(installed: impl IntoIterator<Item = String>, current: &str) -> Vec<String> {
    let mut fonts: Vec<String> = installed.into_iter().filter(|n| !n.trim().is_empty()).collect();
    let extras = BUNDLED_FONTS
        .iter()
        .map(|(name, _, _)| name.to_string())
        .chain(crate::viewer::html::EXTRA_FONT_NAMES.iter().map(|s| s.to_string()))
        .chain(std::iter::once(current.to_string()));
    for extra in extras {
        if !fonts.iter().any(|f| f.eq_ignore_ascii_case(&extra)) {
            fonts.push(extra);
        }
    }
    fonts.sort_by_key(|f| f.to_lowercase());
    fonts.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    fonts
}
