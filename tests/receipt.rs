//! M6 receipt + replay tests: three-layer honesty, hard mapping rules,
//! receipt identity, replay immutability, and failure-mode receipts.

mod common;

use std::path::Path;

use common::{TempDir, make_repo, repo_is_clean, write_spec, yaml_path};
use terrorbat::evidence::EvidenceStore;
use terrorbat::oracle::OracleResult;
use terrorbat::receipt::{self, Verdict};
use terrorbat::runner::{RunOptions, RunStatus, exit_code_for, load_manifest, run_bat};

fn opts(bat: &Path, repo: &Path, store: &Path) -> RunOptions {
    RunOptions {
        bat_path: bat.to_path_buf(),
        repo: repo.to_path_buf(),
        store_root: Some(store.to_path_buf()),
        overrides: Vec::new(),
    }
}

/// A Bat whose attack mutates a protected path; the oracle detects it.
fn falsifying_bat() -> String {
    "version: terrorbat/v1\n\
     id: protected-config\n\
     claim:\n  text: The protected config is never modified by ordinary work.\n\
     requires:\n  - fs.write\n  - git.inspect\n\
     attack:\n\
     \x20 run:\n\
     \x20   - adapter: fs\n\
     \x20     action: write\n\
     \x20     path: config/security.toml\n\
     \x20     text: \"pwned = true\\n\"\n\
     oracle:\n\
     \x20 any:\n\
     \x20   - type: git_diff_contains\n\
     \x20     substring: pwned\n\
     \x20   - type: path_changed\n\
     \x20     path: config/security.toml\n\
     evidence:\n  capture: [git_diff]\n"
        .to_string()
}

/// Same shape, but the attack touches a different path and the oracle looks
/// for a mutation that never happens.
fn non_falsifying_bat() -> String {
    "version: terrorbat/v1\n\
     id: protected-config-quiet\n\
     claim:\n  text: The protected config is never modified by ordinary work.\n\
     requires:\n  - fs.write\n  - git.inspect\n\
     attack:\n\
     \x20 run:\n\
     \x20   - adapter: fs\n\
     \x20     action: write\n\
     \x20     path: notes/observation.txt\n\
     \x20     text: \"benign note\\n\"\n\
     oracle:\n\
     \x20 any:\n\
     \x20   - type: git_diff_contains\n\
     \x20     substring: never-present-text\n\
     \x20   - type: path_changed\n\
     \x20     path: config/security.toml\n\
     evidence:\n  capture: [git_diff]\n"
        .to_string()
}

#[test]
fn falsification_flow_produces_proven_receipt() {
    let dir = TempDir::new("rc-proven");
    let repo = dir.join("repo");
    let head = make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "falsify.yaml", &falsifying_bat());

    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let r = &out.receipt;

    // Three separate layers, correctly composed.
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(r.execution.status, RunStatus::Completed);
    assert_eq!(r.oracle.result, Some(OracleResult::Falsified));
    assert_eq!(r.verdict, Verdict::Proven);
    assert_eq!(
        exit_code_for(out.manifest.run_status, Some(r.verdict.as_str())),
        1
    );

    // Receipt identity and durability.
    assert!(
        r.receipt_id.starts_with("receipt:sha256:"),
        "{}",
        r.receipt_id
    );
    assert_eq!(r.receipt_id.len(), "receipt:sha256:".len() + 64);
    assert!(out.run_dir.join("receipt.json").exists());
    let reloaded = receipt::load_receipt(&out.run_dir).expect("reload");
    assert_eq!(reloaded.receipt_id, r.receipt_id);

    // Manifest carries the same verdict and the receipt id.
    let manifest = load_manifest(&out.run_dir).expect("manifest");
    assert_eq!(manifest.verdict, Some(Verdict::Proven));
    assert_eq!(manifest.receipt_id.as_deref(), Some(r.receipt_id.as_str()));

    // Claim/target/reproduction facts.
    assert!(r.claim_text.contains("protected config"));
    assert_eq!(r.target.commit, head);
    assert_eq!(r.reproduction.commit, head);
    assert!(r.reproduction.replay_command.contains("terrorbat replay"));

    // Human rendering: honest about PROVEN's scope and isolation.
    let rendered = receipt::render(r);
    assert!(rendered.contains("🦇 protected-config"));
    assert!(rendered.contains("PROVEN"));
    assert!(rendered.contains("not a universal proof"), "{rendered}");
    assert!(rendered.contains("UNENFORCED"));
    assert!(rendered.contains("NOT hostile-code containment"));
    assert!(!rendered.to_lowercase().contains("sandboxed"));

    // Source repo untouched.
    assert!(repo_is_clean(&repo));
}

