//! Stateful process lifecycle invariants: handles span steps, generations survive restart, and captures stay bounded.
mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::Path;
use std::time::Duration;

use common::{TempDir, make_repo, write_spec, yaml_path};
use serde_json::{Value, json};
use terrorbat::builtins::{StepCtx, StepOutcome, dispatch};
use terrorbat::process_runtime::{ProcessRegistry, ProcessRuntimeReport};
use terrorbat::runner::{RunOptions, RunStatus, run_bat};

fn fixture() -> &'static str {
    env!("CARGO_BIN_EXE_tb_process_fixture")
}
fn values(value: Value) -> BTreeMap<String, Value> {
    value.as_object().unwrap().clone().into_iter().collect()
}
fn invoke(
    registry: &RefCell<ProcessRegistry>,
    root: &Path,
    action: &str,
    payload: Value,
    index: usize,
) -> Result<StepOutcome, terrorbat::builtins::StepError> {
    let ctx = StepCtx {
        worktree: root,
        spec_dir: root,
        deadline: Some(Duration::from_secs(5)),
        processes: Some(registry),
        phase: "run",
        step_index: index,
    };
    dispatch("process", action, &values(payload), &ctx)
}
fn start(
    registry: &RefCell<ProcessRegistry>,
    root: &Path,
    handle: &str,
    args: &[&str],
    output_limit: Option<usize>,
) {
    let mut payload =
        json!({"handle":handle,"program":fixture(),"args":args,"cwd":".","stdin":"piped"});
    if let Some(limit) = output_limit {
        payload["output_limit_bytes"] = json!(limit);
    }
    invoke(registry, root, "start", payload, 0).expect("start persistent fixture");
}
fn ready(
    registry: &RefCell<ProcessRegistry>,
    root: &Path,
    handle: &str,
    key: &str,
    value: Value,
    timeout_ms: u64,
    index: usize,
) -> StepOutcome {
    let mut payload = json!({"handle":handle,"timeout_ms":timeout_ms});
    payload[key] = value;
    invoke(registry, root, "wait_ready", payload, index).expect("readiness step")
}
fn kill(registry: &RefCell<ProcessRegistry>, root: &Path, handle: &str, index: usize) {
    invoke(
        registry,
        root,
        "kill",
        json!({"handle":handle,"timeout_ms":2000}),
        index,
    )
    .expect("kill owned process group");
}

#[test]
fn persistent_process_roundtrips_stdin_and_cleans_its_tree() {
    let temp = TempDir::new("process-io");
    let registry = RefCell::new(ProcessRegistry::new("execution-fixture"));
    start(&registry, &temp.path, "server", &["server"], None);
    assert_eq!(
        ready(
            &registry,
            &temp.path,
            "server",
            "stdout_contains",
            json!("READY"),
            2000,
            1
        )
        .status,
        terrorbat::supervisor::ExecutionStatus::Completed
    );
    invoke(
        &registry,
        &temp.path,
        "write_stdin",
        json!({"handle":"server","text":"ping","newline":true}),
        2,
    )
    .expect("write same process stdin");
    assert_eq!(
        ready(
            &registry,
            &temp.path,
            "server",
            "stdout_contains",
            json!("ECHO:ping"),
            2000,
            3
        )
        .status,
        terrorbat::supervisor::ExecutionStatus::Completed
    );
    let observed = invoke(
        &registry,
        &temp.path,
        "observe",
        json!({"handle":"server"}),
        4,
    )
    .expect("observe before exit");
    assert!(String::from_utf8_lossy(&observed.stdout).contains("ECHO:ping"));
    kill(&registry, &temp.path, "server", 5);
    let report = registry.borrow().report();
    assert_eq!(report.handles[0].generations.len(), 1);
    assert_eq!(
        report.handles[0].generations[0].termination.as_deref(),
        Some("killed")
    );
    assert!(report.handles[0].generations[0].pid.is_some());
    assert_ne!(report.handles[0].generations[0].process_identity, "");
}

#[test]
fn restart_increments_generation_and_preserves_prior_identity_and_state() {
    let temp = TempDir::new("process-restart");
    let registry = RefCell::new(ProcessRegistry::new("restart-execution"));
    let state = temp.join("state.txt");
    start(
        &registry,
        &temp.path,
        "worker",
        &["state", "state.txt"],
        None,
    );
    assert_eq!(
        ready(
            &registry,
            &temp.path,
            "worker",
            "stdout_contains",
            json!("GEN:1"),
            2000,
            1
        )
        .status,
        terrorbat::supervisor::ExecutionStatus::Completed
    );
    kill(&registry, &temp.path, "worker", 2);
    invoke(
        &registry,
        &temp.path,
        "restart",
        json!({"handle":"worker"}),
        3,
    )
    .expect("restart terminal generation");
    assert_eq!(
        ready(
            &registry,
            &temp.path,
            "worker",
            "stdout_contains",
            json!("GEN:2"),
            2000,
            4
        )
        .status,
        terrorbat::supervisor::ExecutionStatus::Completed
    );
    let report = registry.borrow().report();
    let generations = &report.handles[0].generations;
    assert_eq!(generations.len(), 2);
    assert_eq!(generations[0].generation, 1);
    assert_eq!(generations[1].generation, 2);
    assert_ne!(
        generations[0].process_identity,
        generations[1].process_identity
    );
    assert_eq!(std::fs::read_to_string(state).unwrap(), "2");
    kill(&registry, &temp.path, "worker", 5);
}

