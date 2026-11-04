//! Что читатель уже прошёл в проекте. Состояние читателя, а не документации,
//! поэтому лежит рядом с настройками, а не в каталоге neurodoc.
//!
//! Отдельный файл на проект: список прочитанного растёт и переписывается при
//! каждом переходе, настройкам такое соседство ни к чему. Сброс прогресса —
//! удаление файла. Формат JSON `{"visited": {id: millis}}` — как на Android.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use super::paths;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadingProgress {
    /// Файл (в нижнем регистре) → когда он был отмечен прочитанным, миллисекунды эпохи.
    visited: HashMap<String, i64>,
}

impl ReadingProgress {
    pub fn is_read(&self, id: &str) -> bool {
        self.visited.contains_key(&id.to_lowercase())
    }

    pub fn mark(&mut self, id: &str, read: bool) {
        let key = id.to_lowercase();
        if read {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
            self.visited.insert(key, now);
        } else {
            self.visited.remove(&key);
        }
    }

    pub fn from_json(json: &Value) -> Self {
        let mut visited = HashMap::new();
        if let Some(node) = json.get("visited").and_then(Value::as_object) {
            for (key, at) in node {
                visited.insert(key.to_lowercase(), at.as_i64().unwrap_or(0));
            }
        }
        Self { visited }
    }

    pub fn to_json(&self) -> Value {
        let mut node = Map::new();
        let mut keys: Vec<&String> = self.visited.keys().collect();
        keys.sort();
        for key in keys {
            node.insert(key.clone(), Value::from(self.visited[key]));
        }
        let mut root = Map::new();
        root.insert("visited".into(), Value::Object(node));
        Value::Object(root)
    }
}

pub struct ReadingProgressStore {
    dir: PathBuf,
}

impl ReadingProgressStore {
    pub fn new() -> Self {
        Self { dir: paths::config_dir().join("reading") }
    }

    pub fn with_dir(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf() }
    }

    pub fn file_for(&self, project_id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", paths::safe_file_name(project_id)))
    }

    /// Прогресс с диска; повреждённый файл не мешает читать — прогресс просто пуст.
    pub fn load(&self, project_id: &str) -> ReadingProgress {
        let Ok(text) = fs::read_to_string(self.file_for(project_id)) else { return ReadingProgress::default() };
        serde_json::from_str::<Value>(&text)
            .map(|json| ReadingProgress::from_json(&json))
            .unwrap_or_default()
    }

    pub fn save(&self, project_id: &str, progress: &ReadingProgress) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        fs::write(self.file_for(project_id), serde_json::to_string(&progress.to_json())?)
    }

    pub fn reset(&self, project_id: &str) {
        // остаётся в памяти — сброс не критичная операция
        let _ = fs::remove_file(self.file_for(project_id));
    }
}

impl Default for ReadingProgressStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_case_insensitive_and_roundtrip() {
        let mut p = ReadingProgress::default();
        p.mark("ABC", true);
        assert!(p.is_read("abc"));
        let back = ReadingProgress::from_json(&p.to_json());
        assert!(back.is_read("Abc"));
        p.mark("abc", false);
        assert!(!p.is_read("ABC"));
    }
}
