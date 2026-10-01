//! Shared test fixtures: temp dirs and disposable Git repositories.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn unique_tag(tag: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("terrorbat-test-{tag}-{}-{n}-{nanos}", std::process::id())
}

/// A temp directory that deletes itself on drop (best effort).
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        let path = std::env::temp_dir().join(unique_tag(tag));
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir { path }
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git executable runs");
    assert!(
        out.status.success(),
        "git {args:?} failed in {}: {}",
        repo.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Initialise a small clean Git repository; returns its HEAD commit SHA.
pub fn make_repo(dir: &Path) -> String {
    std::fs::create_dir_all(dir).expect("repo dir");
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "terrorbat@test.local"]);
    git(dir, &["config", "user.name", "Terror Bat Test"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    std::fs::write(dir.join("README.md"), "fixture repo\n").expect("write readme");
    std::fs::create_dir_all(dir.join("config")).expect("config dir");
    std::fs::write(dir.join("config/security.toml"), "protected = true\n").expect("write config");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "initial"]);
    git(dir, &["rev-parse", "HEAD"])
}

/// True when `git status --porcelain` is empty for the repo.
pub fn repo_is_clean(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain"]).is_empty()
}

pub fn write_spec(dir: &Path, name: &str, yaml: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("spec dir");
    let path = dir.join(name);
    std::fs::write(&path, yaml).expect("write spec");
    path
}

/// Single-quote a Windows path for safe embedding in YAML (single-quoted
/// YAML scalars treat backslashes literally).
pub fn yaml_path(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "''"))
}
