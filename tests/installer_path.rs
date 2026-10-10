//! Windows installer PATH-ownership regressions: the uninstaller must
//! remove only PATH entries the installer appended itself (ownership
//! marker), never pre-existing ones. Every scenario runs inside a single
//! disposable `pwsh` session with `-PathScope Process`, so CI never
//! touches the real user PATH; temp dirs are removed by a Drop guard even
//! when an assertion panics.
#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

struct TempInstall {
    path: PathBuf,
}

impl TempInstall {
    fn fresh(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("tb-path-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp install dir");
        Self { path }
    }
}

impl Drop for TempInstall {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn session(script: &str) -> String {
    let out = Command::new("pwsh")
        .args(["-NoProfile", "-Command", script])
        .output()
        .expect("pwsh runs");
    assert!(
        out.status.success(),
        "pwsh session failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn install_ps1() -> String {
    manifest_dir()
        .join("install")
        .join("install.ps1")
        .display()
        .to_string()
}

fn uninstall_ps1() -> String {
    manifest_dir()
        .join("install")
        .join("uninstall.ps1")
        .display()
        .to_string()
}

fn debug_src() -> String {
    manifest_dir()
        .join("target")
        .join("debug")
        .display()
        .to_string()
}

fn install_cmd(dir: &str, extra: &str) -> String {
    format!(
        "& '{}' -InstallDir '{dir}' -AddToUserPath -PathScope Process -SourceDir '{}' {extra}",
        install_ps1(),
        debug_src()
    )
}

fn install_no_path_cmd(dir: &str) -> String {
    format!(
        "& '{}' -InstallDir '{dir}' -PathScope Process -SourceDir '{}'",
        install_ps1(),
        debug_src()
    )
}

fn uninstall_cmd(dir: &str) -> String {
    format!(
        "& '{}' -InstallDir '{dir}' -PathScope Process",
        uninstall_ps1()
    )
}

fn report(out: &str, name: &str, want: bool) {
    // PowerShell renders booleans as True/False.
    let want = if want { "True" } else { "False" };
    assert!(
        out.lines().any(|l| l.trim() == format!("{name}={want}")),
        "expected {name}={want} in:\n{out}"
    );
}

#[test]
fn uninstall_preserves_pre_existing_path_entry() {
    let dir = TempInstall::fresh("preexisting");
    let dir = dir.path.display().to_string();
    let out = session(&format!(
        "$ErrorActionPreference='Stop'; \
         $env:Path += \";{dir}\"; \
         {install}; \
         'MARKER=' + (Test-Path '{dir}\\.terrorbats-path-added'); \
         {uninstall}; \
         'INPATH=' + (($env:Path -split ';') -contains '{dir}')",
        install = install_cmd(&dir, ""),
        uninstall = uninstall_cmd(&dir),
    ));
    report(&out, "MARKER", false);
    // Old code removed the entry here; the fix must preserve it.
    report(&out, "INPATH", true);
}

#[test]
fn uninstall_removes_installer_added_path_entry_exactly() {
    let dir = TempInstall::fresh("owned");
    let dir = dir.path.display().to_string();
    let out = session(&format!(
        "$ErrorActionPreference='Stop'; \
         $before = $env:Path; \
         {install}; \
         'MARKER=' + (Test-Path '{dir}\\.terrorbats-path-added'); \
         'ADDED=' + (($env:Path -split ';') -contains '{dir}'); \
         {uninstall}; \
         'RESTORED=' + ($env:Path -ceq $before); \
         'MARKER_GONE=' + (-not (Test-Path '{dir}\\.terrorbats-path-added'))",
        install = install_cmd(&dir, ""),
        uninstall = uninstall_cmd(&dir),
    ));
    report(&out, "MARKER", true);
    report(&out, "ADDED", true);
    // Full-string equality: ordering and unrelated entries unchanged.
    report(&out, "RESTORED", true);
    report(&out, "MARKER_GONE", true);
}

#[test]
fn install_without_path_opt_in_claims_no_ownership() {
    let dir = TempInstall::fresh("nooptin");
    let dir = dir.path.display().to_string();
    let out = session(&format!(
        "$ErrorActionPreference='Stop'; \
         $before = $env:Path; \
         {install}; \
         'MARKER=' + (Test-Path '{dir}\\.terrorbats-path-added'); \
         {uninstall}; \
         'UNTOUCHED=' + ($env:Path -ceq $before)",
        install = install_no_path_cmd(&dir),
        uninstall = uninstall_cmd(&dir),
    ));
    report(&out, "MARKER", false);
    report(&out, "UNTOUCHED", true);
}

#[test]
fn force_reinstall_preserves_existing_ownership() {
    let dir = TempInstall::fresh("force");
    let dir = dir.path.display().to_string();
    let out = session(&format!(
        "$ErrorActionPreference='Stop'; \
         $before = $env:Path; \
         {install}; \
         {reinstall}; \
         'MARKER=' + (Test-Path '{dir}\\.terrorbats-path-added'); \
         'SINGLE=' + ((($env:Path -split ';') | Where-Object {{ $_ -eq '{dir}' }}).Count -eq 1); \
         {uninstall}; \
         'RESTORED=' + ($env:Path -ceq $before)",
        install = install_cmd(&dir, ""),
        reinstall = install_cmd(&dir, "-Force"),
        uninstall = uninstall_cmd(&dir),
    ));
    report(&out, "MARKER", true);
    report(&out, "SINGLE", true);
    report(&out, "RESTORED", true);
}

#[test]
fn similar_sibling_entries_remain_untouched() {
    let dir = TempInstall::fresh("sibling");
    let dir = dir.path.display().to_string();
    let sibling = format!("{dir}-Other");
    let out = session(&format!(
        "$ErrorActionPreference='Stop'; \
         $env:Path += \";{sibling}\"; \
         {install}; \
         'ADDED=' + (($env:Path -split ';') -contains '{dir}'); \
         {uninstall}; \
         'SIBLING=' + (($env:Path -split ';') -contains '{sibling}'); \
         'REAL_GONE=' + ((($env:Path -split ';') -contains '{dir}') -eq $false)",
        install = install_cmd(&dir, ""),
        uninstall = uninstall_cmd(&dir),
    ));
    // Exact entry matching: the sibling must not satisfy the presence check.
    report(&out, "ADDED", true);
    report(&out, "SIBLING", true);
    report(&out, "REAL_GONE", true);
}

#[test]
fn repeated_uninstall_is_idempotent() {
    let dir = TempInstall::fresh("repeat");
    let dir = dir.path.display().to_string();
    let out = session(&format!(
        "$ErrorActionPreference='Stop'; \
         $before = $env:Path; \
         {install}; \
         {uninstall}; \
         $afterFirst = $env:Path; \
         {uninstall2}; \
         'STABLE=' + ($env:Path -ceq $afterFirst); \
         'RESTORED=' + ($env:Path -ceq $before)",
        install = install_cmd(&dir, ""),
        uninstall = uninstall_cmd(&dir),
        uninstall2 = uninstall_cmd(&dir),
    ));
    report(&out, "STABLE", true);
    report(&out, "RESTORED", true);
}

#[test]
fn stale_marker_without_path_entry_removes_nothing() {
    let dir = TempInstall::fresh("stale");
    let dir = dir.path.display().to_string();
    let out = session(&format!(
        "$ErrorActionPreference='Stop'; \
         $before = $env:Path; \
         Set-Content -NoNewline -Encoding ascii '{dir}\\.terrorbats-path-added' 'stale'; \
         {uninstall}; \
         'UNTOUCHED=' + ($env:Path -ceq $before); \
         'MARKER_GONE=' + (-not (Test-Path '{dir}\\.terrorbats-path-added'))",
        uninstall = uninstall_cmd(&dir),
    ));
    report(&out, "UNTOUCHED", true);
    report(&out, "MARKER_GONE", true);
}