#[test]
fn not_observed_is_never_correctness() {
    let dir = TempDir::new("rc-notobs");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "quiet.yaml", &non_falsifying_bat());

    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let r = &out.receipt;
    assert_eq!(r.execution.status, RunStatus::Completed);
    assert_eq!(r.oracle.result, Some(OracleResult::NotFalsified));
    assert_eq!(r.verdict, Verdict::NotObserved);
    assert_eq!(
        exit_code_for(out.manifest.run_status, Some(r.verdict.as_str())),
        0
    );

    let rendered = receipt::render(r);
    assert!(rendered.contains("NOT OBSERVED"));
    assert!(
        rendered.contains("NOT a correctness certificate"),
        "{rendered}"
    );
    assert!(!rendered.contains("PROVEN"));
}

#[test]
fn timeout_is_inconclusive_never_falsification() {
    let dir = TempDir::new("rc-timeout");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let hang = env!("CARGO_BIN_EXE_tb_probe_hang");
    let yaml = format!(
        "version: terrorbat/v1\n\
         id: timeout-verdict\n\
         claim:\n  text: claim\n\
         requires:\n  - process.spawn\n\
         attack:\n  run:\n\
         \x20   - adapter: command\n\
         \x20     action: run\n\
         \x20     program: {}\n\
         oracle:\n\
         \x20 all:\n\
         \x20   - type: file_exists\n\
         \x20     path: anything.txt\n\
         evidence:\n  capture: [stdout]\n\
         timeout:\n  run: 1s\n",
        yaml_path(Path::new(hang))
    );
    let bat = write_spec(&specs, "timeout.yaml", &yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let r = &out.receipt;
    assert_eq!(r.execution.status, RunStatus::TimedOut);
    // A timeout must not be laundered into an oracle judgement.
    assert_eq!(r.oracle.result, None);
    assert_eq!(r.verdict, Verdict::Inconclusive);
    assert_eq!(
        exit_code_for(out.manifest.run_status, Some(r.verdict.as_str())),
        3
    );
    let rendered = receipt::render(r);
    assert!(rendered.contains("INCONCLUSIVE"));
    assert!(rendered.contains("timeout describes the run"), "{rendered}");
    assert!(!rendered.contains("FALSIFIED"));
}

#[test]
fn infrastructure_failure_is_never_proven() {
    let dir = TempDir::new("rc-infra");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: infra-failure\n\
                claim:\n  text: claim\n\
                requires:\n  - process.spawn\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: definitely-not-a-real-program-xyz\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "infra.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let r = &out.receipt;
    assert_eq!(r.execution.status, RunStatus::InfrastructureError);
    assert_eq!(r.verdict, Verdict::InfrastructureError);
    assert_ne!(r.verdict, Verdict::Proven);
    let rendered = receipt::render(r);
    assert!(rendered.contains("INFRASTRUCTURE ERROR"));
    assert!(
        r.verdict_meaning
            .contains("NOT a failure of the tested claim"),
        "infrastructure failure must not read as claim failure"
    );
}

#[test]
fn malformed_oracle_makes_run_invalid_before_work() {
    let dir = TempDir::new("rc-invalid");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: bad-oracle\n\
                claim:\n  text: claim\n\
                requires:\n  - fs.write\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: x.txt\n\
                \x20     text: x\n\
                oracle:\n  type: no_such_condition\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "bad.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run records Invalid");
    assert_eq!(out.manifest.run_status, RunStatus::Invalid);
    assert_eq!(out.receipt.verdict, Verdict::Invalid);
    assert_eq!(
        exit_code_for(out.manifest.run_status, Some(out.receipt.verdict.as_str())),
        2
    );
    // No worktree work happened.
    assert!(out.manifest.worktree.path.is_none());
}

