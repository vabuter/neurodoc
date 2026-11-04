//! Все git-операции приложения. Работает через системный git: у пользователя уже
//! настроены ключи и учётные данные для github.com/gitlab.com, и приложение ими
//! просто пользуется. Порт `Services/GitService.cs` файл в файл по смыслу.
//!
//! Каждый commit сразу уходит push — локальные коммиты сами по себе приложению не нужны,
//! поэтому отдельной операции commit здесь нет. Конфликты не решаются автоматически:
//! методы возвращают исход, а выбор (затереть свои/серверные или отменить) делает
//! пользователь в UI.
//!
//! Функции синхронные и блокирующие — из UI их зовут через `gio::spawn_blocking`,
//! чтобы главный цикл не замирал на время сетевой операции.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use chrono::{DateTime, FixedOffset, Local};

/// Итог одного вызова git.
#[derive(Debug, Clone)]
pub struct GitResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl GitResult {
    pub fn ok(&self) -> bool {
        self.exit_code == 0
    }

    /// Текст для показа пользователю: stderr в приоритете, stdout — добавкой.
    pub fn error_text(&self) -> String {
        [self.stderr.trim(), self.stdout.trim()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Git завершился с ошибкой, которую нужно показать пользователю.
#[derive(Debug, Clone)]
pub enum GitError {
    Failed { operation: String, result: GitResult },
    Other(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed { operation, result } => write!(f, "{operation}\n{}", result.error_text()),
            Self::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GitError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub date: DateTime<FixedOffset>,
    pub author: String,
    pub message: String,
}

/// Изменение файла в коммите или диапазоне; status — буква git (A/M/D/R…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileChange {
    pub path: String,
    pub status: char,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    Ok,
    RejectedNonFastForward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullOutcome {
    Ok,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    Ok,
    Conflict,
    PushRejected,
}

const STASH_PREFIX: &str = "neurodoc-auto:";

#[derive(Debug, Clone)]
pub struct GitService {
    work_dir: PathBuf,
}

/// Запуск git: без кредов git должен упасть с ошибкой, а не молча ждать ввода
/// в невидимой консоли (GIT_TERMINAL_PROMPT=0); кириллические имена файлов иначе
/// приходят октальными последовательностями (core.quotepath=off). `output()` сам
/// дочитывает оба потока до конца — дедлока на полном пайпе здесь не бывает.
fn run_in(work_dir: &Path, args: &[&str]) -> Result<GitResult, GitError> {
    let output = Command::new("git")
        .args(["-c", "core.quotepath=off"])
        .args(args)
        .current_dir(work_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| GitError::Other(format!("Не удалось запустить git: {e}")))?;
    Ok(GitResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn run_checked_in(work_dir: &Path, args: &[&str]) -> Result<GitResult, GitError> {
    let result = run_in(work_dir, args)?;
    if !result.ok() {
        return Err(GitError::Failed { operation: format!("git {}", args.join(" ")), result });
    }
    Ok(result)
}

/// Есть ли git в PATH. Ответ кэшируется — за жизнь процесса git не появится.
pub fn is_git_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        run_in(&dir, &["--version"]).map(|r| r.ok()).unwrap_or(false)
    })
}

pub fn is_repository(path: &Path) -> bool {
    path.join(".git").is_dir()
}

/// Шаблонное сообщение коммита: дата-время и устройство.
pub fn build_commit_message() -> String {
    let host = fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "linux".to_string());
    format!("{} · {host}", Local::now().format("%Y-%m-%d %H:%M"))
}

/// Клонирует в папку. Непустую папку сначала очищает — подтверждение спрашивает UI.
pub fn clone(url: &str, target_dir: &Path) -> Result<(), GitError> {
    let non_empty = target_dir.is_dir()
        && fs::read_dir(target_dir).map(|mut d| d.next().is_some()).unwrap_or(false);
    if non_empty {
        clear_directory(target_dir).map_err(|e| GitError::Other(format!("Не удалось очистить папку: {e}")))?;
    }
    fs::create_dir_all(target_dir).map_err(|e| GitError::Other(format!("Не удалось создать папку: {e}")))?;
    let result = run_in(target_dir, &["clone", url, "."])?;
    if !result.ok() {
        return Err(GitError::Failed { operation: format!("git clone {url}"), result });
    }
    Ok(())
}

/// Очищает содержимое папки целиком.
fn clear_directory(dir: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

impl GitService {
    pub fn new(work_dir: &Path) -> Self {
        Self { work_dir: work_dir.to_path_buf() }
    }

    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    fn run(&self, args: &[&str]) -> Result<GitResult, GitError> {
        run_in(&self.work_dir, args)
    }

    fn run_checked(&self, args: &[&str]) -> Result<GitResult, GitError> {
        run_checked_in(&self.work_dir, args)
    }

    // ================= Состояние =================

    pub fn current_branch(&self) -> Result<Option<String>, GitError> {
        let result = self.run(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        let name = result.stdout.trim();
        Ok(if result.ok() && !name.is_empty() { Some(name.to_string()) } else { None })
    }

    pub fn local_branches(&self) -> Result<Vec<String>, GitError> {
        let result = self.run_checked(&["for-each-ref", "refs/heads", "--format=%(refname:short)"])?;
        Ok(result.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
    }

    /// Default-ветка проекта. origin/HEAD ставится клоном; после «Опубликовать» его нет,
    /// и тогда одна сетевая операция прописывает его навсегда. Без remote — main/master/первая.
    pub fn default_branch(&self) -> Result<String, GitError> {
        for attempt in 0..2 {
            let head = self.run(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])?;
            if head.ok() {
                let name = head.stdout.trim();
                if let Some(slash) = name.find('/')
                    && slash > 0 {
                        return Ok(name[slash + 1..].to_string());
                    }
            }
            if attempt == 0 {
                self.run(&["remote", "set-head", "origin", "--auto"])?;
            }
        }
        let branches = self.local_branches()?;
        Ok(branches
            .iter()
            .find(|b| *b == "main")
            .or_else(|| branches.iter().find(|b| *b == "master"))
            .or_else(|| branches.first())
            .cloned()
            .unwrap_or_else(|| "main".to_string()))
    }

    pub fn is_dirty(&self) -> Result<bool, GitError> {
        let result = self.run_checked(&["status", "--porcelain"])?;
        Ok(!result.stdout.trim().is_empty())
    }

    // ================= История 1: опубликовать проект =================

    /// Делает каталог репозиторием и отправляет на remote. Каталог не должен быть репозиторием.
    pub fn init_and_publish(&self, remote_url: &str) -> Result<(), GitError> {
        if is_repository(&self.work_dir) {
            return Err(GitError::Other("Каталог уже является git-репозиторием.".to_string()));
        }
        // git commit без identity падает с невнятным советом — проверяем заранее
        let name = self.run(&["config", "--get", "user.name"])?;
        let email = self.run(&["config", "--get", "user.email"])?;
        if name.stdout.trim().is_empty() || email.stdout.trim().is_empty() {
            return Err(GitError::Other(
                "В git не настроено имя автора. Выполните:\n\
                 git config --global user.name \"Имя\"\n\
                 git config --global user.email \"адрес@почты\""
                    .to_string(),
            ));
        }
        self.run_checked(&["init", "-b", "main"])?;
        // каталоги neurodoc — LF; autocrlf пользователя не должен их молча переписывать
        self.run_checked(&["config", "core.autocrlf", "false"])?;
        self.run_checked(&["add", "."])?;
        self.run_checked(&["commit", "-m", "Initial commit"])?;
        self.run_checked(&["remote", "add", "origin", remote_url])?;
        self.run_checked(&["push", "-u", "origin", "main"])?;
        Ok(())
    }

    // ================= История 3: переключение веток =================

    /// Переключается на ветку. Правки текущей ветки прячутся в stash с её меткой,
    /// при возврате на ветку её stash возвращается.
    pub fn switch_branch(&self, branch: &str) -> Result<(), GitError> {
        let current = self.current_branch()?;
        if current.as_deref() == Some(branch) {
            return Ok(());
        }
        if let Some(current) = current
            && self.is_dirty()? {
                let label = format!("{STASH_PREFIX}{current}");
                self.run_checked(&["stash", "push", "--include-untracked", "-m", &label])?;
            }
        self.run_checked(&["switch", branch])?;
        if let Some(stash_ref) = self.find_stash(branch)? {
            self.run_checked(&["stash", "pop", &stash_ref])?;
        }
        Ok(())
    }

    /// Stash-запись ветки, или None. Сверка всей метки: «dev» не должен находить «dev2».
    fn find_stash(&self, branch: &str) -> Result<Option<String>, GitError> {
        let result = self.run_checked(&["stash", "list", "--format=%gd%x1f%gs"])?;
        let wanted = format!("{STASH_PREFIX}{branch}");
        for line in result.stdout.lines().filter(|l| !l.is_empty()) {
            let mut parts = line.split('\u{1f}');
            let (Some(reference), Some(subject), None) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            // git сам приписывает к сообщению стэша «On <ветка>: » — отрезаем до нашей метки
            let subject = subject.trim();
            let subject = match subject.find(": ") {
                Some(colon) => &subject[colon + 2..],
                None => subject,
            };
            if subject == wanted {
                return Ok(Some(reference.trim().to_string()));
            }
        }
        Ok(None)
    }

    // ================= История 4: создание и удаление веток =================

    /// Создаёт ветку от текущей, переключается на неё и сразу отправляет на сервер.
    pub fn create_branch(&self, name: &str) -> Result<(), GitError> {
        self.run_checked(&["switch", "-c", name])?;
        self.run_checked(&["push", "-u", "origin", name])?;
        Ok(())
    }

    /// Удаляет ветку локально и на сервере. Текущую ветку UI сначала должен покинуть
    /// (switch_branch на default), затем звать этот метод.
    pub fn delete_branch(&self, name: &str) -> Result<(), GitError> {
        // работа удаляемой ветки уходит вместе с ней — мёртвый stash копить незачем
        if let Some(stash_ref) = self.find_stash(name)? {
            self.run_checked(&["stash", "drop", &stash_ref])?;
        }
        self.run_checked(&["branch", "-D", name])?;
        let remote = self.run(&["push", "origin", "--delete", name])?;
        // ветки может не быть на сервере (создана до «Опубликовать») — это не ошибка
        if !remote.ok() && !remote.stderr.contains("remote ref does not exist") {
            return Err(GitError::Failed { operation: format!("git push origin --delete {name}"), result: remote });
        }
        Ok(())
    }

    // ================= История 5: сохранить на сервере =================

    /// Commit всех правок и push. RejectedNonFastForward — на сервере чужие коммиты;
    /// выбор (затереть их или отступить) остаётся за пользователем.
    pub fn commit_and_push(&self, message: &str) -> Result<PushOutcome, GitError> {
        self.commit_all(message)?;
        self.push()
    }

    /// Затирает серверную ветку своей. Осознанный выбор пользователя в диалоге конфликта.
    pub fn force_push(&self) -> Result<(), GitError> {
        self.run_checked(&["push", "--force"])?;
        Ok(())
    }

    fn commit_all(&self, message: &str) -> Result<(), GitError> {
        self.run_checked(&["add", "-A"])?;
        let staged = self.run(&["diff", "--cached", "--quiet"])?;
        if staged.exit_code == 1 {
            self.run_checked(&["commit", "-m", message])?;
        }
        Ok(())
    }

    fn push(&self) -> Result<PushOutcome, GitError> {
        let push = self.run(&["push"])?;
        if push.ok() {
            return Ok(PushOutcome::Ok);
        }
        // у свежесозданной ветки нет upstream — прописываем и повторяем
        if push.stderr.to_lowercase().contains("no upstream") {
            let branch = self
                .current_branch()?
                .ok_or_else(|| GitError::Failed { operation: "git push".to_string(), result: push.clone() })?;
            self.run_checked(&["push", "-u", "origin", &branch])?;
            return Ok(PushOutcome::Ok);
        }
        if push.stderr.contains("non-fast-forward")
            || push.stderr.contains("[rejected]")
            || push.stderr.contains("fetch first")
        {
            return Ok(PushOutcome::RejectedNonFastForward);
        }
        Err(GitError::Failed { operation: "git push".to_string(), result: push })
    }

    // ================= История 6: получить с сервера =================

    /// Pull. Conflict — слияние не прошло; репозиторий уже возвращён в исходное состояние.
    pub fn pull(&self) -> Result<PullOutcome, GitError> {
        let pull = self.run(&["pull", "--no-rebase"])?;
        if pull.ok() {
            return Ok(PullOutcome::Ok);
        }
        // состояние определяем по файлам git, а не по тексту ошибки: он локализуем и меняется
        if self.work_dir.join(".git").join("MERGE_HEAD").exists() {
            self.run_checked(&["merge", "--abort"])?;
            return Ok(PullOutcome::Conflict);
        }
        // «local changes would be overwritten»: слияние не начиналось, откатывать нечего
        if pull.stderr.to_lowercase().contains("would be overwritten") {
            return Ok(PullOutcome::Conflict);
        }
        Err(GitError::Failed { operation: "git pull".to_string(), result: pull })
    }

    /// Затирает локальное состояние серверным. Осознанный выбор пользователя в диалоге конфликта.
    pub fn reset_to_remote(&self) -> Result<(), GitError> {
        let branch = self
            .current_branch()?
            .ok_or_else(|| GitError::Other("Не удалось определить текущую ветку.".to_string()))?;
        self.run_checked(&["fetch", "origin"])?;
        self.run_checked(&["reset", "--hard", &format!("origin/{branch}")])?;
        self.run_checked(&["clean", "-fd"])?;
        Ok(())
    }

    // ================= История 7: синхронизировать =================

    /// «Получить, затем сохранить» одной операцией: commit → pull --rebase → push.
    /// К pull дерево уже чистое (всё закоммичено), autostash — страховка.
    pub fn sync(&self, message: &str) -> Result<SyncOutcome, GitError> {
        self.commit_all(message)?;
        let pull = self.run(&["pull", "--rebase", "--autostash"])?;
        if !pull.ok() {
            let git_dir = self.work_dir.join(".git");
            let rebase_in_progress = git_dir.join("rebase-merge").is_dir() || git_dir.join("rebase-apply").is_dir();
            if rebase_in_progress {
                self.run_checked(&["rebase", "--abort"])?;
                return Ok(SyncOutcome::Conflict);
            }
            return Err(GitError::Failed { operation: "git pull --rebase".to_string(), result: pull });
        }
        Ok(match self.push()? {
            PushOutcome::Ok => SyncOutcome::Ok,
            PushOutcome::RejectedNonFastForward => SyncOutcome::PushRejected,
        })
    }

    // ================= История 8: история коммитов =================

    /// Коммиты текущей ветки, новые сверху.
    pub fn history(&self, limit: usize) -> Result<Vec<GitCommit>, GitError> {
        // разделители US/RS: разбор не зависит ни от локали, ни от текста сообщений
        let max = format!("--max-count={limit}");
        let result = self.run(&["log", &max, "--format=%H%x1f%h%x1f%aI%x1f%an%x1f%s%x1e"])?;
        if !result.ok() {
            return Ok(Vec::new()); // пустой репозиторий без коммитов
        }
        let mut commits = Vec::new();
        for record in result.stdout.split('\u{1e}') {
            let fields: Vec<&str> = record.trim_matches(|c| c == '\n' || c == '\r' || c == ' ').split('\u{1f}').collect();
            if fields.len() != 5 {
                continue;
            }
            let Ok(date) = DateTime::parse_from_rfc3339(fields[2]) else { continue };
            commits.push(GitCommit {
                hash: fields[0].to_string(),
                short_hash: fields[1].to_string(),
                date,
                author: fields[3].to_string(),
                message: fields[4].to_string(),
            });
        }
        Ok(commits)
    }

    // ================= История 9: изменённые файлы и дифф =================

    /// Файлы, изменённые одним коммитом.
    pub fn commit_files(&self, hash: &str) -> Result<Vec<GitFileChange>, GitError> {
        let result = self.run_checked(&["show", hash, "--name-status", "--format=", "-M"])?;
        Ok(parse_name_status(&result.stdout))
    }

    /// Файлы, изменённые от коммита до текущего состояния (включая незакоммиченное).
    pub fn changes_since(&self, hash: &str) -> Result<Vec<GitFileChange>, GitError> {
        let result = self.run_checked(&["diff", "--name-status", "-M", hash])?;
        Ok(parse_name_status(&result.stdout))
    }

    /// Unified diff файла: внутри одного коммита либо от коммита до текущего состояния.
    pub fn file_diff(&self, hash: &str, path: &str, since_mode: bool) -> Result<String, GitError> {
        let result = if since_mode {
            self.run_checked(&["diff", hash, "--", path])?
        } else {
            self.run_checked(&["show", "--format=", hash, "--", path])?
        };
        Ok(result.stdout)
    }

    /// Содержимое файла в ревизии — заголовок для уже удалённого файла берётся отсюда.
    pub fn read_blob(&self, rev: &str, path: &str) -> Result<Option<String>, GitError> {
        let result = self.run(&["show", &format!("{rev}:{path}")])?;
        Ok(if result.ok() { Some(result.stdout) } else { None })
    }
}

fn parse_name_status(output: &str) -> Vec<GitFileChange> {
    let mut changes = Vec::new();
    for line in output.lines().filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
        if parts.len() < 2 || parts[0].is_empty() {
            continue;
        }
        let status = parts[0].chars().next().unwrap_or('M');
        // у переименования два пути: старый и новый
        changes.push(if status == 'R' && parts.len() >= 3 {
            GitFileChange { path: parts[2].to_string(), status, old_path: Some(parts[1].to_string()) }
        } else {
            GitFileChange { path: parts[1].to_string(), status, old_path: None }
        });
    }
    changes
}

// ================= Правила UI, общие с Windows-версией =================

/// Имя ветки не должно содержать пробелов и символов ~ ^ : ? * [ \
pub fn branch_name_error(name: &str) -> Option<&'static str> {
    if name.chars().any(char::is_whitespace) || name.contains(['~', '^', ':', '?', '*', '[', '\\']) {
        Some("Имя ветки не должно содержать пробелов и символов ~ ^ : ? * [ \\")
    } else {
        None
    }
}

/// Служебные файлы репозитория в списке изменений не показываются.
pub fn is_service_file(path: &str) -> bool {
    path.rsplit('/').next().unwrap_or(path).starts_with('.')
}

/// Первая непустая строка содержимого — заголовок уже удалённого файла.
pub fn first_non_empty_line(text: &str) -> Option<String> {
    for raw in text.split('\n') {
        let line = raw.trim().trim_start_matches('#').trim();
        if !line.is_empty() {
            return Some(line.chars().take(60).collect());
        }
    }
    None
}

/// Название проекта из адреса репозитория: последний сегмент без «.git».
pub fn project_name_from_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let name = url.rsplit('/').next().unwrap_or(url);
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".git") { name[..name.len() - 4].to_string() } else { name.to_string() }
}

/// Подпись статуса изменения в карточке истории.
pub fn status_label(status: char) -> &'static str {
    match status {
        'A' => "добавлен",
        'D' => "удалён",
        'R' => "переименован",
        _ => "изменён",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_status_with_renames() {
        let out = "M\troot\nR100\told.md\tnew.md\nD\t_content/x\n";
        let changes = parse_name_status(out);
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[1], GitFileChange { path: "new.md".into(), status: 'R', old_path: Some("old.md".into()) });
        assert_eq!(changes[2].status, 'D');
    }

    #[test]
    fn ui_rules() {
        assert!(branch_name_error("feature-auth").is_none());
        assert!(branch_name_error("a b").is_some());
        assert!(branch_name_error("a~b").is_some());
        assert!(is_service_file(".gitignore"));
        assert!(is_service_file("dir/.hidden"));
        assert!(!is_service_file("dir/file"));
        assert_eq!(project_name_from_url("https://gitlab.com/you/project.git/"), "project");
        assert_eq!(project_name_from_url("git@github.com:you/neurodoc.git"), "neurodoc");
        assert_eq!(first_non_empty_line("\n\n# Title\nx").as_deref(), Some("Title"));
    }
}
