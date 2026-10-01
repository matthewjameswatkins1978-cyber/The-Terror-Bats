//! Runner integration tests (M3+M4): disposable worktree execution against
//! real temporary Git repositories, capability preflight, timeouts, capture,
//! cleanup honesty, and source-repo immutability. Windows is a first-class
//! target: these tests run on Windows.

mod common;

use std::path::{Path, PathBuf};

use common::{TempDir, git, make_repo, repo_is_clean, write_spec, yaml_path};
use terrorbat::evidence::EvidenceStore;
use terrorbat::runner::{CleanupStatus, RunOptions, RunOutput, RunStatus, exit_code_for, run_bat};

fn opts(bat: &Path, repo: &Path, store: &Path) -> RunOptions {
    RunOptions {
        bat_path: bat.to_path_buf(),
        repo: repo.to_path_buf(),
        store_root: Some(store.to_path_buf()),
        overrides: Vec::new(),
    }
}

fn mutation_spec(probe: Option<&str>) -> String {
    let run_step = match probe {
        Some(exe) => format!(
            "    - adapter: command\n      action: run\n      program: {}\n",
            yaml_path(Path::new(exe))
        ),
        None => "    - adapter: fs\n      action: write\n      path: config/security.toml\n      text: \"pwned = true\\n\"\n".to_string(),
    };
    let requires = if probe.is_some() {
        "requires:\n  - process.spawn\n  - fs.write\n  - git.inspect\n"
    } else {
        "requires:\n  - fs.write\n  - git.inspect\n"
    };
    format!(
        "version: terrorbat/v1\n\
         id: protected-config\n\
         claim:\n  text: The protected config is never modified by ordinary work.\n\
         {requires}\
         attack:\n\
         \x20 setup:\n\
         \x20   - adapter: git\n\
         \x20     action: worktree.snapshot\n\
         \x20 run:\n\
         {run_step}\
         oracle:\n  all: []\n\
         evidence:\n  capture: [git_diff]\n"
    )
}

#[test]
fn mutation_run_completes_captures_diff_and_cleans_up() {
    let dir = TempDir::new("run-basic");
    let repo = dir.join("repo");
    let head = make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "protected.yaml", &mutation_spec(None));

    let out: RunOutput = run_bat(&opts(&bat, &repo, &store)).expect("run succeeds");
    let m = &out.manifest;

    // Mechanical status and steps.
    assert_eq!(m.run_status, RunStatus::Completed);
    assert_eq!(m.steps.len(), 2, "snapshot + mutation");
    assert!(m.steps.iter().all(|s| s.status == RunStatus::Completed));
    assert_eq!(m.target.commit, head);

    // Mutation landed in the worktree and was captured as evidence.
    let ev_store = EvidenceStore::open(&store).expect("store");
    let diff_ref = m.captures.git_diff.as_ref().expect("diff captured");
    let diff = String::from_utf8(ev_store.get(diff_ref).expect("diff bytes")).expect("utf8");
    assert!(diff.contains("pwned = true"), "diff evidence: {diff}");
    assert!(
        m.captures
            .untracked
            .iter()
            .any(|p| p.contains("security.toml"))
            || diff.contains("config/security.toml"),
        "new/changed file must be visible in mutation evidence"
    );

    // Source repository untouched: clean and still at the pinned commit.
    assert!(repo_is_clean(&repo), "source repo must remain clean");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);

    // Cleanup succeeded and the worktree is really gone.
    assert!(matches!(
        m.worktree.cleanup.status,
        CleanupStatus::Succeeded
    ));
    let wt = m.worktree.path.clone().expect("worktree path recorded");
    assert!(!PathBuf::from(&wt).exists(), "worktree removed");
    let listed = git(&repo, &["worktree", "list"]);
    assert!(
        !listed.contains(&wt),
        "git no longer registers the worktree"
    );

    // Durable run record exists with an operation log.
    let run_dir = PathBuf::from(&m.run_dir);
    assert!(run_dir.join("manifest.json").exists());
    let oplog = std::fs::read_to_string(run_dir.join("operations.jsonl")).expect("oplog");
    assert!(oplog.lines().count() >= 5);
    assert!(oplog.contains("run_start") && oplog.contains("run_finish"));

    // Honesty: limitations state the containment truth.
    let joined = m.limitations.join("\n");
    assert!(joined.contains("NOT hostile-code containment"), "{joined}");
    assert!(joined.contains("UNENFORCED"), "{joined}");

    assert_eq!(exit_code_for(m.run_status, None), 0);
}