#[test]
fn readiness_timeout_and_handle_errors_are_not_target_findings() {
    let temp = TempDir::new("process-negative");
    let registry = RefCell::new(ProcessRegistry::new("negative-execution"));
    start(&registry, &temp.path, "quiet", &["silent"], None);
    let timeout = ready(
        &registry,
        &temp.path,
        "quiet",
        "stdout_contains",
        json!("NEVER"),
        50,
        1,
    );
    assert_eq!(
        timeout.status,
        terrorbat::supervisor::ExecutionStatus::TimedOut
    );
    kill(&registry, &temp.path, "quiet", 2);
    assert!(
        matches!(invoke(&registry, &temp.path, "observe", json!({"handle":"missing"}), 3), Err(terrorbat::builtins::StepError::Malformed(message)) if message.contains("PROCESS_HANDLE_NOT_FOUND"))
    );
    assert!(
        matches!(invoke(&registry, &temp.path, "write_stdin", json!({"handle":"quiet","text":"late"}), 4), Err(terrorbat::builtins::StepError::Io(message)) if message.contains("PROCESS_ALREADY_EXITED"))
    );
}

#[test]
fn tcp_readiness_waits_for_a_live_listener() {
    let temp = TempDir::new("process-tcp");
    let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let registry = RefCell::new(ProcessRegistry::new("tcp-execution"));
    start(
        &registry,
        &temp.path,
        "listener",
        &["tcp", &port.to_string()],
        None,
    );
    let outcome = ready(
        &registry,
        &temp.path,
        "listener",
        "tcp",
        json!({"host":"127.0.0.1","port":port}),
        2000,
        1,
    );
    assert_eq!(
        outcome.status,
        terrorbat::supervisor::ExecutionStatus::Completed
    );
    kill(&registry, &temp.path, "listener", 2);
}

#[test]
fn owned_child_is_terminated_with_its_process_group() {
    let temp = TempDir::new("process-child");
    let registry = RefCell::new(ProcessRegistry::new("child-execution"));
    start(&registry, &temp.path, "parent", &["child"], None);
    let announced = ready(
        &registry,
        &temp.path,
        "parent",
        "stdout_contains",
        json!("CHILD_PID:"),
        2000,
        1,
    );
    assert_eq!(
        announced.status,
        terrorbat::supervisor::ExecutionStatus::Completed
    );
    let output = invoke(
        &registry,
        &temp.path,
        "observe",
        json!({"handle":"parent"}),
        2,
    )
    .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    let child_pid: u32 = text
        .lines()
        .find_map(|line| line.strip_prefix("CHILD_PID:"))
        .expect("fixture reports child identity")
        .parse()
        .unwrap();
    kill(&registry, &temp.path, "parent", 3);
    assert!(
        !process_is_alive(child_pid),
        "owned child {child_pid} survived group kill"
    );
    let generation = &registry.borrow().report().handles[0].generations[0];
    assert_eq!(generation.termination.as_deref(), Some("killed"));
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .expect("tasklist is available on Windows");
    String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
}

#[test]
fn output_retention_is_bounded_and_reports_truncation() {
    let temp = TempDir::new("process-output-limit");
    let registry = RefCell::new(ProcessRegistry::new("output-execution"));
    start(&registry, &temp.path, "flood", &["flood"], Some(1024));
    let out = ready(
        &registry,
        &temp.path,
        "flood",
        "stdout_contains",
        json!("x"),
        2000,
        1,
    );
    assert_eq!(
        out.status,
        terrorbat::supervisor::ExecutionStatus::Completed
    );
    let observed = invoke(
        &registry,
        &temp.path,
        "observe",
        json!({"handle":"flood"}),
        2,
    )
    .unwrap();
    assert!(observed.stdout.len() <= 1024);
    assert!(observed.stdout_total >= observed.stdout.len() as u64);
    assert!(observed.stdout_total > 1024);
    kill(&registry, &temp.path, "flood", 3);
}

