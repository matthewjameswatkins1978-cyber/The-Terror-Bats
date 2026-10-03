//! Packet 1 regression battery: generic attack species.
//!
//! Every species ships a portable positive Bat under `bats/species/`
//! (built-ins only, no shell, no Lantern/Threadmoth coupling, Windows +
//! Linux clean) plus a quiet control exercised here. Each positive must
//! yield its expected finding class with preserved, re-readable
//! evidence; each control must yield NOT OBSERVED; every Completed
//! positive must reproduce under replay in a fresh worktree.
//!
//! All fixtures are deterministic local temp Git repos — never external
//! checkouts, never the network.

mod common;

use std::path::{Path, PathBuf};

use common::{TempDir, make_repo, repo_is_clean, write_spec};
use terrorbat::evidence::EvidenceStore;
use terrorbat::receipt::Verdict;
use terrorbat::runner::{CleanupStatus, RunOptions, RunStatus, run_bat};

fn species_bat(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("bats")
        .join("species")
        .join(name)
}

fn opts(bat: &Path, repo: &Path, store: &Path) -> RunOptions {
    RunOptions {
        bat_path: bat.to_path_buf(),
        repo: repo.to_path_buf(),
        store_root: Some(store.to_path_buf()),
        overrides: Vec::new(),
    }
}

struct Fixture {
    _dir: TempDir,
    repo: PathBuf,
    store: PathBuf,
    specs: PathBuf,
}

fn fixture(tag: &str) -> Fixture {
    let dir = TempDir::new(tag);
    let repo = dir.join("repo");
    make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    std::fs::create_dir_all(&specs).expect("spec dir");
    Fixture {
        _dir: dir,
        repo,
        store,
        specs,
    }
}

/// Read the captured git diff of a run as text (proof the evidence is
/// preserved and re-readable, not merely referenced).
fn diff_text(fx: &Fixture, out: &terrorbat::runner::RunOutput) -> String {
    let store = EvidenceStore::open(&fx.store).expect("store opens");
    let diff_ref = out
        .manifest
        .captures
        .git_diff
        .as_ref()
        .expect("git diff evidence captured");
    let bytes = store.get(diff_ref).expect("diff evidence readable");
    String::from_utf8(bytes).expect("diff is UTF-8")
}

// --- concurrency -----------------------------------------------------------

