//! Bat Campaign v1 tests: serial pack execution, durable tamper-evident
//! campaign receipts, stop-on-proven, pinned baselines, and fail-closed
//! trusted inspection. All fixtures are deterministic local temp Git repos.

mod common;

use std::path::{Path, PathBuf};

use common::{TempDir, make_repo, repo_is_clean, write_spec};
use terrorbat::campaign::{
    self, CampaignOptions, MAX_CAMPAIGN_RUNS, VERDICT_LABELS, exit_code_for_campaign,
    validate_campaign_id, validate_runs,
};
use terrorbat::evidence::EvidenceStore;
use terrorbat::receipt::Verdict;
use terrorbat::runner::RunStatus;

/// A Bat whose attack mutates the protected path; the oracle detects it.
fn falsifying_bat(id: &str) -> String {
    format!(
        "version: terrorbat/v1\n\
         id: {id}\n\
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
    )
}

/// Same shape, but the attack touches a benign path the oracle never flags.
fn quiet_bat(id: &str) -> String {
    format!(
        "version: terrorbat/v1\n\
         id: {id}\n\
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
    )
}

/// A Bat that fails at the infrastructure layer (unknown program).
fn infra_bat(id: &str) -> String {
    format!(
        "version: terrorbat/v1\n\
         id: {id}\n\
         claim:\n  text: claim\n\
         requires:\n  - process.spawn\n\
         attack:\n  run:\n\
         \x20   - adapter: command\n\
         \x20     action: run\n\
         \x20     program: definitely-not-a-real-program-xyz\n\
         oracle:\n  all: []\n\
         evidence:\n  capture: [stdout]\n"
    )
}

struct Fixture {
    _dir: TempDir,
    repo: PathBuf,
    store: PathBuf,
    pack: PathBuf,
    head: String,
}

fn fixture(tag: &str, bats: &[(&str, String)], pack_entries: &str) -> Fixture {
    let dir = TempDir::new(tag);
    let repo = dir.join("repo");
    let head = make_repo(&repo);
    let store = dir.join("store");
    let specs = dir.join("specs");
    for (name, yaml) in bats {
        write_spec(&specs, name, yaml);
    }
    let pack = specs.join("pack.yaml");
    std::fs::write(
        &pack,
        format!("version: terrorbat-pack/v1\nid: test-pack\nbats:\n{pack_entries}"),
    )
    .expect("write pack");
    Fixture {
        _dir: dir,
        repo,
        store,
        pack,
        head,
    }
}

fn opts(fx: &Fixture, runs: u64, stop_on_proven: bool) -> CampaignOptions {
    CampaignOptions {
        pack_path: fx.pack.clone(),
        repo: fx.repo.clone(),
        store_root: Some(fx.store.clone()),
        runs,
        stop_on_proven,
    }
}

fn single_quiet_pack() -> Fixture {
    fixture(
        "camp-quiet",
        &[("q.yaml", quiet_bat("quiet"))],
        "  - path: q.yaml\n",
    )
}

// ---------------------------------------------------------------------------
// Serial execution + ordinary child receipts
// ---------------------------------------------------------------------------

