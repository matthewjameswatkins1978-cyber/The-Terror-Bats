//! Bat Campaign v1: serial pack execution with durable tamper-evident
//! campaign receipts and trusted inspection.
//!
//! A campaign is NOT a retry mechanism: every child is a first-class
//! ordinary run (own execution id, evidence, receipt) in the same store.
//! Later results never overwrite earlier ones — the campaign receipt only
//! *references* child receipts, never copies their evidence payloads.
//!
//! Runtime flow:
//!
//! ```text
//! resolve pack → pin target HEAD → for each iteration × entry (in order):
//!   re-inspect target (dirty or moved HEAD refuses the campaign)
//!   → run_bat (ordinary run, durably persisted)
//!   → trusted reload of the child receipt
//!   → stop only after a durably persisted PROVEN (with --stop-on-proven)
//! → content-addressed campaign receipt in campaigns/<uuid>/campaign.json
//! ```
//!
//! Serial only. No parallelism, no caching, no minimiser, no adapters.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::evidence::{EvidenceStore, sha256_hex};
use crate::oracle::OracleResult;
use crate::receipt::Verdict;
use crate::runner::{RunOptions, RunStatus};

/// The only campaign receipt schema version accepted by W2 MS2.
pub const CAMPAIGN_VERSION: &str = "terrorbat/campaign/v1";

/// Upper bound on `--runs`. Campaigns are serial full executions: the bound
/// keeps them finite and reviewable. Larger counts are rejected as absurd
/// rather than silently accepted.
pub const MAX_CAMPAIGN_RUNS: u64 = 100;

/// Verdict labels in stable presentation order. The summary always carries
/// every label (zeros included) so summary tampering is detectable.
pub const VERDICT_LABELS: [&str; 7] = [
    "PROVEN",
    "REPRODUCED",
    "SUSPECTED",
    "NOT OBSERVED",
    "INCONCLUSIVE",
    "INVALID",
    "INFRASTRUCTURE ERROR",
];

/// One campaign child: a reference to an ordinary run, never a copy of its
/// evidence. `iteration` is 1-based; `entry_index` is the 0-based position
/// in the pack's declared order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignChildRecord {
    pub iteration: u64,
    pub entry_index: usize,
    pub bat: String,
    pub bat_id: String,
    pub execution_id: String,
    pub receipt_id: String,
    pub status: RunStatus,
    pub oracle: Option<OracleResult>,
    pub verdict: Verdict,
}

/// The durable campaign aggregate (schema `terrorbat/campaign/v1`).
///
/// Identity (`campaign:sha256:<hex>`) covers the semantic projection with
/// the campaign id itself excluded. Human labels (`pack_id`, per-child
/// `bat_id`) are recorded for humans but excluded from the hashed content,
/// exactly like pack identity excludes human metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignReceipt {
    pub version: String,
    /// Filled after hashing; excluded (as empty) from the hashed content.
    pub campaign_id: String,
    /// Pack content identity (`pack:sha256:...`).
    pub pack: String,
    /// Human pack label. NOT part of campaign identity.
    pub pack_id: String,
    pub target_repo: String,
    /// Pinned baseline commit: every child ran against exactly this commit.
    pub target_commit: String,
    pub requested_runs: u64,
    /// Iterations in which every pack entry ran. A stop-on-proven halt
    /// mid-iteration leaves that partial iteration uncounted; its children
    /// are still recorded individually below.
    pub completed_runs: u64,
    pub stopped_early: bool,
    pub stop_reason: Option<String>,
    pub children: Vec<CampaignChildRecord>,
    /// Per-verdict counts over the children (all labels, zeros included).
    pub summary: BTreeMap<String, u64>,
}

/// Options for [`run_campaign`]. Mirrors [`RunOptions`](crate::runner::RunOptions).
pub struct CampaignOptions {
    pub pack_path: PathBuf,
    pub repo: PathBuf,
    pub store_root: Option<PathBuf>,
    pub runs: u64,
    pub stop_on_proven: bool,
}

