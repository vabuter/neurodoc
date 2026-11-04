//! Каталог документации в формате neurodoc: корневой файл root + файлы-UUID
//! в папке _content. Держит содержимое всех файлов в памяти (id → контент).
//!
//! Порт `windows/NeurodocEditor/Services/NeurodocCatalog.cs` файл в файл по смыслу —
//! как и `data/NeurodocCatalog.kt` на Android. Расхождение в любом из них — ошибка,
//! а не «особенность платформы». Позиции в тексте здесь байтовые (UTF-8), а не
//! UTF-16, как в C#: так их понимает GtkTextBuffer.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;

use super::text;

pub const ROOT_ID: &str = "root";

/// Папка с neurodoc-файлами. Подчёркивание в начале помечает папку служебной, но,
/// в отличие от точки, не делает её скрытой: каталог смотрят и обычным файловым
/// менеджером, и с телефона. Обычное «content» остаётся свободным под прикладную
/// папку самого проекта.
pub const CONTENT_DIR: &str = "_content";

/// Больше этого файл в память не берём — каталог не хранилище двоичных данных.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Ссылка формата neurodoc: [текст](цель). Группа 1 — текст ссылки (якорь, может быть
/// пустым), группа 2 — цель: UUID файла из _content/ либо путь к файлу каталога
/// ОТНОСИТЕЛЬНО того файла, где написана ссылка. Якорь однострочный и не содержит "]".
/// Цель разбирается в id через [`try_resolve_link`] — сама по себе она неполна.
pub static LINK_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]\n]*)\]\(([^()\s\\]+)\)").unwrap());

/// Ссылка старого формата <uuid>. Ссылкой больше не считается и нужна только
/// миграции каталога на новый формат (см. migration.rs).
pub static LEGACY_LINK_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})>")
        .unwrap()
});

static ID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .unwrap()
});

/// UUID в произвольном месте строки (для разбора пользовательского ввода).
static ANY_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
        .unwrap()
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub file_id: String,
    pub title: String,
    pub prefix: String,
    pub match_text: String,
    pub suffix: String,
    /// Порядковый номер вхождения в файле — по нему результат выделяют в редакторе.
    pub occurrence: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    pub file_id: String,
    pub title: String,
}

/// Ссылка на несуществующий файл: где найдена и куда ведёт.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenLink {
    pub file_id: String,
    pub file_title: String,
    pub target_id: String,
    /// Байтовое смещение ссылки в тексте файла.
    pub position: usize,
    pub context: String,
}

/// Ссылка в тексте файла с уже разобранной целью.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRef {
    pub start: usize,
    pub len: usize,
    /// Цель как написана в тексте — её показывают в «Битых ссылках».
    pub target: String,
    pub anchor: String,
    /// Файл каталога, на который она ведёт, или None: веб-адрес либо путь наружу каталога.
    pub id: Option<String>,
}

impl LinkRef {
    pub fn end(&self) -> usize {
        self.start + self.len
    }
}

/// Ключ словаря: регистр не различаем (OrdinalIgnoreCase в C#, CASE_INSENSITIVE_ORDER
/// на Android). Иначе «ReadMe.md» не находит «README.md», и тот же файл под другим
/// написанием выглядит как чужой — без заголовка и обратных ссылок.
fn key(id: &str) -> String {
    id.to_lowercase()
}

fn eq_ci(a: &str, b: &str) -> bool {
    a.len() == b.len() && key(a) == key(b) || a.to_lowercase() == b.to_lowercase()
}

struct Entry {
    /// Идентификатор как на диске: у пути регистр — часть имени.
    id: String,
    text: String,
}

pub struct NeurodocCatalog {
    root_path: PathBuf,
    files: HashMap<String, Entry>,
    /// Разобранные ссылки файлов. Панели (пути, обратные ссылки, сироты, битые, порядок
    /// чтения) перестраиваются после каждого автосохранения, и без кэша каждая заново
    /// прогоняла регулярку по всему каталогу — пять полных проходов на одну правку.
    ///
    /// Разбор ссылок файла зависит только от его текста и его же папки, поэтому запись
    /// сбрасывает лишь запись изменённого файла, а не весь кэш. Любая новая мутация
    /// `files` обязана сбросить и соответствующую запись здесь.
    links: RefCell<HashMap<String, Rc<Vec<LinkRef>>>>,
}

// ================= Статические помощники формата =================

/// UUID ли это — то есть файл из папки _content.
pub fn is_uuid(id: &str) -> bool {
    ID_REGEX.is_match(id)
}

/// Файл формата neurodoc (root или файл из _content) — в отличие от файлов в собственных
/// форматах, которые лежат в своих папках и читаются в том числе программами.
pub fn is_neurodoc_file(id: &str) -> bool {
    id.eq_ignore_ascii_case(ROOT_ID) || is_uuid(id)
}

/// Папка файла относительно корня каталога: "" для root, "_content" для UUID.
pub fn dir_of(id: &str) -> String {
    if id.eq_ignore_ascii_case(ROOT_ID) {
        return String::new();
    }
    if is_uuid(id) {
        return CONTENT_DIR.to_string();
    }
    match id.rfind('/') {
        Some(slash) => id[..slash].to_string(),
        None => String::new(),
    }
}

