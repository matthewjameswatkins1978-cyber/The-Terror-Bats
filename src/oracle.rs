//! Deterministic oracle engine (M5).
//!
//! Same oracle definition + same evidence inputs ⇒ same oracle result.
//! No model calls, ever. Results are exactly:
//!
//! ```text
//! Falsified / NotFalsified / Undetermined
//! ```
//!
//! Composition is limited to `all` / `any` / `not` with Kleene three-valued
//! semantics — no expression language. A condition names a *falsification
//! observation*: when it fires, the claim is falsified. Missing evidence
//! yields `Undetermined`; the engine never guesses.
//!
//! Empty combinators evaluate to `Undetermined` ("no conditions to judge"),
//! never to a verdict — a vacuous oracle must not manufacture proof.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::builtins::{self, StepCtx};
use crate::evidence::{EvidenceRef, EvidenceStore};
use crate::runner::{Captures, StepRecord};

/// The deterministic judgement about the claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OracleResult {
    Falsified,
    NotFalsified,
    Undetermined,
}

impl OracleResult {
    pub fn label(self) -> &'static str {
        match self {
            OracleResult::Falsified => "FALSIFIED",
            OracleResult::NotFalsified => "NOT FALSIFIED",
            OracleResult::Undetermined => "UNDETERMINED",
        }
    }
}

/// Where a text/JSON condition reads its input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    /// Worktree-relative path, read at oracle time.
    Path(String),
    /// Evidence object of this run.
    Evidence(String),
    /// A step's captured stream, e.g. `run:0` / `setup:1` (+ stderr).
    Step { step: String, stream: String },
}

/// A leaf falsification detector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    FileExists {
        path: String,
    },
    FileAbsent {
        path: String,
    },
    TextContains {
        #[serde(flatten)]
        source: DataSourceFlat,
        substring: String,
    },
    TextMatches {
        #[serde(flatten)]
        source: DataSourceFlat,
        regex: String,
    },
    GitDiffContains {
        substring: String,
    },
    GitDiffMatches {
        regex: String,
    },
    PathChanged {
        path: String,
    },
    PathUnchanged {
        path: String,
    },
    ExitCode {
        step: String,
        #[serde(default)]
        equals: Option<i64>,
        #[serde(default)]
        not_equals: Option<i64>,
    },
    EvidencePresent {
        kind: String,
    },
    JsonValueEquals {
        #[serde(flatten)]
        source: DataSourceFlat,
        pointer: String,
        value: Value,
    },
    /// Deterministic external verifier, run through M2 inside the worktree
    /// before cleanup. A verifier crash/timeout is Undetermined, never an
    /// automatic falsification.
    Command {
        program: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        expect_exit: i64,
    },
}

/// Flattened source selector: exactly one of `path`, `evidence`, or `step`
/// (with optional `stream`, default stdout).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSourceFlat {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub evidence: Option<String>,
    #[serde(default)]
    pub step: Option<String>,
    #[serde(default)]
    pub stream: Option<String>,
}

impl DataSourceFlat {
    pub fn resolve(&self) -> Result<DataSource, String> {
        match (
            self.path.as_ref(),
            self.evidence.as_ref(),
            self.step.as_ref(),
            self.stream.as_ref(),
        ) {
            (Some(p), None, None, None) => Ok(DataSource::Path(p.clone())),
            (None, Some(e), None, None) => Ok(DataSource::Evidence(e.clone())),
            (None, None, Some(s), stream) => Ok(DataSource::Step {
                step: s.clone(),
                stream: stream.cloned().unwrap_or_else(|| "stdout".to_string()),
            }),
            _ => Err(
                "condition source needs exactly one of `path`, `evidence`, or `step` (+optional `stream`)"
                    .to_string(),
            ),
        }
    }
}

/// Parsed oracle expression tree.
#[derive(Debug, Clone)]
pub enum OracleExpr {
    All(Vec<OracleExpr>),
    Any(Vec<OracleExpr>),
    Not(Box<OracleExpr>),
    Cond(Condition),
}

/// One evaluated leaf, recorded for the receipt's judgement trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionOutcome {
    pub condition: String,
    pub result: OracleResult,
    pub note: Option<String>,
}

/// Full evaluation output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleEvaluation {
    pub result: OracleResult,
    pub conditions: Vec<ConditionOutcome>,
}

