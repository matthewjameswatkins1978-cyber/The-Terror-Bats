//! M2 adversarial tests: the supervisor must own, observe, cancel, and
//! terminate process trees — including hostile ones — without leaking
//! descendants. Windows is a first-class target: this suite runs on Windows.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use terrorbats::supervisor::{
    CancelToken, CapturedStream, ExecutionStatus, StdinPolicy, SupervisedCommand,
};

/// Cold process spawns (loader, antivirus) are the dominant timing variance
/// on a loaded machine. Warm the probe binaries once so deadline/cancel
/// tests measure supervisor behaviour, not first-spawn latency.
static WARM: std::sync::OnceLock<()> = std::sync::OnceLock::new();

fn warm() {
    WARM.get_or_init(|| {
        let mut cmd = probe_cmd("exit");
        cmd.args = vec!["0".into()];
        let out = run(&cmd);
        assert_eq!(out.status, ExecutionStatus::Completed);
    });
}

macro_rules! probe {
    ($name:literal) => {
        PathBuf::from(env!(concat!("CARGO_BIN_EXE_", $name)))
    };
}

fn probe_cmd(name: &str) -> SupervisedCommand {
    let program = match name {
        "exit" => probe!("tb_probe_exit"),
        "crash" => probe!("tb_probe_crash"),
        "hang" => probe!("tb_probe_hang"),
        "tree" => probe!("tb_probe_tree"),
        "ignore_term" => probe!("tb_probe_ignore_term"),
        "flood" => probe!("tb_probe_flood"),
        "race" => probe!("tb_probe_race"),
        _ => unreachable!(),
    };
    SupervisedCommand::new(program)
}

fn run(cmd: &SupervisedCommand) -> terrorbats::supervisor::SupervisedOutcome {
    terrorbats::supervisor::run(cmd, &CancelToken::new())
}

fn heartbeat_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "terrorbat-m2-heartbeat-{tag}-{}.log",
        std::process::id()
    ))
}

/// Assert the tree was alive (heartbeats exist) and then died: file size must
/// stop growing after the supervisor returns.
fn assert_tree_terminated(path: &std::path::Path) {
    let size_before = std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("{}: heartbeat file missing: {e}", path.display()))
        .len();
    assert!(size_before > 0, "tree never started producing heartbeats");
    std::thread::sleep(Duration::from_millis(400));
    let size_mid = std::fs::metadata(path).unwrap().len();
    std::thread::sleep(Duration::from_millis(800));
    let size_after = std::fs::metadata(path).unwrap().len();
    assert_eq!(
        size_mid, size_after,
        "heartbeat file kept growing after supervisor return: descendants leaked"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn exit_zero_completes() {
    let mut cmd = probe_cmd("exit");
    cmd.args = vec!["0".into()];
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::Completed);
    assert_eq!(out.exit_code, Some(0));
    assert!(out.error.is_none());
}

#[test]
fn exit_nonzero_completes_with_code() {
    let mut cmd = probe_cmd("exit");
    cmd.args = vec!["42".into()];
    let out = run(&cmd);
    // A failing process is still a completed run, not a supervisor event.
    assert_eq!(out.status, ExecutionStatus::Completed);
    assert_eq!(out.exit_code, Some(42));
}

#[test]
fn crash_reports_honestly_per_platform() {
    let out = run(&probe_cmd("crash"));
    #[cfg(unix)]
    {
        assert_eq!(out.status, ExecutionStatus::Crashed);
        assert_eq!(out.exit_code, None);
    }
    #[cfg(windows)]
    {
        // Windows surfaces even genuine crashes as exit codes; the honest
        // observation is a nonzero completion, never a silent success.
        assert_eq!(out.status, ExecutionStatus::Completed);
        assert!(out.exit_code.unwrap_or(0) != 0);
    }
}

#[test]
fn hang_hits_deadline_and_dies() {
    warm();
    let mut cmd = probe_cmd("hang");
    cmd.deadline = Some(Duration::from_secs(1));
    let start = Instant::now();
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::TimedOut);
    assert_eq!(out.exit_code, None);
    assert!(out.wall_time >= Duration::from_secs(1));
    assert!(
        start.elapsed() < Duration::from_secs(15),
        "supervisor must stay bounded"
    );
}

#[test]
fn child_and_hang_are_killed_together() {
    warm();
    let hb = heartbeat_path("child");
    let _ = std::fs::remove_file(&hb);
    let mut cmd = probe_cmd("tree");
    cmd.args = vec![
        "parent".into(),
        "--heartbeat".into(),
        hb.to_string_lossy().into_owned().into(),
    ];
    cmd.deadline = Some(Duration::from_secs(2));
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::TimedOut);
    assert_tree_terminated(&hb);
}

