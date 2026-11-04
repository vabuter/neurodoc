//! Настройки приложения и список проектов: JSON в папке конфигурации
//! (аналог settings.yaml в Windows-версии и settings.json на Android — имена
//! полей и значений повторяют Android, чтобы форматы были близнецами).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::paths;

/// Тема оформления: как в Windows-версии — Система / Светлая / Тёмная.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::System => "SYSTEM",
            Self::Light => "LIGHT",
            Self::Dark => "DARK",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "SYSTEM" => Some(Self::System),
            "LIGHT" => Some(Self::Light),
            "DARK" => Some(Self::Dark),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    /// Фон области просмотра SVG: "Light", "Dark" или None — «как тема приложения».
    ///
    /// Настройка проекта, а не приложения: рисунки одного проекта нарисованы под один
    /// фон. Пока пользователь не переключал фон в самом просмотре, тут None и работает
    /// тема; первое же переключение запоминается и дальше побеждает тему.
    pub svg_background: Option<String>,
}

impl ProjectInfo {
    pub fn new(name: &str, path: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            path: path.to_string(),
            svg_background: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppSettings {
    pub theme: ThemeMode,
    pub font_family: String,
    pub font_size: f64,
    /// Междустрочный интервал (множитель одинарного интервала).
    pub line_spacing: f64,
    /// Режим «как есть»: файл показывается ровно так, как лежит на диске — без скрытия
    /// служебных символов и без подсветки ссылок. Нужен, чтобы править текст посимвольно.
    pub raw_editing: bool,
    pub current_project_id: Option<String>,
    pub projects: Vec<ProjectInfo>,
}

impl AppSettings {
    /// Consolas есть в любой Windows; на Linux его нет — встроенный JetBrains Mono,
    /// как и на Android.
    pub const DEFAULT_FONT_FAMILY: &'static str = "JetBrains Mono";
    pub const DEFAULT_FONT_SIZE: f64 = 14.0;
    pub const DEFAULT_LINE_SPACING: f64 = 1.0;

    pub fn current_project(&self) -> Option<&ProjectInfo> {
        let id = self.current_project_id.as_deref()?;
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn current_project_mut(&mut self) -> Option<&mut ProjectInfo> {
        let id = self.current_project_id.clone()?;
        self.projects.iter_mut().find(|p| p.id == id)
    }

    /// Повреждённые или устаревшие значения приводятся к допустимым — файл с диска
    /// не должен мешать запуску.
    pub fn normalize(mut self) -> Self {
        self.projects.retain(|p| !p.path.trim().is_empty());
        for p in &mut self.projects {
            if p.id.trim().is_empty() {
                p.id = uuid::Uuid::new_v4().to_string();
            }
        }
        if self.font_family.trim().is_empty() {
            self.font_family = Self::DEFAULT_FONT_FAMILY.to_string();
        }
        if !(8.0..=96.0).contains(&self.font_size) {
            self.font_size = Self::DEFAULT_FONT_SIZE;
        }
        if !(0.8..=3.0).contains(&self.line_spacing) {
            self.line_spacing = Self::DEFAULT_LINE_SPACING;
        }
        if let Some(id) = &self.current_project_id
            && !self.projects.iter().any(|p| &p.id == id) {
                self.current_project_id = self.projects.first().map(|p| p.id.clone());
            }
        self
    }

    pub fn from_json(json: &Value) -> Self {
        let str_of = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        let mut projects = Vec::new();
        if let Some(array) = json.get("projects").and_then(Value::as_array) {
            for item in array {
                let path = str_of(item, "path");
                if path.trim().is_empty() {
                    continue;
                }
                let id = str_of(item, "id");
                let svg = str_of(item, "svgBackground");
                projects.push(ProjectInfo {
                    id: if id.trim().is_empty() { uuid::Uuid::new_v4().to_string() } else { id },
                    name: str_of(item, "name"),
                    path,
                    svg_background: if svg.trim().is_empty() { None } else { Some(svg) },
                });
            }
        }
        let current = str_of(json, "currentProjectId");
        Self {
            theme: ThemeMode::parse(&str_of(json, "theme")).unwrap_or_default(),
            font_family: json
                .get("fontFamily")
                .and_then(Value::as_str)
                .unwrap_or(Self::DEFAULT_FONT_FAMILY)
                .to_string(),
            font_size: json.get("fontSize").and_then(Value::as_f64).unwrap_or(Self::DEFAULT_FONT_SIZE),
            line_spacing: json.get("lineSpacing").and_then(Value::as_f64).unwrap_or(Self::DEFAULT_LINE_SPACING),
            raw_editing: json.get("rawEditing").and_then(Value::as_bool).unwrap_or(false),
            current_project_id: if current.trim().is_empty() { None } else { Some(current) },
            projects,
        }
        .normalize()
    }

    pub fn to_json(&self) -> Value {
        let projects: Vec<Value> = self
            .projects
            .iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "name": p.name,
                    "path": p.path,
                    "svgBackground": p.svg_background.clone().unwrap_or_default(),
                })
            })
            .collect();
        let mut root = Map::new();
        root.insert("theme".into(), json!(self.theme.name()));
        root.insert("fontFamily".into(), json!(self.font_family));
        root.insert("fontSize".into(), json!(self.font_size));
        root.insert("lineSpacing".into(), json!(self.line_spacing));
        root.insert("rawEditing".into(), json!(self.raw_editing));
        root.insert("currentProjectId".into(), json!(self.current_project_id.clone().unwrap_or_default()));
        root.insert("projects".into(), Value::Array(projects));
        Value::Object(root)
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            font_family: Self::DEFAULT_FONT_FAMILY.to_string(),
            font_size: Self::DEFAULT_FONT_SIZE,
            line_spacing: Self::DEFAULT_LINE_SPACING,
            raw_editing: false,
            current_project_id: None,
            projects: Vec::new(),
        }
    }
}

