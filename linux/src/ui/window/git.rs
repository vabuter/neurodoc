//! Git: сохранение и получение изменений, ветки, история, дельты. Каждый commit сразу
//! уходит push — локальные коммиты приложению не нужны. Конфликты решаются
//! перезатиранием по явному выбору пользователя либо отменой.
//!
//! Все вызовы git блокирующие и идут в отдельном потоке (`gio::spawn_blocking`),
//! окно в это время живёт; `git_busy` глушит слежение за диском.

use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{GitFileRow, Win, clear_list, list_row};
use crate::domain::catalog::{self, ROOT_ID};
use crate::domain::git::{self, GitCommit, GitError, GitService, PullOutcome, PushOutcome, SyncOutcome};

/// Блокирующая работа в фоновом потоке с возвратом в главный цикл.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, GitError> {
    gio::spawn_blocking(f).await.map_err(|_| GitError::Other("Поток git завершился аварийно".to_string()))
}

impl Win {
    /// Пересоздаёт сервис при открытии проекта.
    pub(super) fn setup_git(self: &Rc<Self>, project_path: Option<&Path>) {
        {
            let mut st = self.state.borrow_mut();
            st.git = project_path.filter(|p| git::is_repository(p)).map(GitService::new);
            st.git_commits.clear();
            st.git_selected = None;
            st.git_files.clear();
        }
        self.git_card.set_visible(false);
        let w = self.clone();
        glib::spawn_future_local(async move { w.refresh_git_ui().await });
    }

    /// Обновляет ветку в заголовке, кэш веток и меню. После каждой git-операции.
    pub async fn refresh_git_ui(self: &Rc<Self>) {
        let available = blocking(git::is_git_available).await.unwrap_or(false);
        let git = self.state.borrow().git.clone();
        self.state.borrow_mut().git_available = available;
        let (branch, branches, default) = match (&git, available) {
            (Some(g), true) => {
                let g = g.clone();
                blocking(move || (g.current_branch().ok().flatten(), g.local_branches().unwrap_or_default(), g.default_branch().ok()))
                    .await
                    .unwrap_or((None, Vec::new(), None))
            }
            _ => (None, Vec::new(), None),
        };
        {
            let mut st = self.state.borrow_mut();
            st.git_branch = branch.clone();
            st.git_branches = branches;
            st.git_default_branch = default;
        }
        self.title_branch.set_text(&branch.map(|b| format!("·  {b}")).unwrap_or_default());
        self.rebuild_git_menu();
        if self.git_card.is_visible() {
            self.load_git_commits().await;
        }
    }

    // ================= Меню =================

    pub(super) fn wire_git_menu(self: &Rc<Self>) {
        let group = self.git_actions.clone();
        macro_rules! act {
            ($name:expr, |$w:ident| $body:expr) => {{
                let a = gio::SimpleAction::new($name, None);
                let $w = self.clone();
                a.connect_activate(move |_, _| {
                    $body;
                });
                group.add_action(&a);
            }};
        }
        act!("save", |w| w.git_save());
        act!("pull", |w| w.git_pull());
        act!("sync", |w| w.git_sync());
        act!("history", |w| w.git_show_history());
        act!("create-branch", |w| w.git_create_branch());
        act!("publish", |w| w.git_publish());
        act!("clone", |w| w.git_clone());
        let switch = gio::SimpleAction::new_stateful("branch", Some(glib::VariantTy::STRING), &"".to_variant());
        let w = self.clone();
        switch.connect_activate(move |_, param| {
            if let Some(name) = param.and_then(|p| p.get::<String>()) {
                w.git_switch_branch(name);
            }
        });
        group.add_action(&switch);
        let delete = gio::SimpleAction::new("delete-branch", Some(glib::VariantTy::STRING));
        let w = self.clone();
        delete.connect_activate(move |_, param| {
            if let Some(name) = param.and_then(|p| p.get::<String>()) {
                w.git_delete_branch(name);
            }
        });
        group.add_action(&delete);
        self.window.insert_action_group("git", Some(&group));
        self.rebuild_git_menu();
    }

