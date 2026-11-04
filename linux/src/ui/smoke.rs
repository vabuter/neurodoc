//! Самопроверка интерфейса без рук: `NEURODOC_SMOKE=/папка` проходит по файлам,
//! режимам просмотра и диалогам, сохраняя снимок окна после каждого шага.
//! Нужна разработчику (и агенту), в обычной работе не включается.

use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use super::util::snapshot_to_png;
use super::window::Win;
use crate::viewer::ViewMode;

pub fn run(win: Rc<Win>, dir: String) {
    glib::spawn_future_local(async move {
        let pause = || glib::timeout_future(Duration::from_millis(700));
        glib::timeout_future(Duration::from_millis(2000)).await;
        shot(&win, &dir, "01-root").await;

        // первая ссылка root
        let first = win.editor.links_on_line(0).into_iter().chain((1..40).flat_map(|l| win.editor.links_on_line(l))).find(|l| !l.is_web);
        if let Some(link) = first {
            win.open_file(&link.target, true);
            pause().await;
            shot(&win, &dir, "02-linked-file").await;
        }
        win.open_file("contract/auth/start.yaml", true);
        pause().await;
        pause().await;
        shot(&win, &dir, "03-yaml").await;
        win.open_file("README.md", true);
        pause().await;
        pause().await;
        shot(&win, &dir, "04-md").await;
        win.set_view_mode(ViewMode::Text);
        pause().await;
        shot(&win, &dir, "05-md-as-text").await;
        // схема BPMN из _content: расширения нет, формат выбирают кнопкой
        if let Some(svg) = win.smoke_first_svg() {
            win.open_file(&svg, true);
            pause().await;
            win.set_view_mode(ViewMode::Svg);
            pause().await;
            pause().await;
            shot(&win, &dir, "05a-svg").await;
            win.set_view_mode(ViewMode::Text);
        }
        // «как есть»: ссылки видны целиком, ничего не скрыто
        win.open_file("root", true);
        pause().await;
        win.smoke_set_raw(true);
        pause().await;
        shot(&win, &dir, "05b-raw").await;
        win.smoke_set_raw(false);
        pause().await;
        win.open_file("root", true);
        pause().await;
        win.show_find_bar();
        win.set_find_text("fikus");
        win.find_step(1);
        pause().await;
        shot(&win, &dir, "06-find").await;
        // правка при открытой панели поиска раньше роняла приложение (двойное заимствование)
        win.editor.buffer.insert_at_cursor("x");
        pause().await;
        win.editor.buffer.undo();
        win.close_find_bar();
        win.run_project_search("авториз");
        pause().await;
        shot(&win, &dir, "07-search").await;
        // дальше результаты поиска только отнимали бы место у карточек ниже (история git)
        win.run_project_search("");
        win.smoke_show_broken_links_card();
        pause().await;
        shot(&win, &dir, "08-broken").await;
        win.show_settings_dialog();
        pause().await;
        shot(&win, &dir, "09-settings").await;
        win.close_dialogs();
        pause().await;
        win.show_hotkeys_dialog();
        pause().await;
        shot(&win, &dir, "10-hotkeys").await;
        win.close_dialogs();
        pause().await;
        win.show_open_by_id_dialog();
        pause().await;
        shot(&win, &dir, "11-open").await;
        win.close_dialogs();
        pause().await;
        win.show_projects_dialog();
        pause().await;
        shot(&win, &dir, "12-projects").await;
        win.close_dialogs();
        pause().await;
        win.show_tip("copy_link", "Ссылка скопирована", "[пример](5c673c53-8ad9-49eb-9fb6-ffb3954848f0)");
        glib::timeout_future(Duration::from_millis(300)).await;
        shot(&win, &dir, "13-tip").await;
        win.toggle_broken_links_card();
        pause().await;
        shot(&win, &dir, "14-broken-toggle").await;
        // git: история, файлы коммита, дельта (каталог заранее сделан репозиторием)
        win.smoke_git_history();
        pause().await;
        pause().await;
        shot(&win, &dir, "15-git-history").await;
        win.smoke_git_select_first_commit();
        pause().await;
        pause().await;
        shot(&win, &dir, "16-git-files").await;
        win.smoke_git_first_diff();
        pause().await;
        pause().await;
        shot(&win, &dir, "17-git-diff").await;
        // root уже открыт, и open_file ничего не сделает: из дельты выходим в текст сами
        win.open_file("root", true);
        win.set_view_mode(ViewMode::Text);
        win.smoke_set_theme(crate::domain::settings::ThemeMode::Light);
        pause().await;
        pause().await;
        shot(&win, &dir, "18-light").await;
        win.smoke_set_theme(crate::domain::settings::ThemeMode::System);
        pause().await;
        check_scroll_kept(&win).await;
        check_disk_watch(&win).await;
        win.window.close();
    });
}