#[test]
fn runs_3_yields_3_iterations_with_ordinary_persisted_receipts() {
    let fx = single_quiet_pack();
    let out = campaign::run_campaign(&opts(&fx, 3, false)).expect("campaign runs");

    assert_eq!(out.campaign.requested_runs, 3);
    assert_eq!(out.campaign.completed_runs, 3);
    assert!(!out.campaign.stopped_early);
    assert_eq!(out.campaign.children.len(), 3);

    let store = EvidenceStore::open(&fx.store).expect("store");
    for (i, child) in out.campaign.children.iter().enumerate() {
        // Iteration order is serial and declared.
        assert_eq!(child.iteration, (i + 1) as u64);
        assert_eq!(child.entry_index, 0);
        assert_eq!(child.verdict, Verdict::NotObserved);
        // Every child is an ordinary run with a durably persisted receipt.
        let run_dir = store.run_dir(&child.execution_id);
        assert!(run_dir.join("receipt.json").exists());
        assert!(run_dir.join("manifest.json").exists());
        let live = terrorbat::receipt::load_receipt(&run_dir).expect("child verifies");
        assert_eq!(live.receipt_id, child.receipt_id);
        // All children share the pinned baseline.
        assert_eq!(live.target.commit, fx.head);
    }
    // Distinct first-class executions.
    let execs: Vec<_> = out
        .campaign
        .children
        .iter()
        .map(|c| &c.execution_id)
        .collect();
    assert!(execs[0] != execs[1] && execs[1] != execs[2]);

    // Summary counts equal child receipt reality.
    assert_eq!(
        out.campaign.summary.get("NOT OBSERVED"),
        Some(&3),
        "{:?}",
        out.campaign.summary
    );
    assert_eq!(exit_code_for_campaign(&out.campaign), 0);

    // Source target clean afterwards, still at the baseline.
    assert!(repo_is_clean(&fx.repo));
    assert_eq!(
        terrorbat::worktree::inspect_target(&fx.repo)
            .expect("inspect")
            .commit,
        fx.head
    );
}

#[test]
fn mixed_outcomes_are_preserved_independently() {
    let fx = fixture(
        "camp-mixed",
        &[
            ("loud.yaml", falsifying_bat("loud")),
            ("q.yaml", quiet_bat("quiet")),
        ],
        "  - path: loud.yaml\n  - path: q.yaml\n",
    );
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");

    assert_eq!(out.campaign.children.len(), 2);
    assert_eq!(out.campaign.children[0].verdict, Verdict::Proven);
    assert_eq!(out.campaign.children[1].verdict, Verdict::NotObserved);
    // The later NOT OBSERVED never overwrote the earlier PROVEN.
    assert_eq!(out.campaign.summary.get("PROVEN"), Some(&1));
    assert_eq!(out.campaign.summary.get("NOT OBSERVED"), Some(&1));
    assert_eq!(exit_code_for_campaign(&out.campaign), 1);
    assert!(repo_is_clean(&fx.repo));
}

#[test]
fn multi_entry_multi_run_orders_children_serially() {
    let fx = fixture(
        "camp-order",
        &[
            ("loud.yaml", falsifying_bat("loud")),
            ("q.yaml", quiet_bat("quiet")),
        ],
        "  - path: loud.yaml\n  - path: q.yaml\n",
    );
    let out = campaign::run_campaign(&opts(&fx, 2, false)).expect("campaign runs");
    assert_eq!(out.campaign.completed_runs, 2);
    let plan: Vec<(u64, usize)> = out
        .campaign
        .children
        .iter()
        .map(|c| (c.iteration, c.entry_index))
        .collect();
    assert_eq!(plan, vec![(1, 0), (1, 1), (2, 0), (2, 1)]);
}

// ---------------------------------------------------------------------------
// stop-on-proven
// ---------------------------------------------------------------------------

#[test]
fn stop_on_proven_halts_only_after_durable_proven() {
    let fx = fixture(
        "camp-stop",
        &[
            ("q.yaml", quiet_bat("quiet")),
            ("loud.yaml", falsifying_bat("loud")),
        ],
        "  - path: q.yaml\n  - path: loud.yaml\n",
    );
    let out = campaign::run_campaign(&opts(&fx, 5, true)).expect("campaign runs");

    // Quiet ran, loud proved, then scheduling stopped mid-iteration-1.
    assert_eq!(out.campaign.children.len(), 2);
    assert_eq!(out.campaign.children[0].verdict, Verdict::NotObserved);
    assert_eq!(out.campaign.children[1].verdict, Verdict::Proven);
    assert!(out.campaign.stopped_early);
    let reason = out.campaign.stop_reason.clone().expect("reason");
    assert!(reason.contains("stop-on-proven"), "{reason}");
    assert!(
        reason.contains(&out.campaign.children[1].receipt_id),
        "{reason}"
    );
    // Partial iteration is honestly uncounted.
    assert_eq!(out.campaign.completed_runs, 0);
    assert_eq!(out.campaign.requested_runs, 5);

    // The PROVEN receipt was durably persisted BEFORE the halt: it reloads
    // through the trusted path from the store.
    let store = EvidenceStore::open(&fx.store).expect("store");
    let proven_dir = store.run_dir(&out.campaign.children[1].execution_id);
    let live = terrorbat::receipt::load_receipt(&proven_dir).expect("proven receipt durable");
    assert_eq!(live.verdict, Verdict::Proven);

    assert_eq!(exit_code_for_campaign(&out.campaign), 1);
    assert!(repo_is_clean(&fx.repo));
}

