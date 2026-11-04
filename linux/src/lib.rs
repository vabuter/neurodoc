//! Neurodoc Editor для Linux: доменный слой и просмотр собраны библиотекой, чтобы
//! их проверял `cargo test`; оконный интерфейс (GTK4 / libadwaita) — в `ui`.

#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod domain;
pub mod ui;
pub mod viewer;

/// Версия приложения из git-тега (см. build.rs) — показывается в заголовке окна.
pub const VERSION: &str = env!("NEURODOC_VERSION");
