//! W2 MS3 regression battery: campaign invariants that must hold on every
//! repeated execution, plus the MS1 observation that pack entry paths
//! resolve against the pack file's directory.
//!
//! All fixtures are deterministic local temp Git repos — never Lantern or
//! Threadmoth checkouts, never the network.

mod common;

use std::path::PathBuf;

use common::{TempDir, make_repo, repo_is_clean, write_spec};
use terrorbat::campaign::{self, CampaignOptions, summarize};
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
        format!("version: terrorbat-pack/v1\nid: regression-pack\nbats:\n{pack_entries}"),
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

fn loud_quiet_pack(tag: &str) -> Fixture {
    fixture(
        tag,
        &[
            ("loud.yaml", falsifying_bat("loud")),
            ("q.yaml", quiet_bat("quiet")),
        ],
        "  - path: loud.yaml\n  - path: q.yaml\n",
    )
}

// (a) Repeated attacks record independent per-iteration verdicts: a loud
// entry proves every iteration while a quiet entry observes nothing, and
// both realities coexist in one aggregate.
#[test]
fn repeated_attacks_may_yield_different_verdicts_across_iterations() {
    let fx = loud_quiet_pack("reg-differ");
    let out = campaign::run_campaign(&opts(&fx, 2, false)).expect("campaign runs");

    let verdicts: Vec<Verdict> = out.campaign.children.iter().map(|c| c.verdict).collect();
    assert_eq!(
        verdicts,
        vec![
            Verdict::Proven,
            Verdict::NotObserved,
            Verdict::Proven,
            Verdict::NotObserved,
        ],
        "each iteration records its own child verdicts independently"
    );
    // Distinct first-class executions: nothing was folded or deduplicated.
    let mut execs: Vec<_> = out
        .campaign
        .children
        .iter()
        .map(|c| c.execution_id.clone())
        .collect();
    execs.sort();
    execs.dedup();
    assert_eq!(execs.len(), 4);
    assert_eq!(
        terrorbat::campaign::exit_code_for_campaign(&out.campaign),
        1
    );
}

// (b) Ordering loud-then-quiet must not matter: the later NOT OBSERVED is
// appended as its own record, never merged into or over the earlier PROVEN.
#[test]
fn later_proven_never_erases_earlier_not_observed_records() {
    let fx = fixture(
        "reg-preserve",
        &[
            ("q.yaml", quiet_bat("quiet")),
            ("loud.yaml", falsifying_bat("loud")),
        ],
        "  - path: q.yaml\n  - path: loud.yaml\n",
    );
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");

    assert_eq!(out.campaign.children.len(), 2);
    assert_eq!(out.campaign.children[0].verdict, Verdict::NotObserved);
    assert_eq!(out.campaign.children[1].verdict, Verdict::Proven);
    assert_eq!(out.campaign.summary.get("PROVEN"), Some(&1));
    assert_eq!(out.campaign.summary.get("NOT OBSERVED"), Some(&1));

    // Both child receipts survive independently in the store.
    let store = EvidenceStore::open(&fx.store).expect("store");
    for child in &out.campaign.children {
        let live = terrorbat::receipt::load_receipt(&store.run_dir(&child.execution_id))
            .expect("child receipt durable");
        assert_eq!(live.receipt_id, child.receipt_id);
        assert_eq!(live.verdict, child.verdict);
    }
}

