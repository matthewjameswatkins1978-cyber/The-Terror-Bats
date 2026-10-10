//! M6 receipt + replay tests: three-layer honesty, hard mapping rules,
//! receipt identity, replay immutability, and failure-mode receipts.

mod common;

use std::path::{Path, PathBuf};

use common::{TempDir, make_repo, repo_is_clean, write_spec, yaml_path};
use terrorbats::evidence::EvidenceStore;
use terrorbats::oracle::OracleResult;
use terrorbats::receipt::{self, Verdict};
use terrorbats::runner::{RunOptions, RunStatus, exit_code_for, load_manifest, run_bat};

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

/// Single human presentation path for receipt honesty assertions: the
/// Sartorial projection under deterministic pipe capabilities.
fn present(r: &terrorbats::receipt::Receipt) -> String {
    terrorbats::presentation::render_receipt(r, &sartorial_core::Capabilities::piped(100))
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
    assert!(r.reproduction.replay_command.contains("terrorbats replay"));

    // Human rendering: honest about PROVEN's scope and isolation.
    let rendered = present(r);
    assert!(rendered.contains("\\^v^/"));
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

    let rendered = present(r);
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
    let rendered = present(r);
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
    let rendered = present(r);
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
    let rendered = present(&out.receipt);
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
    use terrorbats::receipt::verdict_for;
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

    // M8's optional fields are absent from built-in-only receipt content, so
    // the established serialization and content identity remain compatible.
    let content = serde_json::to_value(&out.receipt).expect("receipt JSON");
    assert!(content.get("adapter_bindings_required").is_none());
    let step = &content["execution"]["steps"][0];
    for field in ["adapter_provenance", "protocol_stdout", "protocol_stderr"] {
        assert!(step.get(field).is_none(), "built-in step gained `{field}`");
    }
    receipt::load_receipt(&out.run_dir).expect("built-in receipt still verifies");
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
    // NOTE: do not reuse make_repo here. Two identical back-to-back commits in
    // the same second share a hash (proven), which would leave the pinned commit
    // present. Distinct initial content guarantees absence, independent of timing.
    std::fs::remove_dir_all(&repo).expect("remove repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    common::git(&repo, &["init", "-q"]);
    common::git(&repo, &["config", "user.email", "terrorbat@test.local"]);
    common::git(&repo, &["config", "user.name", "Terror Bat Test"]);
    std::fs::write(repo.join("README.md"), "unrelated replacement repo\n").expect("write readme");
    common::git(&repo, &["add", "."]);
    common::git(&repo, &["commit", "-q", "-m", "initial"]);
    let fresh = common::git(&repo, &["rev-parse", "HEAD"]);
    assert_ne!(
        fresh, out.manifest.target.commit,
        "fresh repo must not contain the pinned commit"
    );
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
    let by_exec = receipt::locate_run(&ev, &out.execution_id)
        .expect("lookup")
        .expect("by execution id");
    let by_receipt = receipt::locate_run(&ev, &out.receipt.receipt_id)
        .expect("lookup")
        .expect("by receipt id");
    assert_eq!(by_exec, by_receipt);
    assert!(
        receipt::locate_run(&ev, "receipt:sha256:deadbeef")
            .expect("scan")
            .is_none()
    );
    let r = receipt::load_receipt(&by_exec).expect("receipt");
    let rendered = present(&r);
    assert!(rendered.contains(&out.receipt.receipt_id));
}

// ---------------------------------------------------------------------------
// Receipt integrity (closeout): content-addressed receipts verify on read
// exactly like evidence objects — fail closed, never repair, never warn and
// continue. A receipt may not be used by inspect, replay, lookup or
// comparison unless its content still matches its claimed identity.
// ---------------------------------------------------------------------------

fn tamper_receipt(run_dir: &Path, from: &str, to: &str) {
    let path = run_dir.join("receipt.json");
    let text = std::fs::read_to_string(&path).expect("read receipt");
    assert!(
        text.contains(from),
        "tamper target missing from receipt: {from}"
    );
    std::fs::write(&path, text.replacen(from, to, 1)).expect("write tampered receipt");
}

fn forged_id(c: char) -> String {
    format!("receipt:sha256:{}", c.to_string().repeat(64))
}

fn fresh_proven_run(tag: &str) -> (TempDir, PathBuf, PathBuf, terrorbats::runner::RunOutput) {
    let dir = TempDir::new(tag);
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    let bat = write_spec(&specs, "falsify.yaml", &falsifying_bat());
    let out = run_bat(&opts(&bat, &repo, &store)).expect("run");
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    (dir, repo, store, out)
}

/// Run a legitimate receipt through the real framework path, tamper one
/// field on disk, and require every trusted path to fail closed.
fn integrity_case(
    label: &str,
    tamper: impl FnOnce(&terrorbats::receipt::Receipt) -> (String, String),
) {
    let (_dir, _repo, store, out) = fresh_proven_run("rc-integrity");
    let rid = out.receipt.receipt_id.clone();
    let (from, to) = tamper(&out.receipt);
    tamper_receipt(&out.run_dir, &from, &to);

    let load = match receipt::load_receipt(&out.run_dir) {
        Ok(_) => panic!("{label}: tampered receipt must not load"),
        Err(e) => e.to_string(),
    };
    assert!(load.contains("TB-RECEIPT-CORRUPT"), "{label}: {load}");

    let ev = EvidenceStore::open(&store).expect("store");
    let loc = match receipt::locate_run(&ev, &rid) {
        Ok(_) => panic!("{label}: lookup by claimed id must fail closed"),
        Err(e) => e.to_string(),
    };
    assert!(loc.contains("TB-RECEIPT-CORRUPT"), "{label}: {loc}");

    let rep = match receipt::replay(&rid, Some(store.clone())) {
        Ok(_) => panic!("{label}: replay must fail closed"),
        Err(e) => e.to_string(),
    };
    assert!(rep.contains("TB-RECEIPT-CORRUPT"), "{label}: {rep}");
}

#[test]
fn any_receipt_content_mutation_is_refused() {
    type TamperFn = Box<dyn Fn(&terrorbats::receipt::Receipt) -> (String, String)>;
    let cases: Vec<(&str, TamperFn)> = vec![
        (
            "A verdict",
            Box::new(|r| {
                (
                    format!("\"verdict\": \"{}\"", r.verdict.as_str()),
                    "\"verdict\": \"NOT OBSERVED\"".to_string(),
                )
            }),
        ),
        (
            "B oracle result",
            Box::new(|r| {
                (
                    format!(
                        "\"result\": \"{:?}\"",
                        r.oracle.result.expect("oracle result")
                    ),
                    "\"result\": \"NotFalsified\"".to_string(),
                )
            }),
        ),
        (
            "C target commit",
            Box::new(|r| {
                (
                    format!("\"commit\": \"{}\"", r.target.commit),
                    format!("\"commit\": \"{}\"", "a".repeat(40)),
                )
            }),
        ),
        (
            "D evidence reference",
            Box::new(|r| {
                let first = r.evidence[0].reference.0.clone();
                let last = first.chars().last().expect("nonempty");
                let flipped = if last == '0' { '1' } else { '0' };
                let mut forged = first.clone();
                forged.pop();
                forged.push(flipped);
                (
                    format!("\"reference\": \"{first}\""),
                    format!("\"reference\": \"{forged}\""),
                )
            }),
        ),
        (
            "E claim text",
            Box::new(|_| {
                (
                    "never modified by ordinary work".to_string(),
                    "sometimes modified by ordinary work".to_string(),
                )
            }),
        ),
        (
            "F isolation statement",
            Box::new(|_| {
                (
                    "host filesystem containment: UNENFORCED".to_string(),
                    "host filesystem containment: ENFORCED".to_string(),
                )
            }),
        ),
        (
            "H replay command",
            Box::new(|r| {
                (
                    format!("\"replay_command\": \"{}\"", r.reproduction.replay_command),
                    format!("\"replay_command\": \"{}\"", forged_id('c')),
                )
            }),
        ),
    ];
    for (label, make) in cases {
        integrity_case(label, |r| make(r));
    }
}

#[test]
fn forged_receipt_id_is_refused_and_cannot_redirect() {
    let (_dir, _repo, store, out) = fresh_proven_run("rc-forged-id");
    let rid = out.receipt.receipt_id.clone();
    let forged = forged_id('b');
    tamper_receipt(
        &out.run_dir,
        &format!("\"receipt_id\": \"{rid}\""),
        &format!("\"receipt_id\": \"{forged}\""),
    );

    // Case G: load by run dir fails closed (stored id != recomputed id).
    let load = receipt::load_receipt(&out.run_dir).expect_err("must refuse");
    assert!(load.to_string().contains("TB-RECEIPT-CORRUPT"), "{load}");

    let ev = EvidenceStore::open(&store).expect("store");
    // Lookup by the forged id finds the claimant, but verification
    // recomputes the true id: corruption, never trust.
    let loc = receipt::locate_run(&ev, &forged).expect_err("forged lookup must fail closed");
    assert!(loc.to_string().contains("TB-RECEIPT-CORRUPT"), "{loc}");
    // Lookup by the true id finds no claimant: honest not-found, never the
    // impostor.
    assert!(receipt::locate_run(&ev, &rid).expect("scan").is_none());
    // Replay by the forged id fails closed.
    let rep = receipt::replay(&forged, Some(store)).expect_err("replay must refuse");
    assert!(rep.to_string().contains("TB-RECEIPT-CORRUPT"), "{rep}");
}

#[test]
fn corrupt_copy_cannot_impersonate_a_genuine_receipt() {
    let (_dir, _repo, store, out) = fresh_proven_run("rc-impersonate");
    let rid = out.receipt.receipt_id.clone();

    // Impostor: a copy of the run under another execution id, content
    // tampered but still claiming the genuine receipt id.
    let impostor = store.join("runs").join("impostor-exec-id");
    copy_dir(&out.run_dir, &impostor);
    tamper_receipt(
        &impostor,
        "never modified by ordinary work",
        "sometimes modified by ordinary work",
    );

    let ev = EvidenceStore::open(&store).expect("store");
    // (a) Genuine present: the verified receipt wins over any unverified
    // claimant, whatever the scan order.
    let found = receipt::locate_run(&ev, &rid)
        .expect("lookup")
        .expect("found");
    assert_eq!(
        found, out.run_dir,
        "genuine receipt must be found, not the impostor"
    );

    // (b) Genuine removed: corruption surfaces — never the impostor, never
    // a silent not-found.
    std::fs::remove_dir_all(&out.run_dir).expect("remove genuine");
    let err = receipt::locate_run(&ev, &rid).expect_err("corruption must surface");
    assert!(err.to_string().contains("TB-RECEIPT-CORRUPT"), "{err}");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create impostor dir");
    for entry in std::fs::read_dir(from).expect("read run dir") {
        let entry = entry.expect("entry");
        std::fs::copy(entry.path(), to.join(entry.file_name())).expect("copy file");
    }
}

#[test]
fn malformed_receipt_ids_are_rejected() {
    for bad in [
        "receipt:sha256:",
        "receipt:sha256:xyz",
        "abc123",
        &format!("evidence:sha256:{}", "a".repeat(64)),
        &format!("receipt:sha256:{}", "A".repeat(64)),
        &format!("receipt:sha256:{}", "a".repeat(63)),
        &format!("receipt:sha256:{}", "g".repeat(64)),
    ] {
        assert!(
            receipt::validate_receipt_id(bad).is_err(),
            "`{bad}` must be rejected"
        );
    }
    receipt::validate_receipt_id(&format!("receipt:sha256:{}", "0".repeat(64)))
        .expect("canonical shape is accepted");

    // On disk: a malformed stored id fails the verified load closed.
    let (_dir, _repo, store, out) = fresh_proven_run("rc-malformed");
    let rid = out.receipt.receipt_id.clone();
    tamper_receipt(
        &out.run_dir,
        &format!("\"receipt_id\": \"{rid}\""),
        "\"receipt_id\": \"receipt:sha256:xyz\"",
    );
    let load = receipt::load_receipt(&out.run_dir).expect_err("must refuse");
    assert!(load.to_string().contains("TB-RECEIPT-CORRUPT"), "{load}");
    let ev = EvidenceStore::open(&store).expect("store");
    let loc = receipt::locate_run(&ev, "receipt:sha256:xyz").expect_err("scan must fail closed");
    assert!(loc.to_string().contains("TB-RECEIPT-CORRUPT"), "{loc}");
}

#[test]
fn legitimate_receipts_still_verify() {
    // The hashing contract is unchanged: a receipt produced by the real
    // pipeline verifies immediately, with no id regeneration.
    let (_dir, _repo, _store, out) = fresh_proven_run("rc-legit");
    let r = receipt::load_receipt(&out.run_dir).expect("legitimate receipt verifies");
    assert_eq!(r.receipt_id, out.receipt.receipt_id);
    let again = receipt::compute_id(&r).expect("recompute");
    assert_eq!(again, r.receipt_id);
}