#[test]
fn stop_on_proven_first_entry_stops_after_one_child() {
    let fx = fixture(
        "camp-stop1",
        &[
            ("loud.yaml", falsifying_bat("loud")),
            ("q.yaml", quiet_bat("quiet")),
        ],
        "  - path: loud.yaml\n  - path: q.yaml\n",
    );
    let out = campaign::run_campaign(&opts(&fx, 5, true)).expect("campaign runs");
    assert_eq!(out.campaign.children.len(), 1);
    assert!(out.campaign.stopped_early);
}

#[test]
fn stop_on_proven_without_proven_completes_everything() {
    let fx = fixture(
        "camp-nostop",
        &[("q.yaml", quiet_bat("quiet"))],
        "  - path: q.yaml\n",
    );
    let out = campaign::run_campaign(&opts(&fx, 2, true)).expect("campaign runs");
    assert_eq!(out.campaign.children.len(), 2);
    assert!(!out.campaign.stopped_early);
    assert!(out.campaign.stop_reason.is_none());
    assert_eq!(out.campaign.completed_runs, 2);
}

// ---------------------------------------------------------------------------
// Infra failure is never PROVEN
// ---------------------------------------------------------------------------

#[test]
fn infra_failure_cannot_become_proven() {
    let fx = fixture(
        "camp-infra",
        &[("i.yaml", infra_bat("infra"))],
        "  - path: i.yaml\n",
    );
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");
    assert_eq!(out.campaign.children.len(), 1);
    let child = &out.campaign.children[0];
    assert_eq!(child.status, RunStatus::InfrastructureError);
    assert_eq!(child.verdict, Verdict::InfrastructureError);
    assert_ne!(child.verdict, Verdict::Proven);
    assert_eq!(out.campaign.summary.get("INFRASTRUCTURE ERROR"), Some(&1));
    assert_eq!(exit_code_for_campaign(&out.campaign), 3);
}

// ---------------------------------------------------------------------------
// Identity + fail-closed loading
// ---------------------------------------------------------------------------

#[test]
fn campaign_identity_is_stable_and_well_formed() {
    let fx = single_quiet_pack();
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");
    validate_campaign_id(&out.campaign.campaign_id).expect("id shape");
    let again = campaign::compute_id(&out.campaign).expect("recompute");
    assert_eq!(again, out.campaign.campaign_id);
    assert_eq!(out.campaign.version, "terrorbat/campaign/v1");
    // Summary always carries every verdict label (zeros included).
    for label in VERDICT_LABELS {
        assert!(
            out.campaign.summary.contains_key(label),
            "summary must carry `{label}`"
        );
    }
    // Two campaigns over the same inputs are distinct aggregates (child
    // execution ids differ), each self-consistent.
    let out2 = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs again");
    assert_ne!(out.campaign.campaign_id, out2.campaign.campaign_id);
    validate_campaign_id(&out2.campaign.campaign_id).expect("id shape");
}