// (c) stop-on-proven is armed only by durable PROVEN evidence: a campaign
// with no PROVEN runs to completion, and when it fires the PROVEN receipt
// is already durably persisted and reloads through the trusted path.
#[test]
fn stop_on_proven_fires_only_after_durable_proven_evidence() {
    // No PROVEN anywhere: stop_on_proven must not halt anything.
    let fx = fixture(
        "reg-noproven",
        &[("q.yaml", quiet_bat("quiet"))],
        "  - path: q.yaml\n",
    );
    let calm = campaign::run_campaign(&opts(&fx, 2, true)).expect("campaign runs");
    assert_eq!(calm.campaign.children.len(), 2);
    assert!(!calm.campaign.stopped_early);
    assert!(calm.campaign.stop_reason.is_none());
    assert_eq!(calm.campaign.completed_runs, 2);

    // PROVEN present: the halt names the durably persisted receipt, and that
    // receipt reloads through the trusted verification path.
    let fx2 = loud_quiet_pack("reg-stopdurable");
    let out = campaign::run_campaign(&opts(&fx2, 5, true)).expect("campaign runs");
    assert!(out.campaign.stopped_early);
    let proven_child = out
        .campaign
        .children
        .iter()
        .find(|c| c.verdict == Verdict::Proven)
        .expect("a PROVEN child exists");
    let reason = out.campaign.stop_reason.clone().expect("stop reason");
    assert!(reason.contains(&proven_child.receipt_id), "{reason}");
    let store = EvidenceStore::open(&fx2.store).expect("store");
    let live = terrorbat::receipt::load_receipt(&store.run_dir(&proven_child.execution_id))
        .expect("PROVEN receipt durable before the halt");
    assert_eq!(live.verdict, Verdict::Proven);
    assert_eq!(live.receipt_id, proven_child.receipt_id);
}

// (d) The campaign summary is a pure count over the immutable child
// receipts: recomputation matches, and every child matches its live
// trusted receipt.
#[test]
fn campaign_summary_counts_equal_immutable_child_receipts() {
    let fx = loud_quiet_pack("reg-summary");
    let out = campaign::run_campaign(&opts(&fx, 2, false)).expect("campaign runs");

    assert_eq!(out.campaign.summary, summarize(&out.campaign.children));
    let total: u64 = out.campaign.summary.values().sum();
    assert_eq!(total, out.campaign.children.len() as u64);

    let store = EvidenceStore::open(&fx.store).expect("store");
    for child in &out.campaign.children {
        let live = terrorbat::receipt::load_receipt(&store.run_dir(&child.execution_id))
            .expect("child verifies");
        assert_eq!(live.receipt_id, child.receipt_id);
        assert_eq!(live.verdict, child.verdict);
        assert_eq!(live.execution.status, child.status);
        assert_eq!(live.oracle.result, child.oracle);
    }
    // The aggregate itself reloads through the trusted path.
    let reloaded = campaign::load_campaign(&store, &out.campaign_dir).expect("campaign verifies");
    assert_eq!(reloaded.campaign_id, out.campaign_id);
    assert_eq!(reloaded.summary, out.campaign.summary);
}

// (e) Repeated executions leave the source target exactly as found: clean
// status and the same HEAD as the pinned baseline.
#[test]
fn source_target_repo_clean_after_repeated_executions() {
    let fx = loud_quiet_pack("reg-clean");
    let out = campaign::run_campaign(&opts(&fx, 3, false)).expect("campaign runs");
    assert_eq!(out.campaign.children.len(), 6);

    assert!(
        repo_is_clean(&fx.repo),
        "campaign must not leave residue in the source target"
    );
    let now = terrorbat::worktree::inspect_target(&fx.repo).expect("inspect target");
    assert!(!now.dirty);
    assert_eq!(now.commit, fx.head);
    assert_eq!(now.commit, out.campaign.target_commit);
}

// (f) The pinned baseline is the only commit any child ever runs against:
// every live child receipt targets exactly the campaign's target_commit.
#[test]
fn all_children_share_the_pinned_baseline_commit() {
    let fx = loud_quiet_pack("reg-pinned");
    let out = campaign::run_campaign(&opts(&fx, 2, false)).expect("campaign runs");
    assert_eq!(out.campaign.target_commit, fx.head);

    let store = EvidenceStore::open(&fx.store).expect("store");
    for child in &out.campaign.children {
        let live = terrorbat::receipt::load_receipt(&store.run_dir(&child.execution_id))
            .expect("child verifies");
        assert_eq!(
            live.target.commit, out.campaign.target_commit,
            "child {} wandered off the baseline",
            child.execution_id
        );
    }
}

