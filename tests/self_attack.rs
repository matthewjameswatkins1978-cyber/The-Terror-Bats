//! Section M — Terror Bat self-attack (lite).
//!
//! The framework attacks its own crucial invariants through the real
//! pipeline (specs → worktrees → supervisor → evidence → receipts), not
//! through mocks. Invariants covered here:
//!
//!  1. source working tree is not mutated by normal Bat runs
//!  2. timeout terminates descendants (framework level, heartbeat quiescence)
//!  3. unsafe integers cannot enter identity or execution
//!  4. worker failure does not erase prior finalised evidence
//!  5. corrupt evidence objects are detected, never trusted
//!  6. infrastructure failure cannot become PROVEN
//!  7. NOT OBSERVED is never printed as correctness
//!  8. worktree isolation is never labelled sandboxing
//!
//! plus end-to-end runs of every shipped First Flight Bat.
//! The full M10 self-attack remains later work.

mod common;

use std::path::{Path, PathBuf};

use common::{TempDir, git, make_repo, repo_is_clean, write_spec, yaml_path};
use terrorbat::evidence::EvidenceStore;
use terrorbat::receipt::Verdict;
use terrorbat::runner::{CleanupStatus, RunOptions, RunStatus, run_bat};

fn opts(bat: &Path, repo: &Path, store: &Path) -> RunOptions {
    RunOptions {
        bat_path: bat.to_path_buf(),
        repo: repo.to_path_buf(),
        store_root: Some(store.to_path_buf()),
        overrides: Vec::new(),
    }
}

fn opts_with(bat: &Path, repo: &Path, store: &Path, overrides: Vec<String>) -> RunOptions {
    RunOptions {
        bat_path: bat.to_path_buf(),
        repo: repo.to_path_buf(),
        store_root: Some(store.to_path_buf()),
        overrides,
    }
}

fn shipped_bat(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("bats")
        .join(name)
}

/// Invariant 1: a normal (even mutating) Bat run never touches the source
/// working tree — all mutation happens in the disposable worktree.
/// Single human presentation path for honesty assertions.
fn present(r: &terrorbat::receipt::Receipt) -> String {
    terrorbat::presentation::render_receipt(r, &sartorial_core::Capabilities::piped(100))
}

#[test]
fn source_repo_unchanged_by_normal_run() {
    let dir = TempDir::new("sa-source");
    let repo = dir.join("repo");
    let head = make_repo(&repo);
    let store = dir.join("store");
    let out = run_bat(&opts(&shipped_bat("unexpected-change.yaml"), &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert!(repo_is_clean(&repo), "source tree must remain pristine");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(&repo, &["worktree", "list"]).lines().count(),
        1,
        "no worktree may remain registered"
    );
}

/// Invariant 2: a step timeout kills the whole descendant tree — proven at
/// framework level by heartbeat quiescence after the run.
#[test]
fn timeout_terminates_descendants_through_the_framework() {
    let dir = TempDir::new("sa-tree");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let heartbeat = dir.join("heartbeats.log");
    let probe = env!("CARGO_BIN_EXE_tb_probe_tree");
    let yaml = format!(
        "version: terrorbat/v1\n\
         id: tree-timeout\n\
         claim:\n  text: descendant trees are terminated on timeout\n\
         requires:\n  - process.spawn\n\
         attack:\n  run:\n\
         \x20   - adapter: command\n\
         \x20     action: run\n\
         \x20     program: {}\n\
         \x20     args: [parent, --heartbeat, {}]\n\
         oracle:\n  type: file_exists\n  path: never.txt\n\
         evidence:\n  capture: [stdout]\n\
         timeout:\n  run: 2s\n",
        yaml_path(Path::new(probe)),
        yaml_path(&heartbeat)
    );
    let bat = write_spec(&specs, "tree.yaml", &yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::TimedOut);
    assert_eq!(out.receipt.verdict, Verdict::Inconclusive);

    // The tree was alive (heartbeats exist) and is now dead (file quiesces).
    let size_before = std::fs::metadata(&heartbeat)
        .expect("heartbeats prove the tree ran")
        .len();
    assert!(size_before > 0);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let size_mid = std::fs::metadata(&heartbeat).unwrap().len();
    std::thread::sleep(std::time::Duration::from_millis(900));
    let size_after = std::fs::metadata(&heartbeat).unwrap().len();
    assert_eq!(
        size_mid, size_after,
        "heartbeat still growing: descendants leaked through the framework"
    );
}

/// Invariant 3: unsafe integers cannot enter identity or execution — a Bat
/// carrying one is refused before any run machinery starts.
#[test]
fn unsafe_integer_cannot_enter_execution() {
    let dir = TempDir::new("sa-int");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: unsafe-int\n\
                claim:\n  text: claim\n\
                requires:\n  - process.spawn\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: git\n\
                \x20     count: 9007199254740992\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "unsafe.yaml", yaml);
    let err = run_bat(&opts(&bat, &repo, &store)).expect_err("must be refused");
    assert!(err.to_string().contains("JCS-safe"), "{err}");
    let s = EvidenceStore::open(&store).expect("store");
    assert!(s.list_runs().is_empty(), "refused spec must create no run");
}