#[derive(Debug)]
pub struct CampaignOutput {
    pub campaign_id: String,
    pub campaign_dir: PathBuf,
    pub campaign: CampaignReceipt,
}

/// Validate a `--runs` count: a positive bounded integer. Zero, and anything
/// above [`MAX_CAMPAIGN_RUNS`], is an invalid request. There are no infinite
/// or duration-based modes.
pub fn validate_runs(runs: u64) -> Result<u64> {
    if runs == 0 {
        return Err(Error::spec(
            Path::new("<campaign>"),
            "--runs must be a positive integer (got 0)",
        ));
    }
    if runs > MAX_CAMPAIGN_RUNS {
        return Err(Error::spec(
            Path::new("<campaign>"),
            format!(
                "--runs {runs} exceeds the serial-campaign bound of {MAX_CAMPAIGN_RUNS}; \
                 split the work into smaller bounded campaigns"
            ),
        ));
    }
    Ok(runs)
}

/// Canonical campaign-id shape: `campaign:sha256:<64 lowercase hex chars>`.
pub fn validate_campaign_id(id: &str) -> std::result::Result<(), String> {
    let Some(hex) = id.strip_prefix("campaign:sha256:") else {
        return Err(format!(
            "malformed campaign id `{id}`: expected `campaign:sha256:<64 lowercase hex chars>`"
        ));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!(
            "malformed campaign id `{id}`: digest must be exactly 64 lowercase hex characters"
        ));
    }
    Ok(())
}

/// Build the semantic projection hashed for the campaign identity. Human
/// labels and the campaign id itself are excluded; everything else —
/// including per-child execution/receipt ids and the derived summary — is
/// covered, so tampering with any of it breaks the identity.
fn projection(c: &CampaignReceipt) -> Value {
    let children: Vec<Value> = c
        .children
        .iter()
        .map(|ch| {
            json!({
                "iteration": ch.iteration,
                "entry_index": ch.entry_index,
                "bat": ch.bat,
                "execution_id": ch.execution_id,
                "receipt_id": ch.receipt_id,
                "status": format!("{:?}", ch.status),
                "oracle": ch.oracle.map(|o| format!("{o:?}")),
                "verdict": ch.verdict.as_str(),
            })
        })
        .collect();
    json!({
        "version": c.version,
        "pack": c.pack,
        "target_repo": c.target_repo,
        "target_commit": c.target_commit,
        "requested_runs": c.requested_runs,
        "completed_runs": c.completed_runs,
        "stopped_early": c.stopped_early,
        "stop_reason": c.stop_reason,
        "children": children,
        "summary": c.summary,
    })
}

/// Content-address the campaign: canonical JSON (RFC 8785) of the semantic
/// projection, hashed with SHA-256. Mirrors
/// [`compute_id`](crate::receipt::compute_id).
pub fn compute_id(campaign: &CampaignReceipt) -> Result<String> {
    let bytes = crate::canonical::canonical_json(&projection(campaign))?;
    Ok(format!("campaign:sha256:{}", sha256_hex(&bytes)))
}

/// Finish a campaign receipt: compute its id over the content.
pub fn finalise(mut campaign: CampaignReceipt) -> Result<CampaignReceipt> {
    campaign.campaign_id = compute_id(&campaign)?;
    Ok(campaign)
}

pub fn write_campaign(campaign_dir: &Path, campaign: &CampaignReceipt) -> Result<()> {
    let path = campaign_dir.join("campaign.json");
    let text = serde_json::to_string_pretty(campaign)
        .map_err(|e| Error::store(&path, format!("cannot serialise campaign: {e}")))?;
    std::fs::write(&path, text)
        .map_err(|e| Error::store(&path, format!("cannot write campaign: {e}")))
}

/// Read and deserialize a campaign WITHOUT verifying it.
///
/// Private by design: trusted product paths must use [`load_campaign`],
/// which fails closed — including re-verifying every referenced child.
fn parse_campaign_unverified(path: &Path) -> Result<CampaignReceipt> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::store(path, format!("cannot read campaign: {e}")))?;
    serde_json::from_str(text.as_str())
        .map_err(|e| Error::store(path, format!("cannot parse campaign: {e}")))
}

