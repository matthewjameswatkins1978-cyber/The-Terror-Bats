//! M5 oracle engine tests: truth tables, condition registry, determinism,
//! missing-evidence honesty, and command-verifier semantics.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::TempDir;
use serde_json::{Value, json};
use terrorbat::evidence::{EvidenceRef, EvidenceStore};
use terrorbat::oracle::{self, EvalCtx, OracleEvaluation, OracleResult};
use terrorbat::runner::{Captures, RunStatus, StepRecord};

struct Fixture {
    #[allow(dead_code)]
    dir: TempDir,
    store: EvidenceStore,
    worktree: PathBuf,
    steps: Vec<StepRecord>,
    captures: Captures,
}

fn step_record(
    phase: &str,
    index: usize,
    exit_code: Option<i32>,
    stdout: Option<EvidenceRef>,
    stderr: Option<EvidenceRef>,
) -> StepRecord {
    StepRecord {
        phase: phase.to_string(),
        index,
        adapter: "command".to_string(),
        action: "run".to_string(),
        payload: json!({}),
        status: if exit_code.is_some() {
            RunStatus::Completed
        } else {
            RunStatus::TimedOut
        },
        exit_code,
        signal: None,
        stdout,
        stdout_total_bytes: 0,
        stdout_truncated: false,
        stderr,
        stderr_total_bytes: 0,
        stderr_truncated: false,
        adapter_provenance: None,
        protocol_stdout: None,
        protocol_stderr: None,
        wall_ms: 1,
        error: None,
    }
}

fn fixture(tag: &str) -> Fixture {
    let dir = TempDir::new(tag);
    let store = EvidenceStore::open(&dir.join("store")).expect("store");
    let worktree = dir.join("wt");
    std::fs::create_dir_all(worktree.join("config")).expect("worktree");
    std::fs::write(worktree.join("exists.txt"), "needle here\n").expect("write");
    std::fs::write(worktree.join("data.json"), "{\"a\":{\"b\":42}}").expect("json");
    std::fs::write(worktree.join("broken.json"), "{not json").expect("broken");

    let stdout_ref = store.put(b"verification ok\n").expect("stdout ev");
    let steps = vec![
        step_record("run", 0, Some(0), Some(stdout_ref), None),
        step_record("run", 1, None, None, None), // timed-out step: no exit code
    ];
    let diff_ref = store
        .put(b"diff --git a/config/security.toml b/config/security.toml\n+pwned = true\n")
        .expect("diff ev");
    let status_ref = store
        .put(b" M config/security.toml\n?? newfile.txt\n")
        .expect("status ev");
    let captures = Captures {
        base_snapshot: None,
        git_status: Some(status_ref),
        git_diff: Some(diff_ref),
        untracked: vec!["newfile.txt".to_string()],
    };
    Fixture {
        dir,
        store,
        worktree,
        steps,
        captures,
    }
}

fn eval(f: &Fixture, oracle_json: Value) -> OracleEvaluation {
    let expr = oracle::parse(&oracle_json).expect("oracle parses");
    eval_parsed(f, &expr)
}

fn eval_parsed(f: &Fixture, expr: &oracle::OracleExpr) -> OracleEvaluation {
    let mut verifier_steps: Vec<StepRecord> = Vec::new();
    let mut ctx = EvalCtx {
        store: &f.store,
        worktree: &f.worktree,
        spec_dir: &f.worktree,
        steps: &f.steps,
        captures: &f.captures,
        verifier_deadline: Some(Duration::from_secs(30)),
        verifier_steps: &mut verifier_steps,
    };
    oracle::evaluate(expr, &mut ctx)
}

fn parse_err(oracle_json: Value) -> String {
    oracle::parse(&oracle_json)
        .expect_err("must be rejected")
        .to_string()
}

// ---------------------------------------------------------------------------
// Truth tables (Kleene three-valued)
// ---------------------------------------------------------------------------