/// Invariant 4: evidence finalised before a worker failure stays readable
/// after the run.
#[test]
fn worker_failure_preserves_prior_finalised_evidence() {
    let dir = TempDir::new("sa-survive");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let hang = env!("CARGO_BIN_EXE_tb_probe_hang");
    let yaml = format!(
        "version: terrorbat/v1\n\
         id: survive\n\
         claim:\n  text: claim\n\
         requires:\n  - process.spawn\n\
         attack:\n  run:\n\
         \x20   - adapter: command\n\
         \x20     action: run\n\
         \x20     program: git\n\
         \x20     args: [--version]\n\
         \x20   - adapter: command\n\
         \x20     action: run\n\
         \x20     program: {}\n\
         oracle:\n  all: []\n\
         evidence:\n  capture: [stdout]\n\
         timeout:\n  run: 5s\n",
        yaml_path(Path::new(hang))
    );
    let bat = write_spec(&specs, "survive.yaml", &yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::TimedOut);
    let s = EvidenceStore::open(&store).expect("store");
    let first = &out.manifest.steps[0];
    let bytes = s
        .get(first.stdout.as_ref().expect("ref"))
        .expect("readable after failure");
    assert!(String::from_utf8_lossy(&bytes).contains("git version"));
}

/// Invariant 5: corrupt evidence is detected at the run-artifact level and
/// never served as trusted bytes.
#[test]
fn corrupt_run_evidence_is_detected() {
    let dir = TempDir::new("sa-corrupt");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store_root = dir.join("store");
    let out = run_bat(&opts(
        &shipped_bat("unexpected-change.yaml"),
        &repo,
        &store_root,
    ))
    .expect("run");
    let diff_ref = out
        .manifest
        .captures
        .git_diff
        .clone()
        .expect("diff evidence");
    let s = EvidenceStore::open(&store_root).expect("store");
    // Corrupt the stored object behind the store's back.
    let hex = diff_ref.hex().expect("hex");
    let obj = store_root
        .join("objects")
        .join("sha256")
        .join(&hex[..2])
        .join(&hex[2..]);
    std::fs::write(&obj, b"tampered").expect("tamper");
    let err = s.get(&diff_ref).expect_err("corrupt must fail");
    assert!(err.to_string().contains("CORRUPT"), "{err}");
}

/// Invariant 6: infrastructure failure can never become PROVEN.
#[test]
fn infrastructure_failure_cannot_become_proven() {
    let dir = TempDir::new("sa-infra");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    // Oracle WOULD fire if evaluated; the run must not reach evaluation.
    let yaml = "version: terrorbat/v1\n\
                id: infra\n\
                claim:\n  text: claim\n\
                requires:\n  - process.spawn\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: definitely-not-a-real-program-xyz\n\
                oracle:\n  type: evidence_present\n  kind: git_diff\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "infra.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::InfrastructureError);
    assert_eq!(out.receipt.verdict, Verdict::InfrastructureError);
    assert_ne!(out.receipt.verdict, Verdict::Proven);
    assert_eq!(
        out.receipt.oracle.result, None,
        "oracle must not be evaluated"
    );
}

/// Invariant 7: NOT OBSERVED never renders as correctness.
#[test]
fn not_observed_never_printed_as_correctness() {
    let dir = TempDir::new("sa-notobs");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let out = run_bat(&opts(&shipped_bat("command-exit.yaml"), &repo, &store)).expect("run");
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
    let rendered = present(&out.receipt);
    assert!(rendered.contains("NOT OBSERVED"));
    assert!(rendered.contains("NOT a correctness certificate"));
    for forbidden in ["is correct", "verified safe", "ALL GREEN", "PASSED"] {
        assert!(
            !rendered.contains(forbidden),
            "NOT OBSERVED rendering must not contain `{forbidden}`:\n{rendered}"
        );
    }
}

/// Invariant 8: worktree isolation is never labelled sandboxing.
#[test]
fn worktree_isolation_never_labelled_sandbox() {
    let dir = TempDir::new("sa-isolation");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let out = run_bat(&opts(&shipped_bat("unexpected-change.yaml"), &repo, &store)).expect("run");
    let rendered = present(&out.receipt);
    assert!(rendered.contains("NOT hostile-code containment"));
    assert!(rendered.contains("UNENFORCED"));
    assert!(!rendered.to_lowercase().contains("sandboxed"));
    assert!(!rendered.to_lowercase().contains("fully isolated"));
    // The machine-readable receipt carries the same honesty.
    let json = serde_json::to_string(&out.receipt).expect("json");
    assert!(json.contains("UNENFORCED"));
    assert!(json.contains("NOT hostile-code containment"));
}