/// Снимок окна в `dir/name.png`. WebKit посреди загрузки и окно сразу после смены темы
/// иногда отдают пустой узел — тогда несколько повторов с паузой. Ждём именно `await`, а
/// не крутим главный цикл вручную (`MainContext::iteration`): изнутри задачи это запускает
/// соседние задачи вложенно (слежение за диском, например), и GLib аварийно
/// останавливает процесс (EnterError).
async fn shot(win: &Win, dir: &str, name: &str) {
    let path = format!("{dir}/{name}.png");
    let mut result = snapshot_to_png(win.window.upcast_ref(), &path);
    for _ in 0..5 {
        if result.is_ok() {
            break;
        }
        glib::timeout_future(Duration::from_millis(100)).await;
        result = snapshot_to_png(win.window.upcast_ref(), &path);
    }
    match result {
        Ok(()) => eprintln!("smoke: {path}"),
        Err(e) => eprintln!("smoke: снимок {name} не удался: {e}"),
    }
}

/// Прокрутка не должна сбрасываться сама. Раньше слежение за диском перечитывало каталог
/// по кругу (inotify сообщал и о чтении файлов), и просмотр раз в полсекунды прыгал в
/// начало. Проверка — числом в журнале: снимок прокрутку не покажет. Короткий файл
/// проверить нельзя, о чём и пишется.
async fn check_scroll_kept(win: &Rc<Win>) {
    let settle = || glib::timeout_future(Duration::from_millis(2000));
    let verdict = |before: f64, after: f64| if (before - after).abs() < 2.0 { "сохранилась" } else { "СБРОШЕНА" };

    win.open_file("root", true);
    // root уже открыт с шага поиска, и open_file ничего не сделает, а на экране всё ещё
    // дельта git: без выхода в текст мерили бы спрятанный редактор
    win.set_view_mode(ViewMode::Text);
    settle().await;
    // курсор проверяется на любой длине файла, прокрутка — только на длинном
    let (over, off) = hover_cycle(win).await;
    let ok = over == "pointer" && off == "text";
    eprintln!("smoke: курсор над ссылкой «{over}», вне ссылки «{off}» ({})", if ok { "верно" } else { "НЕВЕРНО" });
    let adj = win.editor.scroll.vadjustment();
    let target = ((adj.upper() - adj.page_size()) / 2.0).floor();
    if target < 50.0 {
        eprintln!("smoke: прокрутка редактора не проверялась — root короче окна");
    } else {
        adj.set_value(target);
        settle().await;
        eprintln!("smoke: прокрутка редактора в покое {target} → {} ({})", adj.value(), verdict(target, adj.value()));
        // перечитывание с диска и перекраска прочитанных ссылок перетэговывают весь буфер
        win.reload_from_disk(false);
        win.editor.set_visited(win.visited_set());
        settle().await;
        eprintln!("smoke: прокрутка редактора после перечитывания {target} → {} ({})", adj.value(), verdict(target, adj.value()));
        hover_cycle(win).await;
        settle().await;
        eprintln!("smoke: прокрутка редактора после наведений {target} → {} ({})", adj.value(), verdict(target, adj.value()));
    }

    win.open_file("README.md", true);
    // режим, выбранный раньше в этом же прогоне (шаг «MD как текст»), побеждает расширение
    win.set_view_mode(ViewMode::Md);
    settle().await;
    let room = win.smoke_eval("String(document.scrollingElement.scrollHeight - window.innerHeight)").await;
    let room: f64 = room.and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let target = (room / 2.0).floor();
    if target < 50.0 {
        eprintln!("smoke: прокрутка просмотра не проверялась — README.md короче окна");
        return;
    }
    win.smoke_eval(&format!("window.scrollTo(0, {target}); 'ok'")).await;
    settle().await;
    let after: f64 = win.smoke_eval("String(window.scrollY)").await.and_then(|s| s.parse().ok()).unwrap_or(-1.0);
    eprintln!("smoke: прокрутка просмотра MD в покое {target} → {after} ({})", verdict(target, after));
}

