//! M8 external adapter integration and protocol failure regressions.
mod common;

use common::{TempDir, make_repo, repo_is_clean, write_spec, yaml_path};
use std::path::{Path, PathBuf};
use terrorbat::{
    adapter,
    campaign::{self, CampaignOptions},
    evidence::EvidenceStore,
    receipt::{self, Verdict},
    runner::{self, RunOptions, RunStatus},
};

const FIXTURE: &str = env!("CARGO_BIN_EXE_tb_adapter_fixture");

struct Fx {
    _dir: TempDir,
    repo: PathBuf,
    store: PathBuf,
    specs: PathBuf,
}
fn fx(tag: &str) -> Fx {
    let dir = TempDir::new(tag);
    let repo = dir.join("repo");
    make_repo(&repo);
    Fx {
        store: dir.join("store"),
        specs: dir.join("specs"),
        repo,
        _dir: dir,
    }
}
fn bat(f: &Fx, name: &str, action: &str, mode: &str, oracle: &str, timeout: &str) -> PathBuf {
    let yaml = format!(
        "version: terrorbat/v1\nid: {name}\nclaim:\n  text: External adapter output is judged by the deterministic oracle.\nrequires: [fs.write, process.spawn, git.inspect]\nattack:\n  run:\n    - adapter: fixture\n      action: {action}\n      mode: {mode}\n{timeout}oracle:\n{oracle}\nevidence:\n  capture: [git_diff, stdout, stderr]\n"
    );
    write_spec(&f.specs, name, &yaml)
}
fn binding(f: &Fx, mode: &str) -> PathBuf {
    std::fs::create_dir_all(&f.specs).expect("create spec directory");
    let path = f.specs.join("adapters.yaml");
    let arg = if mode.is_empty() {
        String::new()
    } else {
        format!("    args: [{mode}]\n")
    };
    std::fs::write(
        &path,
        format!(
            "version: terrorbat-adapters/v1\nadapters:\n  fixture:\n    program: {}\n{arg}",
            yaml_path(Path::new(FIXTURE))
        ),
    )
    .expect("write bindings");
    path
}
fn run(f: &Fx, bat: &Path, bindings: Option<&Path>) -> runner::RunOutput {
    let opts = RunOptions {
        bat_path: bat.to_path_buf(),
        repo: f.repo.clone(),
        store_root: Some(f.store.clone()),
        overrides: Vec::new(),
    };
    let loaded = adapter::load_bindings(bindings).expect("bindings parse");
    runner::run_bat_with_adapters(&opts, loaded.as_ref()).expect("run is persisted")
}
const QUIET: &str = "  all: []\n";
#[test]
fn replay_requires_explicit_bindings_and_rejects_description_mismatch() {
    let f = fx("adapter-replay");
    let b = bat(&f, "replay.yaml", "mutate", "mutate", QUIET, "");
    let bind = binding(&f, "");
    let original = run(&f, &b, Some(&bind));
    let missing =
        receipt::replay_with_adapters(&original.receipt.receipt_id, Some(f.store.clone()), None)
            .expect_err("external replay requires explicit bindings");
    assert!(missing.to_string().contains("explicit `--adapters <file>`"));
    let mismatch = binding(&f, "description-change");
    let refused = receipt::replay_with_adapters(
        &original.receipt.receipt_id,
        Some(f.store.clone()),
        Some(&mismatch),
    )
    .expect_err("changed description must refuse replay");
    assert!(refused.to_string().contains("identity mismatch"));
    assert!(repo_is_clean(&f.repo));
}

#[test]
fn describe_stderr_is_recorded_separately_as_evidence() {
    let f = fx("adapter-describe-diagnostic");
    let b = bat(&f, "describe-diag.yaml", "observe", "logical", QUIET, "");
    let bind = binding(&f, "describe-diagnostic");
    let out = run(&f, &b, Some(&bind));
    let step = &out.manifest.steps[0];
    let reference = step
        .adapter_provenance
        .as_ref()
        .unwrap()
        .describe_stderr
        .as_ref()
        .expect("describe diagnostic reference");
    let store = EvidenceStore::open(&f.store).unwrap();
    assert_eq!(
        store.get(reference).unwrap(),
        b"fixture describe diagnostic\n"
    );
}