/// Shipped Bat end-to-end: command-exit on a healthy machine.
#[test]
fn builtin_command_exit_bat_runs() {
    let dir = TempDir::new("sa-cmd");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let out = run_bat(&opts(&shipped_bat("command-exit.yaml"), &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
}

/// Shipped Bat end-to-end: unexpected-change proves mutation detection.
#[test]
fn builtin_unexpected_change_bat_proves_detection() {
    let dir = TempDir::new("sa-change");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let out = run_bat(&opts(&shipped_bat("unexpected-change.yaml"), &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    assert!(
        out.receipt
            .oracle
            .conditions
            .iter()
            .any(|c| c.condition.starts_with("path_changed"))
    );
}

/// Shipped Bat end-to-end: false-success — green-looking output contradicted
/// by deterministic diff evidence.
/// Shipped-Bat end-to-end: success-looking output contradicted by diff/status
/// evidence is PROVEN. The shipped Bat is Windows-specific (`cmd`); Unix runs an
/// equivalent `sh` Bat asserting the same conjunction.
#[test]
fn builtin_false_success_bat_proves_detection() {
    let dir = TempDir::new("sa-false");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    #[cfg(windows)]
    let bat = shipped_bat("false-success.yaml");
    #[cfg(not(windows))]
    let bat = write_spec(
        &dir.join("specs"),
        "false-success-unix.yaml",
        r#"version: terrorbat/v1
id: false-success-unix
claim:
  text: >
    A command step whose output reports success leaves the repository
    unmodified.
requires:
  - process.spawn
  - fs.write
  - git.inspect
attack:
  run:
    - adapter: command
      action: run
      program: sh
      args:
        - -c
        - "echo ALL TESTS PASSED; echo pwned=true > tb-false-success.txt; exit 0"
oracle:
  all:
    - type: text_contains
      step: run:0
      stream: stdout
      substring: ALL TESTS PASSED
    - type: git_diff_contains
      substring: pwned=true
    - type: path_changed
      path: tb-false-success.txt
evidence:
  capture:
    - git_diff
    - stdout
    - stderr
timeout:
  run: 60s
  total: 120s
"#,
    );
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    // All three legs of the conjunction fired.
    let fired = out
        .receipt
        .oracle
        .conditions
        .iter()
        .filter(|c| c.result == terrorbat::oracle::OracleResult::Falsified)
        .count();
    assert_eq!(fired, 3, "{:?}", out.receipt.oracle.conditions);
}

/// Shipped Bat end-to-end: orphan-process times out, cleans up, and the
/// evidence shows the tree was alive.
#[test]
fn builtin_orphan_process_bat_times_out_and_cleans_up() {
    let dir = TempDir::new("sa-orphan");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let probe = env!("CARGO_BIN_EXE_tb_probe_tree");
    let out = run_bat(&opts_with(
        &shipped_bat("orphan-process.yaml"),
        &repo,
        &store,
        vec![format!("probe_program={probe}")],
    ))
    .expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::TimedOut);
    assert_eq!(out.receipt.verdict, Verdict::Inconclusive);
    assert!(matches!(
        out.manifest.worktree.cleanup.status,
        CleanupStatus::Succeeded
    ));
    // The captured status proves the heartbeat file existed while alive.
    let s = EvidenceStore::open(&store).expect("store");
    let status_ref = out.manifest.captures.git_status.as_ref().expect("status");
    let status = String::from_utf8(s.get(status_ref).expect("bytes")).expect("utf8");
    assert!(status.contains("heartbeats"), "status evidence: {status}");
}

/// Closeout invariant: a tampered receipt cannot be inspected or replayed
/// as trusted evidence. Proven through the real product paths (run →
/// receipt on disk → tamper → lookup/load/replay), not a private helper:
/// the product boundary itself must fail closed with TB-RECEIPT-CORRUPT.
#[test]
fn tampered_receipt_cannot_be_inspected_or_replayed() {
    let dir = TempDir::new("sa-receipt-tamper");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let out = run_bat(&opts(&shipped_bat("unexpected-change.yaml"), &repo, &store)).expect("run");
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    let rid = out.receipt.receipt_id.clone();

    // Downgrade the verdict on disk.
    let path = out.run_dir.join("receipt.json");
    let text = std::fs::read_to_string(&path).expect("read receipt");
    std::fs::write(
        &path,
        text.replacen(
            "\"verdict\": \"PROVEN\"",
            "\"verdict\": \"NOT OBSERVED\"",
            1,
        ),
    )
    .expect("write tampered receipt");

    // Every trusted path fails closed.
    let s = EvidenceStore::open(&store).expect("store");
    let loc =
        terrorbat::receipt::locate_run(&s, &rid).expect_err("inspect lookup must fail closed");
    assert!(loc.to_string().contains("TB-RECEIPT-CORRUPT"), "{loc}");
    let load = terrorbat::receipt::load_receipt(&out.run_dir).expect_err("load must fail closed");
    assert!(load.to_string().contains("TB-RECEIPT-CORRUPT"), "{load}");
    let rep =
        terrorbat::receipt::replay(&rid, Some(store.clone())).expect_err("replay must fail closed");
    assert!(rep.to_string().contains("TB-RECEIPT-CORRUPT"), "{rep}");
}