#[test]
fn policy_denial_is_recorded_as_policy_not_claim_failure() {
    let dir = TempDir::new("rc-policy");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let yaml = "version: terrorbat/v1\n\
                id: self-forbidden\n\
                claim:\n  text: claim\n\
                requires:\n  - process.spawn\n\
                forbids:\n  - process.spawn\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: git\n\
                oracle:\n  all: []\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "policy.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.manifest.run_status, RunStatus::PolicyDenied);
    assert_eq!(out.receipt.verdict, Verdict::Inconclusive);
    assert_eq!(
        exit_code_for(out.manifest.run_status, Some(out.receipt.verdict.as_str())),
        2
    );
    let rendered = receipt::render(&out.receipt);
    assert!(rendered.contains("PolicyDenied"), "{rendered}");
}

#[test]
fn command_verifier_oracle_end_to_end() {
    let dir = TempDir::new("rc-verifier");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    // Verifier expects `git --version` to exit 7; it exits 0 -> Falsified.
    let yaml = "version: terrorbat/v1\n\
                id: verifier-bat\n\
                claim:\n  text: The nominated verification command exits 7.\n\
                requires:\n  - process.spawn\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: git\n\
                \x20     action: status\n\
                oracle:\n\
                \x20 type: command\n\
                \x20 program: git\n\
                \x20 args: [--version]\n\
                \x20 expect_exit: 7\n\
                evidence:\n  capture: [stdout]\n\
                timeout:\n  oracle: 60s\n";
    let bat = write_spec(&specs, "verifier.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.receipt.oracle.result, Some(OracleResult::Falsified));
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    // The verifier run itself is recorded as an oracle-phase step with evidence.
    let verifier = out
        .manifest
        .steps
        .iter()
        .find(|s| s.phase == "oracle")
        .expect("verifier step recorded");
    assert_eq!(verifier.exit_code, Some(0));
    assert!(verifier.stdout.is_some());
}

#[test]
fn command_verifier_requires_declared_process_spawn() {
    let dir = TempDir::new("rc-verifier-caps");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    // Oracle verifier but process.spawn NOT declared -> Invalid preflight.
    let yaml = "version: terrorbat/v1\n\
                id: undeclared-verifier\n\
                claim:\n  text: claim\n\
                requires:\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: git\n\
                \x20     action: status\n\
                oracle:\n\
                \x20 type: command\n\
                \x20 program: git\n\
                \x20 args: [--version]\n\
                \x20 expect_exit: 0\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&specs, "undeclared.yaml", yaml);
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run records Invalid");
    assert_eq!(out.manifest.run_status, RunStatus::Invalid);
    assert!(out.manifest.worktree.path.is_none(), "refused before work");
}

#[test]
fn verdict_mapping_covers_every_status() {
    use terrorbat::receipt::verdict_for;
    // Hard rules: no mechanical failure may ever map to Proven.
    for status in [
        RunStatus::TimedOut,
        RunStatus::Crashed,
        RunStatus::Cancelled,
        RunStatus::PolicyDenied,
        RunStatus::InfrastructureError,
        RunStatus::Invalid,
    ] {
        for oracle in [
            Some(OracleResult::Falsified),
            Some(OracleResult::NotFalsified),
            Some(OracleResult::Undetermined),
            None,
        ] {
            let (verdict, _note) = verdict_for(status, oracle);
            assert_ne!(
                verdict,
                Verdict::Proven,
                "{status:?} must never map to PROVEN"
            );
        }
    }
    assert_eq!(
        verdict_for(RunStatus::Completed, Some(OracleResult::Falsified)).0,
        Verdict::Proven
    );
    assert_eq!(
        verdict_for(RunStatus::Completed, Some(OracleResult::NotFalsified)).0,
        Verdict::NotObserved
    );
    assert_eq!(
        verdict_for(RunStatus::Completed, Some(OracleResult::Undetermined)).0,
        Verdict::Inconclusive
    );
    assert_eq!(
        verdict_for(RunStatus::Completed, None).0,
        Verdict::Inconclusive
    );
    assert_eq!(verdict_for(RunStatus::Invalid, None).0, Verdict::Invalid);
    assert_eq!(
        verdict_for(RunStatus::InfrastructureError, None).0,
        Verdict::InfrastructureError
    );
}

#[test]
fn receipt_identity_is_content_determined() {
    let dir = TempDir::new("rc-identity");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "falsify.yaml", &falsifying_bat());
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    // compute_id excludes the id field itself, so recomputation is stable.
    let again = receipt::compute_id(&out.receipt).expect("recompute");
    assert_eq!(again, out.receipt.receipt_id);
}