#[test]
fn dirty_target_is_refused_without_a_run_record() {
    let dir = TempDir::new("run-dirty");
    let repo = dir.join("repo");
    make_repo(&repo);
    std::fs::write(repo.join("README.md"), "uncommitted local change\n").expect("dirty");
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "protected.yaml", &mutation_spec(None));

    let err = run_bat(&opts(&bat, &repo, &store)).expect_err("dirty target refused");
    let msg = err.to_string();
    assert!(msg.contains("dirty"), "{msg}");
    assert!(msg.contains("will not silently"), "{msg}");
    // No run directories were created.
    if store.exists() {
        let s = EvidenceStore::open(&store).expect("store");
        assert!(s.list_runs().is_empty(), "refusal must not create runs");
    }
    assert_eq!(exit_code_for(RunStatus::Invalid, None), 2);
}

#[test]
fn untracked_source_file_also_makes_target_dirty() {
    let dir = TempDir::new("run-untracked");
    let repo = dir.join("repo");
    make_repo(&repo);
    std::fs::write(repo.join("stray.txt"), "stray\n").expect("stray");
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "protected.yaml", &mutation_spec(None));
    let err = run_bat(&opts(&bat, &repo, &store)).expect_err("untracked = dirty");
    assert!(err.to_string().contains("dirty"), "{err}");
    assert!(err.to_string().contains("stray.txt"), "{err}");
}

#[test]
fn undeclared_capability_is_invalid_before_any_work() {
    let dir = TempDir::new("run-undeclared");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    // command.run without process.spawn declared.
    let yaml = "version: terrorbat/v1\n\
                id: bad-caps\n\
                claim:\n  text: claim\n\
                requires:\n  - fs.write\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: git\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "bad.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run records Invalid");
    assert_eq!(out.manifest.run_status, RunStatus::Invalid);
    assert!(out.manifest.worktree.path.is_none(), "no worktree created");
    assert!(matches!(
        out.manifest.worktree.cleanup.status,
        CleanupStatus::NotAttempted
    ));
    let err = out.manifest.steps.first().map(|s| &s.error);
    let _ = err;
    assert_eq!(exit_code_for(out.manifest.run_status, None), 2);
}

#[test]
fn self_forbidden_capability_is_policy_denied() {
    let dir = TempDir::new("run-forbidden");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: self-forbid\n\
                claim:\n  text: claim\n\
                requires:\n  - process.spawn\n\
                forbids:\n  - process.spawn\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: git\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "forbid.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run records PolicyDenied");
    assert_eq!(out.manifest.run_status, RunStatus::PolicyDenied);
    assert_eq!(exit_code_for(out.manifest.run_status, None), 2);
}

#[test]
fn unknown_adapter_action_is_invalid() {
    let dir = TempDir::new("run-unknown");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: unknown-adapter\n\
                claim:\n  text: claim\n\
                requires:\n  - process.spawn\n\
                attack:\n  run:\n\
                \x20   - adapter: nope\n\
                \x20     action: whatever\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "unknown.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run records Invalid");
    assert_eq!(out.manifest.run_status, RunStatus::Invalid);
}