/// Наведение «мыши» на ссылку в видимой части и обратно, десять раз — как при прокрутке
/// колесом над текстом. Возвращает курсор редактора над ссылкой и вне её.
async fn hover_cycle(win: &Rc<Win>) -> (String, String) {
    let cursor = || win.editor.view.cursor().and_then(|c| c.name()).map(|n| n.to_string()).unwrap_or_default();
    let (mut over, mut off) = (String::new(), String::new());
    for _ in 0..10 {
        win.editor.smoke_hover_visible_link(true);
        over = cursor();
        glib::timeout_future(Duration::from_millis(50)).await;
        win.editor.smoke_hover_visible_link(false);
        off = cursor();
        glib::timeout_future(Duration::from_millis(50)).await;
    }
    (over, off)
}


/// Слежение за диском отличает свою запись от чужой по содержимому. Раньше событие
/// считалось своим, если приложение писало что угодно меньше секунды назад, — и чужая
/// правка сразу после автосохранения терялась: перечитывание не назначалось вовсе.
/// Здесь чужая правка приходит через ~200 мс после автосохранения (оно срабатывает через
/// 700 мс после правки), в другой файл и в тот же. Файлы потом возвращаются как были.
async fn check_disk_watch(win: &Rc<Win>) {
    let wait = |ms: u64| glib::timeout_future(Duration::from_millis(ms));
    let Some(dir) = win.smoke_catalog_root() else { return };
    let (root_file, readme) = (dir.join("root"), dir.join("README.md"));
    let (Ok(root_before), Ok(readme_before)) = (std::fs::read_to_string(&root_file), std::fs::read_to_string(&readme)) else {
        eprintln!("smoke: слежение не проверялось — нет root или README.md");
        return;
    };
    let ok = |good: bool| if good { "верно" } else { "НЕВЕРНО" };
    let type_at_end = |text: &str| {
        let mut end = win.editor.buffer.end_iter();
        win.editor.buffer.insert(&mut end, text);
    };
    win.open_file("root", true);
    win.set_view_mode(ViewMode::Text);
    wait(1500).await;

    let before = win.smoke_reload_count();
    type_at_end("\nправка самопроверки");
    wait(2500).await; // автосохранение, событие и дебаунс — с запасом
    let own = win.smoke_reload_count() - before;
    eprintln!("smoke: слежение — своё автосохранение перечитываний: {own} ({})", ok(own == 0));

    let marker = "внешняя правка самопроверки";
    type_at_end(" ещё");
    wait(900).await;
    let _ = std::fs::write(&readme, format!("{readme_before}\n{marker}\n"));
    wait(1500).await;
    let picked = win.smoke_catalog_text("README.md").contains(marker);
    eprintln!("smoke: слежение — чужая правка другого файла сразу после автосохранения {} ({})", if picked { "подхвачена" } else { "ПОТЕРЯНА" }, ok(picked));

    type_at_end(" и ещё");
    wait(900).await;
    let on_disk = std::fs::read_to_string(&root_file).unwrap_or_default();
    let _ = std::fs::write(&root_file, format!("{on_disk}\n{marker}"));
    wait(1500).await;
    let picked = win.editor.raw_text().contains(marker);
    eprintln!("smoke: слежение — чужая правка того же файла сразу после автосохранения {} ({})", if picked { "подхвачена" } else { "ПОТЕРЯНА" }, ok(picked));

    let _ = std::fs::write(&root_file, &root_before);
    let _ = std::fs::write(&readme, &readme_before);
    wait(1500).await;
    let restored = win.editor.raw_text() == root_before.replace("\r\n", "\n");
    eprintln!("smoke: слежение — возврат файлов подхвачен ({})", ok(restored));

    // файл с точкой в пути каталог в памяти не держит (открыт как по ссылке, читается с
    // диска мимо неё) — его чужая правка всё равно должна дойти до редактора
    let dot = dir.join(".smoke-watch.yml");
    let _ = std::fs::write(&dot, "before: 1\n");
    win.open_file(".smoke-watch.yml", true);
    win.set_view_mode(ViewMode::Text);
    wait(1500).await;
    let _ = std::fs::write(&dot, format!("after: 2 # {marker}\n"));
    wait(1500).await;
    let picked = win.editor.raw_text().contains(marker);
    eprintln!("smoke: слежение — чужая правка открытого файла с точкой {} ({})", if picked { "подхвачена" } else { "ПОТЕРЯНА" }, ok(picked));
    win.open_file("root", true);
    let _ = std::fs::remove_file(&dot);
    wait(1000).await;
}
