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
    BatBlock, CapabilitiesRecord, Captures, CleanupRecord, EnvironmentRecord, RunManifest,
    RunOptions, RunOutput, RunStatus, StepRecord, TargetRecord,
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
                "Terror Bats or an external dependency failed in a way that invalidated the \
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
        RunStatus::Unsupported => (
            Verdict::Inconclusive,
            Some(
                "UNSUPPORTED: a required platform capability is unavailable; this is not a target finding"
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
    #[serde(default)]
    pub processes: crate::process_runtime::ProcessRuntimeReport,
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
    #[serde(default, skip_serializing_if = "is_false")]
    pub adapter_bindings_required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_adapters: Vec<crate::adapter::AdapterProvenance>,
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

fn is_false(value: &bool) -> bool {
    !value
}

fn isolation_block() -> IsolationBlock {
    IsolationBlock {
        mode: "git-worktree".to_string(),
        guarantees: vec![
            "worktree mutations reversible (rollback of the disposable worktree)".to_string(),
            "worktree mutations observable (diff/status evidence)".to_string(),
            "source tree protected from Terror Bats' built-in operations".to_string(),
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
        if let Some(r) = &step.protocol_stdout {
            evidence.push(EvidenceItem {
                kind: format!(
                    "{}.{}:{}:protocol_stdout",
                    step.phase, step.adapter, step.index
                ),
                reference: r.clone(),
                truncated: step.protocol_stdout_truncated,
            });
        }
        if let Some(r) = &step.protocol_stderr {
            evidence.push(EvidenceItem {
                kind: format!(
                    "{}.{}:{}:protocol_stderr",
                    step.phase, step.adapter, step.index
                ),
                reference: r.clone(),
                truncated: step.protocol_stderr_truncated,
            });
        }
        if let Some(r) = step
            .adapter_provenance
            .as_ref()
            .and_then(|p| p.describe_stderr.as_ref())
        {
            evidence.push(EvidenceItem {
                kind: format!(
                    "{}.{}:{}:describe_stderr",
                    step.phase, step.adapter, step.index
                ),
                reference: r.clone(),
                truncated: false,
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
            processes: manifest.processes.clone(),
        },
        adapter_bindings_required: manifest.adapter_bindings_required,
        resolved_adapters: manifest.resolved_adapters.clone(),
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
fn replay_command(receipt: &Receipt, id: &str) -> String {
    let has_external = receipt.adapter_bindings_required
        || receipt
            .execution
            .steps
            .iter()
            .any(|step| step.adapter_provenance.is_some());
    if has_external {
        format!("terrorbats replay {id} --adapters adapters.yaml")
    } else {
        format!("terrorbats replay {id}")
    }
}

pub fn finalise(mut receipt: Receipt) -> Result<Receipt> {
    let id = compute_id(&receipt)?;
    receipt.reproduction.replay_command = replay_command(&receipt, &id);
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
    let expected = replay_command(receipt, &receipt.receipt_id);
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
    pub same_process_semantics: bool,
    pub evidence_changes: Vec<String>,
    pub environment_changes: Vec<String>,
    pub notes: Vec<String>,
}

/// Replay a receipt as a NEW execution. The original receipt and evidence are
/// never modified. The base commit must still exist locally; replay never
/// fetches from the network.
pub fn replay(id: &str, store_root: Option<PathBuf>) -> Result<(ReplayReport, RunOutput)> {
    replay_with_adapters(id, store_root, None)
}

pub fn replay_with_adapters(
    id: &str,
    store_root: Option<PathBuf>,
    bindings_path: Option<&Path>,
) -> Result<(ReplayReport, RunOutput)> {
    let bindings = crate::adapter::load_bindings(bindings_path)
        .map_err(|e| Error::spec(bindings_path.unwrap_or(Path::new("<adapters>")), e))?;
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

    let mut expected_adapters: std::collections::BTreeMap<String, String> = original
        .resolved_adapters
        .iter()
        .map(|adapter| (adapter.name.clone(), adapter.description_id.clone()))
        .collect();
    // Receipts predating resolved-adapter provenance can only identify
    // adapters whose steps executed; retain that backward-compatible floor.
    if expected_adapters.is_empty() {
        expected_adapters.extend(original.execution.steps.iter().filter_map(|step| {
            step.adapter_provenance
                .as_ref()
                .map(|p| (p.name.clone(), p.description_id.clone()))
        }));
    }
    if original.adapter_bindings_required && bindings.is_none() {
        return Err(Error::receipt(
            &run_dir,
            "external-adapter replay requires explicit `--adapters <file>` bindings",
        ));
    }
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
                original.target.commit.chars().take(12).collect::<String>(),
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
    let new_out = crate::runner::run_bat_with_adapter_expectations(
        &opts,
        bindings.as_ref(),
        (!expected_adapters.is_empty()).then_some(&expected_adapters),
    )?;
    if new_out
        .manifest
        .limitations
        .iter()
        .any(|note| note.starts_with("adapter description identities changed:"))
    {
        return Err(Error::receipt(
            &new_out.run_dir,
            "adapter description identity mismatch; refusing replay",
        ));
    }
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
        same_process_semantics: same_process_semantics(
            &original.execution.processes,
            &new.execution.processes,
            &original.execution.steps,
            &new.execution.steps,
        ),
        evidence_changes,
        environment_changes,
        notes,
    }
}

fn same_process_semantics(
    original: &crate::process_runtime::ProcessRuntimeReport,
    replay: &crate::process_runtime::ProcessRuntimeReport,
    original_steps: &[crate::runner::StepRecord],
    replay_steps: &[crate::runner::StepRecord],
) -> bool {
    if original.cleanup.attempted != replay.cleanup.attempted
        || original.cleanup.terminated.len() != replay.cleanup.terminated.len()
        || original.cleanup.forced.len() != replay.cleanup.forced.len()
        || original.cleanup.survivors.len() != replay.cleanup.survivors.len()
        || original.cleanup.errors.len() != replay.cleanup.errors.len()
        || original.handles.len() != replay.handles.len()
        || !same_process_steps(original_steps, replay_steps)
    {
        return false;
    }
    original.handles.iter().zip(&replay.handles).all(|(a, b)| {
        a.handle == b.handle
            && a.generations.len() == b.generations.len()
            && a.generations.iter().zip(&b.generations).all(|(x, y)| {
                x.generation == y.generation
                    && x.termination == y.termination
                    && x.exit_code == y.exit_code
                    && x.signal == y.signal
                    && x.events_truncated == y.events_truncated
                    && x.events.len() == y.events.len()
                    && x.events.iter().zip(&y.events).all(|(u, v)| {
                        u.phase == v.phase
                            && u.step_index == v.step_index
                            && u.event == v.event
                            && u.detail == v.detail
                    })
            })
    })
}

fn same_process_steps(
    original: &[crate::runner::StepRecord],
    replay: &[crate::runner::StepRecord],
) -> bool {
    let original = original.iter().filter(|step| step.adapter == "process");
    let replay = replay.iter().filter(|step| step.adapter == "process");
    let mut original = original.peekable();
    let mut replay = replay.peekable();
    loop {
        match (original.next(), replay.next()) {
            (Some(a), Some(b)) => {
                if a.phase != b.phase
                    || a.index != b.index
                    || a.action != b.action
                    || a.payload != b.payload
                    || a.status != b.status
                    || a.exit_code != b.exit_code
                    || a.signal != b.signal
                    || a.stdout.as_ref().map(|r| &r.0) != b.stdout.as_ref().map(|r| &r.0)
                    || a.stdout_total_bytes != b.stdout_total_bytes
                    || a.stdout_truncated != b.stdout_truncated
                    || a.stderr.as_ref().map(|r| &r.0) != b.stderr.as_ref().map(|r| &r.0)
                    || a.stderr_total_bytes != b.stderr_total_bytes
                    || a.stderr_truncated != b.stderr_truncated
                {
                    return false;
                }
            }
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// Captures accessor kept for tests/tools that work from a manifest.
pub fn captures_of(manifest: &RunManifest) -> &Captures {
    &manifest.captures
}