/// Verify a campaign's own integrity: id format, then recomputed canonical
/// identity. Fails closed; never repairs, never warns-and-continues.
fn verify_campaign(campaign: &CampaignReceipt, path: &Path) -> Result<()> {
    validate_campaign_id(&campaign.campaign_id)
        .map_err(|m| Error::campaign(path, format!("{m}\n\nCode: TB-CAMPAIGN-CORRUPT")))?;
    let computed = compute_id(campaign)?;
    if computed != campaign.campaign_id {
        return Err(Error::campaign(
            path,
            format!(
                "CORRUPT campaign: stored campaign id does not match canonical campaign \
                 contents.\n\nstored:\n  {}\n\ncomputed:\n  {}\n\nRefusing to inspect this \
                 campaign.\n\nCode: TB-CAMPAIGN-CORRUPT",
                campaign.campaign_id, computed
            ),
        ));
    }
    Ok(())
}

/// Authoritative campaign load: read → deserialize → validate id format →
/// recompute canonical identity → compare → re-verify every referenced
/// child through the trusted receipt path → trust only if all equal.
///
/// A corrupt campaign, a corrupt child, a child whose live receipt no longer
/// matches the campaign record, or a missing child all refuse the aggregate
/// with a TB-CAMPAIGN-CORRUPT error. Success is never rendered over
/// corruption; missing children are named explicitly.
pub fn load_campaign(store: &EvidenceStore, campaign_dir: &Path) -> Result<CampaignReceipt> {
    let path = campaign_dir.join("campaign.json");
    let campaign = parse_campaign_unverified(&path)?;
    verify_campaign(&campaign, &path)?;
    for child in &campaign.children {
        let run_dir = store.run_dir(&child.execution_id);
        if !run_dir.join("manifest.json").exists() {
            return Err(Error::campaign(
                &path,
                format!(
                    "CORRUPT campaign: child execution `{}` (iteration {} entry {}, receipt {}) \
                     is missing from the store — its run directory no longer exists. The \
                     aggregate cannot be trusted without every child.\n\nCode: \
                     TB-CAMPAIGN-CORRUPT",
                    child.execution_id, child.iteration, child.entry_index, child.receipt_id,
                ),
            ));
        }
        // Trusted path: fails closed on any child tampering.
        let live = crate::receipt::load_receipt(&run_dir).map_err(|e| {
            Error::campaign(
                &path,
                format!(
                    "CORRUPT campaign: child execution `{}` (receipt {}) fails trusted \
                     verification: {e}\n\nCode: TB-CAMPAIGN-CORRUPT",
                    child.execution_id, child.receipt_id,
                ),
            )
        })?;
        if live.receipt_id != child.receipt_id
            || live.execution_id != child.execution_id
            || live.verdict != child.verdict
            || live.execution.status != child.status
            || live.oracle.result != child.oracle
        {
            return Err(Error::campaign(
                &path,
                format!(
                    "CORRUPT campaign: child execution `{}` no longer matches its live \
                     receipt (receipt id, status, oracle result, or verdict changed).\n\nCode: \
                     TB-CAMPAIGN-CORRUPT",
                    child.execution_id,
                ),
            ));
        }
        if live.target.commit != campaign.target_commit {
            return Err(Error::campaign(
                &path,
                format!(
                    "CORRUPT campaign: child execution `{}` ran against commit `{}` but the \
                     campaign baseline is `{}`. A campaign never wanders across \
                     commits.\n\nCode: TB-CAMPAIGN-CORRUPT",
                    child.execution_id,
                    short_commit(&live.target.commit),
                    short_commit(&campaign.target_commit),
                ),
            ));
        }
    }
    let expected = summarize(&campaign.children);
    if expected != campaign.summary {
        return Err(Error::campaign(
            &path,
            "CORRUPT campaign: stored summary counts do not match the child records.\n\nCode: \
             TB-CAMPAIGN-CORRUPT",
        ));
    }
    Ok(campaign)
}