#[test]
fn valid_mutation_outputs_diagnostics_provenance_and_ordinary_proof() {
    let f = fx("adapter-mutate");
    let b = bat(
        &f,
        "mutate.yaml",
        "mutate",
        "mutate",
        "  any:\n    - type: git_diff_contains\n      substring: written by external adapter\n",
        "",
    );
    let bind = binding(&f, "");
    let out = run(&f, &b, Some(&bind));
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    let step = &out.manifest.steps[0];
    assert_eq!(
        step.adapter_provenance.as_ref().unwrap().protocol,
        adapter::PROTOCOL
    );

    assert!(step.stderr.as_ref().is_some());
    let store = EvidenceStore::open(&f.store).unwrap();
    assert_eq!(store.get(step.stderr.as_ref().unwrap()).unwrap(), b"");
    assert!(repo_is_clean(&f.repo));
}

#[test]
fn logical_nonzero_exit_is_completed_and_the_oracle_decides() {
    let f = fx("adapter-logical-exit");
    let b = bat(
        &f,
        "logical.yaml",
        "observe",
        "nonzero",
        "  any:\n    - type: exit_code\n      step: run:0\n      equals: 17\n",
        "",
    );
    let bind = binding(&f, "");
    let out = run(&f, &b, Some(&bind));
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.manifest.steps[0].exit_code, Some(17));
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    let store = EvidenceStore::open(&f.store).unwrap();
    let step = &out.manifest.steps[0];
    assert_eq!(
        store.get(step.stdout.as_ref().unwrap()).unwrap(),
        b"logical command failed"
    );
    assert_eq!(store.get(step.stderr.as_ref().unwrap()).unwrap(), b"");
}

#[test]
fn missing_binding_unknown_action_and_capability_fail_before_worktree() {
    let f = fx("adapter-preflight");
    let b = bat(&f, "unknown.yaml", "unknown", "logical", QUIET, "");
    let no_binding = run(&f, &b, None);
    assert_eq!(no_binding.manifest.run_status, RunStatus::Invalid);
    assert!(no_binding.manifest.worktree.path.is_none());
    assert!(
        no_binding
            .manifest
            .limitations
            .iter()
            .any(|note| note.contains("explicit --adapters binding file"))
    );
    let bind = binding(&f, "");
    let unknown = run(&f, &b, Some(&bind));
    assert_eq!(unknown.manifest.run_status, RunStatus::Invalid);
    assert!(unknown.manifest.worktree.path.is_none());
    let missing = bat(&f, "missing-cap.yaml", "mutate", "mutate", QUIET, "");
    let text = std::fs::read_to_string(&missing).unwrap().replace(
        "requires: [fs.write, process.spawn, git.inspect]",
        "requires: [git.inspect]",
    );
    std::fs::write(&missing, text).unwrap();
    let out = run(&f, &missing, Some(&bind));
    assert_eq!(out.manifest.run_status, RunStatus::Invalid);
    assert!(out.manifest.worktree.path.is_none());
    assert!(repo_is_clean(&f.repo));
}

#[test]
fn explicitly_forbidden_capability_is_policy_denied_before_mutation() {
    let f = fx("adapter-forbid");
    let b = bat(&f, "forbid.yaml", "mutate", "mutate", QUIET, "");
    let text = std::fs::read_to_string(&b)
        .unwrap()
        .replace("attack:\n", "forbids: [fs.write]\nattack:\n");
    std::fs::write(&b, text).unwrap();
    let out = run(&f, &b, Some(&binding(&f, "")));
    assert_eq!(out.manifest.run_status, RunStatus::PolicyDenied);
    assert!(out.manifest.worktree.path.is_none());
}

#[test]
fn describe_protocol_name_and_missing_program_fail_as_infrastructure() {
    let f = fx("adapter-describe-fail");
    let b = bat(&f, "describe.yaml", "mutate", "mutate", QUIET, "");
    for mode in [
        "version-mismatch",
        "name-mismatch",
        "describe-crash",
        "describe-hang",
    ] {
        let out = run(&f, &b, Some(&binding(&f, mode)));
        assert_eq!(
            out.manifest.run_status,
            RunStatus::InfrastructureError,
            "mode {mode}"
        );
        assert!(out.manifest.worktree.path.is_none());
    }
    let missing = f.specs.join("missing.yaml");
    std::fs::write(&missing,"version: terrorbat-adapters/v1\nadapters:\n  fixture:\n    program: missing-terrorbat-adapter-executable\n").unwrap();
    let out = run(&f, &b, Some(&missing));
    assert_eq!(out.manifest.run_status, RunStatus::InfrastructureError);
    assert!(out.manifest.worktree.path.is_none());
}