/// Parse the opaque oracle YAML value into a typed expression tree.
/// Malformed oracles are errors (the run becomes Invalid), never guesses.
pub fn parse(oracle: &Value) -> Result<OracleExpr, String> {
    let obj = oracle
        .as_object()
        .ok_or_else(|| "oracle must be a mapping".to_string())?;
    if obj.contains_key("type") {
        // Condition node: `type` plus its fields. Combinators may not be
        // mixed into a condition node.
        for key in obj.keys() {
            if matches!(key.as_str(), "all" | "any" | "not") {
                return Err(format!(
                    "oracle node mixes `type` with combinator `{key}`; use nesting instead"
                ));
            }
        }
        let cond: Condition = serde_json::from_value(oracle.clone())
            .map_err(|e| format!("invalid oracle condition: {e}"))?;
        validate_condition(&cond)?;
        return Ok(OracleExpr::Cond(cond));
    }
    if obj.len() != 1 {
        return Err(format!(
            "oracle node must have exactly one key (`all`, `any`, `not`, or `type`); found {}",
            obj.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    let (key, value) = obj.iter().next().expect("len 1");
    match key.as_str() {
        "all" | "any" => {
            let items = value
                .as_array()
                .ok_or_else(|| format!("`{key}` must be a list"))?;
            let mut children = Vec::with_capacity(items.len());
            for item in items {
                children.push(parse(item)?);
            }
            Ok(if key == "all" {
                OracleExpr::All(children)
            } else {
                OracleExpr::Any(children)
            })
        }
        "not" => Ok(OracleExpr::Not(Box::new(parse(value)?))),
        other => Err(format!(
            "unknown oracle node `{other}`; supported: all, any, not, type"
        )),
    }
}

fn validate_condition(cond: &Condition) -> Result<(), String> {
    match cond {
        Condition::GitDiffMatches { regex } => {
            Regex::new(regex).map_err(|e| format!("invalid regex `{regex}`: {e}"))?;
        }
        Condition::TextMatches { regex, source } => {
            Regex::new(regex).map_err(|e| format!("invalid regex `{regex}`: {e}"))?;
            validate_source(source)?;
        }
        Condition::TextContains { source, .. } | Condition::JsonValueEquals { source, .. } => {
            validate_source(source)?;
        }
        Condition::ExitCode {
            equals,
            not_equals,
            step,
        } => {
            if (equals.is_some() && not_equals.is_some())
                || (equals.is_none() && not_equals.is_none())
            {
                return Err(
                    "exit_code requires exactly one of `equals` or `not_equals`".to_string()
                );
            }
            parse_step_ref(step)?;
        }
        _ => {}
    }
    Ok(())
}

fn validate_source(source: &DataSourceFlat) -> Result<(), String> {
    let ds = source.resolve()?;
    if let DataSource::Step { step, stream } = &ds {
        parse_step_ref(step)?;
        if !matches!(stream.as_str(), "stdout" | "stderr") {
            return Err(format!(
                "stream must be `stdout` or `stderr`, got `{stream}`"
            ));
        }
    }
    Ok(())
}

/// True when any `command` verifier appears in the expression tree.
pub fn uses_command_verifier(expr: &OracleExpr) -> bool {
    match expr {
        OracleExpr::All(v) | OracleExpr::Any(v) => v.iter().any(uses_command_verifier),
        OracleExpr::Not(c) => uses_command_verifier(c),
        OracleExpr::Cond(Condition::Command { .. }) => true,
        OracleExpr::Cond(_) => false,
    }
}

/// Parse `phase:index` step references ("run:0", "setup:2", "oracle:0").
pub fn parse_step_ref(r: &str) -> Result<(String, usize), String> {
    let (phase, index) = r
        .split_once(':')
        .ok_or_else(|| format!("step reference `{r}` must look like `run:0`"))?;
    if !matches!(phase, "setup" | "run" | "oracle") {
        return Err(format!(
            "step reference `{r}` has unknown phase `{phase}` (setup/run/oracle)"
        ));
    }
    let index: usize = index
        .parse()
        .map_err(|_| format!("step reference `{r}` has a non-numeric index"))?;
    Ok((phase.to_string(), index))
}

/// Everything a condition may read.
pub struct EvalCtx<'a> {
    pub store: &'a EvidenceStore,
    pub worktree: &'a Path,
    pub spec_dir: &'a Path,
    pub steps: &'a [StepRecord],
    pub captures: &'a Captures,
    /// Remaining budget for command verifiers, if any.
    pub verifier_deadline: Option<Duration>,
    /// Verifier runs are appended here (phase "oracle") and their evidence
    /// stored, so receipts can reference everything the oracle consumed.
    pub verifier_steps: &'a mut Vec<StepRecord>,
}

/// Evaluate the parsed oracle. Deterministic: pure function of the ctx data,
/// except `command` verifiers, which re-run against the same pinned worktree
/// state and are themselves recorded as evidence.
pub fn evaluate(expr: &OracleExpr, ctx: &mut EvalCtx<'_>) -> OracleEvaluation {
    let mut conditions = Vec::new();
    let result = eval(expr, ctx, &mut conditions);
    OracleEvaluation { result, conditions }
}

fn eval(expr: &OracleExpr, ctx: &mut EvalCtx<'_>, out: &mut Vec<ConditionOutcome>) -> OracleResult {
    match expr {
        OracleExpr::All(children) => {
            if children.is_empty() {
                out.push(ConditionOutcome {
                    condition: "all: []".to_string(),
                    result: OracleResult::Undetermined,
                    note: Some("empty combinator: nothing to judge".to_string()),
                });
                return OracleResult::Undetermined;
            }
            let mut results = Vec::new();
            for child in children {
                results.push(eval(child, ctx, out));
            }
            // Kleene AND: false wins, then unknown, then true.
            if results.contains(&OracleResult::NotFalsified) {
                OracleResult::NotFalsified
            } else if results.contains(&OracleResult::Undetermined) {
                OracleResult::Undetermined
            } else {
                OracleResult::Falsified
            }
        }
        OracleExpr::Any(children) => {
            if children.is_empty() {
                out.push(ConditionOutcome {
                    condition: "any: []".to_string(),
                    result: OracleResult::Undetermined,
                    note: Some("empty combinator: nothing to judge".to_string()),
                });
                return OracleResult::Undetermined;
            }
            let mut results = Vec::new();
            for child in children {
                results.push(eval(child, ctx, out));
            }
            // Kleene OR: true wins, then unknown, then false.
            if results.contains(&OracleResult::Falsified) {
                OracleResult::Falsified
            } else if results.contains(&OracleResult::Undetermined) {
                OracleResult::Undetermined
            } else {
                OracleResult::NotFalsified
            }
        }
        OracleExpr::Not(child) => match eval(child, ctx, out) {
            OracleResult::Falsified => OracleResult::NotFalsified,
            OracleResult::NotFalsified => OracleResult::Falsified,
            OracleResult::Undetermined => OracleResult::Undetermined,
        },
        OracleExpr::Cond(cond) => {
            let (result, note) = eval_condition(cond, ctx);
            out.push(ConditionOutcome {
                condition: describe(cond),
                result,
                note,
            });
            result
        }
    }
}

fn describe(cond: &Condition) -> String {
    match cond {
        Condition::FileExists { path } => format!("file_exists {path}"),
        Condition::FileAbsent { path } => format!("file_absent {path}"),
        Condition::TextContains { substring, .. } => format!("text_contains {substring:?}"),
        Condition::TextMatches { regex, .. } => format!("text_matches /{regex}/"),
        Condition::GitDiffContains { substring } => format!("git_diff_contains {substring:?}"),
        Condition::GitDiffMatches { regex } => format!("git_diff_matches /{regex}/"),
        Condition::PathChanged { path } => format!("path_changed {path}"),
        Condition::PathUnchanged { path } => format!("path_unchanged {path}"),
        Condition::ExitCode {
            step,
            equals,
            not_equals,
        } => match (equals, not_equals) {
            (Some(n), _) => format!("exit_code {step} equals {n}"),
            (_, Some(n)) => format!("exit_code {step} not_equals {n}"),
            _ => format!("exit_code {step}"),
        },
        Condition::EvidencePresent { kind } => format!("evidence_present {kind}"),
        Condition::JsonValueEquals { pointer, .. } => format!("json_value_equals {pointer}"),
        Condition::Command {
            program,
            expect_exit,
            ..
        } => format!("command {program} expect_exit {expect_exit}"),
    }
}

fn eval_condition(cond: &Condition, ctx: &mut EvalCtx<'_>) -> (OracleResult, Option<String>) {
    match cond {
        Condition::FileExists { path } => match builtins::resolve_in_worktree(ctx.worktree, path) {
            Ok(p) => (
                if p.exists() {
                    OracleResult::Falsified
                } else {
                    OracleResult::NotFalsified
                },
                None,
            ),
            Err(e) => (OracleResult::Undetermined, Some(e.to_string())),
        },
        Condition::FileAbsent { path } => match builtins::resolve_in_worktree(ctx.worktree, path) {
            Ok(p) => (
                if p.exists() {
                    OracleResult::NotFalsified
                } else {
                    OracleResult::Falsified
                },
                None,
            ),
            Err(e) => (OracleResult::Undetermined, Some(e.to_string())),
        },
        Condition::TextContains { source, substring } => match load_source(source, ctx) {
            Ok(bytes) => (
                if contains_substring(&bytes, substring) {
                    OracleResult::Falsified
                } else {
                    OracleResult::NotFalsified
                },
                None,
            ),
            Err(note) => (OracleResult::Undetermined, Some(note)),
        },
        Condition::TextMatches { source, regex } => match load_source(source, ctx) {
            Ok(bytes) => match Regex::new(regex) {
                Ok(re) => (
                    if re.is_match(&String::from_utf8_lossy(&bytes)) {
                        OracleResult::Falsified
                    } else {
                        OracleResult::NotFalsified
                    },
                    None,
                ),
                Err(e) => (
                    OracleResult::Undetermined,
                    Some(format!("invalid regex: {e}")),
                ),
            },
            Err(note) => (OracleResult::Undetermined, Some(note)),
        },
        Condition::GitDiffContains { substring } => match diff_bytes(ctx) {
            Ok(bytes) => (
                if contains_substring(&bytes, substring) {
                    OracleResult::Falsified
                } else {
                    OracleResult::NotFalsified
                },
                None,
            ),
            Err(note) => (OracleResult::Undetermined, Some(note)),
        },
        Condition::GitDiffMatches { regex } => match diff_bytes(ctx) {
            Ok(bytes) => match Regex::new(regex) {
                Ok(re) => (
                    if re.is_match(&String::from_utf8_lossy(&bytes)) {
                        OracleResult::Falsified
                    } else {
                        OracleResult::NotFalsified
                    },
                    None,
                ),
                Err(e) => (
                    OracleResult::Undetermined,
                    Some(format!("invalid regex: {e}")),
                ),
            },
            Err(note) => (OracleResult::Undetermined, Some(note)),
        },
        Condition::PathChanged { path } => path_change_state(path, ctx, true),
        Condition::PathUnchanged { path } => path_change_state(path, ctx, false),
        Condition::ExitCode {
            step,
            equals,
            not_equals,
        } => {
            let Ok((phase, index)) = parse_step_ref(step) else {
                return (
                    OracleResult::Undetermined,
                    Some(format!("bad step reference `{step}`")),
                );
            };
            let Some(record) = find_step(ctx.steps, &phase, index) else {
                return (
                    OracleResult::Undetermined,
                    Some(format!("step `{step}` did not execute")),
                );
            };
            let Some(code) = record.exit_code else {
                return (
                    OracleResult::Undetermined,
                    Some(format!(
                        "step `{step}` produced no exit code (status {:?})",
                        record.status
                    )),
                );
            };
            let fired = match (equals, not_equals) {
                (Some(n), _) => code as i64 == *n,
                (_, Some(n)) => code as i64 != *n,
                _ => unreachable!("validated at parse time"),
            };
            (
                if fired {
                    OracleResult::Falsified
                } else {
                    OracleResult::NotFalsified
                },
                None,
            )
        }
        Condition::EvidencePresent { kind } => {
            let present = match kind.as_str() {
                "git_diff" => ctx.captures.git_diff.is_some(),
                "git_status" => ctx.captures.git_status.is_some(),
                "base_snapshot" => ctx.captures.base_snapshot.is_some(),
                other => {
                    return (
                        OracleResult::Undetermined,
                        Some(format!("unknown evidence kind `{other}`")),
                    );
                }
            };
            (
                if present {
                    OracleResult::Falsified
                } else {
                    OracleResult::NotFalsified
                },
                None,
            )
        }
        Condition::JsonValueEquals {
            source,
            pointer,
            value,
        } => match load_source(source, ctx) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(doc) => match json_pointer(&doc, pointer) {
                    Some(found) => (
                        if found == value {
                            OracleResult::Falsified
                        } else {
                            OracleResult::NotFalsified
                        },
                        None,
                    ),
                    None => (
                        OracleResult::Undetermined,
                        Some(format!("pointer `{pointer}` does not resolve")),
                    ),
                },
                Err(e) => (
                    OracleResult::Undetermined,
                    Some(format!("source is not valid JSON: {e}")),
                ),
            },
            Err(note) => (OracleResult::Undetermined, Some(note)),
        },
        Condition::Command {
            program,
            args,
            cwd,
            expect_exit,
        } => run_verifier(program, args, cwd.as_deref(), *expect_exit, ctx),
    }
}