#[test]
fn grandchild_is_killed_too() {
    warm();
    let hb = heartbeat_path("grandchild");
    let _ = std::fs::remove_file(&hb);
    let mut cmd = probe_cmd("tree");
    cmd.args = vec![
        "child".into(),
        "--heartbeat".into(),
        hb.to_string_lossy().into_owned().into(),
    ];
    cmd.deadline = Some(Duration::from_secs(2));
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::TimedOut);
    assert_tree_terminated(&hb);
}

#[test]
fn graceful_refusal_still_terminates() {
    warm();
    let mut cmd = probe_cmd("ignore_term");
    cmd.deadline = Some(Duration::from_secs(1));
    cmd.grace_period = Duration::from_secs(1);
    let start = Instant::now();
    let out = run(&cmd);
    // On Unix this proves SIGKILL-after-SIGTERM; on Windows it proves the
    // forced job kill path. Either way the tree must die, boundedly.
    assert_eq!(out.status, ExecutionStatus::TimedOut);
    assert!(
        start.elapsed() < Duration::from_secs(15),
        "forced termination must stay bounded"
    );
}

#[test]
fn large_stdout_is_bounded_and_honest() {
    let mib = 8u64;
    let mut cmd = probe_cmd("flood");
    cmd.args = vec!["--stdout-mib".into(), mib.to_string().into()];
    cmd.capture_limit = 64 * 1024;
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::Completed);
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(out.stdout.total_bytes, mib * 1024 * 1024);
    assert_eq!(out.stdout.bytes.len(), 64 * 1024);
    assert!(out.stdout.truncated());
    assert!(out.stdout.bytes.iter().all(|&b| b == b'o'));
}

#[test]
fn large_stderr_is_bounded_and_honest() {
    let mut cmd = probe_cmd("flood");
    cmd.args = vec!["--stderr-mib".into(), "4".into()];
    cmd.capture_limit = 1024;
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::Completed);
    assert_eq!(out.stderr.total_bytes, 4 * 1024 * 1024);
    assert_eq!(out.stderr.bytes.len(), 1024);
    assert!(out.stderr.truncated());
}

#[test]
fn concurrent_streams_do_not_deadlock() {
    let mut cmd = probe_cmd("flood");
    cmd.args = vec![
        "--stdout-mib".into(),
        "4".into(),
        "--stderr-mib".into(),
        "4".into(),
    ];
    cmd.capture_limit = 32 * 1024;
    let start = Instant::now();
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::Completed);
    assert_eq!(out.stdout.total_bytes, 4 * 1024 * 1024);
    assert_eq!(out.stderr.total_bytes, 4 * 1024 * 1024);
    assert!(
        start.elapsed() < Duration::from_secs(60),
        "pipes must be pumped concurrently"
    );
}

#[test]
fn small_output_passes_through_exactly() {
    let mut cmd = probe_cmd("exit");
    cmd.args = vec!["--echo-stdin".into(), "0".into()];
    cmd.stdin = StdinPolicy::Bytes(b"hello supervisor".to_vec());
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::Completed);
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(out.stdout.bytes, b"hello supervisor");
    assert!(!out.stdout.truncated());
    assert_eq!(out.stdout.total_bytes, 16);
}