#[test]
fn timeout_terminates_step_and_cleans_worktree() {
    let dir = TempDir::new("run-timeout");
    let repo = dir.join("repo");
    let head = make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let hang = env!("CARGO_BIN_EXE_tb_probe_hang");
    let yaml = format!(
        "version: terrorbat/v1\n\
         id: timeout-bat\n\
         claim:\n  text: claim\n\
         requires:\n  - process.spawn\n\
         attack:\n  run:\n\
         \x20   - adapter: command\n\
         \x20     action: run\n\
         \x20     program: {}\n\
         oracle:\n  all: []\n\
         evidence:\n  capture: [stdout]\n\
         timeout:\n  run: 1s\n  total: 30s\n",
        yaml_path(Path::new(hang))
    );
    let bat = write_spec(&specs, "timeout.yaml", &yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run records TimedOut");
    let m = &out.manifest;
    assert_eq!(m.run_status, RunStatus::TimedOut);
    let step = m
        .steps
        .iter()
        .find(|s| s.adapter == "command")
        .expect("step");
    assert_eq!(step.status, RunStatus::TimedOut);
    assert!(matches!(
        m.worktree.cleanup.status,
        CleanupStatus::Succeeded
    ));
    assert!(!PathBuf::from(m.worktree.path.as_ref().unwrap()).exists());
    // Source repo untouched.
    assert!(repo_is_clean(&repo));
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(exit_code_for(m.run_status, None), 3);
}

#[test]
fn nonzero_exit_continues_and_oracle_owns_meaning() {
    let dir = TempDir::new("run-nonzero");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    // Step 0 exits nonzero (Completed!), step 1 must still run.
    let yaml = "version: terrorbat/v1\n\
                id: nonzero-continues\n\
                claim:\n  text: claim\n\
                requires:\n  - process.spawn\n  - fs.write\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: git\n\
                \x20     args: [rev-parse, definitely-not-a-real-ref]\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: marker.txt\n\
                \x20     text: reached\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "nonzero.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let m = &out.manifest;
    assert_eq!(m.run_status, RunStatus::Completed);
    let s0 = &m.steps[0];
    assert_eq!(s0.status, RunStatus::Completed);
    assert!(
        s0.exit_code.is_some_and(|c| c != 0),
        "nonzero exit recorded"
    );
    assert_eq!(
        m.steps[1].status,
        RunStatus::Completed,
        "later step still ran"
    );
}

#[test]
fn path_escape_attempt_is_policy_denied_and_halts() {
    let dir = TempDir::new("run-escape");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: escape-attempt\n\
                claim:\n  text: claim\n\
                requires:\n  - fs.write\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: ../../escaped.txt\n\
                \x20     text: nope\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: never-reached.txt\n\
                \x20     text: nope\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "escape.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let m = &out.manifest;
    assert_eq!(m.run_status, RunStatus::PolicyDenied);
    assert_eq!(m.steps[0].status, RunStatus::PolicyDenied);
    let err = m.steps[0].error.clone().unwrap_or_default();
    assert!(err.contains("escapes the worktree"), "{err}");
    assert_eq!(m.steps.len(), 1, "halted before the next step");
    // Nothing escaped next to the temp root.
    assert!(!dir.join("escaped.txt").exists());
}

#[test]
fn evidence_survives_a_later_step_failure() {
    let dir = TempDir::new("run-survive");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let hang = env!("CARGO_BIN_EXE_tb_probe_hang");
    let yaml = format!(
        "version: terrorbat/v1\n\
         id: evidence-survival\n\
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
    let m = &out.manifest;
    assert_eq!(m.run_status, RunStatus::TimedOut);
    // The FIRST step's evidence was finalised before the second step hung
    // and was killed; it must still be readable after the run.
    let first = &m.steps[0];
    assert_eq!(first.status, RunStatus::Completed);
    let ev = EvidenceStore::open(&store).expect("store");
    let stdout = ev
        .get(first.stdout.as_ref().expect("stdout ref"))
        .expect("readable");
    assert!(
        String::from_utf8_lossy(&stdout).contains("git version"),
        "prior evidence must survive later worker failure"
    );
}

#[test]
fn repo_path_with_spaces_and_unicode_works() {
    let dir = TempDir::new("run-unicode");
    let nested = dir.join("ff repo ünicode 中文");
    let repo = nested.join("proj");
    make_repo(&repo);
    let store = dir.join("store ünicode");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "protected.yaml", &mutation_spec(None));
    let out = run_bat(&opts(&bat, &repo, &store)).expect("unicode path run");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert!(repo_is_clean(&repo));
}

#[test]
fn two_runs_do_not_contaminate_each_other() {
    let dir = TempDir::new("run-twice");
    let repo = dir.join("repo");
    let head = make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "protected.yaml", &mutation_spec(None));
    let a = run_bat(&opts(&bat, &repo, &store)).expect("run a");
    let b = run_bat(&opts(&bat, &repo, &store)).expect("run b");
    assert_ne!(a.execution_id, b.execution_id);
    assert_eq!(a.manifest.run_status, RunStatus::Completed);
    assert_eq!(b.manifest.run_status, RunStatus::Completed);
    assert_eq!(a.manifest.target.commit, head);
    assert_eq!(b.manifest.target.commit, head);
    // Both run dirs exist independently.
    let ev = EvidenceStore::open(&store).expect("store");
    let runs = ev.list_runs();
    assert!(runs.contains(&a.execution_id));
    assert!(runs.contains(&b.execution_id));
    assert!(repo_is_clean(&repo));
}

#[test]
fn manifest_json_is_wellformed_and_reloadable() {
    let dir = TempDir::new("run-manifest");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "protected.yaml", &mutation_spec(None));
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let manifest_path = PathBuf::from(&out.manifest.run_dir).join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).expect("manifest text");
    let value: serde_json::Value = serde_json::from_str(&text).expect("manifest parses");
    assert_eq!(value["version"], "terrorbat/run/v0");
    assert_eq!(value["execution_id"], out.execution_id);
    let reloaded = terrorbat::runner::load_manifest(&PathBuf::from(&out.manifest.run_dir))
        .expect("reload manifest");
    assert_eq!(reloaded.run_status, RunStatus::Completed);
    assert_eq!(reloaded.bat.bat_sha, out.manifest.bat.bat_sha);
    // Bat identity matches the identity pipeline for the same spec.
    let identified = terrorbat::identify_spec_file(&bat, &terrorbat::ParamOverrides::default())
        .expect("identify");
    assert_eq!(reloaded.bat.bat_sha, identified.identities.bat);
}