#[test]
fn malformed_multiple_and_extra_protocol_stdout_fail_closed() {
    let f = fx("adapter-corruption");
    let b = bat(&f, "corrupt.yaml", "observe", "logical", QUIET, "");
    let bind = binding(&f, "");
    for mode in ["malformed", "multiple", "extra-output", "version-mismatch"] {
        let path = bat(&f, &format!("{mode}.yaml"), "observe", mode, QUIET, "");
        let out = run(&f, &path, Some(&bind));
        assert_eq!(
            out.manifest.run_status,
            RunStatus::InfrastructureError,
            "mode {mode}"
        );
        assert_ne!(out.receipt.verdict, Verdict::Proven);
    }
    assert_eq!(
        run(&f, &b, Some(&bind)).manifest.run_status,
        RunStatus::Completed
    );
}

#[test]
fn adapter_stderr_is_separate_and_execute_timeout_is_never_proven() {
    let f = fx("adapter-stderr-timeout");
    let bind = binding(&f, "");
    let diagnostic = bat(&f, "diagnostic.yaml", "observe", "diagnostic", QUIET, "");
    let out = run(&f, &diagnostic, Some(&bind));
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    let step = &out.manifest.steps[0];
    let store = EvidenceStore::open(&f.store).unwrap();
    assert_eq!(
        store.get(step.protocol_stderr.as_ref().unwrap()).unwrap(),
        b"fixture protocol diagnostic\n"
    );
    let timeout = bat(
        &f,
        "timeout.yaml",
        "observe",
        "hang",
        QUIET,
        "timeout:\n  run: 1s\n",
    );
    let out = run(&f, &timeout, Some(&bind));
    assert_eq!(out.manifest.run_status, RunStatus::TimedOut);
    assert_ne!(out.receipt.verdict, Verdict::Proven);
    assert!(repo_is_clean(&f.repo));
}

#[test]
fn campaign_runs_external_adapter_as_an_ordinary_child() {
    let f = fx("adapter-campaign");
    bat(
        &f,
        "campaign-bat.yaml",
        "mutate",
        "mutate",
        "  any:\n    - type: git_diff_contains\n      substring: written by external adapter\n",
        "",
    );
    let bind = binding(&f, "");
    let pack = f.specs.join("pack.yaml");
    std::fs::write(
        &pack,
        "version: terrorbat-pack/v1\nid: external\nbats:\n  - path: campaign-bat.yaml\n",
    )
    .unwrap();
    let opts = CampaignOptions {
        pack_path: pack,
        repo: f.repo.clone(),
        store_root: Some(f.store.clone()),
        runs: 1,
        stop_on_proven: false,
    };
    let out = campaign::run_campaign_with_adapters(&opts, Some(&bind)).expect("campaign");
    assert_eq!(out.campaign.children.len(), 1);
    assert_eq!(out.campaign.children[0].verdict, Verdict::Proven);
    assert!(repo_is_clean(&f.repo));
}

#[test]
fn adapter_reported_statuses_and_process_crash_never_falsify_claims() {
    let f = fx("adapter-statuses");
    for (mode, expected) in [
        ("invalid", RunStatus::Invalid),
        ("policy-denied", RunStatus::PolicyDenied),
        ("infra", RunStatus::InfrastructureError),
    ] {
        let spec = bat(&f, &format!("{mode}.yaml"), "observe", mode, QUIET, "");
        let out = run(&f, &spec, Some(&binding(&f, "")));
        assert_eq!(out.manifest.run_status, expected, "mode {mode}");
        assert_ne!(out.receipt.verdict, Verdict::Proven, "mode {mode}");
    }

    let spec = bat(&f, "crash.yaml", "observe", "logical", QUIET, "");
    let out = run(&f, &spec, Some(&binding(&f, "execute-crash")));
    assert_eq!(out.manifest.run_status, RunStatus::InfrastructureError);
    assert_ne!(out.receipt.verdict, Verdict::Proven);
    assert!(repo_is_clean(&f.repo));
}

#[test]
fn adapter_bindings_reject_unknown_fields_duplicate_names_and_reserved_builtins() {
    let f = fx("adapter-binding-schema");
    std::fs::create_dir_all(&f.specs).unwrap();
    let path = f.specs.join("adapters.yaml");
    for (label, yaml) in [
        (
            "unknown",
            "version: terrorbat-adapters/v1\nadapters:\n  external:\n    program: adapter\n    environment: forbidden\n",
        ),
        (
            "duplicate",
            "version: terrorbat-adapters/v1\nadapters:\n  external:\n    program: first\n  external:\n    program: second\n",
        ),
        (
            "reserved",
            "version: terrorbat-adapters/v1\nadapters:\n  fs:\n    program: adapter\n",
        ),
    ] {
        std::fs::write(&path, yaml).unwrap();
        assert!(
            adapter::load_bindings(Some(&path)).is_err(),
            "{label} binding accepted"
        );
    }
}
