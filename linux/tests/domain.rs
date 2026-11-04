//! Проверка доменного слоя на копии эталонного каталога example/ во временной папке.
//! Сам example/ при этом не меняется (правило CLAUDE.md).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use neurodoc_editor::domain::catalog::{self, NeurodocCatalog, ROOT_ID};
use neurodoc_editor::domain::git::{self, GitService, PullOutcome, PushOutcome, SyncOutcome};
use neurodoc_editor::domain::links;
use neurodoc_editor::domain::migration;
use neurodoc_editor::domain::text;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Временная папка, удаляемая по выходу из теста.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("neurodoc-test-{}-{label}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn example_copy(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("example");
    copy_dir(&source, dir.path());
    dir
}

fn eq_ci(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

#[test]
fn opens_example_catalog() {
    let dir = example_copy("open");
    let catalog = NeurodocCatalog::open_or_create(dir.path()).unwrap();
    let ids = catalog.all_ids();
    assert_eq!(ids[0], ROOT_ID);
    assert!(ids.iter().any(|id| catalog::is_uuid(id)));
    assert!(ids.iter().any(|id| id == "contract/dto/Account.yaml"));
    assert!(ids.iter().any(|id| id == "README.md"), "имя файла хранится как на диске");
    assert!(!ids.iter().any(|id| id.starts_with("_content/")), "_content не попадает второй раз");
    assert!(catalog.exists("readme.md") && catalog.exists("README.md"));
    assert_eq!(catalog.canonical_id("readme.md"), "README.md");
    assert_eq!(catalog.title("contract/dto/Account.yaml", 60), "Account.yaml");
    assert!(!catalog.title(ROOT_ID, 60).is_empty());
    // файл в собственном формате — цель, а не источник: примеры ссылок из README
    // не считаются ни обратными ссылками, ни битыми
    assert!(catalog.broken_links().iter().all(|b| b.file_id != "README.md"));
    assert!(!catalog.build_graph().contains_key("readme.md"));
}

#[test]
fn resolves_path_links_from_content_file() {
    let dir = example_copy("paths");
    let catalog = NeurodocCatalog::open_or_create(dir.path()).unwrap();
    // «Растения — концепция» ссылается на DTO путём ../contract/dto/Plant.yaml
    let source = "895bfa7b-1ef3-4c82-bd48-f16ab70d02ea";
    let links = catalog.links(source);
    assert!(links.iter().any(|(_, _, id, _)| id == "contract/dto/Plant.yaml"), "{links:?}");
    assert!(catalog.exists("contract/dto/Plant.yaml"));
    let back = catalog.backlinks("contract/dto/Plant.yaml");
    assert!(back.iter().any(|b| b.file_id == source));
}

#[test]
fn graph_queries_are_consistent() {
    let dir = example_copy("graph");
    let catalog = NeurodocCatalog::open_or_create(dir.path()).unwrap();
    let ids = catalog.all_ids();

    for id in ids.iter().filter(|id| catalog::is_neurodoc_file(id)).take(40) {
        for b in catalog.backlinks(id) {
            assert!(catalog.links(&b.file_id).iter().any(|(_, _, t, _)| eq_ci(t, id)), "{} → {id}", b.file_id);
        }
        let paths = catalog.shortest_paths_from_root(id, 50);
        if id == ROOT_ID {
            assert!(paths.is_empty());
            continue;
        }
        let len = paths.first().map(Vec::len);
        for path in &paths {
            assert_eq!(Some(path.len()), len, "все пути минимальной длины");
            assert_eq!(path[0], ROOT_ID);
            assert!(eq_ci(path.last().unwrap(), id));
            for pair in path.windows(2) {
                assert!(catalog.links(&pair[0]).iter().any(|(_, _, t, _)| eq_ci(t, &pair[1])), "{pair:?}");
            }
        }
    }

    for orphan in catalog.orphan_files() {
        assert!(catalog.backlinks(&orphan.file_id).is_empty());
        assert_ne!(orphan.file_id, ROOT_ID);
    }

    let order = catalog.build_reading_order();
    assert_eq!(order[0], ROOT_ID);
    let mut seen = std::collections::HashSet::new();
    for id in &order {
        assert!(catalog::is_neurodoc_file(id));
        assert!(seen.insert(id.to_lowercase()), "дубль в порядке чтения: {id}");
    }
    let neurodoc_count = ids.iter().filter(|id| catalog::is_neurodoc_file(id)).count();
    assert_eq!(order.len(), neurodoc_count);

    for broken in catalog.broken_links() {
        assert!(!catalog.exists(&broken.target_id));
        assert!(catalog::is_neurodoc_file(&broken.file_id));
    }
}

#[test]
fn search_finds_text_case_insensitively() {
    let dir = example_copy("search");
    let catalog = NeurodocCatalog::open_or_create(dir.path()).unwrap();
    let results = catalog.search("FIKUS", 200);
    assert!(!results.is_empty());
    let first = &results[0];
    assert!(eq_ci(&first.match_text, "fikus"));
    let content = catalog.read_file(&first.file_id);
    let pos = text::nth_occurrence(&content, "fikus", first.occurrence).unwrap();
    assert!(eq_ci(&content[pos..pos + "fikus".len()], "fikus"));
    assert!(catalog.search("", 200).is_empty());
    assert_eq!(catalog.search("fikus", 3).len(), 3);
}

#[test]
fn write_keeps_on_disk_spelling_and_bytes() {
    let dir = example_copy("write");
    let mut catalog = NeurodocCatalog::open_or_create(dir.path()).unwrap();
    catalog.write_file("readme.md", "новый текст\r\nвторая").unwrap();
    assert_eq!(fs::read_to_string(dir.path().join("README.md")).unwrap(), "новый текст\r\nвторая");
    assert!(!dir.path().join("readme.md").exists(), "дубликата с другим регистром нет");
    catalog.reload();
    assert_eq!(catalog.read_file("ReadMe.md").as_ref(), "новый текст\r\nвторая");

    let id = catalog.create_new_file().unwrap();
    assert!(catalog::is_uuid(&id) && id == id.to_lowercase());
    assert!(dir.path().join("_content").join(&id).exists());
    catalog.delete_file(&id).unwrap();
    assert!(!catalog.exists(&id));
    assert!(catalog.file_path("../x").is_err());
}

/// Слежение за диском отличает свою запись от чужой по содержимому, а не по времени:
/// чужая правка сразу после автосохранения раньше терялась.
#[test]
fn matches_disk_tells_own_writes_from_outside_ones() {
    let dir = example_copy("matches");
    let root = dir.path();
    let mut catalog = NeurodocCatalog::open_or_create(root).unwrap();
    for rel in ["root", "README.md", "contract/auth/start.yaml"] {
        assert!(catalog.matches_disk(&root.join(rel)), "после открытия совпадает: {rel}");
    }

    // своя запись — совпадает, сколько бы ни прошло времени
    let id = catalog.create_new_file().unwrap();
    let path = root.join("_content").join(&id);
    catalog.write_file(&id, "своё").unwrap();
    assert!(catalog.matches_disk(&path));
    // чужая — не совпадает, даже в тот же файл сразу после своей
    fs::write(&path, "чужое").unwrap();
    assert!(!catalog.matches_disk(&path));
    catalog.reload();
    assert!(catalog.matches_disk(&path));
    // чужая запись того же текста ничего не меняет
    fs::write(&path, "чужое").unwrap();
    assert!(catalog.matches_disk(&path));

    // своё удаление — файла нет ни на диске, ни в памяти
    catalog.delete_file(&id).unwrap();
    assert!(catalog.matches_disk(&path));
    // чужое удаление известного файла — изменение
    fs::remove_file(root.join("contract/auth/start.yaml")).unwrap();
    assert!(!catalog.matches_disk(&root.join("contract/auth/start.yaml")));
    catalog.reload();

    // новый текстовый файл перечитывание заведёт, двоичный и скрытый — нет
    fs::write(root.join("contract/new.yaml"), "a: 1").unwrap();
    assert!(!catalog.matches_disk(&root.join("contract/new.yaml")));
    fs::create_dir_all(root.join("img")).unwrap();
    fs::write(root.join("img/x.png"), b"\x89PNG\0\0").unwrap();
    assert!(catalog.matches_disk(&root.join("img/x.png")));
    // двоичность определяется по началу файла: нуль дальше — пусть решает перечитывание
    let mut late_nul = vec![b'a'; 20 * 1024];
    late_nul.push(0);
    fs::write(root.join("img/late.bin"), late_nul).unwrap();
    assert!(!catalog.matches_disk(&root.join("img/late.bin")));
    fs::write(root.join(".root.swp"), "x").unwrap();
    assert!(catalog.matches_disk(&root.join(".root.swp")));
    assert!(catalog.matches_disk(&root.join("contract/.new.yaml.swp")));
    // файл с точкой, попавший в память записью (открыт по ссылке и отредактирован),
    // сверяется как любой известный: чужую правку в нём терять нельзя
    catalog.write_file(".gitlab-ci.yml", "stages: [build]").unwrap();
    assert!(catalog.matches_disk(&root.join(".gitlab-ci.yml")));
    fs::write(root.join(".gitlab-ci.yml"), "stages: [test]").unwrap();
    assert!(!catalog.matches_disk(&root.join(".gitlab-ci.yml")));
    // унесённая папка с точки, где лежал известный файл, — тоже изменение
    catalog.write_file(".github/ci.yml", "on: push").unwrap();
    fs::rename(root.join(".github"), root.join("github-old")).unwrap();
    assert!(!catalog.matches_disk(&root.join(".github")));
    assert!(!catalog.matches_disk(root));
    // перечитывание заводит и файл, чей путь разбор ссылки не примет; его пропажа — тоже изменение
    fs::write(root.join("contract/12:30.md"), "заметка").unwrap();
    catalog.reload();
    fs::remove_file(root.join("contract/12:30.md")).unwrap();
    assert!(!catalog.matches_disk(&root.join("contract/12:30.md")));
    catalog.reload();
    // тот же файл в другом регистре — для ФС Linux это другой файл
    fs::write(root.join("readme.md"), "x").unwrap();
    assert!(!catalog.matches_disk(&root.join("readme.md")));
    fs::remove_file(root.join("readme.md")).unwrap();

    // папка: переименованная уносит известные файлы, новая — всегда «изменился»
    catalog.reload();
    fs::rename(root.join("contract"), root.join("contract2")).unwrap();
    assert!(!catalog.matches_disk(&root.join("contract")));
    assert!(!catalog.matches_disk(&root.join("contract2")));
    assert!(!catalog.matches_disk(&root.join("_content")));
    // пропавшая незнакомая папка ничего не меняет, путь вне каталога — сомнение
    assert!(catalog.matches_disk(&root.join("nowhere")));
    assert!(!catalog.matches_disk(Path::new("/tmp/elsewhere.txt")));
}

#[test]
fn remove_links_keeps_anchor_text() {
    let dir = TempDir::new("unlink");
    let mut catalog = NeurodocCatalog::open_or_create(dir.path()).unwrap();
    let id = catalog.create_new_file().unwrap();
    let other = catalog.create_new_file().unwrap();
    catalog.write_file(ROOT_ID, &format!("см. [якорь]({id}) и []({id}), а [другой]({other}).\n")).unwrap();
    assert_eq!(catalog.remove_links_to(&id).unwrap(), 1);
    assert_eq!(catalog.read_file(ROOT_ID).as_ref(), format!("см. якорь и , а [другой]({other}).\n"));
    assert!(catalog.backlinks(&id).is_empty());
    assert_eq!(catalog.backlinks(&other).len(), 1);
}

#[test]
fn broken_links_and_migration() {
    let dir = TempDir::new("migrate");
    let mut catalog = NeurodocCatalog::open_or_create(dir.path()).unwrap();
    let id = catalog.create_new_file().unwrap();
    let upper = id.to_uppercase();
    catalog.write_file(ROOT_ID, &format!("старая <{upper}> ссылка, [битая](deadbeef-dead-dead-dead-deaddeaddead) и [наружу](../x)\n")).unwrap();

    let broken = catalog.broken_links();
    assert_eq!(broken.len(), 2, "{broken:?}");
    assert_eq!(broken[0].target_id, "deadbeef-dead-dead-dead-deaddeaddead");
    assert_eq!(broken[1].target_id, "../x");
    assert!(broken[0].context.contains("старая"));

    let scan = migration::scan(&catalog);
    assert_eq!((scan.files, scan.links), (1, 1));
    assert!(scan.needs_migration());
    // резервная копия — вне каталога, иначе она копировалась бы сама в себя
    let backup_dir = TempDir::new("backup");
    let backup = backup_dir.path().join("copy");
    let result = migration::migrate_with_backup(&mut catalog, backup.clone()).unwrap();
    assert_eq!((result.files, result.links), (1, 1));
    assert!(catalog.read_file(ROOT_ID).contains(&format!("[]({id})")));
    assert!(fs::read_to_string(backup.join("root")).unwrap().contains(&format!("<{upper}>")));
    assert!(!migration::scan(&catalog).needs_migration());
    assert_eq!(catalog.links(ROOT_ID).len(), 2);
}

#[test]
fn clipboard_target_requires_existing_file() {
    let dir = example_copy("clip");
    let catalog = NeurodocCatalog::open_or_create(dir.path()).unwrap();
    assert_eq!(
        links::parse_clipboard_target("[Plant](../contract/dto/Plant.yaml)", &catalog).as_deref(),
        Some("contract/dto/Plant.yaml")
    );
    assert_eq!(links::parse_clipboard_target("просто текст", &catalog), None);
    assert_eq!(links::parse_clipboard_target("../contract/nope.yaml", &catalog), None);
    let uuid = catalog.all_ids().into_iter().find(|id| catalog::is_uuid(id)).unwrap();
    assert_eq!(links::parse_clipboard_target(&uuid.to_uppercase(), &catalog).as_deref(), Some(uuid.as_str()));
}

/// Весь рабочий цикл git на локальном bare-репозитории: без сети и токенов.
#[test]
fn git_workflow_on_local_bare_repository() {
    if !git::is_git_available() {
        eprintln!("git не найден — тест пропущен");
        return;
    }
    let root = TempDir::new("git");
    // identity автора — своя, чтобы не зависеть от глобальной настройки машины
    let gitconfig = root.path().join("gitconfig");
    fs::write(&gitconfig, "[user]\n\tname = Test\n\temail = test@example.com\n").unwrap();
    // SAFETY: переменные окружения задаются до запуска дочерних git-процессов, один раз
    unsafe {
        std::env::set_var("GIT_CONFIG_GLOBAL", &gitconfig);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    }

    let bare = root.path().join("remote.git");
    fs::create_dir_all(&bare).unwrap();
    // у сервера HEAD смотрит на main (как у GitLab); без этого клон пустого HEAD выходит без файлов
    assert!(std::process::Command::new("git").args(["init", "--bare", "-q", "-b", "main"]).current_dir(&bare).status().unwrap().success());
    let url = format!("file://{}", bare.display());

    // история 1: опубликовать
    let a_dir = root.path().join("a");
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("example"), &a_dir);
    let a = GitService::new(&a_dir);
    assert!(!git::is_repository(&a_dir));
    a.init_and_publish(&url).unwrap();
    assert!(git::is_repository(&a_dir));
    assert_eq!(a.current_branch().unwrap().as_deref(), Some("main"));
    assert_eq!(a.history(200).unwrap().len(), 1);
    assert_eq!(a.default_branch().unwrap(), "main");

    // история 5: сохранить на сервере
    fs::write(a_dir.join("root"), "правка A\n").unwrap();
    assert!(a.is_dirty().unwrap());
    assert_eq!(a.commit_and_push(&git::build_commit_message()).unwrap(), PushOutcome::Ok);
    let history = a.history(200).unwrap();
    assert_eq!(history.len(), 2);
    assert!(history[0].message.contains(" · "));
    let files = a.commit_files(&history[0].hash).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!((files[0].path.as_str(), files[0].status), ("root", 'M'));
    assert!(a.file_diff(&history[0].hash, "root", false).unwrap().contains("+правка A"));
    assert!(a.read_blob(&format!("{}^", history[0].hash), "root").unwrap().is_some());

    // истории 3–4: ветки и stash при переключении
    a.create_branch("dev").unwrap();
    assert_eq!(a.current_branch().unwrap().as_deref(), Some("dev"));
    fs::write(a_dir.join("root"), "незакоммиченная правка в dev\n").unwrap();
    a.switch_branch("main").unwrap();
    assert_eq!(fs::read_to_string(a_dir.join("root")).unwrap(), "правка A\n", "правки dev спрятаны");
    a.switch_branch("dev").unwrap();
    assert_eq!(fs::read_to_string(a_dir.join("root")).unwrap(), "незакоммиченная правка в dev\n", "правки dev вернулись");
    a.switch_branch("main").unwrap();
    assert!(a.local_branches().unwrap().contains(&"dev".to_string()));
    a.delete_branch("dev").unwrap();
    assert!(!a.local_branches().unwrap().contains(&"dev".to_string()));

    // история 2: клонировать
    let b_dir = root.path().join("b");
    git::clone(&url, &b_dir).unwrap();
    let b = GitService::new(&b_dir);
    assert_eq!(fs::read_to_string(b_dir.join("root")).unwrap(), "правка A\n");

    // история 6: получить с сервера
    fs::write(b_dir.join("root"), "правка B\n").unwrap();
    assert_eq!(b.commit_and_push("B").unwrap(), PushOutcome::Ok);
    assert_eq!(a.pull().unwrap(), PullOutcome::Ok);
    assert_eq!(fs::read_to_string(a_dir.join("root")).unwrap(), "правка B\n");

    // конфликт: push отвергнут, sync не сливает, reset берёт серверное
    fs::write(b_dir.join("root"), "правка B2\n").unwrap();
    assert_eq!(b.commit_and_push("B2").unwrap(), PushOutcome::Ok);
    fs::write(a_dir.join("root"), "правка A2\n").unwrap();
    assert_eq!(a.commit_and_push("A2").unwrap(), PushOutcome::RejectedNonFastForward);
    assert_eq!(a.sync("sync").unwrap(), SyncOutcome::Conflict);
    assert!(!a_dir.join(".git").join("rebase-merge").exists(), "rebase откачен");
    a.reset_to_remote().unwrap();
    assert_eq!(fs::read_to_string(a_dir.join("root")).unwrap(), "правка B2\n");
    assert!(!a.is_dirty().unwrap());
    assert!(a.changes_since(&history[0].hash).unwrap().iter().any(|c| c.path == "root"));
}