    /// Меню строится заново при каждом изменении состояния: набор пунктов зависит
    /// от репозитория, занятости и списка веток.
    fn rebuild_git_menu(&self) {
        let st = self.state.borrow();
        let menu = gio::Menu::new();
        if !st.git_available {
            let item = gio::MenuItem::new(Some("git не найден — установите git (sudo dnf install git)"), None);
            item.set_action_and_target_value(Some("git.none"), None);
            menu.append_item(&item);
        } else {
            let repo = st.git.is_some() && !st.git_busy;
            let ops = gio::Menu::new();
            for (label, action) in [("Сохранить на сервере", "git.save"), ("Получить с сервера", "git.pull"), ("Синхронизировать", "git.sync")] {
                ops.append(Some(label), if repo { Some(action) } else { None });
            }
            menu.append_section(None, &ops);
            let branches = gio::Menu::new();
            let list = gio::Menu::new();
            for b in &st.git_branches {
                let item = gio::MenuItem::new(Some(b), None);
                if repo {
                    item.set_action_and_target_value(Some("git.branch"), Some(&b.to_variant()));
                }
                list.append_item(&item);
            }
            branches.append_section(None, &list);
            let manage = gio::Menu::new();
            manage.append(Some("Создать ветку…"), if repo { Some("git.create-branch") } else { None });
            let delete = gio::Menu::new();
            for b in st.git_branches.iter().filter(|b| Some(*b) != st.git_default_branch.as_ref()) {
                let item = gio::MenuItem::new(Some(b), None);
                if repo {
                    item.set_action_and_target_value(Some("git.delete-branch"), Some(&b.to_variant()));
                }
                delete.append_item(&item);
            }
            manage.append_submenu(Some("Удалить ветку"), &delete);
            branches.append_section(None, &manage);
            let sub = gio::MenuItem::new_submenu(Some("Ветки"), &branches);
            menu.append_item(&sub);
            let history = gio::Menu::new();
            history.append(Some("История коммитов"), if repo { Some("git.history") } else { None });
            menu.append_section(None, &history);
            let remote = gio::Menu::new();
            if st.catalog.is_some() && st.git.is_none() {
                remote.append(Some("Опубликовать проект на сервере…"), if !st.git_busy { Some("git.publish") } else { None });
            }
            remote.append(Some("Клонировать с сервера…"), if !st.git_busy { Some("git.clone") } else { None });
            menu.append_section(None, &remote);
        }
        // отметка текущей ветки — состояние stateful-действия
        let current = st.git_branch.clone().unwrap_or_default();
        drop(st);
        if let Some(action) = self.git_actions.lookup_action("branch") {
            action.change_state(&current.to_variant());
        }
        self.git_button.set_menu_model(Some(&menu));
    }

    // ================= Общий раннер операций =================

    /// Каркас каждой git-операции: сохранить правки, показать занятость, выполнить,
    /// перечитать каталог, обновить ветку. Ошибки git всегда показываются пользователю.
    async fn run_git(self: &Rc<Self>, error_title: &str, op: impl AsyncFnOnce(Rc<Win>, GitService) -> Result<(), GitError>) {
        let git = {
            let st = self.state.borrow();
            if st.git_busy {
                return;
            }
            let Some(g) = st.git.clone() else { return };
            g
        };
        self.save_current_file();
        self.state.borrow_mut().git_busy = true;
        self.git_spinner.set_visible(true);
        self.git_spinner.start();
        self.rebuild_git_menu();
        let result = op(self.clone(), git).await;
        self.state.borrow_mut().git_busy = false;
        self.git_spinner.stop();
        self.git_spinner.set_visible(false);
        if let Err(e) = result {
            self.show_message(error_title, &e.to_string(), "Закрыть").await;
        }
        self.reload_from_disk(false);
        self.ensure_current_file_exists();
        self.refresh_git_ui().await;
    }

    /// После pull/checkout/reset текущий файл мог исчезнуть — возвращаемся в root.
    fn ensure_current_file_exists(self: &Rc<Self>) {
        let gone = {
            let st = self.state.borrow();
            match (&st.catalog, &st.current_id) {
                (Some(c), Some(id)) => !c.exists(id),
                _ => false,
            }
        };
        if !gone {
            return;
        }
        self.state.borrow_mut().current_id = None;
        self.open_file(ROOT_ID, false);
        self.show_tip("git", "Файл исчез после операции git", "Открыт корневой файл");
    }

    fn branch_label(&self) -> String {
        format!("Ветка {}", self.state.borrow().git_branch.clone().unwrap_or_else(|| "?".to_string()))
    }

    // ================= Истории 5–7 =================