#[test]
fn all_requires_every_condition_to_fire() {
    let f = fixture("or-all");
    // one fires, one does not -> NotFalsified (false wins)
    let r = eval(
        &f,
        json!({"all": [
            {"type": "file_exists", "path": "exists.txt"},
            {"type": "file_exists", "path": "missing.txt"},
        ]}),
    );
    assert_eq!(r.result, OracleResult::NotFalsified);
    // both fire -> Falsified
    let r = eval(
        &f,
        json!({"all": [
            {"type": "file_exists", "path": "exists.txt"},
            {"type": "git_diff_contains", "substring": "pwned"},
        ]}),
    );
    assert_eq!(r.result, OracleResult::Falsified);
    // fired + undetermined (missing evidence) -> Undetermined, never a guess
    let r = eval(
        &f,
        json!({"all": [
            {"type": "file_exists", "path": "exists.txt"},
            {"type": "text_contains", "evidence": format!("evidence:sha256:{}", "a".repeat(64)), "substring": "x"},
        ]}),
    );
    assert_eq!(r.result, OracleResult::Undetermined);
}

#[test]
fn any_fires_on_first_falsification() {
    let f = fixture("or-any");
    let r = eval(
        &f,
        json!({"any": [
            {"type": "file_exists", "path": "missing.txt"},
            {"type": "git_diff_contains", "substring": "pwned"},
        ]}),
    );
    assert_eq!(r.result, OracleResult::Falsified);
    // none fire -> NotFalsified
    let r = eval(
        &f,
        json!({"any": [
            {"type": "file_exists", "path": "missing.txt"},
            {"type": "git_diff_contains", "substring": "nothing-here"},
        ]}),
    );
    assert_eq!(r.result, OracleResult::NotFalsified);
    // none fire + one undetermined -> Undetermined
    let r = eval(
        &f,
        json!({"any": [
            {"type": "file_exists", "path": "missing.txt"},
            {"type": "text_contains", "evidence": format!("evidence:sha256:{}", "b".repeat(64)), "substring": "x"},
        ]}),
    );
    assert_eq!(r.result, OracleResult::Undetermined);
}

#[test]
fn not_inverts_and_preserves_undetermined() {
    let f = fixture("or-not");
    let r = eval(
        &f,
        json!({"not": {"type": "file_exists", "path": "exists.txt"}}),
    );
    assert_eq!(r.result, OracleResult::NotFalsified);
    let r = eval(
        &f,
        json!({"not": {"type": "file_exists", "path": "missing.txt"}}),
    );
    assert_eq!(r.result, OracleResult::Falsified);
    let r = eval(
        &f,
        json!({"not": {"type": "text_contains", "evidence": format!("evidence:sha256:{}", "c".repeat(64)), "substring": "x"}}),
    );
    assert_eq!(r.result, OracleResult::Undetermined);
}

#[test]
fn empty_combinators_are_undetermined_never_a_verdict() {
    let f = fixture("or-empty");
    assert_eq!(
        eval(&f, json!({"all": []})).result,
        OracleResult::Undetermined
    );
    assert_eq!(
        eval(&f, json!({"any": []})).result,
        OracleResult::Undetermined
    );
}

// ---------------------------------------------------------------------------
// Condition registry
// ---------------------------------------------------------------------------

#[test]
fn file_conditions_judge_worktree_state() {
    let f = fixture("or-file");
    assert_eq!(
        eval(&f, json!({"type": "file_exists", "path": "exists.txt"})).result,
        OracleResult::Falsified
    );
    assert_eq!(
        eval(&f, json!({"type": "file_exists", "path": "missing.txt"})).result,
        OracleResult::NotFalsified
    );
    assert_eq!(
        eval(&f, json!({"type": "file_absent", "path": "missing.txt"})).result,
        OracleResult::Falsified
    );
    // Escaping paths are undetermined, never silently judged.
    let r = eval(&f, json!({"type": "file_exists", "path": "../outside.txt"}));
    assert_eq!(r.result, OracleResult::Undetermined);
}

