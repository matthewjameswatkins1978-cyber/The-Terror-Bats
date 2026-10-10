//! M8 external adapter integration and protocol failure regressions.
mod common;

use common::{TempDir, make_repo, repo_is_clean, write_spec, yaml_path};
use std::path::{Path, PathBuf};
use terrorbats::{
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

fn assert_protocol_overflow(mode: &str, stream: &str, byte: u8) {
    let f = fx(mode);
    let b = bat(
        &f,
        "overflow.yaml",
        "observe",
        mode,
        QUIET,
        "timeout:\n  run: 10s\n",
    );
    let bind = binding(&f, "");
    let out = run(&f, &b, Some(&bind));
    assert_eq!(out.manifest.run_status, RunStatus::InfrastructureError);
    assert_ne!(out.receipt.verdict, Verdict::Proven);
    let step = &out.manifest.steps[0];
    let (reference, total, truncated) = if stream == "stdout" {
        (
            step.protocol_stdout.as_ref().unwrap(),
            step.protocol_stdout_total_bytes,
            step.protocol_stdout_truncated,
        )
    } else {
        (
            step.protocol_stderr.as_ref().unwrap(),
            step.protocol_stderr_total_bytes,
            step.protocol_stderr_truncated,
        )
    };
    assert_eq!(total, 1_048_576 + 37);
    assert!(truncated);
    let store = EvidenceStore::open(&f.store).unwrap();
    assert_eq!(store.get(reference).unwrap(), vec![byte; 1_048_576]);
    let evidence = out
        .receipt
        .evidence
        .iter()
        .find(|item| item.kind.ends_with(&format!(":protocol_{stream}")))
        .unwrap();
    assert!(evidence.truncated);
    assert_eq!(&evidence.reference, reference);
    assert!(
        out.receipt
            .limitations
            .iter()
            .any(|note| note.contains(&format!(
                "protocol {stream} truncated: 1048576 of 1048613 bytes retained"
            )))
    );
    let loaded = receipt::load_receipt(&out.run_dir).unwrap();
    assert_eq!(
        loaded.execution.steps[0].protocol_stdout_total_bytes,
        step.protocol_stdout_total_bytes
    );
    assert_eq!(
        loaded.execution.steps[0].protocol_stderr_total_bytes,
        step.protocol_stderr_total_bytes
    );
    assert!(repo_is_clean(&f.repo));
}

#[test]
fn protocol_stdout_overflow_retains_honest_evidence() {
    assert_protocol_overflow("stdout-overflow", "stdout", b'x');
}

#[test]
fn protocol_stderr_overflow_retains_honest_evidence() {
    assert_protocol_overflow("stderr-overflow", "stderr", b'y');
}

#[test]
fn truncated_protocol_stdout_retains_raw_evidence_and_fails_closed() {
    let f = fx("protocol-success-overflow");
    let b = bat(
        &f,
        "success-overflow.yaml",
        "observe",
        "valid-stdout-overflow",
        QUIET,
        "timeout:\n  run: 10s\n",
    );
    let bind = binding(&f, "");
    let out = run(&f, &b, Some(&bind));
    assert_eq!(out.manifest.run_status, RunStatus::InfrastructureError);
    let step = &out.manifest.steps[0];
    assert_eq!(step.protocol_stdout_total_bytes, 1_048_576 + 37);
    assert!(step.protocol_stdout_truncated);
    let reference = step
        .protocol_stdout
        .as_ref()
        .expect("raw protocol stdout retained");
    let store = EvidenceStore::open(&f.store).expect("evidence store");
    let raw = store
        .get(reference)
        .expect("retained protocol bytes readable");
    assert_eq!(raw.len(), 1_048_576);
    assert_eq!(raw.last(), Some(&b'\n'));
    let evidence = out
        .receipt
        .evidence
        .iter()
        .find(|item| item.kind.ends_with(":protocol_stdout"))
        .expect("protocol stdout evidence exists");
    assert!(evidence.truncated);
    assert_eq!(&evidence.reference, reference);
    assert!(
        out.receipt
            .limitations
            .iter()
            .any(|note| note
                .contains("protocol stdout truncated: 1048576 of 1048613 bytes retained"))
    );
    assert!(repo_is_clean(&f.repo));
}
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

fn named_campaign_bat(f: &Fx, name: &str, adapter_name: &str) -> PathBuf {
    let (requires, action, payload) = if adapter_name == "fs" {
        (
            "[fs.write, git.inspect]",
            "write",
            format!("      path: notes/{name}.txt\n      text: campaign fixture\n"),
        )
    } else {
        ("[process.spawn, git.inspect]", "observe", String::new())
    };
    write_spec(
        &f.specs,
        name,
        &format!(
            "version: terrorbat/v1\nid: {name}\nclaim:\n  text: Adapter provenance remains pinned across this campaign.\nrequires: {requires}\nattack:\n  run:\n    - adapter: {adapter_name}\n      action: {action}\n{payload}oracle:\n  all: []\nevidence:\n  capture: [git_diff]\n"
        ),
    )
}

fn heterogeneous_pack(f: &Fx) -> PathBuf {
    let entries = [
        ("first.yaml", "alpha"),
        ("builtin.yaml", "fs"),
        ("third.yaml", "beta"),
        ("again.yaml", "alpha"),
    ];
    for (file, adapter_name) in entries {
        named_campaign_bat(f, file, adapter_name);
    }
    let pack = f.specs.join("heterogeneous-pack.yaml");
    std::fs::write(
        &pack,
        "version: terrorbat-pack/v1\nid: heterogeneous\nbats:\n  - path: first.yaml\n  - path: builtin.yaml\n  - path: third.yaml\n  - path: again.yaml\n",
    )
    .expect("write heterogeneous pack");
    pack
}

fn named_bindings(
    f: &Fx,
    alpha_mode: &str,
    alpha_state: Option<&Path>,
    beta_mode: &str,
) -> PathBuf {
    let alpha_args = match alpha_state {
        Some(path) => format!("    args: [{alpha_mode}, {}]\n", yaml_path(path)),
        None => format!("    args: [{alpha_mode}]\n"),
    };
    let path = f.specs.join("named-adapters.yaml");
    std::fs::write(
        &path,
        format!(
            "version: terrorbat-adapters/v1\nadapters:\n  alpha:\n    program: {}\n{alpha_args}  beta:\n    program: {}\n    args: [{beta_mode}]\n",
            yaml_path(Path::new(FIXTURE)),
            yaml_path(Path::new(FIXTURE)),
        ),
    )
    .expect("write named adapter bindings");
    path
}

#[test]
fn campaigns_allow_heterogeneous_adapters_and_reject_repeated_name_drift() {
    let f = fx("adapter-campaign-heterogeneous");
    let pack = heterogeneous_pack(&f);
    let opts = CampaignOptions {
        pack_path: pack,
        repo: f.repo.clone(),
        store_root: Some(f.store.clone()),
        runs: 1,
        stop_on_proven: false,
    };
    let bindings = named_bindings(&f, "alpha", None, "beta");
    let campaign = campaign::run_campaign_with_adapters(&opts, Some(&bindings))
        .expect("A -> built-in -> B -> A should succeed");
    assert_eq!(campaign.campaign.children.len(), 4);
    let store = EvidenceStore::open(&f.store).expect("store");
    let built_in = store.run_dir(&campaign.campaign.children[1].execution_id);
    let built_in_receipt = receipt::load_receipt(&built_in).expect("built-in child receipt");
    let value = serde_json::to_value(&built_in_receipt).expect("serialise receipt");
    assert!(!value.as_object().unwrap().contains_key("resolved_adapters"));
    for step in value["execution"]["steps"].as_array().unwrap() {
        for field in [
            "protocol_stdout_total_bytes",
            "protocol_stdout_truncated",
            "protocol_stderr_total_bytes",
            "protocol_stderr_truncated",
        ] {
            assert!(
                !step.as_object().unwrap().contains_key(field),
                "built-in step serialized {field}"
            );
        }
    }
    assert!(
        !value
            .as_object()
            .unwrap()
            .contains_key("adapter_bindings_required")
    );

    let drift = fx("adapter-campaign-drift");
    let pack = heterogeneous_pack(&drift);
    let opts = CampaignOptions {
        pack_path: pack,
        repo: drift.repo.clone(),
        store_root: Some(drift.store.clone()),
        runs: 1,
        stop_on_proven: false,
    };
    let state = drift._dir.join("alpha-describe-count");
    let bindings = named_bindings(&drift, "alpha-drift", Some(&state), "beta");
    let error = campaign::run_campaign_with_adapters(&opts, Some(&bindings))
        .expect_err("the repeated alpha description must remain pinned");
    assert!(
        error.to_string().contains("description identity changed"),
        "{error}"
    );
    assert!(repo_is_clean(&f.repo));
    assert!(repo_is_clean(&drift.repo));
}

#[test]
fn trusted_campaign_load_rejects_rehashed_conflicting_adapter_receipts() {
    let f = fx("adapter-campaign-load-conflict");
    let bat = named_campaign_bat(&f, "alpha.yaml", "alpha");
    let pack = f.specs.join("pack.yaml");
    std::fs::write(
        &pack,
        "version: terrorbat-pack/v1\nid: alpha\nbats:\n  - path: alpha.yaml\n",
    )
    .unwrap();
    let opts = CampaignOptions {
        pack_path: pack,
        repo: f.repo.clone(),
        store_root: Some(f.store.clone()),
        runs: 1,
        stop_on_proven: false,
    };
    let binding_v1 = named_bindings(&f, "alpha", None, "beta");
    let base =
        campaign::run_campaign_with_adapters(&opts, Some(&binding_v1)).expect("base campaign");
    let binding_v2 = named_bindings(&f, "alpha-v2", None, "beta");
    let second = run(&f, &bat, Some(&binding_v2));
    let live = receipt::load_receipt(&second.run_dir).expect("second receipt is genuine");
    assert_ne!(
        base.campaign.children[0].receipt_id, live.receipt_id,
        "the conflicting child is a separate verified receipt"
    );
    let mut forged = base.campaign.clone();
    forged.children.push(campaign::CampaignChildRecord {
        iteration: 1,
        entry_index: 1,
        bat: live.bat.bat_sha.clone(),
        bat_id: live.bat.id.clone(),
        execution_id: live.execution_id.clone(),
        receipt_id: live.receipt_id.clone(),
        status: live.execution.status,
        oracle: live.oracle.result,
        verdict: live.verdict,
    });
    forged.summary = campaign::summarize(&forged.children);
    let forged = campaign::finalise(forged).expect("rehash aggregate");
    campaign::write_campaign(&base.campaign_dir, &forged).expect("write rehashed aggregate");
    let store = EvidenceStore::open(&f.store).expect("store");
    let error = campaign::load_campaign(&store, &base.campaign_dir)
        .expect_err("trusted load must recompute adapter consistency");
    assert!(error.to_string().contains("TB-CAMPAIGN-CORRUPT"), "{error}");
    assert!(
        error
            .to_string()
            .contains("conflicting description identities"),
        "{error}"
    );
}

#[test]
fn replay_checks_resolved_adapter_identity_when_external_step_never_executes() {
    let f = fx("adapter-replay-preflight-only");
    let bat = write_spec(
        &f.specs,
        "preflight-only.yaml",
        "version: terrorbat/v1\nid: preflight-only\nclaim:\n  text: Resolved adapters remain part of replay identity.\nrequires: [process.spawn, git.inspect]\nattack:\n  setup:\n    - adapter: command\n      action: run\n      program: terrorbat-intentionally-missing-setup-command\n  run:\n    - adapter: fixture\n      action: observe\noracle:\n  all: []\nevidence:\n  capture: [git_diff]\n",
    );
    let original = run(&f, &bat, Some(&binding(&f, "")));
    assert_eq!(original.manifest.run_status, RunStatus::InfrastructureError);
    assert_eq!(original.receipt.resolved_adapters.len(), 1);
    assert!(
        original
            .receipt
            .execution
            .steps
            .iter()
            .all(|step| { step.adapter != "fixture" || step.adapter_provenance.is_none() })
    );
    let changed = binding(&f, "description-change");
    let error = receipt::replay_with_adapters(
        &original.receipt.receipt_id,
        Some(f.store.clone()),
        Some(&changed),
    )
    .expect_err("preflight-resolved identity drift must refuse replay");
    assert!(error.to_string().contains("identity mismatch"), "{error}");
    assert_eq!(
        common::git(&f.repo, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
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