#[test]
fn malformed_campaign_ids_are_rejected() {
    for bad in [
        "campaign:sha256:".to_string(),
        "campaign:sha256:xyz".to_string(),
        "receipt:sha256:".to_string() + &"a".repeat(64),
        "campaign:sha256:".to_string() + &"A".repeat(64),
        "campaign:sha256:".to_string() + &"a".repeat(63),
    ] {
        assert!(
            validate_campaign_id(&bad).is_err(),
            "`{bad}` must be rejected"
        );
    }
    validate_campaign_id(&format!("campaign:sha256:{}", "0".repeat(64)))
        .expect("canonical shape accepted");
}

fn tamper_campaign_file(dir: &Path, from: &str, to: &str) {
    let path = dir.join("campaign.json");
    let text = std::fs::read_to_string(&path).expect("read campaign");
    assert!(text.contains(from), "tamper target missing: {from}");
    std::fs::write(&path, text.replacen(from, to, 1)).expect("write tampered campaign");
}

#[test]
fn campaign_tamper_is_detected() {
    let fx = single_quiet_pack();
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");

    tamper_campaign_file(
        &out.campaign_dir,
        "\"verdict\": \"NOT OBSERVED\"",
        "\"verdict\": \"PROVEN\"",
    );
    let store = EvidenceStore::open(&fx.store).expect("store");
    let err = campaign::load_campaign(&store, &out.campaign_dir).expect_err("must refuse");
    assert!(err.to_string().contains("TB-CAMPAIGN-CORRUPT"), "{err}");
    // Lookup by the claimed id fails closed too — never silent not-found.
    let loc = campaign::locate_campaign(&store, &out.campaign_id).expect_err("lookup refuses");
    assert!(loc.to_string().contains("TB-CAMPAIGN-CORRUPT"), "{loc}");
}

#[test]
fn child_tamper_fails_trusted_campaign_inspect() {
    let fx = single_quiet_pack();
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");
    let store = EvidenceStore::open(&fx.store).expect("store");

    // Tamper the child's receipt on disk (flip its claim text).
    let child_dir = store.run_dir(&out.campaign.children[0].execution_id);
    let rpath = child_dir.join("receipt.json");
    let text = std::fs::read_to_string(&rpath).expect("read receipt");
    std::fs::write(&rpath, text.replacen("benign note", "pwned = true", 1)).expect("tamper child");
    // The receipt path itself fails closed...
    assert!(
        terrorbat::receipt::load_receipt(&child_dir)
            .expect_err("receipt must refuse")
            .to_string()
            .contains("TB-RECEIPT-CORRUPT")
    );
    // ...and so does the campaign aggregate that references it.
    let err = campaign::load_campaign(&store, &out.campaign_dir).expect_err("must refuse");
    assert!(err.to_string().contains("TB-CAMPAIGN-CORRUPT"), "{err}");
}

#[test]
fn missing_child_is_surfaced_explicitly() {
    let fx = single_quiet_pack();
    let out = campaign::run_campaign(&opts(&fx, 2, false)).expect("campaign runs");
    let store = EvidenceStore::open(&fx.store).expect("store");

    let missing = out.campaign.children[0].execution_id.clone();
    std::fs::remove_dir_all(store.run_dir(&missing)).expect("remove child run");
    let err = campaign::load_campaign(&store, &out.campaign_dir).expect_err("must refuse");
    let msg = err.to_string();
    assert!(msg.contains("TB-CAMPAIGN-CORRUPT"), "{msg}");
    assert!(msg.contains(&missing), "missing child must be named: {msg}");
}

#[test]
fn summary_tamper_is_detected() {
    let fx = single_quiet_pack();
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");
    // Rewriting the summary changes the hashed content AND breaks the
    // summary-vs-children check; either way the aggregate must refuse.
    tamper_campaign_file(
        &out.campaign_dir,
        "\"NOT OBSERVED\": 1",
        "\"NOT OBSERVED\": 7",
    );
    let store = EvidenceStore::open(&fx.store).expect("store");
    let err = campaign::load_campaign(&store, &out.campaign_dir).expect_err("must refuse");
    assert!(err.to_string().contains("TB-CAMPAIGN-CORRUPT"), "{err}");
}