#[test]
fn text_conditions_read_paths_evidence_and_steps() {
    let f = fixture("or-text");
    assert_eq!(
        eval(
            &f,
            json!({"type": "text_contains", "path": "exists.txt", "substring": "needle"})
        )
        .result,
        OracleResult::Falsified
    );
    let diff_ref = f.captures.git_diff.clone().unwrap();
    assert_eq!(
        eval(
            &f,
            json!({"type": "text_matches", "evidence": diff_ref.0, "regex": "(?m)^\\+pwned"})
        )
        .result,
        OracleResult::Falsified
    );
    assert_eq!(
        eval(
            &f,
            json!({"type": "text_contains", "step": "run:0", "substring": "verification ok"})
        )
        .result,
        OracleResult::Falsified
    );
    // Step with no stderr evidence -> Undetermined, not a guess.
    let r = eval(
        &f,
        json!({"type": "text_contains", "step": "run:0", "stream": "stderr", "substring": "x"}),
    );
    assert_eq!(r.result, OracleResult::Undetermined);
    // Unknown step -> Undetermined.
    let r = eval(
        &f,
        json!({"type": "text_contains", "step": "run:42", "substring": "x"}),
    );
    assert_eq!(r.result, OracleResult::Undetermined);
}

#[test]
fn diff_conditions_read_captured_diff() {
    let f = fixture("or-diff");
    assert_eq!(
        eval(
            &f,
            json!({"type": "git_diff_contains", "substring": "pwned = true"})
        )
        .result,
        OracleResult::Falsified
    );
    assert_eq!(
        eval(
            &f,
            json!({"type": "git_diff_matches", "regex": "security\\.toml"})
        )
        .result,
        OracleResult::Falsified
    );
    assert_eq!(
        eval(
            &f,
            json!({"type": "git_diff_contains", "substring": "absent-text"})
        )
        .result,
        OracleResult::NotFalsified
    );
}

#[test]
fn path_change_conditions_use_status_and_untracked() {
    let f = fixture("or-path");
    assert_eq!(
        eval(
            &f,
            json!({"type": "path_changed", "path": "config/security.toml"})
        )
        .result,
        OracleResult::Falsified
    );
    assert_eq!(
        eval(&f, json!({"type": "path_changed", "path": "newfile.txt"})).result,
        OracleResult::Falsified,
        "untracked files count as changed"
    );
    assert_eq!(
        eval(&f, json!({"type": "path_changed", "path": "README.md"})).result,
        OracleResult::NotFalsified
    );
    assert_eq!(
        eval(&f, json!({"type": "path_unchanged", "path": "README.md"})).result,
        OracleResult::Falsified
    );
}

#[test]
fn exit_code_conditions_require_a_real_code() {
    let f = fixture("or-exit");
    assert_eq!(
        eval(
            &f,
            json!({"type": "exit_code", "step": "run:0", "equals": 0})
        )
        .result,
        OracleResult::Falsified
    );
    assert_eq!(
        eval(
            &f,
            json!({"type": "exit_code", "step": "run:0", "not_equals": 0})
        )
        .result,
        OracleResult::NotFalsified
    );
    // Step that never produced an exit code (timed out): Undetermined.
    assert_eq!(
        eval(
            &f,
            json!({"type": "exit_code", "step": "run:1", "equals": 0})
        )
        .result,
        OracleResult::Undetermined
    );
    // Missing step: Undetermined.
    assert_eq!(
        eval(
            &f,
            json!({"type": "exit_code", "step": "setup:3", "equals": 0})
        )
        .result,
        OracleResult::Undetermined
    );
}

#[test]
fn evidence_present_checks_capture_kinds() {
    let f = fixture("or-present");
    assert_eq!(
        eval(&f, json!({"type": "evidence_present", "kind": "git_diff"})).result,
        OracleResult::Falsified
    );
    assert_eq!(
        eval(
            &f,
            json!({"type": "evidence_present", "kind": "base_snapshot"})
        )
        .result,
        OracleResult::NotFalsified
    );
    assert_eq!(
        eval(&f, json!({"type": "evidence_present", "kind": "bogus"})).result,
        OracleResult::Undetermined
    );
}

#[test]
fn json_value_equals_uses_pointer_lookup() {
    let f = fixture("or-json");
    assert_eq!(
        eval(
            &f,
            json!({"type": "json_value_equals", "path": "data.json", "pointer": "/a/b", "value": 42})
        )
        .result,
        OracleResult::Falsified
    );
    assert_eq!(
        eval(
            &f,
            json!({"type": "json_value_equals", "path": "data.json", "pointer": "/a/b", "value": 43})
        )
        .result,
        OracleResult::NotFalsified
    );
    // Missing pointer -> Undetermined.
    assert_eq!(
        eval(
            &f,
            json!({"type": "json_value_equals", "path": "data.json", "pointer": "/a/missing", "value": 42})
        )
        .result,
        OracleResult::Undetermined
    );
    // Invalid JSON -> Undetermined.
    assert_eq!(
        eval(
            &f,
            json!({"type": "json_value_equals", "path": "broken.json", "pointer": "/a", "value": 1})
        )
        .result,
        OracleResult::Undetermined
    );
}