/// A `path_changed`/`path_unchanged` judgement from captured state evidence.
/// `want_changed`: true → fires (Falsified) when the path changed.
fn path_change_state(
    path: &str,
    ctx: &EvalCtx<'_>,
    want_changed: bool,
) -> (OracleResult, Option<String>) {
    let status = match &ctx.captures.git_status {
        Some(r) => match ctx.store.get(r) {
            Ok(b) => String::from_utf8_lossy(&b).to_string(),
            Err(e) => return (OracleResult::Undetermined, Some(e.to_string())),
        },
        None => {
            return (
                OracleResult::Undetermined,
                Some("git status capture missing".to_string()),
            );
        }
    };
    let needle_slash = path.replace('\\', "/");
    let changed = status.lines().any(|line| {
        let entry = line.get(3..).unwrap_or("").trim();
        let entry = entry.replace('\\', "/");
        // Porcelain paths, including rename targets ("old -> new").
        entry == needle_slash
            || entry.ends_with(&format!(" -> {needle_slash}"))
            || entry.split(" -> ").any(|p| p == needle_slash)
    }) || ctx
        .captures
        .untracked
        .iter()
        .any(|u| u.replace('\\', "/") == needle_slash);
    let fired = if want_changed { changed } else { !changed };
    (
        if fired {
            OracleResult::Falsified
        } else {
            OracleResult::NotFalsified
        },
        None,
    )
}