    fn git_save(self: &Rc<Self>) {
        let w = self.clone();
        glib::spawn_future_local(async move {
            w.run_git("Не удалось сохранить на сервере", async |w, git| {
                let message = git::build_commit_message();
                let outcome = blocking(move || git.commit_and_push(&message)).await??;
                if outcome == PushOutcome::RejectedNonFastForward {
                    w.prompt_force_push().await?;
                } else {
                    w.show_tip("git", "Сохранено на сервере", &w.branch_label());
                }
                Ok(())
            })
            .await;
        });
    }

    fn git_pull(self: &Rc<Self>) {
        let w = self.clone();
        glib::spawn_future_local(async move {
            w.run_git("Не удалось получить с сервера", async |w, git| {
                let outcome = blocking(move || git.pull()).await??;
                if outcome == PullOutcome::Conflict {
                    w.prompt_take_server().await?;
                } else {
                    w.show_tip("git", "Получено с сервера", &w.branch_label());
                }
                Ok(())
            })
            .await;
        });
    }

    fn git_sync(self: &Rc<Self>) {
        let w = self.clone();
        glib::spawn_future_local(async move {
            w.run_git("Не удалось синхронизировать", async |w, git| {
                let message = git::build_commit_message();
                let outcome = blocking(move || git.sync(&message)).await??;
                match outcome {
                    SyncOutcome::Conflict => w.prompt_sync_conflict().await?,
                    SyncOutcome::PushRejected => w.prompt_force_push().await?,
                    SyncOutcome::Ok => w.show_tip("git", "Синхронизировано", &w.branch_label()),
                }
                Ok(())
            })
            .await;
        });
    }

    // ================= Диалоги конфликтов =================

    async fn prompt_force_push(self: &Rc<Self>) -> Result<(), GitError> {
        let answer = self
            .ask(
                "Конфликт с сервером",
                "На сервере есть изменения, которых нет у вас.\n\n\
                 Можно перезаписать серверную версию своей — серверные изменения будут потеряны. \
                 Либо отмените операцию и разрешите конфликт в специализированном git-приложении.",
                &[("cancel", "Отмена", false), ("force", "Мои затирают серверные", false)],
                "cancel",
                "cancel",
            )
            .await;
        if answer != "force" {
            return Ok(());
        }
        let git = self.state.borrow().git.clone().ok_or_else(|| GitError::Other("нет репозитория".into()))?;
        blocking(move || git.force_push()).await??;
        self.show_tip("git", "Сохранено на сервере", "Серверная версия перезаписана");
        Ok(())
    }

    async fn prompt_take_server(self: &Rc<Self>) -> Result<(), GitError> {
        let answer = self
            .ask(
                "Конфликт с сервером",
                "Серверные изменения конфликтуют с вашими.\n\n\
                 Можно взять серверную версию — ваши локальные изменения будут потеряны. \
                 Либо отмените операцию и разрешите конфликт в специализированном git-приложении.",
                &[("cancel", "Отмена", false), ("server", "Серверные затирают мои", false)],
                "cancel",
                "cancel",
            )
            .await;
        if answer != "server" {
            return Ok(());
        }
        let git = self.state.borrow().git.clone().ok_or_else(|| GitError::Other("нет репозитория".into()))?;
        blocking(move || git.reset_to_remote()).await??;
        self.show_tip("git", "Получено с сервера", "Локальная версия перезаписана");
        Ok(())
    }

    async fn prompt_sync_conflict(self: &Rc<Self>) -> Result<(), GitError> {
        let answer = self
            .ask(
                "Конфликт с сервером",
                "Ваши изменения и серверные конфликтуют.\n\n\
                 «Взять серверные» — ваши изменения будут потеряны.\n\
                 «Взять мои» — серверные изменения будут потеряны.\n\
                 Либо отмените операцию и разрешите конфликт в специализированном git-приложении.",
                &[("cancel", "Отмена", false), ("server", "Взять серверные", false), ("mine", "Взять мои", false)],
                "cancel",
                "cancel",
            )
            .await;
        let git = self.state.borrow().git.clone().ok_or_else(|| GitError::Other("нет репозитория".into()))?;
        match answer.as_str() {
            "server" => {
                blocking(move || git.reset_to_remote()).await??;
                self.show_tip("git", "Синхронизировано", "Взята серверная версия");
            }
            "mine" => {
                blocking(move || git.force_push()).await??;
                self.show_tip("git", "Синхронизировано", "Взята ваша версия");
            }
            _ => {}
        }
        Ok(())
    }

    // ================= Истории 3–4: ветки =================