#[test]
fn locate_finds_campaign_by_id() {
    let fx = single_quiet_pack();
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");
    let store = EvidenceStore::open(&fx.store).expect("store");
    let found = campaign::locate_campaign(&store, &out.campaign_id)
        .expect("lookup")
        .expect("found by campaign id");
    assert_eq!(found, out.campaign_dir);
    assert!(
        campaign::locate_campaign(&store, &format!("campaign:sha256:{}", "0".repeat(64)))
            .expect("scan")
            .is_none()
    );
}

// ---------------------------------------------------------------------------
// Request validation + target pinning
// ---------------------------------------------------------------------------

#[test]
fn invalid_runs_counts_are_rejected() {
    assert!(validate_runs(1).is_ok());
    assert!(validate_runs(MAX_CAMPAIGN_RUNS).is_ok());
    let zero = validate_runs(0).expect_err("0 rejected");
    assert!(zero.to_string().contains("positive"), "{zero}");
    let absurd = validate_runs(MAX_CAMPAIGN_RUNS + 1).expect_err("absurd rejected");
    assert!(absurd.to_string().contains("bound"), "{absurd}");
    let huge = validate_runs(u64::MAX).expect_err("huge rejected");
    assert!(huge.to_string().contains("bound"), "{huge}");
}

#[test]
fn zero_runs_rejected_before_any_child_runs() {
    let fx = single_quiet_pack();
    let bad = CampaignOptions {
        runs: 0,
        ..opts(&fx, 1, false)
    };
    campaign::run_campaign(&bad).expect_err("0 runs refused");
    let store = EvidenceStore::open(&fx.store).expect("store");
    assert!(store.list_runs().is_empty());
    assert!(store.list_campaigns().is_empty());
}

#[test]
fn dirty_target_refuses_campaign_without_records() {
    let fx = single_quiet_pack();
    std::fs::write(fx.repo.join("README.md"), "uncommitted local change\n").expect("dirty");
    let err = campaign::run_campaign(&opts(&fx, 1, false)).expect_err("dirty refused");
    assert!(err.to_string().contains("dirty"), "{err}");
    let store = EvidenceStore::open(&fx.store).expect("store");
    assert!(store.list_runs().is_empty(), "refusal must not create runs");
    assert!(
        store.list_campaigns().is_empty(),
        "refusal must not create campaigns"
    );
}

// ---------------------------------------------------------------------------
// Presentation honesty
// ---------------------------------------------------------------------------

#[test]
fn campaign_rendering_is_honest() {
    let fx = fixture(
        "camp-render",
        &[("loud.yaml", falsifying_bat("loud"))],
        "  - path: loud.yaml\n",
    );
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");
    let rendered = terrorbat::presentation::render_campaign(
        &out.campaign,
        &sartorial_core::Capabilities::piped(100),
    );
    assert!(rendered.contains("\\^v^/"), "{rendered}");
    assert!(rendered.contains("BAT CAMPAIGN"), "{rendered}");
    assert!(rendered.contains("PROVEN"), "{rendered}");
    // The FIRST PROVEN receipt reference is surfaced.
    assert!(
        rendered.contains(&out.campaign.children[0].receipt_id),
        "{rendered}"
    );

    let fx2 = single_quiet_pack();
    let out2 = campaign::run_campaign(&opts(&fx2, 1, false)).expect("campaign runs");
    let rendered2 = terrorbat::presentation::render_campaign(
        &out2.campaign,
        &sartorial_core::Capabilities::piped(100),
    );
    assert!(rendered2.contains("NOT OBSERVED"), "{rendered2}");
    for word in ["PASSED", "SAFE", "CORRECT"] {
        assert!(
            !rendered2.contains(word),
            "NOT OBSERVED must never read as `{word}`: {rendered2}"
        );
    }
}
