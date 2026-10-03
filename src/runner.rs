//! Bat attack execution (M3) with durable evidence (M4).
//!
//! Runtime flow:
//!
//! ```text
//! parse / resolve / identify
//!   → target repository preflight (clean-only, exact commit pinned)
//!   → capability declaration preflight
//!   → disposable Git worktree (%TEMP%\terrorbat\<execution-id>\worktree)
//!   → attack.setup[] then attack.run[] (strict order, bounded budgets)
//!   → post-state capture (status / intent-to-add / diff / untracked)
//!   → worktree cleanup (bounded retry, truthful result)
//!   → manifest + operation log finalisation
//! ```
//!
//! Run-level statuses are mechanical only (Completed / TimedOut / Crashed /
//! Cancelled / PolicyDenied / Invalid / InfrastructureError). Epistemic
//! verdicts belong to receipts, never here.
//!
//! Durability claim (narrow, honest): evidence finalised by Terror Bat
//! survives failure of supervised child processes. Power-loss durability is
//! not claimed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::adapter::{self, AdapterProvenance, ResolvedAdapters};
use crate::builtins::{self, StepCtx, StepError};
use crate::error::{Error, Result};
use crate::evidence::{EvidenceRef, EvidenceStore, OperationLog, unix_ms};
use crate::identity::Identities;
use crate::oracle::{self, EvalCtx, OracleEvaluation, OracleExpr};
use crate::receipt::{self, OracleBlock, Verdict};
use crate::spec::{BatSpec, Capability};
use crate::supervisor::{CancelToken, ExecutionStatus, SupervisedCommand};
use crate::worktree::{self, TargetState};
use crate::{IdentifiedSpec, ParamOverrides};

pub const TERRORBAT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Mechanical run-level status. Never an epistemic verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Completed,
    TimedOut,
    Crashed,
    Cancelled,
    PolicyDenied,
    Invalid,
    InfrastructureError,
}

impl From<ExecutionStatus> for RunStatus {
    fn from(s: ExecutionStatus) -> RunStatus {
        match s {
            ExecutionStatus::Completed => RunStatus::Completed,
            ExecutionStatus::TimedOut => RunStatus::TimedOut,
            ExecutionStatus::Crashed => RunStatus::Crashed,
            ExecutionStatus::Cancelled => RunStatus::Cancelled,
            ExecutionStatus::InfrastructureError => RunStatus::InfrastructureError,
        }
    }
}