    fn git_switch_branch(self: &Rc<Self>, branch: String) {
        let w = self.clone();
        glib::spawn_future_local(async move {
            w.run_git("Не удалось переключить ветку", async |w, git| {
                let b = branch.clone();
                blocking(move || git.switch_branch(&b)).await??;
                w.show_tip("git", "Ветка переключена", &branch);
                Ok(())
            })
            .await;
        });
    }

    fn git_create_branch(self: &Rc<Self>) {
        let w = self.clone();
        glib::spawn_future_local(async move {
            let Some(name) = w.prompt_text("Создать ветку", "Имя ветки", "например, feature-auth").await else { return };
            let name = name.trim().to_string();
            if name.is_empty() {
                return;
            }
            if let Some(error) = git::branch_name_error(&name) {
                w.show_message("Создать ветку", error, "Закрыть").await;
                return;
            }
            w.run_git("Не удалось создать ветку", async |w, git| {
                let n = name.clone();
                blocking(move || git.create_branch(&n)).await??;
                w.show_tip("git", "Ветка создана и отправлена на сервер", &name);
                Ok(())
            })
            .await;
        });
    }

    fn git_delete_branch(self: &Rc<Self>, name: String) {
        let w = self.clone();
        glib::spawn_future_local(async move {
            let answer = w
                .ask(
                    "Удалить ветку?",
                    &format!("Ветка «{name}» будет удалена и локально, и на сервере."),
                    &[("cancel", "Отмена", false), ("delete", "Удалить", false)],
                    "cancel",
                    "cancel",
                )
                .await;
            if answer != "delete" {
                return;
            }
            let current = w.state.borrow().git_branch.clone();
            w.run_git("Не удалось удалить ветку", async |w, git| {
                let n = name.clone();
                let leave_first = current.as_deref() == Some(name.as_str());
                blocking(move || {
                    // текущую ветку сначала покидаем — уходим на default
                    if leave_first {
                        let default = git.default_branch()?;
                        git.switch_branch(&default)?;
                    }
                    git.delete_branch(&n)
                })
                .await??;
                w.show_tip("git", "Ветка удалена", &name);
                Ok(())
            })
            .await;
        });
    }

    // ================= Истории 1–2: опубликовать, клонировать =================

    fn git_publish(self: &Rc<Self>) {
        let root = {
            let st = self.state.borrow();
            let Some(c) = st.catalog.as_ref() else { return };
            c.root_path().to_path_buf()
        };
        let w = self.clone();
        glib::spawn_future_local(async move {
            if git::is_repository(&root) {
                w.show_message("Опубликовать проект", "Каталог уже является git-репозиторием.", "Закрыть").await;
                return;
            }
            let Some(url) = w.prompt_text("Опубликовать проект на сервере", "Адрес пустого репозитория", "https://gitlab.com/you/project.git").await
            else {
                return;
            };
            let url = url.trim().to_string();
            if url.is_empty() {
                return;
            }
            // публикация делает каталог репозиторием — пересоздаём сервис до раннера
            w.state.borrow_mut().git = Some(GitService::new(&root));
            w.run_git("Не удалось опубликовать проект", async |w, git| {
                let u = url.clone();
                let result = blocking(move || git.init_and_publish(&u)).await;
                match result {
                    Ok(Ok(())) => {
                        w.show_tip("git", "Проект опубликован", &url);
                        Ok(())
                    }
                    Ok(Err(e)) | Err(e) => {
                        // репозиторий не состоялся — не притворяемся, что он есть
                        if !git::is_repository(&root) {
                            w.state.borrow_mut().git = None;
                        }
                        Err(e)
                    }
                }
            })
            .await;
        });
    }

    fn git_clone(self: &Rc<Self>) {
        let w = self.clone();
        self.show_clone_dialog(move |project| {
            {
                let mut st = w.state.borrow_mut();
                st.settings.projects.push(project.clone());
                st.settings.current_project_id = Some(project.id.clone());
            }
            w.save_settings();
            w.open_project(&project);
        });
    }

    // ================= Истории 8–9: история и дельты =================

