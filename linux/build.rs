//! Версия приложения из git-тега — повторяет таргет SetVersionFromGit из csproj
//! Windows-версии и функцию git() из build.gradle.kts Android-версии.
//!
//! Числовая часть берётся только из тега (`git describe --abbrev=0`), строка для
//! показа — вывод `git describe --dirty --always` без ведущего v. Сборка без git
//! (архив без .git, git не в PATH) не падает, а честно показывает 0.0.0-nogit:
//! заметный маркер лучше правдоподобной выдумки.

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let dir = Path::new(&manifest_dir);
    let repo = dir.parent().unwrap_or(dir);

    let tag = git(dir, &["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"]);
    let full = git(dir, &["describe", "--tags", "--dirty", "--always", "--match", "v[0-9]*"]);

    let numeric = tag.strip_prefix('v').unwrap_or("0.0.0").to_string();
    let display = match full.strip_prefix('v') {
        Some(v) => v.to_string(),
        None if !full.is_empty() => format!("0.0.0+g{full}"),
        None => "0.0.0-nogit".to_string(),
    };

    println!("cargo:rustc-env=NEURODOC_VERSION={display}");
    println!("cargo:rustc-env=NEURODOC_VERSION_NUMERIC={numeric}");
    println!("cargo:warning=Версия сборки: {display}");

    // Пересчитывать версию при смене коммита, тега или правке исходников (суффикс -dirty)
    for rel in [".git/HEAD", ".git/index", ".git/packed-refs", ".git/refs/tags", ".git/refs/heads"] {
        let p = repo.join(rel);
        if p.exists() {
            println!("cargo:rerun-if-changed={}", p.display());
        }
    }
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");
}