/// Find a campaign directory by campaign id.
///
/// Integrity-safe like [`locate_run`](crate::receipt::locate_run): a corrupt
/// campaign claiming the requested id surfaces as TB-CAMPAIGN-CORRUPT
/// instead of masquerading as "not found"; a genuine verified campaign is
/// preferred over any unverified claimant.
pub fn locate_campaign(store: &EvidenceStore, id: &str) -> Result<Option<PathBuf>> {
    let mut corruption: Option<Error> = None;
    for uuid in store.list_campaigns() {
        let dir = store.campaign_dir(&uuid);
        let path = dir.join("campaign.json");
        let Ok(candidate) = parse_campaign_unverified(&path) else {
            continue;
        };
        if candidate.campaign_id != id {
            continue;
        }
        match load_campaign(store, &dir) {
            Ok(_) => return Ok(Some(dir)),
            Err(e) => corruption = Some(e),
        }
    }
    match corruption {
        Some(e) => Err(e),
        None => Ok(None),
    }
}

fn short_commit(commit: &str) -> String {
    commit.chars().take(12).collect()
}

/// Per-verdict counts over child records (all labels, zeros included).
pub fn summarize(children: &[CampaignChildRecord]) -> BTreeMap<String, u64> {
    let mut summary: BTreeMap<String, u64> =
        VERDICT_LABELS.iter().map(|l| (l.to_string(), 0)).collect();
    for child in children {
        *summary
            .entry(child.verdict.as_str().to_string())
            .or_insert(0) += 1;
    }
    summary
}

/// Campaign-level exit code, following
/// [`exit_code_for`](crate::runner::exit_code_for) conventions: any PROVEN
/// child is exit 1; a malformed aggregate or a child that never tested
/// anything properly (Invalid / PolicyDenied) is exit 2; an aggregate with
/// nothing but NOT OBSERVED is exit 0; everything else is inconclusive or
/// infrastructure (exit 3). The campaign receipt stays authoritative — the
/// exit code is only a summary.
pub fn exit_code_for_campaign(campaign: &CampaignReceipt) -> u8 {
    if campaign.children.is_empty() {
        return 2;
    }
    if campaign
        .children
        .iter()
        .any(|c| c.verdict == Verdict::Proven)
    {
        return 1;
    }
    if campaign
        .children
        .iter()
        .any(|c| matches!(c.status, RunStatus::Invalid | RunStatus::PolicyDenied))
    {
        return 2;
    }
    if campaign
        .children
        .iter()
        .all(|c| c.verdict == Verdict::NotObserved)
    {
        return 0;
    }
    3
}