fn find_step<'a>(steps: &'a [StepRecord], phase: &str, index: usize) -> Option<&'a StepRecord> {
    steps.iter().find(|s| s.phase == phase && s.index == index)
}

fn contains_substring(haystack: &[u8], needle: &str) -> bool {
    // Byte-level search: works for UTF-8 and degrades gracefully otherwise.
    let n = needle.as_bytes();
    if n.is_empty() {
        return true;
    }
    haystack.windows(n.len()).any(|window| window == n)
}

fn diff_bytes(ctx: &EvalCtx<'_>) -> Result<Vec<u8>, String> {
    let r = ctx
        .captures
        .git_diff
        .as_ref()
        .ok_or_else(|| "git diff capture missing".to_string())?;
    ctx.store.get(r).map_err(|e| e.to_string())
}

fn load_source(source: &DataSourceFlat, ctx: &EvalCtx<'_>) -> Result<Vec<u8>, String> {
    match source.resolve()? {
        DataSource::Path(rel) => {
            let p = builtins::resolve_in_worktree(ctx.worktree, &rel).map_err(|e| e.to_string())?;
            std::fs::read(&p).map_err(|e| format!("cannot read `{rel}`: {e}"))
        }
        DataSource::Evidence(r) => {
            let eref = EvidenceRef(r);
            ctx.store.get(&eref).map_err(|e| e.to_string())
        }
        DataSource::Step { step, stream } => {
            let (phase, index) = parse_step_ref(&step)?;
            let record = find_step(ctx.steps, &phase, index)
                .ok_or_else(|| format!("step `{step}` did not execute"))?;
            let r = match stream.as_str() {
                "stdout" => record.stdout.as_ref(),
                "stderr" => record.stderr.as_ref(),
                other => return Err(format!("unknown stream `{other}`")),
            };
            let r = r.ok_or_else(|| format!("step `{step}` has no {stream} evidence"))?;
            ctx.store.get(r).map_err(|e| e.to_string())
        }
    }
}