#[test]
fn live_bat_receipt_and_replay_preserve_semantics_with_fresh_processes() {
    let temp = TempDir::new("process-replay");
    let repo = temp.join("target");
    let target_sha = make_repo(&repo);
    let bat_dir = temp.join("bats");
    let spec = format!(
        r#"version: terrorbat/v1
id: persistent-stdin-roundtrip
claim:
  text: The persistent worker returns deterministic replies to requests.
requires: [process.start, process.readiness, process.stdin, process.observe, process.kill]
attack:
  run:
    - adapter: process
      action: start
      handle: worker
      program: {}
      args: [server]
      cwd: .
      stdin: piped
    - adapter: process
      action: wait_ready
      handle: worker
      stdout_contains: READY
      timeout_ms: 2000
    - adapter: process
      action: write_stdin
      handle: worker
      text: ping
      newline: true
    - adapter: process
      action: wait_ready
      handle: worker
      stdout_contains: ECHO:ping
      timeout_ms: 2000
    - adapter: process
      action: observe
      handle: worker
    - adapter: process
      action: kill
      handle: worker
      timeout_ms: 2000
oracle:
  all: []
evidence:
  capture: [stdout, stderr]
timeout:
  run: 15s
"#,
        yaml_path(Path::new(fixture()))
    );
    let bat = write_spec(&bat_dir, "stateful.yaml", &spec);
    let store = temp.join("store");
    let output = run_bat(&RunOptions {
        bat_path: bat,
        repo: repo.clone(),
        store_root: Some(store.clone()),
        overrides: vec![],
    })
    .expect("live stateful Bat");
    assert_eq!(
        output.manifest.run_status,
        RunStatus::Completed,
        "manifest: {:#?}",
        output.manifest
    );
    assert_eq!(output.receipt.target.commit, target_sha);
    let first = &output.receipt.execution.processes.handles[0].generations[0];
    assert!(first.events.iter().any(|event| event.event == "ready"));
    assert!(
        first
            .events
            .iter()
            .any(|event| event.event == "stdin_write")
    );
    assert_eq!(first.termination.as_deref(), Some("killed"));
    assert!(output.receipt.execution.processes.cleanup.attempted);
    assert!(
        !output
            .manifest
            .capabilities
            .unenforced
            .iter()
            .any(|capability| capability.starts_with("process."))
    );
    let original_id = output.receipt.receipt_id.clone();
    let (replay, replayed) =
        terrorbat::receipt::replay(&original_id, Some(store)).expect("stateful replay");
    assert_eq!(replayed.manifest.run_status, RunStatus::Completed);
    assert_eq!(replay.new_execution_id, replayed.execution_id);
    assert_eq!(replay.original_receipt_id, original_id);
    assert!(replay.same_process_semantics);
    let next = &replayed.receipt.execution.processes.handles[0].generations[0];
    assert_ne!(first.process_identity, next.process_identity);
    assert_eq!(
        first.events.iter().map(|e| &e.event).collect::<Vec<_>>(),
        next.events.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert_eq!(
        output.receipt.receipt_id, original_id,
        "replay must not modify original receipt"
    );
}

#[test]
fn receipt_process_default_remains_historically_readable() {
    let value = json!({"handles":[],"cleanup":{"attempted":false,"terminated":[],"forced":[],"survivors":[],"errors":[]}});
    let report: ProcessRuntimeReport = serde_json::from_value(value).unwrap();
    assert!(report.handles.is_empty());
}

#[cfg(windows)]
#[test]
fn graceful_termination_is_explicitly_unsupported_on_windows() {
    let temp = TempDir::new("process-terminate-unsupported");
    let registry = RefCell::new(ProcessRegistry::new("terminate-execution"));
    start(&registry, &temp.path, "server", &["server"], None);
    ready(
        &registry,
        &temp.path,
        "server",
        "stdout_contains",
        json!("READY"),
        2000,
        1,
    );
    assert!(matches!(
        invoke(
            &registry,
            &temp.path,
            "terminate",
            json!({"handle":"server"}),
            2
        ),
        Err(terrorbat::builtins::StepError::Unsupported(_))
    ));
    kill(&registry, &temp.path, "server", 3);
}

#[test]
fn bat_timeout_cleans_persistent_process_and_records_cleanup() {
    let temp = TempDir::new("process-timeout-cleanup");
    let repo = temp.join("target");
    make_repo(&repo);
    let bat_dir = temp.join("bats");
    let spec = format!(
        r#"version: terrorbat/v1
id: process-timeout-cleanup
claim:
  text: A silent worker reaches READY.
requires: [process.start, process.readiness]
attack:
  run:
    - adapter: process
      action: start
      handle: quiet
      program: {}
      args: [silent]
      cwd: .
    - adapter: process
      action: wait_ready
      handle: quiet
      stdout_contains: READY
      timeout_ms: 50
oracle:
  all: []
evidence:
  capture: [stdout, stderr]
timeout:
  run: 5s
"#,
        yaml_path(Path::new(fixture()))
    );
    let bat = write_spec(&bat_dir, "timeout.yaml", &spec);
    let output = run_bat(&RunOptions {
        bat_path: bat,
        repo,
        store_root: Some(temp.join("store")),
        overrides: vec![],
    })
    .unwrap();
    assert_eq!(output.manifest.run_status, RunStatus::TimedOut);
    let cleanup = &output.receipt.execution.processes.cleanup;
    assert!(cleanup.attempted);
    assert!(cleanup.survivors.is_empty());
    assert!(cleanup.terminated.len() + cleanup.forced.len() >= 1);
}
