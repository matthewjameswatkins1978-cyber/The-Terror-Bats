//! Receipts (M6): the primary product surface, and replay.
//!
//! Three separate truth layers, never recombined:
//!
//! ```text
//! WHAT HAPPENED TO THE RUN?   → execution status (RunStatus)
//! WHAT DID THE ORACLE SAY?    → OracleResult
//! WHAT MAY WE RESPONSIBLY CLAIM? → Verdict
//! ```
//!
//! Hard mapping rules (M0.1, non-negotiable):
//! crash ≠ claim falsified · timeout ≠ claim falsified · cancellation ≠
//! claim falsified · policy denial ≠ claim falsified · model opinion can
//! never create PROVEN · NotFalsified ≠ correctness.
//!
//! The receipt is content-addressed (`receipt:sha256:...`) over its
//! canonical JSON with the receipt id itself excluded from the hashed
//! content. Replay creates a NEW execution; the original receipt is never
//! mutated.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::evidence::{EvidenceRef, EvidenceStore, sha256_hex};
use crate::oracle::{ConditionOutcome, OracleResult};
use crate::runner::{
    BatBlock, CapabilitiesRecord, Captures, CleanupRecord, CleanupStatus, EnvironmentRecord,
    RunManifest, RunOptions, RunOutput, RunStatus, StepRecord, TargetRecord, run_bat,
};

pub const RECEIPT_VERSION: &str = "terrorbat/receipt/v0";

/// Epistemic verdict — what may responsibly be claimed. REPRODUCED and
/// SUSPECTED exist in the schema (architecture §4) but First Flight has no
/// legitimate evidence path that produces them; they are never manufactured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    #[serde(rename = "PROVEN")]
    Proven,
    #[serde(rename = "REPRODUCED")]
    Reproduced,
    #[serde(rename = "SUSPECTED")]
    Suspected,
    #[serde(rename = "NOT OBSERVED")]
    NotObserved,
    #[serde(rename = "INCONCLUSIVE")]
    Inconclusive,
    #[serde(rename = "INVALID")]
    Invalid,
    #[serde(rename = "INFRASTRUCTURE ERROR")]
    InfrastructureError,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Proven => "PROVEN",
            Verdict::Reproduced => "REPRODUCED",
            Verdict::Suspected => "SUSPECTED",
            Verdict::NotObserved => "NOT OBSERVED",
            Verdict::Inconclusive => "INCONCLUSIVE",
            Verdict::Invalid => "INVALID",
            Verdict::InfrastructureError => "INFRASTRUCTURE ERROR",
        }
    }

    /// One honest sentence about what this verdict does and does not mean.
    pub fn meaning(self) -> &'static str {
        match self {
            Verdict::Proven => {
                "Deterministic evidence established that the tested claim was falsified under \
                 the recorded conditions. This is not a universal proof about all states."
            }
            Verdict::Reproduced => {
                "The relevant behaviour was recreated under stated conditions, but deterministic \
                 evidence is still insufficient to establish the claim violation."
            }
            Verdict::Suspected => {
                "A plausible problem was identified but not reproducibly established."
            }
            Verdict::NotObserved => {
                "This attack did not falsify the claim. This is NOT a correctness certificate \
                 and NOT proof the system is safe."
            }
            Verdict::Inconclusive => {
                "The experiment did not produce enough information to support or reject the claim."
            }
            Verdict::Invalid => "The Bat or experiment was malformed; nothing was tested.",
            Verdict::InfrastructureError => {
                "Terror Bat or an external dependency failed in a way that invalidated the \
                 experiment. Infrastructure failure is NOT a failure of the tested claim."
            }
        }
    }
}