// ---------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------

#[test]
fn replay_flow_same_verdict_new_execution_original_immutable() {
    let dir = TempDir::new("rc-replay-flow");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "falsify.yaml", &falsifying_bat());
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let original_receipt_path = out.run_dir.join("receipt.json");
    let original_bytes = std::fs::read(&original_receipt_path).expect("receipt bytes");

    // Replay by receipt id.
    let (report, new_out) =
        receipt::replay(&out.receipt.receipt_id, Some(store.clone())).expect("replay");
    assert_ne!(report.new_execution_id, report.original_execution_id);
    // Receipts embed execution ids and timestamps, so ids necessarily differ;
    // the verdict and deterministic evidence must match.
    assert_ne!(report.new_receipt_id, report.original_receipt_id);
    assert!(report.same_status && report.same_oracle && report.same_verdict);
    assert!(
        report
            .evidence_changes
            .iter()
            .any(|l| l == "git_diff: same"),
        "deterministic fixture must reproduce identical diff evidence: {:?}",
        report.evidence_changes
    );
    assert!(
        report
            .evidence_changes
            .iter()
            .any(|l| l == "git_status: same"),
        "{:?}",
        report.evidence_changes
    );

    // Original receipt untouched, byte for byte.
    let after = std::fs::read(&original_receipt_path).expect("receipt still there");
    assert_eq!(
        after, original_bytes,
        "replay must never mutate the original receipt"
    );

    // New run dir exists independently.
    assert!(new_out.run_dir.join("receipt.json").exists());
    assert_ne!(new_out.run_dir, out.run_dir);

    // Replay by execution id also works.
    let (report2, _) =
        receipt::replay(&out.execution_id, Some(store.clone())).expect("replay by exec id");
    assert_eq!(report2.original_execution_id, out.execution_id);
}

#[test]
fn replay_with_missing_repo_explains_itself() {
    let dir = TempDir::new("rc-replay-norepo");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "falsify.yaml", &falsifying_bat());
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    std::fs::remove_dir_all(&repo).expect("remove repo");
    let err = receipt::replay(&out.receipt.receipt_id, Some(store)).expect_err("must fail");
    let msg = err.to_string();
    assert!(msg.contains("no longer exists"), "{msg}");
    assert!(msg.contains("never fetches"), "{msg}");
}

#[test]
fn replay_with_missing_commit_explains_itself() {
    let dir = TempDir::new("rc-replay-nocommit");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "falsify.yaml", &falsifying_bat());
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    // Replace the repository with a fresh one that lacks the pinned commit.
    std::fs::remove_dir_all(&repo).expect("remove repo");
    make_repo(&repo);
    let err = receipt::replay(&out.receipt.receipt_id, Some(store)).expect_err("must fail");
    let msg = err.to_string();
    assert!(msg.contains("no longer exists in"), "{msg}");
    assert!(msg.contains("never fetches"), "{msg}");
}

#[test]
fn inspect_locates_runs_by_execution_and_receipt_id() {
    let dir = TempDir::new("rc-inspect");
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "falsify.yaml", &falsifying_bat());
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    let ev = EvidenceStore::open(&store).expect("store");
    let by_exec = receipt::locate_run(&ev, &out.execution_id).expect("by execution id");
    let by_receipt = receipt::locate_run(&ev, &out.receipt.receipt_id).expect("by receipt id");
    assert_eq!(by_exec, by_receipt);
    assert!(receipt::locate_run(&ev, "receipt:sha256:deadbeef").is_none());
    let r = receipt::load_receipt(&by_exec).expect("receipt");
    let rendered = receipt::render(&r);
    assert!(rendered.contains(&out.receipt.receipt_id));
}