// (g) Infrastructure failure is recorded as infrastructure failure — never
// as falsification, never as success, never as a halt of the remaining
// schedule.
#[test]
fn interruption_or_infra_failure_recorded_as_such_never_as_falsification() {
    let fx = fixture(
        "reg-infra",
        &[
            ("i.yaml", infra_bat("infra")),
            ("q.yaml", quiet_bat("quiet")),
        ],
        "  - path: i.yaml\n  - path: q.yaml\n",
    );
    let out = campaign::run_campaign(&opts(&fx, 1, false)).expect("campaign runs");

    assert_eq!(out.campaign.children.len(), 2);
    let infra = &out.campaign.children[0];
    assert_eq!(infra.status, RunStatus::InfrastructureError);
    assert_eq!(infra.verdict, Verdict::InfrastructureError);
    assert_ne!(infra.verdict, Verdict::Proven);
    assert_ne!(infra.verdict, Verdict::NotObserved);
    // The schedule continued past the failure: the quiet child still ran.
    assert_eq!(out.campaign.children[1].verdict, Verdict::NotObserved);
    assert_eq!(out.campaign.summary.get("INFRASTRUCTURE ERROR"), Some(&1));
    assert_eq!(out.campaign.summary.get("PROVEN"), Some(&0));
    assert_eq!(
        terrorbat::campaign::exit_code_for_campaign(&out.campaign),
        3
    );
    assert!(repo_is_clean(&fx.repo));
}

// MS1 observation: pack entry paths resolve against the pack file's
// directory, so a pack nested in a subdirectory names its bats relative to
// itself — never relative to the process working directory.
#[test]
fn pack_in_subdirectory_resolves_bat_paths_relative_to_pack_file() {
    let dir = TempDir::new("reg-subdir");
    let bat = "version: terrorbat/v1\n\
         id: subdir-bat\n\
         claim:\n  text: Claim is falsifiable.\n\
         attack:\n  run:\n    - adapter: demo\n      action: noop\n\
         oracle:\n  all: []\n\
         evidence:\n  capture:\n    - stdout\n";
    write_spec(&dir.path, "bat.yaml", bat);
    let sub = dir.path.join("packs").join("nested");
    std::fs::create_dir_all(&sub).expect("nested pack dir");
    std::fs::write(
        sub.join("pack.yaml"),
        "version: terrorbat-pack/v1\nid: nested-pack\nbats:\n  - path: ../../bat.yaml\n",
    )
    .expect("write nested pack");

    let identified = terrorbat::pack::identify_pack_file(&sub.join("pack.yaml"))
        .expect("nested pack identifies");
    assert_eq!(identified.entries.len(), 1);
    assert_eq!(identified.entries[0].path, "../../bat.yaml");
    assert_eq!(identified.entries[0].human_id, "subdir-bat");
    let direct = terrorbat::identify_spec_file(
        &dir.path.join("bat.yaml"),
        &terrorbat::ParamOverrides::default(),
    )
    .expect("bat identifies directly");
    assert_eq!(identified.entries[0].bat, direct.identities.bat);

    // And a pack whose relative path would only resolve from the process
    // working directory (not from the pack file) is rejected.
    std::fs::write(
        sub.join("bad.yaml"),
        "version: terrorbat-pack/v1\nid: bad-pack\nbats:\n  - path: bat.yaml\n",
    )
    .expect("write bad pack");
    let err = terrorbat::pack::identify_pack_file(&sub.join("bad.yaml"))
        .expect_err("cwd-relative path must not resolve");
    assert!(err.to_string().contains("bat.yaml"), "{err}");
}