    pub(super) fn wire_git_lists(self: &Rc<Self>) {
        let w = self.clone();
        self.git_commits_list.connect_row_activated(move |_, row| {
            let commit = w.state.borrow().git_commits.get(row.index() as usize).cloned();
            if let Some(commit) = commit {
                w.state.borrow_mut().git_selected = Some(commit);
                // снятие тумблера само перезагружает список; иначе грузим явно
                if w.git_since_toggle.is_active() {
                    w.git_since_toggle.set_active(false);
                } else {
                    w.load_git_files();
                }
            }
        });
        let w = self.clone();
        self.git_files_list.connect_row_activated(move |_, row| {
            let file = w.state.borrow().git_files.get(row.index() as usize).cloned();
            let Some(file) = file else { return };
            let exists = w.state.borrow().catalog.as_ref().is_some_and(|c| file.file_id.as_deref().is_some_and(|id| c.exists(id)));
            if file.is_deleted || !exists {
                w.show_tip("git", "Файла больше нет в каталоге", &file.path);
                return;
            }
            w.open_file(file.file_id.as_deref().unwrap(), true);
        });
    }

    /// Шаги самопроверки (NEURODOC_SMOKE): история, выбор коммита, дельта первого файла.
    pub fn smoke_git_history(self: &Rc<Self>) {
        self.git_show_history();
    }

    pub fn smoke_git_select_first_commit(self: &Rc<Self>) {
        if let Some(row) = self.git_commits_list.row_at_index(0) {
            row.activate();
        }
    }

    pub fn smoke_git_first_diff(self: &Rc<Self>) {
        let Some(row) = self.git_files_list.row_at_index(0) else { return };
        let mut child = row.child().and_then(|b| b.first_child());
        while let Some(c) = child {
            if let Ok(b) = c.clone().downcast::<gtk::Button>() {
                b.emit_clicked();
                return;
            }
            child = c.next_sibling();
        }
    }

    fn git_show_history(self: &Rc<Self>) {
        self.git_card.set_visible(true);
        self.show_git_commits_pane();
        let w = self.clone();
        glib::spawn_future_local(async move { w.load_git_commits().await });
    }

    /// Режим карточки: список коммитов.
    pub(super) fn show_git_commits_pane(&self) {
        self.state.borrow_mut().git_selected = None;
        self.git_header.set_text(&format!("История · {}", self.state.borrow().git_branch.clone().unwrap_or_else(|| "?".into())));
        self.git_back_button.set_visible(false);
        self.git_since_toggle.set_visible(false);
        self.git_files_list.set_visible(false);
        self.git_commits_list.set_visible(true);
        let empty = self.state.borrow().git_commits.is_empty();
        self.git_empty.set_text("Коммитов пока нет");
        self.git_empty.set_visible(empty);
    }

