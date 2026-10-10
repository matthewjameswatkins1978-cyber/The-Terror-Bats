//! Built-in primitive tests: Windows path safety (absolute/UNC/drive/`..`/
//! junction escape), fs payload rules, and command dispatch errors.
//!
//! Honesty note: these tests prove Terror Bats' *built-in* path validation.
//! They do not prove sandboxing of arbitrary child processes — nothing in
//! worktree mode does.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::TempDir;
use terrorbats::builtins::{StepCtx, StepError, dispatch, resolve_in_worktree};

fn ctx<'a>(worktree: &'a Path, spec_dir: &'a Path) -> StepCtx<'a> {
    StepCtx {
        worktree,
        spec_dir,
        deadline: Some(std::time::Duration::from_secs(30)),
        processes: None,
        phase: "test",
        step_index: 0,
    }
}

#[test]
fn relative_paths_resolve_inside_worktree() {
    let dir = TempDir::new("bi-ok");
    let wt = dir.join("wt");
    std::fs::create_dir_all(wt.join("a")).expect("wt");
    let p = resolve_in_worktree(&wt, "a/b.txt").expect("resolve");
    assert!(p.starts_with(wt.canonicalize().unwrap()));
    // Internal `..` that stays inside is fine.
    let p2 = resolve_in_worktree(&wt, "a/../a/b.txt").expect("internal dotdot");
    assert!(p2.ends_with("a/b.txt") || p2.to_string_lossy().ends_with("a\\b.txt"));
}

#[test]
fn escaping_paths_are_rejected() {
    let dir = TempDir::new("bi-escape");
    let wt = dir.join("wt");
    std::fs::create_dir_all(&wt).expect("wt");
    // Cross-platform rejections: Unix absolute paths, POSIX-reserved `//`,
    // `..` traversal escaping the worktree, and empty / root-resolving inputs.
    for bad in [
        "//server/share/file",
        "/etc/passwd",
        "../outside.txt",
        "a/../../outside.txt",
        "..",
        "",
        ".",
    ] {
        let err = resolve_in_worktree(&wt, bad).err();
        assert!(err.is_some(), "path `{bad}` must be rejected");
        assert!(
            matches!(err, Some(StepError::Policy(_))),
            "path `{bad}` must be a policy rejection"
        );
    }
    // Windows drive / UNC / backslash-rooted / ADS syntax is Windows-only
    // reasoning (see `resolve_in_worktree`); it must stay rejected on Windows.
    #[cfg(windows)]
    for bad in [
        "C:\\Users\\someone\\secret.txt",
        "c:/windows/system32",
        "\\\\server\\share\\file",
        "\\rooted",
        "a:b",
    ] {
        let err = resolve_in_worktree(&wt, bad).err();
        assert!(err.is_some(), "path `{bad}` must be rejected");
        assert!(
            matches!(err, Some(StepError::Policy(_))),
            "path `{bad}` must be a policy rejection"
        );
    }
    // On Unix a backslash is an ordinary filename character and `:` carries no
    // drive meaning, so these are legitimate worktree-relative names.
    #[cfg(unix)]
    for ok in [
        "C:\\Users\\someone\\secret.txt",
        "c:/windows/system32",
        "\\\\server\\share\\file",
        "\\rooted",
        "a:b",
    ] {
        let p = resolve_in_worktree(&wt, ok).expect("unix filename must resolve");
        assert!(
            p.starts_with(wt.canonicalize().unwrap()),
            "unix filename `{ok}` must stay inside the worktree"
        );
    }
}