#[test]
fn cancel_during_run_terminates_tree() {
    warm();
    let hb = heartbeat_path("cancel");
    let _ = std::fs::remove_file(&hb);
    let mut cmd = probe_cmd("tree");
    cmd.args = vec![
        "parent".into(),
        "--heartbeat".into(),
        hb.to_string_lossy().into_owned().into(),
    ];
    let cancel = CancelToken::new();
    let canceller = cancel.clone();
    // Run the supervisor on a thread; cancel only after the tree has proven
    // it is alive (heartbeat file non-empty). No fixed sleep, no race.
    let handle = std::thread::spawn(move || terrorbats::supervisor::run(&cmd, &canceller));
    let start = Instant::now();
    loop {
        let alive = std::fs::metadata(&hb).map(|m| m.len() > 0).unwrap_or(false);
        if alive {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "tree never started"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    cancel.cancel();
    let run_start = Instant::now();
    let out = handle.join().unwrap();
    assert_eq!(out.status, ExecutionStatus::Cancelled);
    assert!(run_start.elapsed() < Duration::from_secs(15));
    assert_tree_terminated(&hb);
}

#[test]
fn pre_cancelled_token_spawns_nothing() {
    // A program that does not exist would be InfrastructureError if spawn
    // were attempted; Cancelled proves the supervisor never spawned.
    let mut cmd = SupervisedCommand::new("definitely-not-a-real-program-xyz");
    cmd.deadline = Some(Duration::from_secs(30));
    let cancel = CancelToken::new();
    cancel.cancel();
    let start = Instant::now();
    let out = terrorbats::supervisor::run(&cmd, &cancel);
    assert_eq!(out.status, ExecutionStatus::Cancelled);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn timeout_race_stays_internally_consistent() {
    warm();
    // Sleep and deadline coincide: either outcome is legitimate, but each
    // must be self-consistent (TimedOut never carries an exit code; a
    // Completed race exit carries the probe's code).
    let mut cmd = probe_cmd("race");
    cmd.args = vec![
        "--sleep-ms".into(),
        "1500".into(),
        "--code".into(),
        "7".into(),
    ];
    cmd.deadline = Some(Duration::from_millis(1500));
    let out = run(&cmd);
    match out.status {
        ExecutionStatus::Completed => assert_eq!(out.exit_code, Some(7)),
        ExecutionStatus::TimedOut => assert_eq!(out.exit_code, None),
        other => panic!("race must end Completed or TimedOut, got {other:?}"),
    }
}

#[test]
fn missing_program_is_infrastructure_error() {
    let cmd = SupervisedCommand::new("definitely-not-a-real-program-xyz");
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::InfrastructureError);
    assert!(out.exit_code.is_none());
    let msg = out.error.unwrap_or_default();
    assert!(msg.contains("definitely-not-a-real-program-xyz"), "{msg}");
}

#[test]
fn env_policy_is_honoured() {
    let var: std::ffi::OsString = "TERRORBAT_M2_PROBE_VAR".into();
    // Inherited environment passes through by default.
    let mut cmd = probe_cmd("exit");
    cmd.args = vec![
        "--print-env".into(),
        var.to_string_lossy().into_owned().into(),
        "0".into(),
    ];
    cmd.env.set.push((var.clone(), "bar".into()));
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::Completed);
    assert_eq!(out.stdout.as_str_lossy().trim(), "bar");

    // A cleared environment with no set value observes <unset>.
    let mut cmd = probe_cmd("exit");
    cmd.args = vec![
        "--print-env".into(),
        var.to_string_lossy().into_owned().into(),
        "0".into(),
    ];
    cmd.env.clear = true;
    // SystemRoot is required on Windows to spawn anything; keep the bare
    // minimum so the test stays hermetic yet functional.
    #[cfg(windows)]
    if let Ok(root) = std::env::var("SystemRoot") {
        cmd.env.set.push(("SystemRoot".into(), root.into()));
    }
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::Completed);
    assert_eq!(out.stdout.as_str_lossy().trim(), "<unset>");
}

#[test]
fn working_directory_is_honoured() {
    let dir = std::env::temp_dir();
    let mut cmd = probe_cmd("exit");
    cmd.args = vec!["--print-cwd".into(), "0".into()];
    cmd.cwd = Some(dir.clone());
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::Completed);
    let printed = PathBuf::from(out.stdout.as_str_lossy().trim());
    assert_eq!(printed.canonicalize().unwrap(), dir.canonicalize().unwrap());
}

#[test]
fn sequential_runs_do_not_contaminate_each_other() {
    let mut first = probe_cmd("exit");
    first.args = vec!["3".into()];
    let mut second = probe_cmd("exit");
    second.args = vec!["4".into()];
    let a = run(&first);
    let b = run(&second);
    assert_eq!((a.exit_code, b.exit_code), (Some(3), Some(4)));
}

#[test]
fn capture_limit_zero_keeps_totals() {
    let mut cmd = probe_cmd("flood");
    cmd.args = vec!["--stdout-mib".into(), "1".into()];
    cmd.capture_limit = 0;
    let out = run(&cmd);
    assert_eq!(out.status, ExecutionStatus::Completed);
    assert!(out.stdout.bytes.is_empty());
    assert_eq!(out.stdout.total_bytes, 1024 * 1024);
    assert!(out.stdout.truncated());
}

#[test]
fn stream_helpers_report_truncation() {
    let full = CapturedStream {
        bytes: b"abc".to_vec(),
        total_bytes: 3,
    };
    assert!(!full.truncated());
    let cut = CapturedStream {
        bytes: b"ab".to_vec(),
        total_bytes: 3,
    };
    assert!(cut.truncated());
}
