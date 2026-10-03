//! Universal adapters Phase 1: discovery honesty, new filesystem actions,
//! UNSUPPORTED mapping, and the snapshot pattern (positive + control).
//!
//! Principle under test: adapters report reality; Bats and oracles decide
//! meaning. Controls return NOT OBSERVED; UNSUPPORTED is never a finding.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{TempDir, make_repo, write_spec};
use terrorbat::builtins::{StepCtx, StepError, dispatch};
use terrorbat::oracle::OracleResult;
use terrorbat::receipt::{Verdict, verdict_for};
use terrorbat::runner::{RunOptions, RunStatus, exit_code_for, run_bat};

fn ctx<'a>(worktree: &'a Path, spec_dir: &'a Path) -> StepCtx<'a> {
    StepCtx {
        worktree,
        spec_dir,
        deadline: Some(std::time::Duration::from_secs(30)),
    }
}

fn str_payload(pairs: &[(&str, &str)]) -> BTreeMap<String, serde_json::Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), serde_json::json!(v)))
        .collect()
}

fn opts(bat: &Path, repo: &Path, store: &Path) -> RunOptions {
    RunOptions {
        bat_path: bat.to_path_buf(),
        repo: repo.to_path_buf(),
        store_root: Some(store.to_path_buf()),
        overrides: Vec::new(),
    }
}

// --- discovery -----------------------------------------------------------

#[test]
fn catalogue_lists_twelve_adapters_with_honest_statuses() {
    let all = terrorbat::adapters::all();
    let names = terrorbat::adapters::names();
    assert_eq!(all.len(), 12, "catalogue size is a contract");
    assert_eq!(names.len(), 12);
    for expect in [
        "command",
        "filesystem",
        "git",
        "process",
        "stdio",
        "json",
        "http",
        "tcp",
        "test-runner",
        "database",
        "snapshot",
        "external",
    ] {
        assert!(
            names.contains(&expect.to_string()),
            "missing adapter {expect}"
        );
    }
    let status = |name: &str| {
        all.iter()
            .find(|a| a.name == name)
            .map(|a| a.status.clone())
            .expect("adapter present")
    };
    assert_eq!(status("command"), "builtin-stable");
    assert_eq!(status("filesystem"), "builtin-stable");
    assert_eq!(status("git"), "builtin-stable");
    assert_eq!(status("process"), "builtin-partial");
    for name in ["stdio", "json", "test-runner", "database", "snapshot"] {
        assert_eq!(
            status(name),
            "composition",
            "{name} has no dedicated runtime"
        );
    }
    for name in ["http", "tcp"] {
        assert_eq!(status(name), "planned", "{name} must not be pretended");
    }
    assert_eq!(status("external"), "external-protocol");
}

#[test]
fn unknown_adapter_is_none() {
    assert!(terrorbat::adapters::find("my-weird-web-service-adapter").is_none());
    assert!(terrorbat::adapters::find("").is_none());
}

#[test]
fn every_operation_declares_a_non_empty_capability() {
    for a in terrorbat::adapters::all() {
        assert!(
            !a.operations.is_empty(),
            "adapter {} has no operations",
            a.name
        );
        for op in &a.operations {
            assert!(
                !op.capability.trim().is_empty(),
                "{}.{} lacks a capability",
                a.name,
                op.action
            );
            assert!(
                !op.description.trim().is_empty(),
                "{}.{} lacks a description",
                a.name,
                op.action
            );
        }
        assert!(
            !a.platform_notes.is_empty(),
            "adapter {} hides platform limits",
            a.name
        );
    }
}

// --- filesystem actions --------------------------------------------------

#[test]
fn fs_write_then_read_roundtrip() {
    let dir = TempDir::new("ua-roundtrip");
    let wt = dir.join("wt");
    let spec_dir = dir.join("specs");
    std::fs::create_dir_all(&wt).expect("wt");
    std::fs::create_dir_all(&spec_dir).expect("specs");
    let c = ctx(&wt, &spec_dir);

    let out = dispatch(
        "fs",
        "write",
        &str_payload(&[("path", "deep/nested/file.txt"), ("text", "hello bytes")]),
        &c,
    )
    .expect("write");
    assert_eq!(out.exit_code, Some(0));

    let out = dispatch(
        "fs",
        "read",
        &str_payload(&[("path", "deep/nested/file.txt")]),
        &c,
    )
    .expect("read");
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(out.stdout, b"hello bytes");
    assert_eq!(out.stdout_total, 11);
}

#[test]
fn fs_read_missing_is_an_honest_error_never_a_finding() {
    let dir = TempDir::new("ua-missing");
    let wt = dir.join("wt");
    let spec_dir = dir.join("specs");
    std::fs::create_dir_all(&wt).expect("wt");
    std::fs::create_dir_all(&spec_dir).expect("specs");
    let c = ctx(&wt, &spec_dir);

    let err = dispatch("fs", "read", &str_payload(&[("path", "nope.txt")]), &c)
        .expect_err("missing file must error");
    assert!(
        matches!(err, StepError::Io(_)),
        "missing file is Io, got {err:?}"
    );

    // Escape attempts stay policy refusals.
    let err = dispatch(
        "fs",
        "read",
        &str_payload(&[("path", "../outside.txt")]),
        &c,
    )
    .expect_err("escape must be refused");
    assert!(
        matches!(err, StepError::Policy(_)),
        "escape is Policy, got {err:?}"
    );
}