#[test]
fn concurrency_loser_overwrite_is_proven_with_durable_evidence() {
    let fx = fixture("sp-conc-pos");
    let out = run_bat(&opts(
        &species_bat("concurrency-same-key.yaml"),
        &fx.repo,
        &fx.store,
    ))
    .expect("species bat runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    // Durable-state corruption, not a leaked error: the loser's bytes are
    // really in the preserved diff.
    assert!(diff_text(&fx, &out).contains("writer=second"));
    assert!(repo_is_clean(&fx.repo), "source target untouched");
}

#[test]
fn concurrency_identical_writes_stay_quiet() {
    let fx = fixture("sp-conc-ctl");
    let yaml = "version: terrorbat/v1\n\
                id: concurrency-control\n\
                claim:\n  text: Contended writers corrupt durable state.\n\
                requires:\n  - fs.write\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: shared/key.txt\n\
                \x20     text: \"writer=only\\n\"\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: shared/key.txt\n\
                \x20     text: \"writer=only\\n\"\n\
                oracle:\n  type: git_diff_contains\n  substring: writer=second\n\
                evidence:\n  capture: [git_diff]\n";
    let bat = write_spec(&fx.specs, "control.yaml", yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("control runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
}

// --- durability ------------------------------------------------------------

#[test]
fn durability_write_survives_into_observable_state() {
    let fx = fixture("sp-dur-pos");
    let out = run_bat(&opts(
        &species_bat("durability-reopen.yaml"),
        &fx.repo,
        &fx.store,
    ))
    .expect("species bat runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    assert!(diff_text(&fx, &out).contains("durable-marker=1"));
    // The reopen seam: replay in a fresh worktree reproduces the verdict
    // from fresh durable state.
    let (report, replayed) =
        terrorbat::receipt::replay(&out.receipt.receipt_id, Some(fx.store.clone()))
            .expect("replay runs");
    assert!(report.same_verdict, "{report:?}");
    assert!(report.same_oracle, "{report:?}");
    assert_eq!(replayed.receipt.verdict, Verdict::Proven);
    assert_ne!(report.new_execution_id, report.original_execution_id);
}

#[test]
fn durability_untouched_path_stays_quiet() {
    let fx = fixture("sp-dur-ctl");
    let yaml = "version: terrorbat/v1\n\
                id: durability-control\n\
                claim:\n  text: The durable path is never modified.\n\
                requires:\n  - fs.write\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: durable/other.txt\n\
                \x20     text: \"unrelated\\n\"\n\
                oracle:\n  type: path_changed\n  path: durable/state.txt\n\
                evidence:\n  capture: [git_diff]\n";
    let bat = write_spec(&fx.specs, "control.yaml", yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("control runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
}

// --- idempotency -----------------------------------------------------------

#[test]
fn idempotency_divergent_repeat_is_proven() {
    let fx = fixture("sp-idem-pos");
    let out = run_bat(&opts(
        &species_bat("idempotency-repeat.yaml"),
        &fx.repo,
        &fx.store,
    ))
    .expect("species bat runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    assert!(diff_text(&fx, &out).contains("attempt=2"));
}

#[test]
fn idempotency_identical_repeat_stays_quiet() {
    let fx = fixture("sp-idem-ctl");
    let yaml = "version: terrorbat/v1\n\
                id: idempotency-control\n\
                claim:\n  text: Repeating the operation leaves no new change.\n\
                requires:\n  - fs.write\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: idem/record.txt\n\
                \x20     text: \"attempt=1\\n\"\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: idem/record.txt\n\
                \x20     text: \"attempt=1\\n\"\n\
                oracle:\n  type: git_diff_contains\n  substring: attempt=2\n\
                evidence:\n  capture: [git_diff]\n";
    let bat = write_spec(&fx.specs, "control.yaml", yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("control runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
}

// --- git integrity ---------------------------------------------------------

#[test]
fn git_integrity_raw_crlf_bytes_are_proven() {
    let fx = fixture("sp-git-pos");
    let out = run_bat(&opts(
        &species_bat("git-integrity-crlf.yaml"),
        &fx.repo,
        &fx.store,
    ))
    .expect("species bat runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    // Raw, uncanonicalised evidence: the marker survived byte-for-byte.
    assert!(diff_text(&fx, &out).contains("line-one"));
}

#[test]
fn git_integrity_other_path_stays_quiet() {
    let fx = fixture("sp-git-ctl");
    let yaml = "version: terrorbat/v1\n\
                id: git-integrity-control\n\
                claim:\n  text: Checkout bytes equal blob bytes here.\n\
                requires:\n  - fs.write\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: eol/other.txt\n\
                \x20     text: \"unrelated\\n\"\n\
                oracle:\n  type: path_changed\n  path: eol/probe.txt\n\
                evidence:\n  capture: [git_diff]\n";
    let bat = write_spec(&fx.specs, "control.yaml", yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("control runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
}

// --- process leaks ---------------------------------------------------------

#[test]
fn process_nonzero_exit_is_recorded_never_folded() {
    let fx = fixture("sp-proc-pos");
    let out = run_bat(&opts(
        &species_bat("process-exit-code.yaml"),
        &fx.repo,
        &fx.store,
    ))
    .expect("species bat runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    // Honesty: the non-zero code is captured in the step record itself.
    let step = out
        .manifest
        .steps
        .iter()
        .find(|s| s.phase == "run" && s.index == 0)
        .expect("run step recorded");
    assert_ne!(step.exit_code, Some(0));
}

#[test]
fn process_zero_exit_stays_quiet() {
    let fx = fixture("sp-proc-ctl");
    let yaml = "version: terrorbat/v1\n\
                id: process-control\n\
                claim:\n  text: Every step here reports success.\n\
                requires:\n  - process.spawn\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: git\n\
                \x20     args: [--version]\n\
                oracle:\n  type: exit_code\n  step: run:0\n  not_equals: 0\n\
                evidence:\n  capture: [stdout]\n";
    let bat = write_spec(&fx.specs, "control.yaml", yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("control runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
}

#[test]
fn process_timeout_kills_tree_and_finds_nothing() {
    let fx = fixture("sp-proc-timeout");
    let probe = env!("CARGO_BIN_EXE_tb_probe_tree");
    let heartbeat = fx._dir.join("heartbeats.log");
    let yaml = format!(
        "version: terrorbat/v1\n\
         id: process-timeout\n\
         claim:\n  text: A timed-out tree leaves no surviving writer.\n\
         requires:\n  - process.spawn\n\
         attack:\n  run:\n\
         \x20   - adapter: command\n\
         \x20     action: run\n\
         \x20     program: {}\n\
         \x20     args: [parent, --heartbeat, {}]\n\
         oracle:\n  type: file_exists\n  path: never.txt\n\
         evidence:\n  capture: [stdout]\n\
         timeout:\n  run: 2s\n",
        common::yaml_path(Path::new(probe)),
        common::yaml_path(&heartbeat)
    );
    let bat = write_spec(&fx.specs, "timeout.yaml", &yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("run completes");
    // A timeout describes the run, never the claim: INCONCLUSIVE by the
    // hard mapping — explicitly NOT a finding, and the cleanup proof is
    // what this case actually establishes.
    assert_eq!(out.manifest.run_status, RunStatus::TimedOut);
    assert_eq!(out.receipt.verdict, Verdict::Inconclusive);
    assert_ne!(out.receipt.verdict, Verdict::Proven);
    assert!(matches!(
        out.manifest.worktree.cleanup.status,
        CleanupStatus::Succeeded
    ));
    let before = std::fs::metadata(&heartbeat)
        .expect("heartbeats prove the tree ran")
        .len();
    assert!(before > 0);
    std::thread::sleep(std::time::Duration::from_millis(900));
    let after = std::fs::metadata(&heartbeat).unwrap().len();
    assert_eq!(before, after, "descendants leaked past the kill");
}

// --- false success ---------------------------------------------------------

#[test]
fn false_success_exit_zero_with_mutation_is_proven() {
    let fx = fixture("sp-false-pos");
    let out = run_bat(&opts(
        &species_bat("false-success-portable.yaml"),
        &fx.repo,
        &fx.store,
    ))
    .expect("species bat runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    // All three legs of the conjunction fired — green output overruled
    // by final state, never the reverse.
    let fired = out
        .receipt
        .oracle
        .conditions
        .iter()
        .filter(|c| c.result == terrorbat::oracle::OracleResult::Falsified)
        .count();
    assert_eq!(fired, 3, "{:?}", out.receipt.oracle.conditions);
}

#[test]
fn false_success_verifier_alone_stays_quiet() {
    let fx = fixture("sp-false-ctl");
    let yaml = "version: terrorbat/v1\n\
                id: false-success-control\n\
                claim:\n  text: A success-reporting step leaves no mutation.\n\
                requires:\n  - process.spawn\n  - fs.write\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: command\n\
                \x20     action: run\n\
                \x20     program: git\n\
                \x20     args: [--version]\n\
                oracle:\n  all:\n\
                \x20   - type: exit_code\n\
                \x20     step: run:0\n\
                \x20     equals: 0\n\
                \x20   - type: path_changed\n\
                \x20     path: tb-false-success.txt\n\
                \x20   - type: git_diff_contains\n\
                \x20     substring: pwned=true\n\
                evidence:\n  capture: [git_diff, stdout]\n";
    let bat = write_spec(&fx.specs, "control.yaml", yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("control runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
}

// --- replay ----------------------------------------------------------------

#[test]
fn replay_reproduces_every_time_without_mutating_the_original() {
    let fx = fixture("sp-replay-pos");
    let out = run_bat(&opts(
        &species_bat("replay-fidelity.yaml"),
        &fx.repo,
        &fx.store,
    ))
    .expect("species bat runs");
    assert_eq!(out.receipt.verdict, Verdict::Proven);

    for _ in 0..2 {
        let (report, replayed) =
            terrorbat::receipt::replay(&out.receipt.receipt_id, Some(fx.store.clone()))
                .expect("repeated replay runs");
        assert!(report.same_verdict, "{report:?}");
        assert!(report.same_oracle, "{report:?}");
        assert!(report.same_status, "{report:?}");
        assert_eq!(replayed.receipt.verdict, Verdict::Proven);
        assert_ne!(report.new_execution_id, report.original_execution_id);
        assert_ne!(report.new_receipt_id, report.original_receipt_id);
    }
    // The original receipt still loads through the trusted path, byte
    // for byte what the first run wrote.
    let store = EvidenceStore::open(&fx.store).expect("store opens");
    let live = terrorbat::receipt::load_receipt(&store.run_dir(&out.receipt.execution_id))
        .expect("original receipt intact");
    assert_eq!(live.receipt_id, out.receipt.receipt_id);
    assert_eq!(live.verdict, Verdict::Proven);
}

#[test]
fn replay_quiet_control_stays_quiet() {
    let fx = fixture("sp-replay-ctl");
    let yaml = "version: terrorbat/v1\n\
                id: replay-control\n\
                claim:\n  text: This attack leaves no durable trace.\n\
                requires:\n  - fs.write\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: notes/observation.txt\n\
                \x20     text: \"benign\\n\"\n\
                oracle:\n  type: path_changed\n  path: replay/trace.txt\n\
                evidence:\n  capture: [git_diff]\n";
    let bat = write_spec(&fx.specs, "control.yaml", yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("control runs");
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
    let (report, replayed) =
        terrorbat::receipt::replay(&out.receipt.receipt_id, Some(fx.store.clone()))
            .expect("replay runs");
    assert!(report.same_verdict, "{report:?}");
    assert_eq!(replayed.receipt.verdict, Verdict::NotObserved);
}

// --- filesystem boundaries -------------------------------------------------

#[test]
fn filesystem_unicode_name_does_not_smuggle_a_write() {
    let fx = fixture("sp-fs-pos");
    let out = run_bat(&opts(
        &species_bat("filesystem-boundary-unicode.yaml"),
        &fx.repo,
        &fx.store,
    ))
    .expect("species bat runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::Proven);
    // The ASCII marker leg carries the finding deterministically on
    // every platform, regardless of porcelain quoting rules.
    assert!(diff_text(&fx, &out).contains("boundary-marker=1"));
}

#[test]
fn filesystem_other_path_stays_quiet() {
    let fx = fixture("sp-fs-ctl");
    let yaml = "version: terrorbat/v1\n\
                id: filesystem-control\n\
                claim:\n  text: The boundary path is never modified.\n\
                requires:\n  - fs.write\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: plain.txt\n\
                \x20     text: \"plain\\n\"\n\
                oracle:\n  type: git_diff_contains\n  substring: boundary-marker=1\n\
                evidence:\n  capture: [git_diff]\n";
    let bat = write_spec(&fx.specs, "control.yaml", yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("control runs");
    assert_eq!(out.manifest.run_status, RunStatus::Completed);
    assert_eq!(out.receipt.verdict, Verdict::NotObserved);
}

#[test]
fn filesystem_escape_is_refused_as_policy_never_a_finding() {
    let fx = fixture("sp-fs-escape");
    let yaml = "version: terrorbat/v1\n\
                id: filesystem-escape\n\
                claim:\n  text: Writes stay inside the worktree.\n\
                requires:\n  - fs.write\n  - git.inspect\n\
                attack:\n  run:\n\
                \x20   - adapter: fs\n\
                \x20     action: write\n\
                \x20     path: ../escape.txt\n\
                \x20     text: \"escaped\\n\"\n\
                oracle:\n  type: path_changed\n  path: escape.txt\n\
                evidence:\n  capture: [git_diff]\n";
    let bat = write_spec(&fx.specs, "escape.yaml", yaml);
    let out = run_bat(&opts(&bat, &fx.repo, &fx.store)).expect("run records refusal");
    // The guard holds: refusal is PolicyDenied, hard-mapped to
    // INCONCLUSIVE. A refusal says nothing about the claim and must
    // never be presentable as PROVEN.
    assert_eq!(out.manifest.run_status, RunStatus::PolicyDenied);
    assert_eq!(out.receipt.verdict, Verdict::Inconclusive);
    assert_ne!(out.receipt.verdict, Verdict::Proven);
    assert!(repo_is_clean(&fx.repo));
}

// --- pack coherence ----------------------------------------------------------

#[test]
fn species_pack_resolves_to_eight_portable_bats() {
    let pack = Path::new(env!("CARGO_MANIFEST_DIR")).join("packs/species.yaml");
    let identified = terrorbat::pack::identify_pack_file(&pack).expect("species pack identifies");
    assert_eq!(identified.entries.len(), 8);
    for entry in &identified.entries {
        assert!(
            entry.path.starts_with("../bats/species/"),
            "species pack must resolve only to generic species bats, got {}",
            entry.path
        );
    }
}

#[test]
fn species_pack_campaign_proves_every_member() {
    let fx = fixture("sp-pack-campaign");
    let pack = Path::new(env!("CARGO_MANIFEST_DIR")).join("packs/species.yaml");
    let opts = terrorbat::campaign::CampaignOptions {
        pack_path: pack,
        repo: fx.repo.clone(),
        store_root: Some(fx.store.clone()),
        runs: 1,
        stop_on_proven: false,
    };
    let out = terrorbat::campaign::run_campaign(&opts).expect("species campaign runs");
    assert_eq!(out.campaign.children.len(), 8);
    for child in &out.campaign.children {
        assert_eq!(
            child.verdict,
            Verdict::Proven,
            "species member {} did not prove",
            child.bat_id
        );
    }
    assert_eq!(
        terrorbat::campaign::exit_code_for_campaign(&out.campaign),
        1
    );
    assert!(repo_is_clean(&fx.repo));
}