/// Execute a pack serially as a campaign.
///
/// Every child is an ordinary [`run_bat`](crate::runner::run_bat) execution
/// in the same store. The target HEAD is pinned at campaign start and
/// re-checked before every child: a dirty target or a moved HEAD refuses
/// the campaign (already-completed children persist as ordinary runs, but
/// no campaign receipt is written for a refused campaign).
pub fn run_campaign(opts: &CampaignOptions) -> Result<CampaignOutput> {
    validate_runs(opts.runs)?;
    let identified = crate::pack::identify_pack_file(&opts.pack_path)?;

    let store_root = match &opts.store_root {
        Some(p) => p.clone(),
        None => EvidenceStore::default_root()?,
    };
    let store = EvidenceStore::open(&store_root)?;

    // Pin the target baseline. Dirty targets are refused before anything runs.
    let baseline = crate::worktree::inspect_target(&opts.repo)?;
    if baseline.dirty {
        let preview: String = baseline
            .status_porcelain
            .lines()
            .take(10)
            .collect::<Vec<_>>()
            .join("\n");
        return Err(Error::spec(
            &opts.repo,
            format!(
                "Target repository is dirty.\n\n\
                 Terror Bat will not run a campaign over local changes.\n\n\
                 Resolve the target state before running this campaign.\n\n\
                 Offending entries (first 10):\n{preview}"
            ),
        ));
    }
    let pinned_commit = baseline.commit.clone();
    let target_repo = baseline.root.to_string_lossy().to_string();

    let campaign_uuid = uuid::Uuid::new_v4().to_string();
    let campaign_dir = store.create_campaign_dir(&campaign_uuid)?;

    let mut children: Vec<CampaignChildRecord> = Vec::new();
    let mut completed_runs: u64 = 0;
    let mut stopped_early = false;
    let mut stop_reason: Option<String> = None;

    let mut halted = false;
    for iteration in 1..=opts.runs {
        if halted {
            break;
        }
        for (entry_index, entry) in identified.entries.iter().enumerate() {
            // Re-pin before EVERY child: the campaign never wanders.
            let current = crate::worktree::inspect_target(&opts.repo)?;
            if current.dirty {
                return Err(Error::spec(
                    &opts.repo,
                    format!(
                        "Target repository became dirty during the campaign (before iteration \
                         {iteration} entry {entry_index}). Refusing the campaign: the {} \
                         completed child run(s) persist as ordinary runs, but no campaign \
                         receipt is written for a campaign that cannot pin its baseline.",
                        children.len()
                    ),
                ));
            }
            if current.commit != pinned_commit {
                return Err(Error::spec(
                    &opts.repo,
                    format!(
                        "Target HEAD moved during the campaign (before iteration {iteration} \
                         entry {entry_index}): baseline `{}` vs current `{}`. Refusing the \
                         campaign: the {} completed child run(s) persist as ordinary runs, \
                         but no campaign receipt is written for a campaign that wanders \
                         across commits.",
                        short_commit(&pinned_commit),
                        short_commit(&current.commit),
                        children.len()
                    ),
                ));
            }
            let run_opts = RunOptions {
                bat_path: entry.resolved.clone(),
                repo: opts.repo.clone(),
                store_root: Some(store_root.clone()),
                overrides: entry.overrides.clone(),
            };
            let out = crate::runner::run_bat(&run_opts)?;
            // run_bat durably persists the receipt before returning; reload
            // through the trusted path so only verified content enters the
            // campaign record.
            let receipt = crate::receipt::load_receipt(&out.run_dir)?;
            let proven = receipt.verdict == Verdict::Proven;
            let receipt_id = receipt.receipt_id.clone();
            children.push(CampaignChildRecord {
                iteration,
                entry_index,
                bat: entry.bat.clone(),
                bat_id: entry.human_id.clone(),
                execution_id: out.execution_id.clone(),
                receipt_id: receipt_id.clone(),
                status: out.manifest.run_status,
                oracle: receipt.oracle.result,
                verdict: receipt.verdict,
            });
            if opts.stop_on_proven && proven {
                stopped_early = true;
                stop_reason = Some(format!(
                    "stop-on-proven: iteration {iteration} entry {entry_index} ({}) returned \
                     PROVEN (receipt {receipt_id}); the receipt was durably persisted before \
                     halting",
                    entry.human_id,
                ));
                halted = true;
                break;
            }
        }
        if !halted {
            completed_runs = iteration;
        }
    }

    let summary = summarize(&children);
    let campaign = finalise(CampaignReceipt {
        version: CAMPAIGN_VERSION.to_string(),
        campaign_id: String::new(),
        pack: identified.identity.clone(),
        pack_id: identified.human_id.clone(),
        target_repo,
        target_commit: pinned_commit,
        requested_runs: opts.runs,
        completed_runs,
        stopped_early,
        stop_reason,
        children,
        summary,
    })?;
    write_campaign(&campaign_dir, &campaign)?;
    // Trusted reload proves the receipt is durably readable before we claim it.
    let verified = load_campaign(&store, &campaign_dir)?;
    Ok(CampaignOutput {
        campaign_id: verified.campaign_id.clone(),
        campaign_dir,
        campaign: verified,
    })
}