/// Ссылка на внешний ресурс, а не на файл каталога.
pub fn is_web_target(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Разбирает цель ссылки в id файла. Путь считается ОТ ФАЙЛА, в котором написана ссылка,
/// поэтому нужен `source_id`.
///
/// None, если цель — веб-адрес, абсолютный путь или ведёт за пределы каталога:
/// по формату ссылок наружу не бывает, и разыменовывать их нельзя.
pub fn try_resolve_link(source_id: &str, target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() || is_web_target(target) {
        return None;
    }
    if is_uuid(target) {
        return Some(target.to_lowercase());
    }
    // абсолютные пути и диски каталогу не принадлежат
    if target.contains(':') || target.starts_with('/') || target.starts_with('\\') {
        return None;
    }

    let source_dir = dir_of(source_id);
    let mut parts: Vec<&str> = source_dir.split('/').filter(|s| !s.is_empty()).collect();
    let normalized = target.replace('\\', "/");
    for segment in normalized.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            // подъём выше корня каталога — ссылка наружу, такой в формате нет
            parts.pop()?;
            continue;
        }
        parts.push(segment);
    }
    if parts.is_empty() {
        return None;
    }

    // приводим к каноническому виду: _content/<uuid> — это тот же id, что и <uuid>
    if parts.len() == 2 && parts[0].eq_ignore_ascii_case(CONTENT_DIR) && is_uuid(parts[1]) {
        return Some(parts[1].to_lowercase());
    }
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case(ROOT_ID) {
        return Some(ROOT_ID.to_string());
    }
    Some(parts.join("/"))
}

/// Путь цели относительно папки, из которой на неё сошлются, — обратная операция
/// к разбору ссылки. Папку берут через [`dir_of`].
pub fn make_relative_target(source_dir: &str, target_id: &str) -> String {
    if is_uuid(target_id) {
        return target_id.to_string();
    }
    let from: Vec<&str> = source_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = target_id.split('/').filter(|s| !s.is_empty()).collect();
    let mut common = 0;
    while common < from.len() && common < to.len() && eq_ci(from[common], to[common]) {
        common += 1;
    }
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend_from_slice(&to[common..]);
    if parts.is_empty() { target_id.to_string() } else { parts.join("/") }
}

/// Годен ли id как имя файла каталога: за его пределы формат ссылок не выпускает.
pub fn is_valid_id(id: &str) -> bool {
    !id.trim().is_empty()
        && !id.contains("..")
        && !id.contains(':')
        && !id.starts_with('/')
        && !id.starts_with('\\')
}

/// Разбирает пользовательский ввод в id файла: "root", голый UUID, ссылка "<uuid>"
/// или произвольный текст, внутри которого есть UUID.
pub fn try_parse_id(input: &str) -> Option<String> {
    if input.trim().is_empty() {
        return None;
    }
    let trimmed = input.trim().trim_matches(|c| matches!(c, '<' | '>' | '(' | ')')).trim();
    if trimmed.eq_ignore_ascii_case(ROOT_ID) {
        return Some(ROOT_ID.to_string());
    }
    ANY_ID_REGEX.find(input).map(|m| m.as_str().to_lowercase())
}

pub fn shorten(id: &str) -> String {
    if id.chars().count() > 8 { format!("{}…", id.chars().take(8).collect::<String>()) } else { id.to_string() }
}

/// Текст без BOM: по формату его не бывает, но C# ReadAllText его отбрасывает молча,
/// и файл с BOM не должен вести себя на Linux иначе, чем на Windows.
fn strip_bom(text: String) -> String {
    match text.strip_prefix('\u{FEFF}') {
        Some(rest) => rest.to_string(),
        None => text,
    }
}

/// Идентификатор, под которым перечитывание завело бы файл с этим путём (относительно
/// корня каталога, через «/»), или None, если такого файла в каталоге быть не может.
fn id_for_relative_path(rel: &str) -> Option<String> {
    if rel == ROOT_ID {
        return Some(ROOT_ID.to_string());
    }
    if let Some(name) = rel.strip_prefix(CONTENT_DIR).and_then(|r| r.strip_prefix('/')) {
        return is_uuid(name).then(|| name.to_lowercase());
    }
    is_valid_id(rel).then(|| rel.to_string())
}

/// Сколько байт начала файла смотрит [`starts_binary`].
const BINARY_PROBE_BYTES: u64 = 8 * 1024;

/// Двоичный ли файл — по байту 0 в начале. Перечитывание отсеивает файл с нулём где
/// угодно, но читать его целиком на каждое событие нельзя: генераторы картинок пишут
/// кусками, событий сотни, и каждое читало бы до 4 МБ в главном потоке (замер: 9 с на
/// PNG в 3 МБ). Нуль в начале — точно двоичный: from_utf8_lossy ноль не глотает, и
/// перечитывание такой файл тоже отбросит. Без нуля ответ — «изменился»: дальше
/// разберётся одно перечитывание.
fn starts_binary(path: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::with_capacity(BINARY_PROBE_BYTES as usize);
    fs::File::open(path).and_then(|f| f.take(BINARY_PROBE_BYTES).read_to_end(&mut head)).is_ok() && head.contains(&0)
}