    async fn load_git_commits(self: &Rc<Self>) {
        let Some(git) = self.state.borrow().git.clone() else { return };
        let commits = match blocking(move || git.history(200)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) | Err(e) => {
                self.show_message("История коммитов", &e.to_string(), "Закрыть").await;
                return;
            }
        };
        clear_list(&self.git_commits_list);
        for c in &commits {
            let meta = format!("{}  {}  {}", c.short_hash, c.date.with_timezone(&chrono::Local).format("%d.%m.%Y %H:%M"), c.author);
            self.git_commits_list.append(&list_row(&c.message, &meta, &["nd-dim", "nd-size-10", "nd-mono"]));
        }
        let selected_gone = {
            let mut st = self.state.borrow_mut();
            st.git_commits = commits;
            st.git_selected.as_ref().is_some_and(|s| !st.git_commits.iter().any(|c| c.hash == s.hash))
        };
        self.git_header.set_text(&format!("История · {}", self.state.borrow().git_branch.clone().unwrap_or_else(|| "?".into())));
        let empty = self.state.borrow().git_commits.is_empty();
        self.git_empty.set_visible(empty && self.git_commits_list.is_visible());
        // выбранный коммит после перечитывания мог остаться — возвращаемся к списку
        if selected_gone {
            self.show_git_commits_pane();
        }
    }

    /// Режим карточки: файлы выбранного коммита либо всё изменённое с него.
    pub(super) fn load_git_files(self: &Rc<Self>) {
        let (git, commit) = {
            let st = self.state.borrow();
            match (&st.git, &st.git_selected, &st.catalog) {
                (Some(g), Some(c), Some(_)) => (g.clone(), c.clone()),
                _ => return,
            }
        };
        let since = self.git_since_toggle.is_active();
        let w = self.clone();
        glib::spawn_future_local(async move {
            let hash = commit.hash.clone();
            let g = git.clone();
            let changes = match blocking(move || if since { g.changes_since(&hash) } else { g.commit_files(&hash) }).await {
                Ok(Ok(c)) => c,
                Ok(Err(e)) | Err(e) => {
                    w.show_message("Изменённые файлы", &e.to_string(), "Закрыть").await;
                    return;
                }
            };
            let mut rows = Vec::new();
            for change in changes {
                if git::is_service_file(&change.path) {
                    continue;
                }
                rows.push(w.build_git_file_row(&git, &commit, &change, since).await);
            }
            // пока ждали git, пользователь мог выбрать другой коммит — устаревший ответ не показываем
            let still_selected = w.state.borrow().git_selected.as_ref().is_some_and(|c| c.hash == commit.hash);
            if !still_selected || w.git_since_toggle.is_active() != since {
                return;
            }
            clear_list(&w.git_files_list);
            for r in &rows {
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
                texts.set_hexpand(true);
                let t = gtk::Label::new(Some(&r.title));
                t.set_xalign(0.0);
                t.set_ellipsize(gtk::pango::EllipsizeMode::End);
                t.add_css_class("nd-size-13");
                texts.append(&t);
                let d = gtk::Label::new(Some(&r.detail));
                d.set_xalign(0.0);
                d.set_ellipsize(gtk::pango::EllipsizeMode::End);
                d.add_css_class("nd-dim");
                d.add_css_class("nd-size-10");
                d.add_css_class("nd-mono");
                texts.append(&d);
                b.append(&texts);
                let diff = gtk::Button::with_label("±");
                diff.add_css_class("nd-small-button");
                diff.set_valign(gtk::Align::Center);
                diff.set_tooltip_text(Some("Что изменилось в файле"));
                let w2 = w.clone();
                let row = r.clone();
                let commit2 = commit.clone();
                let git2 = git.clone();
                diff.connect_clicked(move |_| w2.git_show_diff(&git2, &commit2, &row, since));
                b.append(&diff);
                w.git_files_list.append(&gtk::ListBoxRow::builder().child(&b).activatable(true).build());
            }
            w.git_header.set_text(&format!("{} {}", if since { "С коммита" } else { "Коммит" }, commit.short_hash));
            w.git_back_button.set_visible(true);
            w.git_since_toggle.set_visible(true);
            w.git_commits_list.set_visible(false);
            w.git_files_list.set_visible(true);
            w.git_empty.set_visible(rows.is_empty());
            w.state.borrow_mut().git_files = rows;
        });
    }

    async fn build_git_file_row(&self, git: &GitService, commit: &GitCommit, change: &git::GitFileChange, since: bool) -> GitFileRow {
        // путь в репозитории -> идентификатор файла каталога
        let id = if change.path == ROOT_ID { Some(ROOT_ID.to_string()) } else { catalog::try_resolve_link(ROOT_ID, &change.path) };
        let deleted = change.status == 'D';
        let title_from_catalog = {
            let st = self.state.borrow();
            match (&st.catalog, &id) {
                (Some(c), Some(id)) if !deleted && c.exists(id) => Some(c.title(id, 60)),
                _ => None,
            }
        };
        let title = match title_from_catalog {
            Some(t) => t,
            None => {
                // файла уже нет — заголовок берём из прежней ревизии
                let rev = if since { commit.hash.clone() } else { format!("{}^", commit.hash) };
                let path = change.old_path.clone().unwrap_or_else(|| change.path.clone());
                let g = git.clone();
                let text = blocking(move || g.read_blob(&rev, &path).ok().flatten()).await.ok().flatten();
                text.as_deref().and_then(git::first_non_empty_line).unwrap_or_else(|| change.path.clone())
            }
        };
        GitFileRow {
            path: change.path.clone(),
            file_id: id,
            title,
            detail: format!("{}  {}", git::status_label(change.status), change.path),
            is_deleted: deleted,
        }
    }

    fn git_show_diff(self: &Rc<Self>, git: &GitService, commit: &GitCommit, row: &GitFileRow, since: bool) {
        let w = self.clone();
        let g = git.clone();
        let (hash, path, title) = (commit.hash.clone(), row.path.clone(), row.title.clone());
        glib::spawn_future_local(async move {
            let (h, p) = (hash.clone(), path.clone());
            match blocking(move || g.file_diff(&h, &p, since)).await {
                Ok(Ok(diff)) => w.show_diff(&format!("{hash}{}{path}", if since { "~" } else { "·" }), &title, &diff),
                Ok(Err(e)) | Err(e) => w.show_message("Дельта файла", &e.to_string(), "Закрыть").await,
            }
        });
    }
}