#[test]
fn fs_list_stat_digest_remove_flow() {
    let dir = TempDir::new("ua-flow");
    let wt = dir.join("wt");
    let spec_dir = dir.join("specs");
    std::fs::create_dir_all(&wt).expect("wt");
    std::fs::create_dir_all(&spec_dir).expect("specs");
    let c = ctx(&wt, &spec_dir);

    for (path, text) in [("b.txt", "b"), ("a.txt", "a"), ("sub/c.txt", "c")] {
        dispatch(
            "fs",
            "write",
            &str_payload(&[("path", path), ("text", text)]),
            &c,
        )
        .expect("write");
    }

    let out = dispatch("fs", "list", &str_payload(&[("path", ".")]), &c).expect("list");
    let listing = String::from_utf8(out.stdout).expect("utf8 listing");
    assert_eq!(
        listing, "a.txt\nb.txt\nsub/\n",
        "sorted listing, dirs suffixed"
    );

    let out = dispatch("fs", "stat", &str_payload(&[("path", "a.txt")]), &c).expect("stat");
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stat JSON");
    assert_eq!(meta["kind"], serde_json::json!("file"));
    assert_eq!(meta["size"], serde_json::json!(1));
    assert!(meta["sha256"].as_str().is_some_and(|s| s.len() == 64));

    let out = dispatch("fs", "stat", &str_payload(&[("path", "ghost.txt")]), &c)
        .expect("stat of missing reports reality");
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stat JSON");
    assert_eq!(meta["kind"], serde_json::json!("missing"));

    let out = dispatch("fs", "digest", &str_payload(&[("path", ".")]), &c).expect("digest");
    let digest = String::from_utf8(out.stdout).expect("utf8 digest");
    let lines: Vec<&str> = digest.lines().collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted, "digest output is canonical (sorted)");
    assert!(lines.iter().any(|l| l.ends_with("  a.txt")), "{digest}");
    assert!(lines.iter().any(|l| l == &"dir  sub/"), "{digest}");

    dispatch("fs", "remove", &str_payload(&[("path", "b.txt")]), &c).expect("remove");
    let out = dispatch("fs", "stat", &str_payload(&[("path", "b.txt")]), &c).expect("stat");
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stat JSON");
    assert_eq!(meta["kind"], serde_json::json!("missing"));

    // Non-empty directories are refused (never recursive).
    let err = dispatch("fs", "remove", &str_payload(&[("path", "sub")]), &c)
        .expect_err("recursive remove refused");
    assert!(matches!(err, StepError::Io(_)), "got {err:?}");
}

// --- UNSUPPORTED ---------------------------------------------------------

#[test]
fn unsupported_is_inconclusive_never_proven() {
    for oracle in [
        None,
        Some(OracleResult::Falsified),
        Some(OracleResult::NotFalsified),
        Some(OracleResult::Undetermined),
    ] {
        let (verdict, note) = verdict_for(RunStatus::Unsupported, oracle);
        assert_eq!(
            verdict,
            Verdict::Inconclusive,
            "UNSUPPORTED is never a finding"
        );
        let note = note.expect("UNSUPPORTED carries a note");
        assert!(note.contains("UNSUPPORTED"), "{note}");
    }
    assert_eq!(
        exit_code_for(RunStatus::Unsupported, Some("INCONCLUSIVE")),
        3,
        "UNSUPPORTED exits inconclusive, not failure (1) nor false pass semantics"
    );
}

// --- snapshot pattern: positive + control --------------------------------

const PROVEN_BAT: &str = "version: terrorbat/v1\n\
id: snapshot-detects-change\n\
claim:\n  text: Writing a file leaves the worktree unmodified.\n\
requires:\n  - fs.write\n  - fs.read\n  - git.inspect\n\
attack:\n  setup:\n    - adapter: fs\n      action: digest\n      path: .\n  run:\n    - adapter: fs\n      action: write\n      path: tb-snapshot-probe.txt\n      text: probe\n\
oracle:\n  type: path_changed\n  path: tb-snapshot-probe.txt\n\
evidence:\n  capture: [git_diff, stdout, stderr]\n";

const CONTROL_BAT: &str = "version: terrorbat/v1\n\
id: snapshot-quiet-control\n\
claim:\n  text: An untouched probe path shows no change.\n\
requires:\n  - fs.read\n  - git.inspect\n\
attack:\n  run:\n    - adapter: fs\n      action: digest\n      path: .\n\
oracle:\n  type: path_changed\n  path: tb-control-untouched.txt\n\
evidence:\n  capture: [git_diff, stdout, stderr]\n";

#[test]
fn snapshot_pattern_proves_change_with_universal_adapters_only() {
    let dir = TempDir::new("ua-proven");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "proven.yaml", PROVEN_BAT);

    let out = run_bat(&opts(&bat, &repo, &store)).expect("run succeeds");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
}

#[test]
fn quiet_control_returns_not_observed() {
    let dir = TempDir::new("ua-control");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "control.yaml", CONTROL_BAT);

    let out = run_bat(&opts(&bat, &repo, &store)).expect("run succeeds");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
}