fn read_text(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(strip_bom(String::from_utf8_lossy(&bytes).into_owned()))
}

// ================= Каталог =================

impl NeurodocCatalog {
    pub fn open_or_create(path: &Path) -> io::Result<Self> {
        Self::ensure_structure(path)?;
        let mut catalog = Self {
            root_path: path.to_path_buf(),
            files: HashMap::new(),
            links: RefCell::new(HashMap::new()),
        };
        catalog.reload();
        Ok(catalog)
    }

    /// Создаёт каталог проекта, файл root, папку _content и ReadMe.md, если их ещё нет.
    pub fn ensure_structure(path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)?;
        fs::create_dir_all(path.join(CONTENT_DIR))?;
        let root_file = path.join(ROOT_ID);
        if !root_file.exists() {
            fs::write(&root_file, "")?;
        }
        let readme_exists = fs::read_dir(path)?
            .flatten()
            .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("readme.md"));
        if !readme_exists {
            fs::write(path.join("ReadMe.md"), README_TEMPLATE)?;
        }
        Ok(())
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn content_path(&self) -> PathBuf {
        self.root_path.join(CONTENT_DIR)
    }

    pub fn reload(&mut self) {
        self.files.clear();
        self.links.borrow_mut().clear();
        let root_text = self.read_from_disk(ROOT_ID);
        self.files.insert(key(ROOT_ID), Entry { id: ROOT_ID.to_string(), text: root_text });

        if let Ok(entries) = fs::read_dir(self.content_path()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !ID_REGEX.is_match(&name) {
                    continue;
                }
                // нечитабельный файл пропускаем
                if let Ok(text) = read_text(&entry.path()) {
                    let id = name.to_lowercase();
                    self.files.insert(id.clone(), Entry { id, text });
                }
            }
        }

        // Кроме _content в каталоге бывают папки с файлами в собственных форматах
        // (YAML, SVG, конфиги) — на них тоже ссылаются, по пути от файла-источника.
        for file in enumerate_domain_files(&self.root_path) {
            let Ok(rel) = file.strip_prefix(&self.root_path) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if rel.eq_ignore_ascii_case(ROOT_ID) {
                continue;
            }
            let Ok(meta) = fs::metadata(&file) else { continue };
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }
            let Ok(text) = read_text(&file) else { continue };
            if text.contains('\0') {
                continue; // двоичный файл читать нечем
            }
            self.files.insert(key(&rel), Entry { id: rel, text });
        }
    }

    /// Все идентификаторы: root первым, остальные в порядке байтов.
    pub fn all_ids(&self) -> Vec<String> {
        let mut ids: Vec<&str> = self.files.values().map(|e| e.id.as_str()).collect();
        ids.sort_by(|a, b| {
            let ka = if *a == ROOT_ID { "" } else { a };
            let kb = if *b == ROOT_ID { "" } else { b };
            ka.cmp(kb)
        });
        ids.into_iter().map(str::to_string).collect()
    }

    /// Написание id, под которым файл известен каталогу (как на диске), или как передан.
    pub fn canonical_id(&self, id: &str) -> String {
        self.files.get(&key(id)).map(|e| e.id.clone()).unwrap_or_else(|| id.to_string())
    }

    /// Путь файла на диске.
    ///
    /// Ошибка — id ведёт за пределы каталога. Такие сюда попадать не должны: их отсеивает
    /// [`try_resolve_link`]. Раньше здесь была молчаливая подмена на root — от неё
    /// [`Self::exists`] отвечал «файл есть», а запись пошла бы в root.
    pub fn file_path(&self, id: &str) -> Result<PathBuf, String> {
        if id.eq_ignore_ascii_case(ROOT_ID) {
            return Ok(self.root_path.join(ROOT_ID));
        }
        if is_uuid(id) {
            return Ok(self.content_path().join(id.to_lowercase()));
        }
        if !is_valid_id(id) {
            return Err(format!("Путь ведёт за пределы каталога: {id}"));
        }
        Ok(self.root_path.join(id))
    }

    /// Текст файла: из памяти, а для незнакомого id — с диска (или пустой).
    pub fn read_file(&self, id: &str) -> Cow<'_, str> {
        match self.files.get(&key(id)) {
            Some(e) => Cow::Borrowed(e.text.as_str()),
            None => Cow::Owned(self.read_from_disk(id)),
        }
    }

    fn read_from_disk(&self, id: &str) -> String {
        match self.file_path(id) {
            Ok(path) if path.is_file() => read_text(&path).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Не изменит ли перечитывание ничего для этого пути: файл на диске совпадает с тем,
    /// что каталог держит в памяти, либо его нет ни там, ни там. Нужно слежению за
    /// диском — собственная запись поднимает то же событие, что и чужая.
    ///
    /// Сверяется содержимое, а не время записи. Раньше событие считалось своим, если
    /// приложение писало что угодно меньше секунды назад, — и чужая правка, пришедшая
    /// сразу после автосохранения (git pull в терминале, другая программа), терялась:
    /// перечитывание не назначалось вовсе. Сверка по содержимому узнаёт свою запись и
    /// при запоздавшем событии, а чужую не пропустит даже в тот же файл.
    ///
    /// Сомнение решается в пользу «изменился»: лишнее перечитывание безвредно, а
    /// пропущенное оставляет в памяти устаревший текст, который автосохранение потом
    /// запишет поверх чужой правки. Поэтому папка, незнакомый файл и путь вне каталога —
    /// всегда «изменился».
    pub fn matches_disk(&self, path: &Path) -> bool {
        let Ok(rel) = path.strip_prefix(&self.root_path) else { return false };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.is_empty() {
            return false; // событие о самой папке каталога
        }
        let id = id_for_relative_path(&rel);
        let meta = fs::symlink_metadata(path).ok();
        // Файл, который каталог держит в памяти, сверяется всегда — даже с точкой в пути
        // или больше 4 МБ: такие перечитывание не заводит, но в память их кладёт запись
        // (правка файла, открытого по ссылке вроде .gitlab-ci.yml).
        if let Some(id) = &id
            && let Some(e) = self.files.get(&key(id))
        {
            // тот же, если на диске под тем же написанием (ФС различает регистр) и текст,
            // прочитанный так же, как при перечитывании, совпадает; пропал — изменился
            return meta.is_some_and(|m| m.is_file()) && e.id == *id && read_text(path).is_ok_and(|t| t == e.text);
        }
        // Дальше — незнакомые. Пропавший путь что-то меняет, только если это папка с
        // известными каталогу файлами (её унесли целиком).
        let Some(meta) = meta else {
            // по самому пути: перечитывание заводит и файлы, чей путь разбор ссылки не
            // примет (12:30.md), — их ключ не восстановить через id_for_relative_path
            let prefix = format!("{}/", key(&rel));
            return !self.files.contains_key(&key(&rel))
                && !rel.eq_ignore_ascii_case(CONTENT_DIR)
                && !self.files.keys().any(|k| k.starts_with(&prefix));
        };
        // в папки с точки перечитывание не заходит и такие файлы не читает (.git, свапы
        // редакторов): для каталога их изменения ничего не значат
        if rel.split('/').any(|part| part.starts_with('.')) {
            return true;
        }
        let Some(id) = id else { return false };
        // незнакомый файл перечитывание не заведёт, только если отбросит: слишком большой
        // или двоичный (root и файлы _content так не отсеиваются); папка — всегда изменение
        meta.is_file() && !is_neurodoc_file(&id) && (meta.len() > MAX_FILE_BYTES || starts_binary(path))
    }

    pub fn write_file(&mut self, id: &str, text: &str) -> io::Result<()> {
        let k = key(id);
        // Пишем по тому написанию, под которым файл уже известен: файловая система
        // Linux различает регистр, и ссылка «readme.md» на ReadMe.md иначе создала бы
        // второй файл, а после перечитывания выигрывал бы случайный из двух.
        let spelled = match self.files.get(&k) {
            Some(e) => e.id.clone(),
            None if is_uuid(id) => id.to_lowercase(),
            None => id.to_string(),
        };
        let path = self.file_path(&spelled).map_err(|m| io::Error::new(io::ErrorKind::InvalidInput, m))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, text)?;
        self.files.insert(k.clone(), Entry { id: spelled, text: text.to_string() });
        self.links.borrow_mut().remove(&k);
        Ok(())
    }

    /// Разбирает то, что ввёл пользователь, в id файла ЭТОГО каталога: «root», UUID
    /// (в том числе внутри произвольного текста) или путь вида contract/auth/start.yaml.
    ///
    /// [`try_parse_id`] про пути ничего не знает — он ищет UUID в отрыве от каталога.
    /// А в списке файлов проекта такие пути есть, и без этой проверки выбранный там
    /// файл не открывался: диалог отвечал «это не похоже на UUID файла».
    pub fn try_resolve_input(&self, input: &str) -> Option<String> {
        if input.trim().is_empty() {
            return None;
        }
        if let Some(parsed) = try_parse_id(input)
            && self.exists(&parsed) {
                return Some(parsed);
            }
        let path = input.trim().replace('\\', "/");
        if !is_valid_id(&path) || !self.exists(&path) {
            return None;
        }
        Some(self.canonical_id(&path))
    }

    /// Есть ли такой файл в каталоге. Негодный id — всегда «нет».
    pub fn exists(&self, id: &str) -> bool {
        is_valid_id(id)
            && (self.files.contains_key(&key(id))
                || self.file_path(id).map(|p| p.is_file()).unwrap_or(false))
    }

    pub fn create_new_file(&mut self) -> io::Result<String> {
        let id = uuid::Uuid::new_v4().to_string().to_lowercase();
        self.write_file(&id, "")?;
        Ok(id)
    }

    pub fn delete_file(&mut self, id: &str) -> io::Result<()> {
        if id == ROOT_ID {
            return Ok(());
        }
        let k = key(id);
        // удаляем по тому написанию, под которым файл известен (ФС различает регистр)
        let spelled = self.files.get(&k).map(|e| e.id.clone()).unwrap_or_else(|| id.to_string());
        let path = self.file_path(&spelled).map_err(|m| io::Error::new(io::ErrorKind::InvalidInput, m))?;
        if path.is_file() {
            fs::remove_file(&path)?;
        }
        self.files.remove(&k);
        self.links.borrow_mut().remove(&k);
        Ok(())
    }

    /// Снимает во всех файлах каталога ссылки на указанный: [текст](цель) превращается
    /// в текст. Возвращает число изменённых файлов.
    ///
    /// Текст ссылки остаётся намеренно — его писал автор, и он часть предложения.
    /// Удаление вместе со ссылкой рвало бы фразу в чужом файле. Ссылка без текста
    /// (пустой якорь) исчезает целиком: сохранять там нечего.
    pub fn remove_links_to(&mut self, id: &str) -> io::Result<usize> {
        let mut changed = 0;
        let sources: Vec<String> = self.link_source_ids().map(str::to_string).collect();
        for file_id in sources {
            if eq_ci(&file_id, id) {
                continue;
            }
            let text = self.read_file(&file_id).into_owned();
            // один и тот же файл адресуется разными путями, поэтому сверяем разрешённую цель
            let updated = LINK_REGEX.replace_all(&text, |m: &regex::Captures| {
                match try_resolve_link(&file_id, &m[2]) {
                    Some(target) if eq_ci(&target, id) => m[1].to_string(),
                    _ => m[0].to_string(),
                }
            });
            if updated != text {
                let updated = updated.into_owned();
                self.write_file(&file_id, &updated)?;
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// Заголовок файла — его первая непустая строка. У файла в собственном формате
    /// первая строка служебная (комментарий, ключ), поэтому там заголовок — имя файла.
    pub fn title(&self, id: &str, max_len: usize) -> String {
        if !is_neurodoc_file(id) {
            return id.rsplit('/').next().unwrap_or(id).to_string();
        }
        // построчно, без разрезания всего файла: заголовок нужен каждой строке панелей
        let text = self.read_file(id);
        for raw in text.split('\n') {
            let line = raw.trim().trim_start_matches('#').trim();
            if line.is_empty() {
                continue;
            }
            return text::truncate_chars(line, max_len);
        }
        if id == ROOT_ID { "root".to_string() } else { shorten(id) }
    }

    /// Файлы, в которых имеет смысл искать ссылки: root и _content.
    ///
    /// Файлы в собственных форматах (YAML, SVG, конфиги) — цели ссылок, а не источники.
    /// Разбирать их как neurodoc опасно: последовательность вида [текст](что-то) там
    /// встречается сама по себе. На этом попался ReadMe.md — примеры из описания формата
    /// приложение считало настоящими ссылками и жаловалось на битые.
    fn link_source_ids(&self) -> impl Iterator<Item = &str> {
        self.files.values().filter(|e| is_neurodoc_file(&e.id)).map(|e| e.id.as_str())
    }

    /// Ссылки файла с уже разрешёнными целями: (start, len, id, anchor). Цель зависит от
    /// того, где написана ссылка, поэтому разбирать её в отрыве от файла-источника нельзя.
    pub fn links(&self, source_id: &str) -> Vec<(usize, usize, String, String)> {
        self.parsed_links(source_id)
            .iter()
            .filter_map(|l| l.id.as_ref().map(|id| (l.start, l.len, id.clone(), l.anchor.clone())))
            .collect()
    }

    /// Все ссылки файла, включая неразрешимые. Список кэшируется и общий на всех
    /// вызывающих — менять его нельзя.
    pub fn parsed_links(&self, source_id: &str) -> Rc<Vec<LinkRef>> {
        let k = key(source_id);
        if let Some(cached) = self.links.borrow().get(&k) {
            return cached.clone();
        }
        let text = self.read_file(source_id);
        let mut result = Vec::new();
        for m in LINK_REGEX.captures_iter(&text) {
            let whole = m.get(0).unwrap();
            // регистр цели не трогаем: у пути он часть имени, а UUID приводит try_resolve_link
            let target = m.get(2).map(|g| g.as_str()).unwrap_or("");
            result.push(LinkRef {
                start: whole.start(),
                len: whole.len(),
                target: target.to_string(),
                anchor: m.get(1).map(|g| g.as_str()).unwrap_or("").to_string(),
                id: try_resolve_link(source_id, target),
            });
        }
        let rc = Rc::new(result);
        self.links.borrow_mut().insert(k, rc.clone());
        rc
    }

    /// Граф ссылок: ключ файла (в нижнем регистре) → id файлов, на которые он ссылается.
    pub fn build_graph(&self) -> HashMap<String, Vec<String>> {
        let mut graph = HashMap::new();
        for id in self.link_source_ids() {
            let mut seen = HashSet::new();
            let mut next = Vec::new();
            for link in self.parsed_links(id).iter() {
                if let Some(target) = &link.id
                    && seen.insert(key(target)) {
                        next.push(self.canonical_id(target));
                    }
            }
            graph.insert(key(id), next);
        }
        graph
    }

    /// Файлы, ссылающиеся на указанный.
    pub fn backlinks(&self, id: &str) -> Vec<Backlink> {
        // Подстрокой больше не найти: один и тот же файл из разных папок адресуется
        // разными путями, поэтому ссылки каждого файла разрешаем по-настоящему.
        let mut result: Vec<Backlink> = self
            .link_source_ids()
            .filter(|source| !eq_ci(source, id))
            .filter(|source| {
                self.parsed_links(source).iter().any(|l| l.id.as_deref().is_some_and(|t| eq_ci(t, id)))
            })
            .map(|source| Backlink { file_id: source.to_string(), title: self.title(source, 60) })
            .collect();
        result.sort_by_key(|b| b.title.to_lowercase());
        result
    }

    /// Ссылки на несуществующие файлы. Дубли допустимы: одна и та же цель
    /// в разных местах — это разные записи, к каждой можно перейти.
    pub fn broken_links(&self) -> Vec<BrokenLink> {
        let mut result = Vec::new();
        for id in self.all_ids().into_iter().filter(|id| is_neurodoc_file(id)) {
            let mut title: Option<String> = None;
            let links = self.parsed_links(&id);
            let text = self.read_file(&id);
            for link in links.iter() {
                if is_web_target(&link.target) {
                    continue; // веб-адрес файлом каталога не бывает
                }
                // не разобралась — значит ведёт наружу каталога, это тоже битая ссылка
                let target = link.id.clone().unwrap_or_else(|| link.target.clone());
                if self.exists(&target) {
                    continue;
                }
                let file_title = title.get_or_insert_with(|| self.title(&id, 40)).clone();
                let line_start = text::line_start_at(&text, link.start);
                let line_end = text::line_end_at(&text, link.start);
                let context = text::truncate_chars(text[line_start..line_end].trim(), 70);
                result.push(BrokenLink {
                    file_id: id.clone(),
                    file_title,
                    target_id: target,
                    position: link.start,
                    context,
                });
            }
        }
        result
    }

    /// Файлы (кроме root), на которые нет ни одной ссылки из других файлов.
    pub fn orphan_files(&self) -> Vec<Backlink> {
        let mut referenced: HashSet<String> = HashSet::new();
        for id in self.link_source_ids() {
            for link in self.parsed_links(id).iter() {
                if let Some(target) = &link.id
                    && !eq_ci(target, id) {
                        referenced.insert(key(target));
                    }
            }
        }
        // только neurodoc-файлы: файлы в собственных форматах живут своей жизнью,
        // и «сирота» среди них — не повод для тревоги
        let mut result: Vec<Backlink> = self
            .files
            .values()
            .filter(|e| e.id != ROOT_ID && is_neurodoc_file(&e.id) && !referenced.contains(&key(&e.id)))
            .map(|e| Backlink { file_id: e.id.clone(), title: self.title(&e.id, 60) })
            .collect();
        result.sort_by_key(|b| b.title.to_lowercase());
        result
    }

    /// Кратчайшие пути от root до указанного файла — их показывают хлебные крошки.
    ///
    /// Сначала обход в ширину считает расстояния, и только потом перечисляются пути, где
    /// каждый шаг приближает к цели ровно на единицу. Прежний обход в глубину с отсечкой по
    /// числу найденных путей до короткого мог вообще не дойти: лимит выбирали длинные пути,
    /// найденные раньше (проверено — вместо пути длиной 3 показывался путь длиной 5).
    pub fn shortest_paths_from_root(&self, target: &str, max_paths: usize) -> Vec<Vec<String>> {
        let mut result: Vec<Vec<String>> = Vec::new();
        if target.eq_ignore_ascii_case(ROOT_ID) {
            return result;
        }
        let target_key = key(target);
        let graph = self.build_graph();
        let mut distance: HashMap<String, usize> = HashMap::new();
        distance.insert(key(ROOT_ID), 0);
        let mut queue = VecDeque::new();
        queue.push_back(key(ROOT_ID));
        while let Some(current) = queue.pop_front() {
            let Some(next) = graph.get(&current) else { continue };
            let d = distance[&current];
            for n in next {
                let nk = key(n);
                if distance.contains_key(&nk) {
                    continue;
                }
                distance.insert(nk.clone(), d + 1);
                // дальше идут только файлы, которые сами бывают источниками ссылок;
                // контракт .yaml может быть целью, но своих исходящих у него нет
                if graph.contains_key(&nk) {
                    queue.push_back(nk);
                }
            }
        }
        let Some(&target_distance) = distance.get(&target_key) else { return result };

        fn walk(
            node: &str,
            path: &mut Vec<String>,
            result: &mut Vec<Vec<String>>,
            graph: &HashMap<String, Vec<String>>,
            distance: &HashMap<String, usize>,
            target_key: &str,
            target_distance: usize,
            max_paths: usize,
        ) {
            if result.len() >= max_paths {
                return;
            }
            let node_key = key(node);
            if node_key == target_key {
                result.push(path.clone());
                return;
            }
            let d = distance[&node_key];
            if d >= target_distance {
                return;
            }
            let Some(next) = graph.get(&node_key) else { return };
            for n in next {
                match distance.get(&key(n)) {
                    Some(&nd) if nd == d + 1 => {}
                    _ => continue,
                }
                path.push(n.clone());
                walk(n, path, result, graph, distance, target_key, target_distance, max_paths);
                path.pop();
                if result.len() >= max_paths {
                    return;
                }
            }
        }

        let mut path = vec![ROOT_ID.to_string()];
        walk(ROOT_ID, &mut path, &mut result, &graph, &distance, &target_key, target_distance, max_paths);
        result.sort_by_key(|a| a.join("/"));
        result
    }

    /// Порядок чтения всего каталога: обход в глубину от root по порядку ссылок в тексте
    /// (порядок ссылок — это замысел автора). Каждый файл встречается ровно один раз,
    /// циклы отсекаются множеством пройденных.
    ///
    /// Хвостом идут файлы, до которых по ссылкам от root не добраться: без них «прочитать
    /// всё» было бы недостижимо. Порядок не кэшируется — документация меняется между
    /// сеансами, и новые файлы должны просто появляться в списке непрочитанными.
    pub fn build_reading_order(&self) -> Vec<String> {
        let mut order = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        // стек, а не рекурсия: глубина обхода равна длине цепочки ссылок
        let mut stack = vec![ROOT_ID.to_string()];
        while let Some(id) = stack.pop() {
            if !seen.insert(key(&id)) {
                continue;
            }
            order.push(id.clone());

            // файлы в собственных форматах (контракты, конфиги) — справочные данные,
            // их читают по необходимости, а не подряд, поэтому в обход они не входят
            let mut children: Vec<String> = Vec::new();
            for link in self.parsed_links(&id).iter() {
                let Some(target) = &link.id else { continue };
                if !is_neurodoc_file(target) || !self.exists(target) {
                    continue;
                }
                if seen.contains(&key(target)) {
                    continue;
                }
                if !children.iter().any(|c| eq_ci(c, target)) {
                    children.push(target.clone());
                }
            }
            // в стек — в обратном порядке, чтобы первым читался первый по тексту
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        }

        for id in self.all_ids() {
            if is_neurodoc_file(&id) && seen.insert(key(&id)) {
                order.push(id);
            }
        }
        order
    }

    /// Поиск подстроки по всем файлам проекта (без учёта регистра).
    pub fn search(&self, query: &str, max_results: usize) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let Some(re) = text::ci_regex(query) else { return results };
        for id in self.all_ids() {
            let content = self.read_file(&id);
            for (occurrence, m) in re.find_iter(&content).enumerate() {
                if results.len() >= max_results {
                    break;
                }
                let (index, match_end) = (m.start(), m.end());
                let line_start = text::line_start_at(&content, index);
                let line_end = text::line_end_at(&content, match_end);

                let prefix_from = text::back_chars(&content, index, 60, line_start);
                let prefix = format!(
                    "{}{}",
                    if prefix_from > line_start { "…" } else { "" },
                    &content[prefix_from..index]
                );
                let suffix_to = text::fwd_chars(&content, match_end, 90, line_end);
                let suffix = format!(
                    "{}{}",
                    &content[match_end..suffix_to],
                    if suffix_to < line_end { "…" } else { "" }
                );
                results.push(SearchResult {
                    file_id: id.clone(),
                    title: self.title(&id, 40),
                    prefix,
                    match_text: content[index..match_end].to_string(),
                    suffix,
                    occurrence,
                });
            }
            if results.len() >= max_results {
                break;
            }
        }
        results
    }
}

/// Файлы каталога в собственных форматах: всё дерево, кроме служебной папки
/// [`CONTENT_DIR`], папок с точки (.git и подобные) и файлов, имя которых
/// начинается с точки.
///
/// Обходим папки сами, в ширину, не заходя в .git: на настоящем репозитории это
/// десятки тысяч лишних имён при каждом перечитывании. Симлинк-петли отсекаются
/// множеством уже пройденных папок — на Linux они встречаются.
fn enumerate_domain_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut pending = VecDeque::new();
    pending.push_back(root.to_path_buf());
    while let Some(dir) = pending.pop_front() {
        if let Ok(canonical) = fs::canonicalize(&dir)
            && !visited.insert(canonical) {
                continue;
            }
        let Ok(entries) = fs::read_dir(&dir) else { continue }; // нечитаемую папку пропускаем целиком
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                // _content отбрасываем ПО ИМЕНИ: точка его не прячет, и без этой
                // проверки файлы-UUID попали бы в каталог второй раз — как файлы
                // в собственном формате с путём «_content/<uuid>»
                if name.eq_ignore_ascii_case(CONTENT_DIR) {
                    continue;
                }
                subdirs.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
        subdirs.sort();
        pending.extend(subdirs);
    }
    files
}

pub const README_TEMPLATE: &str = "# Каталог документации в формате neurodoc

Формат называется neurodoc.
Формат neurodoc представляет собой набор текстовых файлов.
Внутри каждого файла находится блок знаний или информации.
Все файлы в кодировке UTF-8.
Начинать читать надо с корневого файла - он всегда называется root.
Остальные файлы находятся в папке _content.
Их имена являются уникальными идентификаторами в рамках данного каталога.
Идентификаторы - это UUID в нижнем регистре.
В тексте любого файла может встретиться ссылка вида [текст](uuid). Например, вот так: [подход к разработке](5c673c53-8ad9-49eb-9fb6-ffb3954848f0).
Это значит, что это ссылка на другой файл в папке _content с именем 5c673c53-8ad9-49eb-9fb6-ffb3954848f0, а \"подход к разработке\" - её текст (якорь), встроенный в предложение.
Текст ссылки может быть пустым: [](5c673c53-8ad9-49eb-9fb6-ffb3954848f0). Тогда вместо него показывают заголовок целевого файла - его первую непустую строку.
Текст ссылки не содержит символа \"]\" и переводов строк.
UUID сам по себе ссылкой не считается. UUID, обрамлённый символами \"<\" и \">\", - это ссылка старого формата, она больше ссылкой не считается.
Кроме папки _content в каталоге могут быть папки с файлами в собственных форматах (YAML, SVG, конфиги). Имена у них не UUID, а доменные: эти файлы читают не только люди, но и программы.
В ссылке вместо UUID может стоять путь к такому файлу - относительно того файла, в котором написана ссылка. Например, ссылка [auth.start](../contract/auth/start.yaml), написанная в файле из папки _content, ведёт на файл contract/auth/start.yaml этого же каталога.
Ссылка по пути ведёт только внутрь каталога neurodoc. За пределы каталога ссылок нет.
Таким образом, файлы образуют граф знаний, который имеет в качестве корня знания из файла root.
Для быстрой обработки каталога neurodoc рекомендуется построить в памяти hash-таблицу (mapping) \"имя файла\" -> \"контент\".
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_links_relative_to_source() {
        let uuid = "5c673c53-8ad9-49eb-9fb6-ffb3954848f0";
        assert_eq!(try_resolve_link(ROOT_ID, &uuid.to_uppercase()).as_deref(), Some(uuid));
        assert_eq!(try_resolve_link(uuid, "../contract/auth/start.yaml").as_deref(), Some("contract/auth/start.yaml"));
        assert_eq!(try_resolve_link(ROOT_ID, "contract/auth/start.yaml").as_deref(), Some("contract/auth/start.yaml"));
        assert_eq!(try_resolve_link("contract/auth/start.yaml", "../dto/Account.yaml").as_deref(), Some("contract/dto/Account.yaml"));
        assert_eq!(try_resolve_link(uuid, &format!("../_content/{uuid}")).as_deref(), Some(uuid));
        assert_eq!(try_resolve_link(uuid, "../root").as_deref(), Some(ROOT_ID));
        assert_eq!(try_resolve_link(ROOT_ID, "../outside.yaml"), None);
        assert_eq!(try_resolve_link(ROOT_ID, "/etc/passwd"), None);
        assert_eq!(try_resolve_link(ROOT_ID, "C:/x.yaml"), None);
        assert_eq!(try_resolve_link(ROOT_ID, "https://example.com"), None);
        assert_eq!(try_resolve_link(ROOT_ID, "   "), None);
    }

    #[test]
    fn relative_target_is_inverse_of_resolution() {
        assert_eq!(make_relative_target(CONTENT_DIR, "contract/auth/start.yaml"), "../contract/auth/start.yaml");
        assert_eq!(make_relative_target("", "contract/auth/start.yaml"), "contract/auth/start.yaml");
        assert_eq!(make_relative_target("contract/auth", "contract/dto/Account.yaml"), "../dto/Account.yaml");
        assert_eq!(make_relative_target("contract/auth", "contract/auth/start.yaml"), "start.yaml");
        let uuid = "5c673c53-8ad9-49eb-9fb6-ffb3954848f0";
        assert_eq!(make_relative_target(CONTENT_DIR, uuid), uuid);
    }

    #[test]
    fn parses_user_input() {
        let uuid = "5c673c53-8ad9-49eb-9fb6-ffb3954848f0";
        assert_eq!(try_parse_id(" ROOT ").as_deref(), Some(ROOT_ID));
        assert_eq!(try_parse_id(&format!("<{}>", uuid.to_uppercase())).as_deref(), Some(uuid));
        assert_eq!(try_parse_id(&format!("см. [текст]({uuid})")).as_deref(), Some(uuid));
        assert_eq!(try_parse_id("contract/auth/start.yaml"), None);
        assert!(is_valid_id("contract/auth/start.yaml"));
        assert!(!is_valid_id("../x"));
        assert!(!is_valid_id("/abs"));
        assert!(!is_valid_id("c:/x"));
    }
}