/// The hard mapping from mechanical layers to the responsible claim.
pub fn verdict_for(status: RunStatus, oracle: Option<OracleResult>) -> (Verdict, Option<String>) {
    match status {
        RunStatus::Invalid => (Verdict::Invalid, None),
        RunStatus::InfrastructureError => (
            Verdict::InfrastructureError,
            Some("infrastructure failure is not a failure of the tested claim".to_string()),
        ),
        RunStatus::TimedOut => (
            Verdict::Inconclusive,
            Some(
                "the run timed out; a timeout describes the run, not the system under test"
                    .to_string(),
            ),
        ),
        RunStatus::Crashed => (
            Verdict::Inconclusive,
            Some("the run crashed; a crash is not evidence that the claim failed".to_string()),
        ),
        RunStatus::Cancelled => (
            Verdict::Inconclusive,
            Some("the run was cancelled; cancellation is not evidence about the claim".to_string()),
        ),
        RunStatus::PolicyDenied => (
            Verdict::Inconclusive,
            Some(
                "a capability policy denial stopped the run; it says nothing about the claim"
                    .to_string(),
            ),
        ),
        RunStatus::Completed => match oracle {
            Some(OracleResult::Falsified) => (Verdict::Proven, None),
            Some(OracleResult::NotFalsified) => (
                Verdict::NotObserved,
                Some("NOT OBSERVED is not certification or correctness".to_string()),
            ),
            Some(OracleResult::Undetermined) => (
                Verdict::Inconclusive,
                Some("the oracle could not decide from the available evidence".to_string()),
            ),
            None => (
                Verdict::Inconclusive,
                Some("no oracle result was recorded".to_string()),
            ),
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleBlock {
    /// `None` when the oracle was not evaluated (run did not complete, or
    /// the Bat was invalid).
    pub result: Option<OracleResult>,
    pub note: Option<String>,
    pub conditions: Vec<ConditionOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionBlock {
    pub status: RunStatus,
    pub started_unix_ms: u64,
    pub finished_unix_ms: u64,
    pub wall_ms: u64,
    pub steps: Vec<StepRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub kind: String,
    pub reference: EvidenceRef,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationBlock {
    pub mode: String,
    pub guarantees: Vec<String>,
    pub non_guarantees: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReproductionBlock {
    pub repo: String,
    pub commit: String,
    pub terrorbat_version: String,
    pub git_version: String,
    pub replay_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub version: String,
    /// Filled after hashing; excluded (as empty) from the hashed content.
    pub receipt_id: String,
    pub execution_id: String,
    pub bat: BatBlock,
    pub claim_text: String,
    pub target: TargetRecord,
    pub execution: ExecutionBlock,
    pub oracle: OracleBlock,
    pub verdict: Verdict,
    pub verdict_meaning: String,
    pub evidence: Vec<EvidenceItem>,
    pub environment: EnvironmentRecord,
    pub capabilities: CapabilitiesRecord,
    pub isolation: IsolationBlock,
    pub cleanup: CleanupRecord,
    pub limitations: Vec<String>,
    pub reproduction: ReproductionBlock,
}

fn isolation_block() -> IsolationBlock {
    IsolationBlock {
        mode: "git-worktree".to_string(),
        guarantees: vec![
            "worktree mutations reversible (rollback of the disposable worktree)".to_string(),
            "worktree mutations observable (diff/status evidence)".to_string(),
            "source tree protected from Terror Bat's built-in operations".to_string(),
            "process-tree ownership within the M2 model".to_string(),
        ],
        non_guarantees: vec![
            "host filesystem containment: UNENFORCED".to_string(),
            "network containment: UNENFORCED".to_string(),
            "git remote write prohibition: UNENFORCED at host level".to_string(),
            "credential isolation: UNENFORCED".to_string(),
            "NOT hostile-code containment".to_string(),
        ],
    }
}

/// Build the receipt from the final manifest. One data model; the human
/// rendering is generated from the same struct (no second truth source).
pub fn build(manifest: &RunManifest, claim_text: &str, oracle: OracleBlock) -> Receipt {
    let (verdict, _note) = verdict_for(manifest.run_status, oracle.result);
    let mut evidence = Vec::new();
    if let Some(r) = &manifest.captures.base_snapshot {
        evidence.push(EvidenceItem {
            kind: "base_snapshot".to_string(),
            reference: r.clone(),
            truncated: false,
        });
    }
    if let Some(r) = &manifest.captures.git_status {
        evidence.push(EvidenceItem {
            kind: "git_status".to_string(),
            reference: r.clone(),
            truncated: false,
        });
    }
    if let Some(r) = &manifest.captures.git_diff {
        evidence.push(EvidenceItem {
            kind: "git_diff".to_string(),
            reference: r.clone(),
            truncated: false,
        });
    }
    for step in &manifest.steps {
        if let Some(r) = &step.stdout {
            evidence.push(EvidenceItem {
                kind: format!("{}.{}:{}:stdout", step.phase, step.adapter, step.index),
                reference: r.clone(),
                truncated: step.stdout_truncated,
            });
        }
        if let Some(r) = &step.stderr {
            evidence.push(EvidenceItem {
                kind: format!("{}.{}:{}:stderr", step.phase, step.adapter, step.index),
                reference: r.clone(),
                truncated: step.stderr_truncated,
            });
        }
    }
    Receipt {
        version: RECEIPT_VERSION.to_string(),
        receipt_id: String::new(),
        execution_id: manifest.execution_id.clone(),
        bat: manifest.bat.clone(),
        claim_text: claim_text.to_string(),
        target: manifest.target.clone(),
        execution: ExecutionBlock {
            status: manifest.run_status,
            started_unix_ms: manifest.started_unix_ms,
            finished_unix_ms: manifest.finished_unix_ms,
            wall_ms: manifest.wall_ms,
            steps: manifest.steps.clone(),
        },
        oracle,
        verdict,
        verdict_meaning: verdict.meaning().to_string(),
        evidence,
        environment: manifest.environment.clone(),
        capabilities: manifest.capabilities.clone(),
        isolation: isolation_block(),
        cleanup: manifest.worktree.cleanup.clone(),
        limitations: manifest.limitations.clone(),
        reproduction: ReproductionBlock {
            repo: manifest.target.repo.clone(),
            commit: manifest.target.commit.clone(),
            terrorbat_version: manifest.versions.terrorbat.clone(),
            git_version: manifest.versions.git.clone(),
            replay_command: String::new(), // filled once the receipt id exists
        },
    }
}

/// Content-address the receipt: canonical JSON (RFC 8785) with the receipt
/// id excluded, hashed with SHA-256.
pub fn compute_id(receipt: &Receipt) -> Result<String> {
    let mut to_hash = receipt.clone();
    to_hash.receipt_id = String::new();
    to_hash.reproduction.replay_command = String::new();
    let value = serde_json::to_value(&to_hash).map_err(|e| {
        Error::store(
            Path::new("<receipt>"),
            format!("cannot serialise receipt: {e}"),
        )
    })?;
    let bytes = crate::canonical::canonical_json(&value)?;
    Ok(format!("receipt:sha256:{}", sha256_hex(&bytes)))
}

/// Finish a receipt: compute its id and fill the replay command.
pub fn finalise(mut receipt: Receipt) -> Result<Receipt> {
    let id = compute_id(&receipt)?;
    receipt.reproduction.replay_command = format!("terrorbat replay {id}");
    receipt.receipt_id = id;
    Ok(receipt)
}

pub fn write_receipt(run_dir: &Path, receipt: &Receipt) -> Result<()> {
    let path = run_dir.join("receipt.json");
    let text = serde_json::to_string_pretty(receipt)
        .map_err(|e| Error::store(&path, format!("cannot serialise receipt: {e}")))?;
    std::fs::write(&path, text)
        .map_err(|e| Error::store(&path, format!("cannot write receipt: {e}")))
}

/// Read and deserialize a receipt WITHOUT verifying it.
///
/// Private by design: trusted product paths must use [`load_receipt`],
/// which fails closed. This helper exists only so lookup can match a
/// claimed id and then verify it — corruption is surfaced, never hidden.
fn parse_receipt_unverified(path: &Path) -> Result<Receipt> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::store(path, format!("cannot read receipt: {e}")))?;
    serde_json::from_str(text.as_str())
        .map_err(|e| Error::store(path, format!("cannot parse receipt: {e}")))
}

/// Canonical receipt-id shape: `receipt:sha256:<64 lowercase hex chars>`.
/// No other algorithm is accepted under the v0 receipt schema; future
/// identity versions require an explicit protocol/version decision.
pub fn validate_receipt_id(id: &str) -> std::result::Result<(), String> {
    let Some(hex) = id.strip_prefix("receipt:sha256:") else {
        return Err(format!(
            "malformed receipt id `{id}`: expected `receipt:sha256:<64 lowercase hex chars>`"
        ));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!(
            "malformed receipt id `{id}`: digest must be exactly 64 lowercase hex characters"
        ));
    }
    Ok(())
}

/// Verify a receipt's integrity, reusing the one existing identity
/// definition ([`compute_id`]): id format, recomputed canonical identity,
/// and the derived replay command. Fails closed; never repairs, never
/// overwrites, never continues with a warning.
fn verify_receipt(receipt: &Receipt, path: &Path) -> Result<()> {
    validate_receipt_id(&receipt.receipt_id)
        .map_err(|m| Error::receipt(path, format!("{m}\n\nCode: TB-RECEIPT-CORRUPT")))?;
    let computed = compute_id(receipt)?;
    if computed != receipt.receipt_id {
        return Err(Error::receipt(
            path,
            format!(
                "CORRUPT receipt: stored receipt id does not match canonical receipt \
                 contents.\n\nstored:\n  {}\n\ncomputed:\n  {}\n\nRefusing to inspect or \
                 replay this receipt.\n\nCode: TB-RECEIPT-CORRUPT",
                receipt.receipt_id, computed
            ),
        ));
    }
    // `replay_command` is deliberately excluded from the content hash, so it
    // cannot be trusted from the file: a tampered replay command must not
    // become a trusted executable instruction. It must equal its derivation.
    let expected = format!("terrorbat replay {}", receipt.receipt_id);
    if receipt.reproduction.replay_command != expected {
        return Err(Error::receipt(
            path,
            format!(
                "CORRUPT receipt: stored replay command does not match its derivation \
                 rule.\n\nstored:\n  {}\n\nexpected:\n  {}\n\nRefusing to inspect or replay \
                 this receipt.\n\nCode: TB-RECEIPT-CORRUPT",
                receipt.reproduction.replay_command, expected
            ),
        ));
    }
    Ok(())
}

/// Authoritative receipt load: read → deserialize → validate id format →
/// recompute canonical identity → compare → trust only if equal.
/// Evidence objects verify their digest on read; receipts do the same.
pub fn load_receipt(run_dir: &Path) -> Result<Receipt> {
    let path = run_dir.join("receipt.json");
    let receipt = parse_receipt_unverified(&path)?;
    verify_receipt(&receipt, &path)?;
    Ok(receipt)
}

/// Find a run directory by execution id or receipt id.
///
/// Receipt-id lookup is integrity-safe: a corrupt receipt claiming the
/// requested id surfaces as a TB-RECEIPT-CORRUPT error instead of
/// masquerading as "not found"; a genuine verified receipt is preferred
/// over any unverified claimant; a corrupt receipt can never redirect a
/// lookup or poison a replay.
pub fn locate_run(store: &EvidenceStore, id: &str) -> Result<Option<PathBuf>> {
    let direct = store.run_dir(id);
    if direct.join("manifest.json").exists() {
        return Ok(Some(direct));
    }
    let mut corruption: Option<Error> = None;
    for run in store.list_runs() {
        let dir = store.run_dir(&run);
        let path = dir.join("receipt.json");
        let Ok(candidate) = parse_receipt_unverified(&path) else {
            continue;
        };
        if candidate.receipt_id != id {
            continue;
        }
        match verify_receipt(&candidate, &path) {
            Ok(()) => return Ok(Some(dir)),
            Err(e) => corruption = Some(e),
        }
    }
    match corruption {
        Some(e) => Err(e),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Human rendering (generated from the same receipt data model)
// ---------------------------------------------------------------------------

pub fn render(r: &Receipt) -> String {
    let mut out = String::new();
    out.push_str(&format!("🦇 {}\n\n", r.bat.id));

    out.push_str("Claim\n");
    for line in r.claim_text.trim().lines() {
        out.push_str(&format!("  {line}\n"));
    }
    out.push('\n');

    out.push_str("Target\n");
    out.push_str(&format!("  {}\n", r.target.root));
    out.push_str(&format!("  {}\n\n", short(&r.target.commit)));

    out.push_str("Attack\n");
    for phase in ["setup", "run"] {
        let total = r
            .execution
            .steps
            .iter()
            .filter(|s| s.phase == phase)
            .count();
        let done = r
            .execution
            .steps
            .iter()
            .filter(|s| s.phase == phase && s.status == RunStatus::Completed)
            .count();
        out.push_str(&format!("  {phase:<6} {done}/{total} completed\n"));
    }
    out.push('\n');

    out.push_str("Execution\n");
    out.push_str(&format!("  {:?}\n\n", r.execution.status));

    out.push_str("Oracle\n");
    match r.oracle.result {
        Some(result) => {
            out.push_str(&format!("  {}\n", result.label()));
            for c in &r.oracle.conditions {
                let note = c
                    .note
                    .as_ref()
                    .map(|n| format!(" — {n}"))
                    .unwrap_or_default();
                out.push_str(&format!("  · {} → {:?}{note}\n", c.condition, c.result));
            }
        }
        None => {
            let note = r
                .oracle
                .note
                .as_deref()
                .unwrap_or("run did not complete; oracle not evaluated");
            out.push_str(&format!("  NOT EVALUATED ({note})\n"));
        }
    }
    out.push('\n');

    out.push_str("Verdict\n");
    out.push_str(&format!("  {}\n", r.verdict.as_str()));
    out.push_str(&format!("  {}\n\n", r.verdict_meaning));

    out.push_str("Evidence\n");
    let mut shown = 0;
    for item in &r.evidence {
        if matches!(
            item.kind.as_str(),
            "git_diff" | "git_status" | "base_snapshot"
        ) {
            let trunc = if item.truncated { " (truncated)" } else { "" };
            out.push_str(&format!("  {:<13} {}{trunc}\n", item.kind, item.reference));
            shown += 1;
        }
    }
    let streams = r.evidence.len() - shown;
    if streams > 0 {
        out.push_str(&format!(
            "  {streams} step stream object(s) in receipt.json\n"
        ));
    }
    out.push('\n');

    out.push_str("Isolation\n");
    out.push_str(&format!(
        "  disposable Git worktree ({})\n",
        r.isolation.mode
    ));
    for g in &r.isolation.guarantees {
        out.push_str(&format!("  ✓ {g}\n"));
    }
    for n in &r.isolation.non_guarantees {
        out.push_str(&format!("  ✗ {n}\n"));
    }
    out.push('\n');

    out.push_str("Cleanup\n");
    match r.cleanup.status {
        CleanupStatus::Succeeded => out.push_str("  succeeded\n"),
        CleanupStatus::NotAttempted => out.push_str("  not attempted\n"),
        CleanupStatus::Failed => {
            out.push_str("  FAILED\n");
            if let Some(err) = &r.cleanup.error {
                for line in err.lines() {
                    out.push_str(&format!("  {line}\n"));
                }
            }
        }
    }
    out.push('\n');

    if !r.limitations.is_empty() {
        out.push_str("Limitations\n");
        for l in &r.limitations {
            out.push_str(&format!("  - {l}\n"));
        }
        out.push('\n');
    }

    out.push_str("Reproduce\n");
    out.push_str(&format!("  {}\n\n", r.reproduction.replay_command));

    out.push_str(&format!("Execution  {}\n", r.execution_id));
    out.push_str(&format!("Receipt    {}\n", r.receipt_id));
    out
}

fn short(sha: &str) -> String {
    if sha.len() > 12 {
        sha[..12].to_string()
    } else {
        sha.to_string()
    }
}

// ---------------------------------------------------------------------------
// Replay (M6): same-machine, local-repository oriented.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ReplayReport {
    pub original_execution_id: String,
    pub new_execution_id: String,
    pub original_receipt_id: String,
    pub new_receipt_id: String,
    pub same_status: bool,
    pub same_oracle: bool,
    pub same_verdict: bool,
    pub evidence_changes: Vec<String>,
    pub environment_changes: Vec<String>,
    pub notes: Vec<String>,
}

/// Replay a receipt as a NEW execution. The original receipt and evidence are
/// never modified. The base commit must still exist locally; replay never
/// fetches from the network.
pub fn replay(id: &str, store_root: Option<PathBuf>) -> Result<(ReplayReport, RunOutput)> {
    let store_root = match store_root {
        Some(p) => p,
        None => EvidenceStore::default_root()?,
    };
    let store = EvidenceStore::open(&store_root)?;
    let run_dir = locate_run(&store, id)?.ok_or_else(|| {
        Error::store(
            &store_root,
            format!(
                "no run or receipt with id `{id}` in store `{}`",
                store_root.display()
            ),
        )
    })?;
    // Verified load: a tampered receipt can never seed a replay.
    let original = load_receipt(&run_dir)?;

    // Recover the Bat source: prefer the original file when it still exists
    // and still hashes to the recorded spec source (fixture fidelity for
    // `from:` references); otherwise materialise the stored bytes.
    let spec_bytes = store.get(&original.bat.spec_source)?;
    let recorded_hash = original
        .bat
        .spec_source
        .hex()
        .unwrap_or_default()
        .to_string();
    let source_matches = |p: &Path| -> bool {
        std::fs::read(p)
            .map(|b| sha256_hex(&b) == recorded_hash)
            .unwrap_or(false)
    };
    let mut notes = Vec::new();
    let spec_path = match original.bat.source_path.as_deref().map(PathBuf::from) {
        Some(p) if source_matches(&p) => p,
        other => {
            match &other {
                Some(p) => notes.push(format!(
                    "original Bat file `{}` changed or vanished; replaying from the stored \
                     spec source (`from:` fixtures relative to the original spec directory \
                     may not resolve)",
                    p.display()
                )),
                None => notes.push(
                    "no original Bat path recorded; replaying from the stored spec source"
                        .to_string(),
                ),
            }
            materialise_spec(&spec_bytes, &original.bat.id)?
        }
    };

    // Target availability: repo must exist and still contain the commit.
    let repo = PathBuf::from(&original.target.repo);
    if !repo.exists() {
        return Err(Error::store(
            &repo,
            format!(
                "replay cannot proceed: the original repository `{}` no longer exists. \
                 Replay is local-only and never fetches from the network.",
                repo.display()
            ),
        ));
    }
    let check = crate::worktree::git(
        &repo,
        &[
            "cat-file",
            "-e",
            &format!("{}^{{commit}}", original.target.commit),
        ],
    )?;
    if check.exit_code != Some(0) {
        return Err(Error::store(
            &repo,
            format!(
                "replay cannot proceed: commit `{}` no longer exists in `{}`. \
                 Replay never fetches from the network; restore the commit locally first.",
                short(&original.target.commit),
                repo.display()
            ),
        ));
    }

    let opts = RunOptions {
        bat_path: spec_path,
        repo,
        store_root: Some(store_root),
        overrides: original.bat.param_overrides.clone(),
    };
    let new_out = run_bat(&opts)?;
    let new_receipt = load_receipt(&new_out.run_dir)?;

    let report = compare(&original, &new_receipt, notes);
    Ok((report, new_out))
}

fn materialise_spec(bytes: &[u8], bat_id: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("terrorbat-replay-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| Error::store(&dir, format!("cannot create replay spec dir: {e}")))?;
    let safe_id: String = bat_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("{safe_id}.yaml"));
    std::fs::write(&path, bytes)
        .map_err(|e| Error::store(&path, format!("cannot write replay spec: {e}")))?;
    Ok(path)
}

fn compare(original: &Receipt, new: &Receipt, notes: Vec<String>) -> ReplayReport {
    let mut evidence_changes = Vec::new();
    for kind in ["base_snapshot", "git_status", "git_diff"] {
        let a = original
            .evidence
            .iter()
            .find(|e| e.kind == kind)
            .map(|e| e.reference.0.clone());
        let b = new
            .evidence
            .iter()
            .find(|e| e.kind == kind)
            .map(|e| e.reference.0.clone());
        match (a, b) {
            (Some(x), Some(y)) if x == y => evidence_changes.push(format!("{kind}: same")),
            (Some(x), Some(y)) => evidence_changes.push(format!("{kind}: CHANGED {x} → {y}")),
            (Some(_), None) => evidence_changes.push(format!("{kind}: MISSING in replay")),
            (None, Some(_)) => evidence_changes.push(format!("{kind}: NEW in replay")),
            (None, None) => evidence_changes.push(format!("{kind}: absent in both")),
        }
    }
    let mut environment_changes = Vec::new();
    for name in &original.environment.declared {
        let a = original.environment.captured.get(name).cloned().flatten();
        let b = new.environment.captured.get(name).cloned().flatten();
        if a != b {
            environment_changes.push(format!(
                "{name}: {} → {}",
                a.unwrap_or_else(|| "<not captured>".to_string()),
                b.unwrap_or_else(|| "<not captured>".to_string())
            ));
        }
    }
    ReplayReport {
        original_execution_id: original.execution_id.clone(),
        new_execution_id: new.execution_id.clone(),
        original_receipt_id: original.receipt_id.clone(),
        new_receipt_id: new.receipt_id.clone(),
        same_status: original.execution.status == new.execution.status,
        same_oracle: original.oracle.result == new.oracle.result,
        same_verdict: original.verdict == new.verdict,
        evidence_changes,
        environment_changes,
        notes,
    }
}

pub fn render_replay(report: &ReplayReport) -> String {
    let mut out = String::new();
    out.push_str("REPLAY\n");
    out.push_str(&format!(
        "  original execution  {}\n",
        report.original_execution_id
    ));
    out.push_str(&format!(
        "  new execution       {}\n",
        report.new_execution_id
    ));
    out.push_str(&format!(
        "  original receipt    {}\n",
        report.original_receipt_id
    ));
    out.push_str(&format!(
        "  new receipt         {}\n\n",
        report.new_receipt_id
    ));
    out.push_str(&format!(
        "  execution status    {}\n",
        if report.same_status {
            "same"
        } else {
            "DIFFERENT"
        }
    ));
    out.push_str(&format!(
        "  oracle result       {}\n",
        if report.same_oracle {
            "same"
        } else {
            "DIFFERENT"
        }
    ));
    out.push_str(&format!(
        "  verdict             {}\n\n",
        if report.same_verdict {
            "same"
        } else {
            "DIFFERENT"
        }
    ));
    out.push_str("Evidence comparison\n");
    for line in &report.evidence_changes {
        out.push_str(&format!("  {line}\n"));
    }
    if !report.environment_changes.is_empty() {
        out.push_str("\nRelevant environment changes\n");
        for line in &report.environment_changes {
            out.push_str(&format!("  {line}\n"));
        }
    }
    if !report.notes.is_empty() {
        out.push_str("\nNotes\n");
        for line in &report.notes {
            out.push_str(&format!("  {line}\n"));
        }
    }
    out.push_str(
        "\nA shared verdict alone does not make two executions equivalent; \
         compare the evidence identities above.\n",
    );
    out
}

/// Captures accessor kept for tests/tools that work from a manifest.
pub fn captures_of(manifest: &RunManifest) -> &Captures {
    &manifest.captures
}
