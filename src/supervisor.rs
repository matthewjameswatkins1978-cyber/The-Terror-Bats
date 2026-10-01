//! OTP-inspired supervised process execution (M2).
//!
//! Every process has an owner. The supervisor owns one process tree: its
//! command, arguments, working directory, environment policy, stdin,
//! stdout/stderr capture, start time, deadline, cancellation, termination,
//! and exit result.
//!
//! ```text
//! Supervisor
//!    │
//!    └── Process
//!         ├── child
//!         └── grandchild
//! ```
//!
//! Termination is tree-scoped, never child-only: on Unix the child runs in
//! its own process group (`setpgid`) and signals go to the group; on Windows
//! the tree runs in a Job Object. This is borrowed from the maintained
//! `command-group` crate rather than reimplemented.
//!
//! Shutdown sequence: cancel/deadline → graceful termination (Unix `SIGTERM`
//! to the group) → short bounded grace period → forced termination
//! (`SIGKILL` to the group / job termination) → reap and collect. The
//! supervisor never waits indefinitely: every phase is bounded.
//!
//! Platform honesty:
//!
//! - Windows has no reliable graceful signal for arbitrary processes, so the
//!   graceful step is skipped there and shutdown proceeds directly to forced
//!   job termination (still bounded).
//! - A Unix child killed by a signal reports [`ExecutionStatus::Crashed`].
//!   On Windows every termination surfaces as an exit code (even a genuine
//!   crash arrives as an `NTSTATUS`-derived code), so abnormal termination
//!   there is observed as [`ExecutionStatus::Completed`] with a nonzero code.
//! - A double-fork daemon that leaves the process group / job can escape
//!   tree termination (and delay pipe EOF). That is outside M2's containment
//!   model; stronger backends arrive in later milestones.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use command_group::{CommandGroup, GroupChild};
#[cfg(unix)]
use command_group::{Signal, UnixChildExt};

/// How often the supervisor polls the child (also bounds timeout precision).
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Absolute failsafe: a tree that survives this long after forced termination
/// is reported as an infrastructure failure rather than waited on forever.
const KILL_FAILSAFE: Duration = Duration::from_secs(30);
/// Default per-stream retained bytes.
pub const DEFAULT_CAPTURE_LIMIT: u64 = 1024 * 1024;
/// Default graceful-shutdown grace period (Unix only; see module docs).
pub const DEFAULT_GRACE_PERIOD: Duration = Duration::from_secs(2);

/// What mechanically happened to the supervised run.
///
/// This is execution status only — never an epistemic verdict about any
/// tested claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStatus {
    /// The process ran to a natural exit (any exit code, including nonzero).
    Completed,
    /// The deadline elapsed; the tree was terminated by the supervisor.
    TimedOut,
    /// The process died abnormally without supervisor intervention
    /// (Unix: killed by a signal).
    Crashed,
    /// Cancellation was requested; the tree was terminated by the supervisor.
    Cancelled,
    /// The supervisor itself failed (spawn failure, wait failure, a tree
    /// that would not die). Details are in [`SupervisedOutcome::error`].
    InfrastructureError,
}

/// How the child's environment is constructed.
#[derive(Debug, Clone, Default)]
pub struct EnvPolicy {
    /// Start from an empty environment instead of inheriting the host's.
    pub clear: bool,
    /// Variables to set (applied after clear/remove).
    pub set: Vec<(OsString, OsString)>,
    /// Variables to remove from the inherited environment.
    pub remove: Vec<OsString>,
}

/// What the child receives on stdin.
#[derive(Debug, Clone, Default)]
pub enum StdinPolicy {
    /// No input (stdin is null).
    #[default]
    Null,
    /// These bytes are written to the child's stdin, then it is closed.
    Bytes(Vec<u8>),
}

/// One supervised command: everything the supervisor owns.
#[derive(Debug, Clone)]
pub struct SupervisedCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: EnvPolicy,
    pub stdin: StdinPolicy,
    /// Wall-clock budget from spawn. `None` means no deadline (cancellation
    /// still applies).
    pub deadline: Option<Duration>,
    /// Bounded graceful-shutdown window (Unix `SIGTERM` → `SIGKILL`).
    pub grace_period: Duration,
    /// Retained bytes per stream (stdout, stderr). Totals are always counted;
    /// see [`CapturedStream`].
    pub capture_limit: u64,
}

impl SupervisedCommand {
    pub fn new(program: impl Into<OsString>) -> Self {
        SupervisedCommand {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: EnvPolicy::default(),
            stdin: StdinPolicy::Null,
            deadline: None,
            grace_period: DEFAULT_GRACE_PERIOD,
            capture_limit: DEFAULT_CAPTURE_LIMIT,
        }
    }
}

/// Bounded capture of one stream: the retained head plus honest totals.
#[derive(Debug, Clone, Default)]
pub struct CapturedStream {
    /// Retained bytes (head of the stream, up to the capture limit).
    pub bytes: Vec<u8>,
    /// Total bytes the child produced on this stream.
    pub total_bytes: u64,
}

