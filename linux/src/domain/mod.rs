//! Доменный слой: формат neurodoc, git, настройки, прогресс чтения, миграция.
//! Ничего из GTK здесь нет — модуль собирается и проверяется `cargo test`
//! на копии каталога example/.

pub mod catalog;
pub mod git;
pub mod links;
pub mod migration;
pub mod paths;
pub mod reading;
pub mod settings;
pub mod text;