// ---------------------------------------------------------------------------
// Command verifier
// ---------------------------------------------------------------------------

#[test]
fn command_verifier_judges_by_exit_code_only() {
    let f = fixture("or-verifier");
    let probe = env!("CARGO_BIN_EXE_tb_probe_exit");
    // Expected exit -> NotFalsified (verifier confirms the invariant holds).
    let r = eval(
        &f,
        json!({"type": "command", "program": probe, "args": ["0"], "expect_exit": 0}),
    );
    assert_eq!(r.result, OracleResult::NotFalsified);
    // Unexpected exit -> Falsified.
    let r = eval(
        &f,
        json!({"type": "command", "program": probe, "args": ["7"], "expect_exit": 0}),
    );
    assert_eq!(r.result, OracleResult::Falsified);
}

#[test]
fn command_verifier_timeout_is_undetermined_not_falsification() {
    let f = fixture("or-verifier-timeout");
    let hang = env!("CARGO_BIN_EXE_tb_probe_hang");
    let expr = oracle::parse(&json!({
        "type": "command", "program": hang, "expect_exit": 0
    }))
    .expect("parses");
    let mut verifier_steps: Vec<StepRecord> = Vec::new();
    let mut ctx = EvalCtx {
        store: &f.store,
        worktree: &f.worktree,
        spec_dir: &f.worktree,
        steps: &f.steps,
        captures: &f.captures,
        verifier_deadline: Some(Duration::from_secs(1)),
        verifier_steps: &mut verifier_steps,
    };
    let r = oracle::evaluate(&expr, &mut ctx);
    assert_eq!(r.result, OracleResult::Undetermined);
    assert_eq!(verifier_steps.len(), 1, "verifier run is recorded");
    assert_eq!(verifier_steps[0].phase, "oracle");
    assert_eq!(verifier_steps[0].status, RunStatus::TimedOut);
    let note = r.conditions[0].note.clone().unwrap_or_default();
    assert!(note.contains("cannot falsify"), "{note}");
}

// ---------------------------------------------------------------------------
// Determinism and malformed input
// ---------------------------------------------------------------------------

#[test]
fn evaluation_is_deterministic() {
    let f = fixture("or-determinism");
    let oracle_json = json!({"any": [
        {"type": "git_diff_contains", "substring": "pwned"},
        {"all": [
            {"type": "exit_code", "step": "run:0", "equals": 0},
            {"not": {"type": "file_exists", "path": "missing.txt"}},
        ]},
    ]});
    let a = eval(&f, oracle_json.clone());
    let b = eval(&f, oracle_json);
    assert_eq!(a.result, b.result);
    assert_eq!(a.conditions.len(), b.conditions.len());
    for (x, y) in a.conditions.iter().zip(&b.conditions) {
        assert_eq!(x.result, y.result);
        assert_eq!(x.condition, y.condition);
    }
}

#[test]
fn malformed_oracles_are_rejected_at_parse_time() {
    let cases = vec![
        (
            json!({"type": "no_such_condition"}),
            "invalid oracle condition",
        ),
        (json!({"all": "not-a-list"}), "must be a list"),
        (json!({"all": [], "any": []}), "exactly one key"),
        (json!({"bogus": []}), "unknown oracle node"),
        (json!([1, 2]), "must be a mapping"),
        (
            json!({"type": "exit_code", "step": "run:0", "equals": 0, "not_equals": 1}),
            "exactly one of",
        ),
        (
            json!({"type": "exit_code", "step": "nonsense", "equals": 0}),
            "step reference",
        ),
        (
            json!({"type": "text_matches", "path": "x", "regex": "("}),
            "invalid regex",
        ),
        (
            json!({"type": "text_contains", "path": "x", "evidence": "y", "substring": "z"}),
            "exactly one of",
        ),
    ];
    for (oracle_json, expected) in cases {
        let err = parse_err(oracle_json.clone());
        assert!(err.contains(expected), "for {oracle_json}: {err}");
    }
}