/// RFC 6901 JSON Pointer lookup — deliberately minimal, no JSONPath.
pub fn json_pointer<'a>(doc: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer.is_empty() {
        return Some(doc);
    }
    let mut current = doc;
    for raw_token in pointer.strip_prefix('/').unwrap_or(pointer).split('/') {
        let token = raw_token.replace("~1", "/").replace("~0", "~");
        current = match current {
            Value::Object(map) => map.get(&token)?,
            Value::Array(items) => {
                let idx: usize = token.parse().ok()?;
                items.get(idx)?
            }
            _ => return None,
        };
    }
    Some(current)
}

/// Run a deterministic verifier command inside the worktree through M2,
/// capture its evidence, and judge by exit code only.
fn run_verifier(
    program: &str,
    args: &[String],
    cwd: Option<&str>,
    expect_exit: i64,
    ctx: &mut EvalCtx<'_>,
) -> (OracleResult, Option<String>) {
    let mut payload: BTreeMap<String, Value> = BTreeMap::new();
    payload.insert("program".into(), Value::String(program.to_string()));
    payload.insert(
        "args".into(),
        Value::Array(args.iter().map(|a| Value::String(a.clone())).collect()),
    );
    if let Some(c) = cwd {
        payload.insert("cwd".into(), Value::String(c.to_string()));
    }
    let step_ctx = StepCtx {
        worktree: ctx.worktree,
        spec_dir: ctx.spec_dir,
        deadline: ctx.verifier_deadline,
        processes: None,
        phase: "oracle",
        step_index: ctx.verifier_steps.len(),
    };
    let index = ctx.verifier_steps.len();
    let started = std::time::Instant::now();
    let dispatched = builtins::dispatch("command", "run", &payload, &step_ctx);
    let record = match dispatched {
        Ok(outcome) => {
            let stdout_ref = ctx.store.put(&outcome.stdout).ok();
            let stderr_ref = ctx.store.put(&outcome.stderr).ok();
            StepRecord {
                phase: "oracle".to_string(),
                index,
                adapter: "command".to_string(),
                action: "run".to_string(),
                payload: Value::Object(
                    payload
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
                status: crate::runner::RunStatus::from(outcome.status),
                exit_code: outcome.exit_code,
                signal: outcome.signal,
                stdout: stdout_ref,
                stdout_total_bytes: outcome.stdout_total,
                stdout_truncated: outcome.stdout_total > outcome.stdout.len() as u64,
                stderr: stderr_ref,
                stderr_total_bytes: outcome.stderr_total,
                stderr_truncated: outcome.stderr_total > outcome.stderr.len() as u64,
                adapter_provenance: None,
                protocol_stdout: None,
                protocol_stderr: None,
                protocol_stdout_total_bytes: 0,
                protocol_stdout_truncated: false,
                protocol_stderr_total_bytes: 0,
                protocol_stderr_truncated: false,

                wall_ms: outcome.wall_time.as_millis() as u64,
                error: outcome.error,
            }
        }
        Err(e) => StepRecord {
            phase: "oracle".to_string(),
            index,
            adapter: "command".to_string(),
            action: "run".to_string(),
            payload: Value::Object(
                payload
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            status: crate::runner::RunStatus::from(&e),
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
            protocol_stdout_total_bytes: 0,
            protocol_stdout_truncated: false,
            protocol_stderr_total_bytes: 0,
            protocol_stderr_truncated: false,

            wall_ms: started.elapsed().as_millis() as u64,
            error: Some(e.to_string()),
        },
    };
    let (result, note) = if record.status == crate::runner::RunStatus::Completed {
        match record.exit_code {
            Some(code) if code as i64 == expect_exit => (
                OracleResult::NotFalsified,
                Some(format!("verifier exited {code} as expected")),
            ),
            Some(code) => (
                OracleResult::Falsified,
                Some(format!(
                    "verifier exited {code}, expected {expect_exit}: the invariant it checks is violated"
                )),
            ),
            None => (
                OracleResult::Undetermined,
                Some("verifier completed without an exit code".to_string()),
            ),
        }
    } else {
        // Crash/timeout/infra failure of the verifier is NEVER automatic
        // falsification.
        (
            OracleResult::Undetermined,
            Some(format!(
                "verifier did not complete ({:?}); a failed verifier cannot falsify",
                record.status
            )),
        )
    };
    ctx.verifier_steps.push(record);
    (result, note)
}