impl From<&StepError> for RunStatus {
    fn from(e: &StepError) -> RunStatus {
        match e {
            StepError::Policy(_) => RunStatus::PolicyDenied,
            StepError::Malformed(_) => RunStatus::Invalid,
            StepError::Io(_) => RunStatus::InfrastructureError,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub phase: String,
    pub index: usize,
    pub adapter: String,
    pub action: String,
    pub payload: Value,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: Option<EvidenceRef>,
    pub stdout_total_bytes: u64,
    pub stdout_truncated: bool,
    pub stderr: Option<EvidenceRef>,
    pub stderr_total_bytes: u64,
    pub stderr_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_provenance: Option<AdapterProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_stdout: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_stderr: Option<EvidenceRef>,
    pub wall_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStatus {
    Succeeded,
    Failed,
    NotAttempted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupRecord {
    pub status: CleanupStatus,
    /// Preserved when cleanup failed: the leftover path for manual recovery.
    pub path: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Captures {
    pub base_snapshot: Option<EvidenceRef>,
    pub git_status: Option<EvidenceRef>,
    pub git_diff: Option<EvidenceRef>,
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatBlock {
    pub id: String,
    pub bat_sha: String,
    pub claim_sha: String,
    pub attack_sha: String,
    pub oracle_sha: String,
    /// Original YAML source, content-addressed (replay input).
    pub spec_source: EvidenceRef,
    /// Canonical semantic projection, content-addressed.
    pub canonical: EvidenceRef,
    pub param_overrides: Vec<String>,
    /// Original Bat file path, recorded for replay fidelity.
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetRecord {
    pub repo: String,
    pub root: String,
    pub commit: String,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesRecord {
    pub declared_requires: Vec<String>,
    pub declared_forbids: Vec<String>,
    /// What machinery actually enforces in worktree mode.
    pub enforced: Vec<String>,
    /// What is advisory only. Never rendered as blocked.
    pub unenforced: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentRecord {
    pub declared: Vec<String>,
    /// Symbolic name → captured value, or null when Terror Bat does not
    /// capture that fact in First Flight.
    pub captured: BTreeMap<String, Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionsRecord {
    pub terrorbat: String,
    pub git: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeRecord {
    pub path: Option<String>,
    pub cleanup: CleanupRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    pub version: String,
    pub execution_id: String,
    pub bat: BatBlock,
    pub target: TargetRecord,
    pub started_unix_ms: u64,
    pub finished_unix_ms: u64,
    pub wall_ms: u64,
    pub run_status: RunStatus,
    pub steps: Vec<StepRecord>,
    pub captures: Captures,
    pub worktree: WorktreeRecord,
    pub capabilities: CapabilitiesRecord,
    pub environment: EnvironmentRecord,
    pub versions: VersionsRecord,
    pub limitations: Vec<String>,
    pub run_dir: String,
    /// Oracle evaluation (M5); `None` when the oracle was not evaluated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle: Option<OracleBlock>,
    /// Epistemic verdict (M6); `None` only for pre-M6 manifests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub adapter_bindings_required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_adapters: Vec<adapter::AdapterProvenance>,
}

pub struct RunOptions {
    pub bat_path: PathBuf,
    pub repo: PathBuf,
    pub store_root: Option<PathBuf>,
    pub overrides: Vec<String>,
}

#[derive(Debug)]
pub struct RunOutput {
    pub execution_id: String,
    pub manifest: RunManifest,
    pub run_dir: PathBuf,
    pub receipt: receipt::Receipt,
}

/// Execute one Bat against one target repository.
pub fn run_bat(opts: &RunOptions) -> Result<RunOutput> {
    run_bat_with_adapters(opts, None)
}

pub fn run_bat_with_adapters(
    opts: &RunOptions,
    bindings: Option<&adapter::BindingsFile>,
) -> Result<RunOutput> {
    run_bat_with_adapter_expectations(opts, bindings, None)
}

pub fn run_bat_with_adapter_expectations(
    opts: &RunOptions,
    bindings: Option<&adapter::BindingsFile>,
    expected: Option<&BTreeMap<String, String>>,
) -> Result<RunOutput> {
    let started_unix = unix_ms() as u64;
    let started = Instant::now();

    // --- parse / resolve / identify -------------------------------------
    let yaml = std::fs::read_to_string(&opts.bat_path).map_err(|e| Error::io(&opts.bat_path, e))?;
    let spec_dir = opts
        .bat_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let overrides = ParamOverrides::parse(&opts.overrides)?;
    let spec = crate::spec::parse_spec(&yaml, &opts.bat_path, &overrides)?;
    let identified: IdentifiedSpec = crate::identify_spec(&spec)?;
    let claim_text = spec.claim.text.clone();

    // --- target preflight: clean-only policy -----------------------------
    let target = worktree::inspect_target(&opts.repo)?;
    if target.dirty {
        let preview: String = target
            .status_porcelain
            .lines()
            .take(10)
            .collect::<Vec<_>>()
            .join("\n");
        return Err(Error::spec(
            &opts.repo,
            format!(
                "Target repository is dirty.\n\n\
                 Terror Bat will not silently exclude or incorporate local changes.\n\n\
                 Resolve the target state before running this Bat.\n\n\
                 Offending entries (first 10):\n{preview}"
            ),
        ));
    }

    // --- store + execution identity --------------------------------------
    let store_root = match &opts.store_root {
        Some(p) => p.clone(),
        None => EvidenceStore::default_root()?,
    };
    let store = EvidenceStore::open(&store_root)?;
    let execution_id = uuid::Uuid::new_v4().to_string();
    let run_dir = store.create_run_dir(&execution_id)?;
    let mut oplog = OperationLog::create(&run_dir)?;
    oplog.record(
        "run_start",
        json!({
            "execution_id": execution_id,
            "bat": identified.human_id,
            "bat_sha": identified.identities.bat,
            "target_root": target.root.to_string_lossy(),
            "target_commit": target.commit,
        }),
    )?;

    let spec_source_ev = store.put(yaml.as_bytes())?;
    let canonical_ev = store.put(identified.canonical_json.as_bytes())?;

    let bat_block = BatBlock {
        id: spec.id.clone(),
        bat_sha: identified.identities.bat.clone(),
        claim_sha: identified.identities.claim.clone(),
        attack_sha: identified.identities.attack.clone(),
        oracle_sha: identified.identities.oracle.clone(),
        spec_source: spec_source_ev,
        canonical: canonical_ev,
        param_overrides: opts.overrides.clone(),
        source_path: Some(opts.bat_path.to_string_lossy().to_string()),
    };
    let target_record = TargetRecord {
        repo: opts.repo.to_string_lossy().to_string(),
        root: target.root.to_string_lossy().to_string(),
        commit: target.commit.clone(),
        branch: target.branch.clone(),
    };
    let git_version = capture_git_version(&target.root);
    let versions = VersionsRecord {
        terrorbat: TERRORBAT_VERSION.to_string(),
        git: git_version.clone(),
    };
    let environment = EnvironmentRecord {
        declared: spec
            .environment
            .as_ref()
            .map(|e| e.relevant.clone())
            .unwrap_or_default(),
        captured: capture_environment(
            &spec
                .environment
                .as_ref()
                .map(|e| e.relevant.clone())
                .unwrap_or_default(),
            &git_version,
        ),
    };
    let capabilities = capabilities_record(&spec);
    let mut limitations = base_limitations();

    let resolved_adapter_provenance = std::cell::RefCell::new(Vec::new());
    // Early-exit finaliser: every path through this function leaves a
    // truthful manifest and receipt behind.
    let finish = |run_status: RunStatus,
                  steps: Vec<StepRecord>,
                  captures: Captures,
                  worktree_rec: WorktreeRecord,
                  mut limitations: Vec<String>,
                  oracle_block: Option<OracleBlock>,
                  oplog: &mut OperationLog|
     -> Result<RunOutput> {
        let finished_unix = unix_ms() as u64;
        limitations.extend(collect_truncation_notes(&steps, &captures));
        let oracle_result = oracle_block.as_ref().and_then(|o| o.result);
        let (verdict, verdict_note) = receipt::verdict_for(run_status, oracle_result);
        if let Some(note) = verdict_note {
            limitations.push(note);
        }
        let oracle_block = oracle_block.unwrap_or(OracleBlock {
            result: None,
            note: Some("oracle not evaluated".to_string()),
            conditions: Vec::new(),
        });
        let mut manifest = RunManifest {
            version: "terrorbat/run/v0".to_string(),
            execution_id: execution_id.clone(),
            bat: clone_bat_block(&bat_block),
            target: target_record.clone(),
            started_unix_ms: started_unix,
            finished_unix_ms: finished_unix,
            wall_ms: started.elapsed().as_millis() as u64,
            run_status,
            steps,
            captures,
            worktree: worktree_rec,
            capabilities: capabilities.clone(),
            environment: environment.clone(),
            versions: versions.clone(),
            limitations,
            run_dir: run_dir.to_string_lossy().to_string(),
            oracle: Some(oracle_block.clone()),
            verdict: Some(verdict),
            receipt_id: None,
            adapter_bindings_required: spec
                .attack
                .setup
                .iter()
                .chain(&spec.attack.run)
                .any(|step| !adapter::is_builtin(&step.adapter)),
            resolved_adapters: resolved_adapter_provenance.borrow().clone(),
        };
        let final_receipt =
            receipt::finalise(receipt::build(&manifest, &claim_text, oracle_block))?;
        manifest.receipt_id = Some(final_receipt.receipt_id.clone());
        oplog.record(
            "run_finish",
            json!({
                "run_status": manifest.run_status,
                "verdict": final_receipt.verdict.as_str(),
                "receipt_id": final_receipt.receipt_id,
                "wall_ms": manifest.wall_ms,
            }),
        )?;
        receipt::write_receipt(&run_dir, &final_receipt)?;
        let manifest_path = run_dir.join("manifest.json");
        let text = serde_json::to_string_pretty(&manifest)
            .map_err(|e| Error::spec(&manifest_path, format!("cannot serialise manifest: {e}")))?;
        std::fs::write(&manifest_path, text)
            .map_err(|e| Error::spec(&manifest_path, format!("cannot write manifest: {e}")))?;
        Ok(RunOutput {
            execution_id: execution_id.clone(),
            manifest,
            run_dir: run_dir.clone(),
            receipt: final_receipt,
        })
    };

    // Parse the oracle definition before any work: a malformed oracle makes
    // the experiment Invalid up-front (M5).
    let oracle_expr = match oracle::parse(&spec.oracle.0) {
        Ok(expr) => expr,
        Err(e) => {
            oplog.record("oracle_invalid", json!({ "error": e }))?;
            limitations.push(format!("oracle definition invalid: {e}"));
            return finish(
                RunStatus::Invalid,
                Vec::new(),
                Captures::default(),
                WorktreeRecord {
                    path: None,
                    cleanup: CleanupRecord {
                        status: CleanupStatus::NotAttempted,
                        path: None,
                        error: None,
                    },
                },
                limitations,
                None,
                &mut oplog,
            );
        }
    };

    // --- capability declaration preflight (before any work) ---------------
    if let Err((status, message)) = preflight_capabilities(&spec, &oracle_expr) {
        oplog.record(
            "preflight_failed",
            json!({ "status": status, "error": message }),
        )?;
        return finish(
            status,
            Vec::new(),
            Captures::default(),
            WorktreeRecord {
                path: None,
                cleanup: CleanupRecord {
                    status: CleanupStatus::NotAttempted,
                    path: None,
                    error: None,
                },
            },
            limitations,
            None,
            &mut oplog,
        );
    }
    let adapter_steps: Vec<(&str, &str, &str)> = spec
        .attack
        .setup
        .iter()
        .map(|s| ("attack.setup", s.adapter.as_str(), s.action.as_str()))
        .chain(
            spec.attack
                .run
                .iter()
                .map(|s| ("attack.run", s.adapter.as_str(), s.action.as_str())),
        )
        .collect();
    let mut resolved_adapters =
        match adapter::resolve(bindings, &adapter_steps, &spec.requires, &spec.forbids) {
            Ok(resolved) => resolved,
            Err(failure) => {
                limitations.push(failure.message.clone());
                oplog.record(
                    "adapter_preflight_failed",
                    json!({ "status": failure.status, "error": failure.message }),
                )?;
                return finish(
                    failure.status,
                    Vec::new(),
                    Captures::default(),
                    WorktreeRecord {
                        path: None,
                        cleanup: CleanupRecord {
                            status: CleanupStatus::NotAttempted,
                            path: None,
                            error: None,
                        },
                    },
                    limitations,
                    None,
                    &mut oplog,
                );
            }
        };

    for (name, resolved) in &mut resolved_adapters.by_name {
        if !resolved.describe_stderr.is_empty() {
            let reference = store.put(&resolved.describe_stderr)?;
            resolved.provenance.describe_stderr = Some(reference.clone());
            oplog.record(
                "adapter_describe_diagnostics",
                json!({ "adapter": name, "evidence": reference }),
            )?;
        }
    }
    *resolved_adapter_provenance.borrow_mut() = resolved_adapters
        .by_name
        .values()
        .map(|resolved| resolved.provenance.clone())
        .collect();
    if let Some(expected) = expected {
        let actual: BTreeMap<String, String> = resolved_adapters
            .by_name
            .iter()
            .map(|(name, a)| (name.clone(), a.provenance.description_id.clone()))
            .collect();
        if expected
            .iter()
            .any(|(name, id)| actual.get(name).is_some_and(|actual| actual != id))
        {
            let message = format!(
                "adapter description identities changed: expected {expected:?}, received {actual:?}"
            );
            limitations.push(message.clone());
            oplog.record("adapter_identity_mismatch", json!({ "error": message }))?;
            return finish(
                RunStatus::Invalid,
                Vec::new(),
                Captures::default(),
                WorktreeRecord {
                    path: None,
                    cleanup: CleanupRecord {
                        status: CleanupStatus::NotAttempted,
                        path: None,
                        error: None,
                    },
                },
                limitations,
                None,
                &mut oplog,
            );
        }
    }
    oplog.record(
        "preflight_ok",
        json!({ "external_adapters": resolved_adapters.by_name.len() }),
    )?;

    // --- disposable worktree ----------------------------------------------
    let wt_path = worktree::worktree_path(&execution_id)?;
    if let Err(e) = worktree::create_worktree(&target.root, &target.commit, &wt_path) {
        oplog.record("worktree_failed", json!({ "error": e.to_string() }))?;
        limitations.push(format!("worktree creation failed: {e}"));
        return finish(
            RunStatus::InfrastructureError,
            Vec::new(),
            Captures::default(),
            WorktreeRecord {
                path: Some(wt_path.to_string_lossy().to_string()),
                cleanup: CleanupRecord {
                    status: CleanupStatus::NotAttempted,
                    path: Some(wt_path.to_string_lossy().to_string()),
                    error: Some(e.to_string()),
                },
            },
            limitations,
            None,
            &mut oplog,
        );
    }
    oplog.record(
        "worktree_created",
        json!({ "path": wt_path.to_string_lossy(), "commit": target.commit }),
    )?;

    // From here on, cleanup is always attempted and its result recorded.
    let result = execute_attack(
        &spec,
        &oracle_expr,
        &spec_dir,
        &wt_path,
        &target,
        &resolved_adapters,
        &execution_id,
        &store,
        &mut oplog,
        &mut limitations,
    );
    let cleanup = cleanup_worktree(&target.root, &wt_path, &mut oplog);
    let worktree_rec = WorktreeRecord {
        path: Some(wt_path.to_string_lossy().to_string()),
        cleanup: cleanup.clone(),
    };
    if matches!(cleanup.status, CleanupStatus::Failed) {
        limitations.push(format!(
            "worktree cleanup FAILED; leftover worktree preserved at `{}` for manual recovery",
            wt_path.display()
        ));
    }
    let (run_status, steps, captures, evaluation) = result?;
    let oracle_block = evaluation.map(|ev| OracleBlock {
        result: Some(ev.result),
        note: None,
        conditions: ev.conditions,
    });
    finish(
        run_status,
        steps,
        captures,
        worktree_rec,
        limitations,
        oracle_block,
        &mut oplog,
    )
}

/// Everything that happens inside the worktree: snapshot, setup, run,
/// post-state capture. Always returns records, even on mechanical failure —
/// observable state is preserved long enough for collection.
// Internal, cohesive orchestration function: bundling its parameters into a
// context struct would only move the eight fields, not remove them.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn execute_attack(
    spec: &BatSpec,
    oracle_expr: &OracleExpr,
    spec_dir: &Path,
    wt_path: &Path,
    target: &TargetState,
    adapters: &ResolvedAdapters,
    execution_id: &str,
    store: &EvidenceStore,
    oplog: &mut OperationLog,
    limitations: &mut Vec<String>,
) -> Result<(
    RunStatus,
    Vec<StepRecord>,
    Captures,
    Option<OracleEvaluation>,
)> {
    let mut steps: Vec<StepRecord> = Vec::new();
    let mut captures = Captures::default();
    let attack_start = Instant::now();
    let total_budget = spec
        .timeout
        .as_ref()
        .and_then(|t| t.total)
        .map(|t| Duration::from_secs(t.0));

    // Base snapshot (HEAD + porcelain status) before any mutation.
    let ctx = StepCtx {
        worktree: wt_path,
        spec_dir,
        deadline: Some(Duration::from_secs(120)),
    };
    match builtins::dispatch("git", "worktree.snapshot", &BTreeMap::new(), &ctx) {
        Ok(out) if out.status == ExecutionStatus::Completed && out.exit_code == Some(0) => {
            captures.base_snapshot = Some(store.put(&out.stdout)?);
            oplog.record(
                "capture",
                json!({ "kind": "base_snapshot", "evidence": captures.base_snapshot }),
            )?;
        }
        Ok(out) => {
            limitations.push(format!(
                "base snapshot capture failed mechanically ({:?})",
                out.status
            ));
        }
        Err(e) => limitations.push(format!("base snapshot capture error: {e}")),
    }

    // setup then run, strict order; setup failure halts before run.
    let mut run_status = RunStatus::Completed;
    for (phase, phase_steps, stage_budget) in [
        (
            "setup",
            &spec.attack.setup,
            spec.timeout.as_ref().and_then(|t| t.setup),
        ),
        (
            "run",
            &spec.attack.run,
            spec.timeout.as_ref().and_then(|t| t.run),
        ),
    ] {
        let stage_start = Instant::now();
        let stage_end = stage_budget.map(|t| stage_start + Duration::from_secs(t.0));
        let halt = execute_phase(
            phase,
            phase_steps,
            spec_dir,
            wt_path,
            stage_end,
            total_budget.map(|b| attack_start + b),
            adapters,
            &target.commit,
            execution_id,
            store,
            oplog,
            &mut steps,
        )?;
        if let Some(status) = halt {
            run_status = status;
            break;
        }
    }

    // Post-state capture happens even after mechanical failure: the point is
    // that observable state survives long enough to be collected.
    capture_post_state(wt_path, target, store, oplog, &mut captures, limitations)?;

    // Oracle evaluation happens while the worktree still exists (command
    // verifiers may need it); cleanup follows afterwards, never before.
    // Only a Completed run is judged — a crashed or timed-out run describes
    // itself, and its verdict comes from the hard mapping rules.
    let mut evaluation: Option<OracleEvaluation> = None;
    if run_status == RunStatus::Completed {
        let oracle_budget = spec
            .timeout
            .as_ref()
            .and_then(|t| t.oracle)
            .map(|t| Duration::from_secs(t.0));
        let total_remaining =
            total_budget.map(|b| (attack_start + b).saturating_duration_since(Instant::now()));
        let verifier_deadline = match (oracle_budget, total_remaining) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => Some(Duration::from_secs(120)),
        };
        let mut verifier_steps: Vec<StepRecord> = Vec::new();
        let eval = {
            let mut ectx = EvalCtx {
                store,
                worktree: wt_path,
                spec_dir,
                steps: &steps,
                captures: &captures,
                verifier_deadline,
                verifier_steps: &mut verifier_steps,
            };
            oracle::evaluate(oracle_expr, &mut ectx)
        };
        oplog.record(
            "oracle_evaluated",
            json!({ "result": eval.result, "conditions": eval.conditions.len() }),
        )?;
        steps.extend(verifier_steps);
        evaluation = Some(eval);
    }

    Ok((run_status, steps, captures, evaluation))
}

/// Execute one ordered phase. Returns `Some(status)` when a mechanical
/// failure halts the run. A Completed step with nonzero exit code does NOT
/// halt — the oracle decides what exit codes mean.
#[allow(clippy::too_many_arguments)]
fn execute_phase(
    phase: &'static str,
    steps: &[crate::spec::Step],
    spec_dir: &Path,
    wt_path: &Path,
    stage_end: Option<Instant>,
    total_end: Option<Instant>,
    adapters: &ResolvedAdapters,
    target_commit: &str,
    execution_id: &str,
    store: &EvidenceStore,
    oplog: &mut OperationLog,
    records: &mut Vec<StepRecord>,
) -> Result<Option<RunStatus>> {
    for (index, step) in steps.iter().enumerate() {
        let deadline_instant = match (stage_end, total_end) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        let remaining = deadline_instant.map(|end| end.saturating_duration_since(Instant::now()));
        if remaining == Some(Duration::ZERO) {
            let record = StepRecord {
                phase: phase.to_string(),
                index,
                adapter: step.adapter.clone(),
                action: step.action.clone(),
                payload: Value::Object(
                    step.payload
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
                status: RunStatus::TimedOut,
                exit_code: None,
                signal: None,
                stdout: None,
                stdout_total_bytes: 0,
                stdout_truncated: false,
                stderr: None,
                stderr_total_bytes: 0,
                stderr_truncated: false,
                adapter_provenance: None,
                protocol_stdout: None,
                protocol_stderr: None,

                wall_ms: 0,
                error: Some(format!(
                    "{phase} budget exhausted before step {index} started"
                )),
            };
            oplog.record(
                "step_skipped_budget",
                json!({ "phase": phase, "index": index }),
            )?;
            records.push(record);
            return Ok(Some(RunStatus::TimedOut));
        }

        oplog.record(
            "step_start",
            json!({
                "phase": phase, "index": index,
                "adapter": step.adapter, "action": step.action,
            }),
        )?;
        let ctx = StepCtx {
            worktree: wt_path,
            spec_dir,
            deadline: remaining,
        };
        let started = Instant::now();
        let (
            status,
            exit_code,
            signal,
            stdout_bytes,
            stdout_total,
            stderr_bytes,
            stderr_total,
            wall_time,
            error,
            provenance,
            protocol_stdout,
            protocol_stderr,
        ) = if adapter::is_builtin(&step.adapter) {
            match builtins::dispatch(&step.adapter, &step.action, &step.payload, &ctx) {
                Ok(out) => (
                    RunStatus::from(out.status),
                    out.exit_code,
                    out.signal,
                    Some(out.stdout),
                    out.stdout_total,
                    Some(out.stderr),
                    out.stderr_total,
                    out.wall_time,
                    out.error,
                    None,
                    None,
                    None,
                ),
                Err(e) => (
                    RunStatus::from(&e),
                    None,
                    None,
                    None,
                    0,
                    None,
                    0,
                    started.elapsed(),
                    Some(e.to_string()),
                    None,
                    None,
                    None,
                ),
            }
        } else {
            let resolved = adapters
                .by_name
                .get(&step.adapter)
                .expect("external steps are resolved during preflight");
            let out = adapter::execute(adapter::AdapterInvocation {
                adapter: resolved,
                action: &step.action,
                payload: &step.payload,
                worktree: wt_path,
                target_commit,
                execution_id,
                phase,
                step_index: index,
                deadline: remaining,
            });
            let stdout_total = out.stdout.len() as u64;
            let stderr_total = out.stderr.len() as u64;
            (
                out.status,
                out.exit_code,
                out.signal,
                Some(out.stdout),
                stdout_total,
                Some(out.stderr),
                stderr_total,
                out.wall_time,
                out.error,
                Some(out.provenance),
                Some(out.protocol_stdout),
                Some(out.protocol_stderr),
            )
        };
        let stdout_ref = stdout_bytes.as_deref().map(|b| store.put(b)).transpose()?;
        let stderr_ref = stderr_bytes.as_deref().map(|b| store.put(b)).transpose()?;
        let protocol_stdout_ref = protocol_stdout
            .as_deref()
            .filter(|b| !b.is_empty())
            .map(|b| store.put(b))
            .transpose()?;
        let protocol_stderr_ref = protocol_stderr
            .as_deref()
            .filter(|b| !b.is_empty())
            .map(|b| store.put(b))
            .transpose()?;
        let record = StepRecord {
            phase: phase.to_string(),
            index,
            adapter: step.adapter.clone(),
            action: step.action.clone(),
            payload: Value::Object(
                step.payload
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            status,
            exit_code,
            signal,
            stdout: stdout_ref,
            stdout_total_bytes: stdout_total,
            stdout_truncated: stdout_bytes
                .as_ref()
                .is_some_and(|b| stdout_total > b.len() as u64),
            stderr: stderr_ref,
            stderr_total_bytes: stderr_total,
            stderr_truncated: stderr_bytes
                .as_ref()
                .is_some_and(|b| stderr_total > b.len() as u64),
            adapter_provenance: provenance,
            protocol_stdout: protocol_stdout_ref,
            protocol_stderr: protocol_stderr_ref,
            wall_ms: wall_time.as_millis() as u64,
            error,
        };
        oplog.record(
            "step_finish",
            json!({
                "phase": phase, "index": index,
                "status": record.status,
                "exit_code": record.exit_code,
                "stdout": record.stdout, "stderr": record.stderr,
                "stdout_truncated": record.stdout_truncated,
                "stderr_truncated": record.stderr_truncated,
                "wall_ms": record.wall_ms,
                "error": record.error,
            }),
        )?;
        let halt = record.status != RunStatus::Completed;
        records.push(record);
        if halt {
            return Ok(records.last().map(|r| r.status));
        }
    }
    Ok(None)
}

/// Capture the mutated state of the worktree: porcelain status (with the
/// true untracked list), then intent-to-add so newly created files appear in
/// the tracked diff, then `git diff HEAD`.
fn capture_post_state(
    wt_path: &Path,
    _target: &TargetState,
    store: &EvidenceStore,
    oplog: &mut OperationLog,
    captures: &mut Captures,
    limitations: &mut Vec<String>,
) -> Result<()> {
    let deadline = Some(Duration::from_secs(120));
    let git = |args: &[&str]| -> crate::supervisor::SupervisedOutcome {
        let mut cmd = SupervisedCommand::new(std::ffi::OsString::from("git"));
        cmd.args = args.iter().map(std::ffi::OsString::from).collect();
        cmd.cwd = Some(wt_path.to_path_buf());
        cmd.deadline = deadline;
        crate::supervisor::run(&cmd, &CancelToken::new())
    };

    let status_out = git(&["status", "--porcelain"]);
    if status_out.status == ExecutionStatus::Completed && status_out.exit_code == Some(0) {
        let text = status_out.stdout.as_str_lossy().to_string();
        captures.untracked = text
            .lines()
            .filter(|l| l.starts_with("??"))
            .filter_map(|l| l.get(3..).map(str::to_string))
            .collect();
        captures.git_status = Some(store.put(text.as_bytes())?);
        oplog.record(
            "capture",
            json!({ "kind": "git_status", "evidence": captures.git_status }),
        )?;
    } else {
        limitations.push("post-run `git status` capture failed".to_string());
    }

    // Intent-to-add makes newly created (untracked) files visible to diff,
    // so final mutation inspection cannot accidentally ignore them. Runs in
    // the disposable worktree only.
    let ita = git(&["add", "--intent-to-add", "."]);
    if ita.exit_code != Some(0) {
        limitations.push(format!(
            "`git add --intent-to-add .` exited {:?}; new files may be missing from diff evidence",
            ita.exit_code
        ));
    }
    let diff_out = git(&["diff", "HEAD"]);
    if diff_out.status == ExecutionStatus::Completed && diff_out.exit_code == Some(0) {
        captures.git_diff = Some(store.put(&diff_out.stdout.bytes)?);
        if diff_out.stdout.truncated() {
            limitations.push("git diff evidence was truncated by the capture limit".to_string());
        }
        oplog.record(
            "capture",
            json!({ "kind": "git_diff", "evidence": captures.git_diff, "truncated": diff_out.stdout.truncated() }),
        )?;
    } else {
        limitations.push("post-run `git diff HEAD` capture failed".to_string());
    }
    Ok(())
}

/// Cleanup with a truthful result. Failures preserve the path for manual
/// recovery and are never silently swallowed.
fn cleanup_worktree(repo_root: &Path, wt_path: &Path, oplog: &mut OperationLog) -> CleanupRecord {
    match worktree::remove_worktree(repo_root, wt_path) {
        Ok(()) => {
            let _ = oplog.record("cleanup", json!({ "status": "succeeded" }));
            CleanupRecord {
                status: CleanupStatus::Succeeded,
                path: None,
                error: None,
            }
        }
        Err(e) => {
            let message = format!(
                "The disposable worktree could not be removed because a process may still hold \
                 a file open.\n\nThe target repository was not modified.\n\nLeftover worktree:\n  \
                 {}\n\nYou may inspect or remove it manually after the process releases the \
                 handle.\n\nCode: TB-WORKTREE-CLEANUP\nDetail: {e}",
                wt_path.display()
            );
            let _ = oplog.record("cleanup", json!({ "status": "failed", "error": e }));
            CleanupRecord {
                status: CleanupStatus::Failed,
                path: Some(wt_path.to_string_lossy().to_string()),
                error: Some(message),
            }
        }
    }
}

/// Declaration/policy preflight: every step's required capability must be
/// declared; a capability the Bat itself forbids is a policy denial. This is
/// declaration checking only — arbitrary child programs remain UNENFORCED.
fn preflight_capabilities(
    spec: &BatSpec,
    oracle_expr: &OracleExpr,
) -> std::result::Result<(), (RunStatus, String)> {
    let declared = capability_names(&spec.requires);
    let forbidden = capability_names(&spec.forbids);
    for (phase, steps) in [
        ("attack.setup", &spec.attack.setup),
        ("attack.run", &spec.attack.run),
    ] {
        for (i, step) in steps.iter().enumerate() {
            let Some(cap) = builtins::required_capability(&step.adapter, &step.action) else {
                if !adapter::is_builtin(&step.adapter) {
                    continue;
                }
                return Err((
                    RunStatus::Invalid,
                    format!(
                        "{phase}[{i}]: unknown built-in adapter action `{}.{}`",
                        step.adapter, step.action
                    ),
                ));
            };
            if forbidden.contains(cap) {
                return Err((
                    RunStatus::PolicyDenied,
                    format!(
                        "{phase}[{i}] (`{}.{}`) requires capability `{cap}`, which this Bat \
                         explicitly forbids",
                        step.adapter, step.action
                    ),
                ));
            }
            if !declared.contains(cap) {
                return Err((
                    RunStatus::Invalid,
                    format!(
                        "{phase}[{i}] (`{}.{}`) requires capability `{cap}`, which is not \
                         declared in `requires`",
                        step.adapter, step.action
                    ),
                ));
            }
        }
    }
    // Command verifiers inside the oracle also spawn processes; the same
    // declaration rule applies before any worktree work.
    if oracle::uses_command_verifier(oracle_expr) {
        if forbidden.contains("process.spawn") {
            return Err((
                RunStatus::PolicyDenied,
                "the oracle command verifier requires capability `process.spawn`, which this \
                 Bat explicitly forbids"
                    .to_string(),
            ));
        }
        if !declared.contains("process.spawn") {
            return Err((
                RunStatus::Invalid,
                "the oracle command verifier requires capability `process.spawn`, which is \
                 not declared in `requires`"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

fn capability_names(caps: &[Capability]) -> BTreeSet<String> {
    caps.iter()
        .map(|c| c.0.split(':').next().unwrap_or(&c.0).trim().to_string())
        .collect()
}

fn is_false(value: &bool) -> bool {
    !value
}

fn capabilities_record(spec: &BatSpec) -> CapabilitiesRecord {
    let mut unenforced: BTreeSet<String> = BTreeSet::new();
    for cap in spec.requires.iter().chain(spec.forbids.iter()) {
        let name = cap.0.split(':').next().unwrap_or(&cap.0).trim().to_string();
        // process.spawn has partial real enforcement (M2 tree ownership);
        // everything else at host level is advisory in worktree mode.
        if name != "process.spawn" {
            unenforced.insert(name);
        }
    }
    // Common host-level capabilities are unenforced whether declared or not.
    for name in [
        "network",
        "fs.read",
        "host.config.write",
        "git.remote.write",
        "credential.read",
    ] {
        unenforced.insert(name.to_string());
    }
    CapabilitiesRecord {
        declared_requires: spec.requires.iter().map(|c| c.0.clone()).collect(),
        declared_forbids: spec.forbids.iter().map(|c| c.0.clone()).collect(),
        enforced: vec![
            "process-tree ownership (M2 model: supervised spawn, group/job termination)"
                .to_string(),
            "built-in step path confinement to the worktree (validation, not a sandbox)"
                .to_string(),
        ],
        unenforced: unenforced.into_iter().collect(),
    }
}

fn capture_environment(declared: &[String], git_version: &str) -> BTreeMap<String, Option<String>> {
    declared
        .iter()
        .map(|name| {
            let value = match name.as_str() {
                "os.name" => Some(std::env::consts::OS.to_string()),
                "os.arch" => Some(std::env::consts::ARCH.to_string()),
                "git.version" => Some(git_version.to_string()),
                "terrorbat.version" => Some(TERRORBAT_VERSION.to_string()),
                // Unknown symbolic names are honestly recorded as not captured.
                _ => None,
            };
            (name.clone(), value)
        })
        .collect()
}

fn capture_git_version(repo: &Path) -> String {
    let mut cmd = SupervisedCommand::new(std::ffi::OsString::from("git"));
    cmd.args = vec!["--version".into()];
    cmd.cwd = Some(repo.to_path_buf());
    cmd.deadline = Some(Duration::from_secs(30));
    let out = crate::supervisor::run(&cmd, &CancelToken::new());
    if out.status == ExecutionStatus::Completed && out.exit_code == Some(0) {
        out.stdout.as_str_lossy().trim().to_string()
    } else {
        "unknown".to_string()
    }
}

fn base_limitations() -> Vec<String> {
    vec![
        "Disposable Git worktree mode is NOT hostile-code containment: host filesystem, \
         network, and credential access are UNENFORCED (advisory declarations only)."
            .to_string(),
        "command.run spawns arbitrary programs; only Terror Bat's built-in operations are \
         path-confined to the worktree."
            .to_string(),
        "Evidence durability covers supervised child-process failure; power-loss and \
         disk-corruption survival are not claimed."
            .to_string(),
    ]
}

fn collect_truncation_notes(steps: &[StepRecord], _captures: &Captures) -> Vec<String> {
    let mut notes = Vec::new();
    for step in steps {
        if step.stdout_truncated {
            notes.push(format!(
                "{}[{}] stdout truncated: {} of {} bytes retained",
                step.phase,
                step.index,
                step.stdout_total_bytes.min(1024 * 1024),
                step.stdout_total_bytes
            ));
        }
        if step.stderr_truncated {
            notes.push(format!(
                "{}[{}] stderr truncated: {} of {} bytes retained",
                step.phase,
                step.index,
                step.stderr_total_bytes.min(1024 * 1024),
                step.stderr_total_bytes
            ));
        }
    }
    notes
}

fn clone_bat_block(b: &BatBlock) -> BatBlock {
    BatBlock {
        id: b.id.clone(),
        bat_sha: b.bat_sha.clone(),
        claim_sha: b.claim_sha.clone(),
        attack_sha: b.attack_sha.clone(),
        oracle_sha: b.oracle_sha.clone(),
        spec_source: b.spec_source.clone(),
        canonical: b.canonical.clone(),
        param_overrides: b.param_overrides.clone(),
        source_path: b.source_path.clone(),
    }
}

/// Stable CLI exit-code mapping (documented in docs/first-flight.md):
/// 0 = completed, no proven falsification; 1 = PROVEN falsification;
/// 2 = invalid request/spec/policy; 3 = infrastructure/inconclusive.
pub fn exit_code_for(run_status: RunStatus, verdict: Option<&str>) -> u8 {
    // Spec/policy problems keep exit 2 even though their verdict reads
    // INVALID/INCONCLUSIVE: the CLI classifies the request problem, while the
    // receipt remains authoritative for epistemic state.
    if matches!(run_status, RunStatus::Invalid | RunStatus::PolicyDenied) {
        return 2;
    }
    if let Some(v) = verdict {
        match v {
            "PROVEN" => return 1,
            "NOT OBSERVED" => return 0,
            _ => return 3, // INCONCLUSIVE / INFRASTRUCTURE ERROR / INVALID
        }
    }
    match run_status {
        RunStatus::Completed => 0,
        RunStatus::Invalid | RunStatus::PolicyDenied => 2,
        RunStatus::TimedOut
        | RunStatus::Crashed
        | RunStatus::Cancelled
        | RunStatus::InfrastructureError => 3,
    }
}

/// Load a manifest from a run directory.
pub fn load_manifest(run_dir: &Path) -> Result<RunManifest> {
    let path = run_dir.join("manifest.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Error::spec(&path, format!("cannot read run manifest: {e}")))?;
    serde_json::from_str(&text)
        .map_err(|e| Error::spec(&path, format!("cannot parse run manifest: {e}")))
}

/// Locate a run directory by execution id, or `None`.
pub fn find_run(store: &EvidenceStore, execution_id: &str) -> Option<PathBuf> {
    let dir = store.run_dir(execution_id);
    dir.join("manifest.json").exists().then_some(dir)
}

/// Identities accessor kept for callers that only have a manifest.
pub fn identities_from_manifest(manifest: &RunManifest) -> Identities {
    Identities {
        bat: manifest.bat.bat_sha.clone(),
        claim: manifest.bat.claim_sha.clone(),
        attack: manifest.bat.attack_sha.clone(),
        oracle: manifest.bat.oracle_sha.clone(),
    }
}