/// Хранилище настроек: settings.json в папке конфигурации.
pub struct SettingsStore {
    file: PathBuf,
}

impl SettingsStore {
    pub fn new() -> Self {
        Self { file: paths::config_dir().join("settings.json") }
    }

    pub fn with_file(file: &Path) -> Self {
        Self { file: file.to_path_buf() }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Настройки с диска; повреждённый файл не мешает запуску — берутся значения по умолчанию.
    pub fn load(&self) -> AppSettings {
        let Ok(text) = fs::read_to_string(&self.file) else { return AppSettings::default() };
        match serde_json::from_str::<Value>(&text) {
            Ok(json) => AppSettings::from_json(&json),
            Err(_) => AppSettings::default(),
        }
    }

    pub fn save(&self, settings: &AppSettings) -> io::Result<()> {
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&settings.to_json())?;
        fs::write(&self.file, text)
    }
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_and_normalize() {
        let mut s = AppSettings { theme: ThemeMode::Dark, font_size: 18.0, ..AppSettings::default() };
        s.projects.push(ProjectInfo::new("Пример", "/tmp/x"));
        s.projects[0].svg_background = Some("Dark".into());
        s.current_project_id = Some(s.projects[0].id.clone());
        let back = AppSettings::from_json(&s.to_json());
        assert_eq!(back, s);

        let broken = serde_json::json!({
            "theme": "weird", "fontFamily": "", "fontSize": 500, "lineSpacing": 0.1,
            "currentProjectId": "missing",
            "projects": [{"id": "", "name": "a", "path": "/p"}, {"name": "b", "path": " "}]
        });
        let n = AppSettings::from_json(&broken);
        assert_eq!(n.theme, ThemeMode::System);
        assert_eq!(n.font_family, AppSettings::DEFAULT_FONT_FAMILY);
        assert_eq!(n.font_size, AppSettings::DEFAULT_FONT_SIZE);
        assert_eq!(n.line_spacing, AppSettings::DEFAULT_LINE_SPACING);
        assert_eq!(n.projects.len(), 1);
        assert!(!n.projects[0].id.is_empty());
        assert_eq!(n.current_project_id.as_deref(), Some(n.projects[0].id.as_str()));
    }
}