impl CapturedStream {
    pub fn empty() -> Self {
        CapturedStream::default()
    }

    /// True when the child produced more than was retained. Never silent:
    /// callers can always see `total_bytes` vs `bytes.len()`.
    pub fn truncated(&self) -> bool {
        self.total_bytes > self.bytes.len() as u64
    }

    pub fn as_str_lossy(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }
}

/// The supervisor's final report for one process tree.
#[derive(Debug, Clone)]
pub struct SupervisedOutcome {
    pub status: ExecutionStatus,
    /// Exit code for naturally-exited processes. `None` when the process never
    /// produced one (signalled on Unix; killed by the supervisor).
    pub exit_code: Option<i32>,
    /// Unix signal number when the process died from a signal.
    pub signal: Option<i32>,
    pub stdout: CapturedStream,
    pub stderr: CapturedStream,
    pub wall_time: Duration,
    /// Supervisor-side failure detail; set for [`ExecutionStatus::InfrastructureError`].
    pub error: Option<String>,
}

/// Cooperative cancellation handle. Cloned handles share one flag; any holder
/// may cancel the run. Cancellation is checked by the supervisor loop — a run
/// already past its final wait may complete before noticing.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        CancelToken::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Run one supervised command to completion, timeout, crash, cancellation,
/// or supervisor failure.
pub fn run(cmd: &SupervisedCommand, cancel: &CancelToken) -> SupervisedOutcome {
    let started = Instant::now();
    if cancel.is_cancelled() {
        return SupervisedOutcome {
            status: ExecutionStatus::Cancelled,
            exit_code: None,
            signal: None,
            stdout: CapturedStream::empty(),
            stderr: CapturedStream::empty(),
            wall_time: started.elapsed(),
            error: None,
        };
    }

    let mut command = Command::new(&cmd.program);
    command
        .args(&cmd.args)
        .stdin(if matches!(cmd.stdin, StdinPolicy::Bytes(_)) {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = &cmd.cwd {
        command.current_dir(cwd);
    }
    if cmd.env.clear {
        command.env_clear();
    }
    for name in &cmd.env.remove {
        command.env_remove(name);
    }
    for (name, value) in &cmd.env.set {
        command.env(name, value);
    }

    let mut child = match command.group_spawn() {
        Ok(child) => child,
        Err(e) => {
            return SupervisedOutcome {
                status: ExecutionStatus::InfrastructureError,
                exit_code: None,
                signal: None,
                stdout: CapturedStream::empty(),
                stderr: CapturedStream::empty(),
                wall_time: started.elapsed(),
                error: Some(format!(
                    "failed to spawn `{}`: {e}",
                    cmd.program.to_string_lossy()
                )),
            };
        }
    };

    let mut stdout_pump = child
        .inner()
        .stdout
        .take()
        .map(|stream| spawn_pump(stream, cmd.capture_limit));
    let mut stderr_pump = child
        .inner()
        .stderr
        .take()
        .map(|stream| spawn_pump(stream, cmd.capture_limit));
    let mut stdin_writer = match &cmd.stdin {
        StdinPolicy::Bytes(data) => child
            .inner()
            .stdin
            .take()
            .map(|stdin| spawn_stdin_writer(stdin, data.clone())),
        StdinPolicy::Null => None,
    };

    // Collects every byte produced before death: pipes reach EOF once the
    // tree is gone, so pumps terminate and are joined here.
    let mut finish = |status: ExecutionStatus,
                      exit: Option<(Option<i32>, Option<i32>)>,
                      error: Option<String>|
     -> SupervisedOutcome {
        let (exit_code, signal) = exit.unwrap_or((None, None));
        let stdout = join_pump(stdout_pump.take());
        let stderr = join_pump(stderr_pump.take());
        if let Some(writer) = stdin_writer.take() {
            let _ = writer.join();
        }
        SupervisedOutcome {
            status,
            exit_code,
            signal,
            stdout,
            stderr,
            wall_time: started.elapsed(),
            error,
        }
    };

    let mut phase = Phase::Running;
    loop {
        if phase == Phase::Running {
            if cancel.is_cancelled() {
                phase = Phase::terminating(ExecutionStatus::Cancelled);
            } else if let Some(deadline) = cmd.deadline
                && started.elapsed() >= deadline
            {
                phase = Phase::terminating(ExecutionStatus::TimedOut);
            }
        }

        match child.try_wait() {
            Err(e) => {
                // Nearly unreachable (the handle was valid at spawn), but a
                // supervisor that cannot observe its child is broken: kill
                // the tree, reap it, and say so.
                let _ = child.kill();
                let _ = child.wait();
                return finish(
                    ExecutionStatus::InfrastructureError,
                    None,
                    Some(format!("failed to wait on supervised child: {e}")),
                );
            }
            Ok(Some(status)) => {
                // Once the budget is exceeded (or cancel requested), the
                // supervisor's verdict wins even if the child exited on its
                // own a moment earlier: it did not finish *within* its
                // ownership. Only genuinely unowned exits are classified
                // on their own terms.
                if let Phase::Terminating { reason, .. } = phase {
                    return finish(reason, None, None);
                }
                let (state, code, signal) = classify_exit(&status);
                return finish(state, Some((code, signal)), None);
            }
            Ok(None) => {}
        }

        if let Phase::Terminating {
            graceful_sent,
            force_sent,
            force_at,
            grace_end,
            ..
        } = &mut phase
        {
            if !*graceful_sent {
                *graceful_sent = true;
                if send_graceful(&child) {
                    *grace_end = Instant::now() + cmd.grace_period;
                } else {
                    // No graceful step available (or the tree is already
                    // gone): proceed directly to forced termination.
                    let _ = child.kill();
                    *force_sent = true;
                    *force_at = Some(Instant::now());
                }
            } else if !*force_sent && Instant::now() >= *grace_end {
                let _ = child.kill();
                *force_sent = true;
                *force_at = Some(Instant::now());
            }
            if let Some(at) = force_at
                && at.elapsed() >= KILL_FAILSAFE
            {
                // Practically unreachable (SIGKILL / job termination is
                // final), but the supervisor must never wait forever.
                // Pumps are deliberately not joined here: a tree that
                // survives forced termination may still hold pipes open.
                return SupervisedOutcome {
                    status: ExecutionStatus::InfrastructureError,
                    exit_code: None,
                    signal: None,
                    stdout: CapturedStream::empty(),
                    stderr: CapturedStream::empty(),
                    wall_time: started.elapsed(),
                    error: Some(
                        "supervised tree survived forced termination; giving up".to_string(),
                    ),
                };
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

/// Best-effort graceful termination. Returns false when no graceful step
/// exists on this platform (Windows) or the tree is already gone, in which
/// case the caller proceeds directly to forced termination.
#[cfg(unix)]
fn send_graceful(child: &GroupChild) -> bool {
    child.signal(Signal::SIGTERM).is_ok()
}

#[cfg(not(unix))]
fn send_graceful(_child: &GroupChild) -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Running,
    Terminating {
        reason: ExecutionStatus,
        graceful_sent: bool,
        force_sent: bool,
        force_at: Option<Instant>,
        /// Valid once `graceful_sent` is true and a graceful step was sent.
        grace_end: Instant,
    },
}

impl Phase {
    fn terminating(reason: ExecutionStatus) -> Self {
        debug_assert!(matches!(
            reason,
            ExecutionStatus::TimedOut | ExecutionStatus::Cancelled
        ));
        Phase::Terminating {
            reason,
            graceful_sent: false,
            force_sent: false,
            force_at: None,
            // Overwritten when the graceful step is actually sent; the
            // initial value is never read because `graceful_sent` gates it.
            grace_end: Instant::now(),
        }
    }
}

/// Classify a natural exit. Unix signal deaths are crashes; anything with an
/// exit code completed (even nonzero — a failing test is still a completed
/// run, not a supervisor event).
#[cfg(unix)]
fn classify_exit(status: &ExitStatus) -> (ExecutionStatus, Option<i32>, Option<i32>) {
    use std::os::unix::process::ExitStatusExt;
    match status.code() {
        Some(code) => (ExecutionStatus::Completed, Some(code), None),
        None => (ExecutionStatus::Crashed, None, status.signal()),
    }
}

#[cfg(not(unix))]
fn classify_exit(status: &ExitStatus) -> (ExecutionStatus, Option<i32>, Option<i32>) {
    match status.code() {
        Some(code) => (ExecutionStatus::Completed, Some(code), None),
        // Unreachable on Windows (every termination yields a code), but a
        // code-less exit is a crash by definition, not a completion.
        None => (ExecutionStatus::Crashed, None, None),
    }
}

fn spawn_pump<R: Read + Send + 'static>(mut reader: R, limit: u64) -> JoinHandle<CapturedStream> {
    thread::spawn(move || {
        let mut retained = Vec::new();
        let mut total: u64 = 0;
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    total += n as u64;
                    let room = limit.saturating_sub(retained.len() as u64) as usize;
                    if room > 0 {
                        retained.extend_from_slice(&buf[..n.min(room)]);
                    }
                }
                // Pipe closed or reset: capture ends. Bytes counted so far
                // stay honest; there is no silent loss beyond `truncated`.
                Err(_) => break,
            }
        }
        CapturedStream {
            bytes: retained,
            total_bytes: total,
        }
    })
}

fn spawn_stdin_writer<W: Write + Send + 'static>(mut writer: W, data: Vec<u8>) -> JoinHandle<()> {
    thread::spawn(move || {
        // A child that exits early closes the pipe; that is a normal end,
        // not an error worth reporting.
        let _ = writer.write_all(&data);
        let _ = writer.flush();
    })
}

fn join_pump(handle: Option<JoinHandle<CapturedStream>>) -> CapturedStream {
    handle
        .and_then(|h| h.join().ok())
        .unwrap_or_else(CapturedStream::empty)
}
