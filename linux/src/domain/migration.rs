//! Перевод каталога с прежнего формата: ссылки <uuid> в [](uuid).
//!
//! Переход полный: старая форма ссылкой больше не считается, и до миграции каталог
//! прежнего формата выглядит пустым, без единой ссылки. Файлы пользователя меняются
//! только после явного подтверждения и только после создания резервной копии всего
//! каталога.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;

use super::catalog::{LEGACY_LINK_REGEX, NeurodocCatalog, README_TEMPLATE};
use super::paths;

/// Что в каталоге осталось от прежнего формата.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyScan {
    pub files: usize,
    pub links: usize,
}

impl LegacyScan {
    pub fn needs_migration(&self) -> bool {
        self.links > 0
    }
}

/// Что сделала миграция и куда положена резервная копия.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationResult {
    pub files: usize,
    pub links: usize,
    pub backup_path: PathBuf,
    pub readme_updated: bool,
}

/// Фраза из старого описания формата — по ней узнаём несвежий ReadMe.md.
const LEGACY_README_MARKERS: [&str; 1] = ["обрамленный символами"];

/// Сколько ссылок старого формата осталось в каталоге.
pub fn scan(catalog: &NeurodocCatalog) -> LegacyScan {
    let mut result = LegacyScan { files: 0, links: 0 };
    for id in catalog.all_ids() {
        let count = LEGACY_LINK_REGEX.find_iter(&catalog.read_file(&id)).count();
        if count == 0 {
            continue;
        }
        result.files += 1;
        result.links += count;
    }
    result
}

/// Куда ляжет резервная копия каталога перед миграцией.
pub fn build_backup_path(project_id: &str) -> PathBuf {
    backup_path_in(&paths::data_dir().join("backups"), project_id)
}

fn backup_path_in(root: &Path, project_id: &str) -> PathBuf {
    root.join(format!("{}-{}", paths::safe_file_name(project_id), Local::now().format("%Y%m%d-%H%M%S")))
}

/// Переводит каталог на новый формат. Сначала копирует каталог целиком, затем заменяет
/// каждую ссылку <uuid> на [](uuid) — пустой якорь означает «показать заголовок цели».
///
/// Ошибка — не удалось создать резервную копию; миграция тогда не начата.
pub fn migrate(catalog: &mut NeurodocCatalog, project_id: &str) -> io::Result<MigrationResult> {
    migrate_with_backup(catalog, build_backup_path(project_id))
}

pub fn migrate_with_backup(catalog: &mut NeurodocCatalog, backup_path: PathBuf) -> io::Result<MigrationResult> {
    copy_directory(catalog.root_path(), &backup_path)?;

    let mut files = 0;
    let mut links = 0;
    for id in catalog.all_ids() {
        let text = catalog.read_file(&id).into_owned();
        let count = LEGACY_LINK_REGEX.find_iter(&text).count();
        if count == 0 {
            continue;
        }
        // заменяем только сами ссылки: остальной текст, включая переводы строк,
        // остаётся байт в байт прежним
        let updated = LEGACY_LINK_REGEX.replace_all(&text, |m: &regex::Captures| format!("[]({})", m[1].to_lowercase()));
        catalog.write_file(&id, &updated)?;
        files += 1;
        links += count;
    }

    let readme_updated = update_readme(catalog.root_path());
    Ok(MigrationResult { files, links, backup_path, readme_updated })
}

/// Обновляет ReadMe.md, если он всё ещё описывает старый формат. Изменённый пользователем
/// файл не трогаем — маркером служит фраза из нашего же прежнего шаблона.
fn update_readme(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else { return false };
    let Some(readme) = entries
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("readme.md")).unwrap_or(false))
    else {
        return false;
    };
    let Ok(text) = fs::read_to_string(&readme) else { return false };
    if !LEGACY_README_MARKERS.iter().any(|m| text.contains(m)) {
        return false;
    }
    // описание формата — не критичная часть каталога, сбой не срывает миграцию
    fs::write(&readme, README_TEMPLATE).is_ok()
}

fn copy_directory(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