#[test]
#[cfg(windows)]
fn junction_escape_is_rejected() {
    let dir = TempDir::new("bi-junction");
    let wt = dir.join("wt");
    let outside = dir.join("outside");
    std::fs::create_dir_all(&wt).expect("wt");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(outside.join("secret.txt"), "secret").expect("secret");
    let link = wt.join("link");
    // Directory junctions need no elevated privileges on Windows.
    let out = std::process::Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            &link.to_string_lossy(),
            &outside.to_string_lossy(),
        ])
        .output()
        .expect("cmd runs");
    if !out.status.success() {
        eprintln!(
            "SKIPPED: junction creation failed on this machine: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let err =
        resolve_in_worktree(&wt, "link/secret.txt").expect_err("junction escape must be rejected");
    match err {
        StepError::Policy(msg) => assert!(msg.contains("junction/symlink"), "{msg}"),
        other => panic!("expected Policy rejection, got {other:?}"),
    }
}

#[test]
fn fs_write_requires_exactly_one_source() {
    let dir = TempDir::new("bi-fswrite");
    let wt = dir.join("wt");
    let spec_dir = dir.join("specs");
    std::fs::create_dir_all(&wt).expect("wt");
    std::fs::create_dir_all(&spec_dir).expect("specs");
    let c = ctx(&wt, &spec_dir);

    // text form, parent dirs created
    let mut payload = BTreeMap::new();
    payload.insert("path".into(), serde_json::json!("deep/nested/file.txt"));
    payload.insert("text".into(), serde_json::json!("hello"));
    let out = dispatch("fs", "write", &payload, &c).expect("text write");
    assert_eq!(out.exit_code, Some(0));
    let written = std::fs::read_to_string(wt.join("deep/nested/file.txt")).expect("written");
    assert_eq!(written, "hello");

    // from form resolves against the spec dir
    std::fs::write(spec_dir.join("fixture.txt"), "fixture bytes").expect("fixture");
    let mut payload = BTreeMap::new();
    payload.insert("path".into(), serde_json::json!("copied.txt"));
    payload.insert("from".into(), serde_json::json!("fixture.txt"));
    dispatch("fs", "write", &payload, &c).expect("from write");
    assert_eq!(
        std::fs::read(wt.join("copied.txt")).expect("copied"),
        b"fixture bytes"
    );

    // both / neither are malformed
    let mut both = BTreeMap::new();
    both.insert("path".into(), serde_json::json!("x.txt"));
    both.insert("text".into(), serde_json::json!("a"));
    both.insert("from".into(), serde_json::json!("fixture.txt"));
    assert!(matches!(
        dispatch("fs", "write", &both, &c),
        Err(StepError::Malformed(_))
    ));
    let neither = BTreeMap::new();
    assert!(matches!(
        dispatch("fs", "write", &neither, &c),
        Err(StepError::Malformed(_))
    ));
}

#[test]
fn fs_write_through_escaping_path_is_policy_denied() {
    let dir = TempDir::new("bi-fsescape");
    let wt = dir.join("wt");
    let spec_dir = dir.join("specs");
    std::fs::create_dir_all(&wt).expect("wt");
    std::fs::create_dir_all(&spec_dir).expect("specs");
    let c = ctx(&wt, &spec_dir);
    let mut payload = BTreeMap::new();
    payload.insert("path".into(), serde_json::json!("../escaped.txt"));
    payload.insert("text".into(), serde_json::json!("nope"));
    assert!(matches!(
        dispatch("fs", "write", &payload, &c),
        Err(StepError::Policy(_))
    ));
    assert!(!dir.join("escaped.txt").exists(), "nothing may escape");
}

#[test]
fn command_run_requires_program() {
    let dir = TempDir::new("bi-cmd");
    let wt = dir.join("wt");
    std::fs::create_dir_all(&wt).expect("wt");
    let c = ctx(&wt, &wt);
    let payload = BTreeMap::new();
    assert!(matches!(
        dispatch("command", "run", &payload, &c),
        Err(StepError::Malformed(_))
    ));
}

#[test]
fn unknown_adapter_action_is_malformed() {
    let dir = TempDir::new("bi-unknown");
    let wt = dir.join("wt");
    std::fs::create_dir_all(&wt).expect("wt");
    let c = ctx(&wt, &wt);
    let payload = BTreeMap::new();
    assert!(matches!(
        dispatch("nope", "whatever", &payload, &c),
        Err(StepError::Malformed(_))
    ));
}

#[test]
fn git_rev_parse_rejects_suspicious_revs() {
    let dir = TempDir::new("bi-revparse");
    let wt = dir.join("wt");
    std::fs::create_dir_all(&wt).expect("wt");
    let c = ctx(&wt, &wt);
    for bad in ["--upload-pack=evil", "a..b", "origin/main:file"] {
        let mut payload = BTreeMap::new();
        payload.insert("rev".into(), serde_json::json!(bad));
        assert!(
            matches!(
                dispatch("git", "rev_parse", &payload, &c),
                Err(StepError::Policy(_))
            ),
            "`{bad}` must be refused"
        );
    }
}
