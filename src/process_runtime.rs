//! Stateful process handles backed by the M2 process-group supervisor.
use crate::{
    builtins::{StepError, StepOutcome, resolve_in_worktree},
    supervisor::ExecutionStatus,
};
use command_group::{CommandGroup, GroupChild};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    ffi::OsString,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const OUT_MAX: usize = 1_048_576;
const OUT_LIMIT_MAX: usize = 16_777_216;
const INPUT_MAX: usize = 1_048_576;
const POLL: Duration = Duration::from_millis(10);
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessRuntimeReport {
    pub handles: Vec<HandleReport>,
    pub cleanup: CleanupReport,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HandleReport {
    pub handle: String,
    pub generations: Vec<GenerationReport>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenerationReport {
    pub generation: u64,
    pub pid: Option<u32>,
    pub process_identity: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub started_unix_ms: u64,
    pub termination: Option<String>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub events: Vec<LifecycleEvent>,
    pub events_truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub phase: String,
    pub step_index: usize,
    pub event: String,
    pub at_unix_ms: u64,
    pub detail: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DescendantObservation {
    /// Handle whose generation this observation belongs to.
    pub handle: String,
    /// Generation number of the observed root.
    pub generation: u64,
    /// Owned root identity (`execution:handle:gN:pidP`).
    pub root: String,
    /// OS pid of the root where known.
    pub root_pid: Option<u32>,
    /// How the observation was made: `proc-pgrp-scan`,
    /// `windows-job-membership-not-enumerated`, or `root-pid-unknown`.
    pub method: String,
    /// Owned descendant pids seen alive inside the group/job boundary.
    /// Escaped descendants are UNSUPPORTED and never appear here.
    pub descendant_pids: Vec<u32>,
    /// Outcome at cleanup end: `root-terminated`, `root-survivor`,
    /// `root-exited`, or `pending` (observed, cleanup not yet finished).
    pub cleanup_outcome: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CleanupReport {
    pub attempted: bool,
    pub terminated: Vec<String>,
    pub forced: Vec<String>,
    pub survivors: Vec<String>,
    pub errors: Vec<String>,
    /// Owned-descendant observations, one per handle generation present at
    /// cleanup start. Defaulted so pre-1C receipts stay readable.
    #[serde(default)]
    pub descendants: Vec<DescendantObservation>,
}
#[derive(Clone)]
pub struct StartSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub cwd_label: String,
    pub env: Vec<(OsString, OsString)>,
    pub stdin: bool,
    pub output_limit: usize,
}
struct Buffer {
    bytes: VecDeque<u8>,
    total: u64,
    limit: usize,
}
impl Buffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: VecDeque::new(),
            total: 0,
            limit,
        }
    }
    fn push(&mut self, b: &[u8]) {
        self.total = self.total.saturating_add(b.len() as u64);
        for x in b {
            if self.bytes.len() == self.limit {
                self.bytes.pop_front();
            }
            self.bytes.push_back(*x)
        }
    }
    fn snapshot(&self, cursor: u64) -> (Vec<u8>, u64, bool, u64) {
        let oldest = self.total.saturating_sub(self.bytes.len() as u64);
        let from = cursor.max(oldest).min(self.total);
        let out = self
            .bytes
            .iter()
            .skip(from.saturating_sub(oldest) as usize)
            .copied()
            .collect::<Vec<_>>();
        let total = self.total.saturating_sub(cursor.min(self.total));
        (
            out,
            total,
            from > cursor || total > self.bytes.len() as u64,
            self.total,
        )
    }
    /// Search only bytes appended after `cursor` (a `total`-space offset).
    ///
    /// Returns the `total`-space offset just past the first match so the
    /// caller can resume after it, the current `total`, and whether bytes
    /// newer than `cursor` were already discarded from the bounded buffer.
    fn contains_since(&self, cursor: u64, needle: &[u8]) -> (Option<u64>, u64, bool) {
        let oldest = self.total.saturating_sub(self.bytes.len() as u64);
        let from = cursor.max(oldest).min(self.total);
        if needle.is_empty() {
            // An empty marker matches immediately, as before; resume at present.
            return (Some(self.total), self.total, from > cursor);
        }
        let window: Vec<u8> = self
            .bytes
            .iter()
            .skip(from.saturating_sub(oldest) as usize)
            .copied()
            .collect();
        let end = window
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|pos| {
                from.saturating_add(pos as u64)
                    .saturating_add(needle.len() as u64)
            });
        (end, self.total, from > cursor)
    }
    fn regex_since(&self, cursor: u64, r: &regex::bytes::Regex) -> (Option<u64>, u64, bool) {
        let oldest = self.total.saturating_sub(self.bytes.len() as u64);
        let from = cursor.max(oldest).min(self.total);
        let window: Vec<u8> = self
            .bytes
            .iter()
            .skip(from.saturating_sub(oldest) as usize)
            .copied()
            .collect();
        let end = r.find(&window).map(|m| from.saturating_add(m.end() as u64));
        (end, self.total, from > cursor)
    }
}
enum Input {
    Write(Vec<u8>),
}
struct Running {
    child: GroupChild,
    stdin: Option<SyncSender<Input>>,
    stdin_join: Option<JoinHandle<()>>,
    out: Arc<Mutex<Buffer>>,
    err: Arc<Mutex<Buffer>>,
    out_join: Option<JoinHandle<()>>,
    err_join: Option<JoinHandle<()>>,
    out_cursor: u64,
    err_cursor: u64,
    /// Generation-relative readiness observation positions, one per stream.
    /// Each successful `wait_ready` advances its stream cursor past the match,
    /// so a later wait only observes bytes appended afterwards. Fresh per
    /// generation because `spawn` constructs a new `Running`.
    ready_out: u64,
    ready_err: u64,
}
struct Generation {
    report: GenerationReport,
    running: Running,
}
struct Handle {
    start: StartSpec,
    generations: Vec<Generation>,
}
pub struct ProcessRegistry {
    execution: String,
    handles: BTreeMap<String, Handle>,
    cleanup: CleanupReport,
    cleaned: bool,
}
impl ProcessRegistry {
    pub fn new(execution: &str) -> Self {
        Self {
            execution: execution.into(),
            handles: BTreeMap::new(),
            cleanup: CleanupReport::default(),
            cleaned: false,
        }
    }
    pub fn start(
        &mut self,
        name: &str,
        spec: StartSpec,
        phase: &str,
        index: usize,
    ) -> Result<StepOutcome, StepError> {
        if name.trim().is_empty() {
            return Err(bad("process.start requires a non-empty `handle`"));
        }
        if self.handles.contains_key(name) {
            return Err(bad(format!(
                "PROCESS_HANDLE_EXISTS: `{name}` already exists; use process.restart after the current generation exits"
            )));
        }
        let g = self.spawn(name, 1, spec.clone(), phase, index)?;
        self.handles.insert(
            name.into(),
            Handle {
                start: spec,
                generations: vec![g],
            },
        );
        Ok(done(Duration::ZERO))
    }
    pub fn restart(
        &mut self,
        name: &str,
        phase: &str,
        index: usize,
    ) -> Result<StepOutcome, StepError> {
        let keys = self.handles.keys().cloned().collect::<Vec<_>>();
        let h = self
            .handles
            .get_mut(name)
            .ok_or_else(|| missing(name, &keys))?;
        let old = h.generations.last_mut().expect("handle has a generation");
        if refresh(old)? {
            return Err(bad(format!(
                "PROCESS_GENERATION_STILL_LIVE: `{name}` generation {} must exit before restart",
                old.report.generation
            )));
        }
        let n = old.report.generation + 1;
        let spec = h.start.clone();
        let g = self.spawn(name, n, spec, phase, index)?;
        self.handles.get_mut(name).unwrap().generations.push(g);
        Ok(done(Duration::ZERO))
    }
    fn spawn(
        &self,
        name: &str,
        n: u64,
        spec: StartSpec,
        phase: &str,
        index: usize,
    ) -> Result<Generation, StepError> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(if spec.stdin {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(c) = &spec.cwd {
            cmd.current_dir(c);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .group_spawn()
            .map_err(|e| StepError::Io(format!("PROCESS_START_FAILED: {e}")))?;
        let pid = child.id();
        let out = Arc::new(Mutex::new(Buffer::new(spec.output_limit)));
        let err = Arc::new(Mutex::new(Buffer::new(spec.output_limit)));
        let out_join = child.inner().stdout.take().map(|p| pump(p, out.clone()));
        let err_join = child.inner().stderr.take().map(|p| pump(p, err.clone()));
        let (stdin, stdin_join) = if spec.stdin {
            if let Some(pipe) = child.inner().stdin.take() {
                let (tx, rx) = mpsc::sync_channel(8);
                (Some(tx), Some(thread::spawn(move || input_loop(pipe, rx))))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        let mut report = GenerationReport {
            generation: n,
            pid: Some(pid),
            process_identity: format!("{}:{name}:g{n}:pid{pid}", self.execution),
            program: spec.program.to_string_lossy().into(),
            args: spec
                .args
                .iter()
                .map(|a| redact(&a.to_string_lossy()))
                .collect(),
            cwd: spec.cwd_label,
            started_unix_ms: now(),
            termination: None,
            exit_code: None,
            signal: None,
            events: Vec::new(),
            events_truncated: false,
        };
        event(
            &mut report,
            phase,
            index,
            if n == 1 { "start" } else { "restart" },
            None,
        );
        Ok(Generation {
            report,
            running: Running {
                child,
                stdin,
                stdin_join,
                out,
                err,
                out_join,
                err_join,
                out_cursor: 0,
                err_cursor: 0,
                ready_out: 0,
                ready_err: 0,
            },
        })
    }
    pub fn write(
        &mut self,
        name: &str,
        data: Vec<u8>,
        newline: bool,
        close: bool,
        phase: &str,
        index: usize,
    ) -> Result<StepOutcome, StepError> {
        if data.len() > INPUT_MAX {
            return Err(bad(format!("stdin payload exceeds {INPUT_MAX} bytes")));
        }
        let g = self.active(name)?;
        if !refresh(g)? {
            return Err(ioerr(format!(
                "PROCESS_ALREADY_EXITED: `{name}` has exited"
            )));
        }
        let tx = g.running.stdin.as_ref().ok_or_else(|| {
            StepError::Unsupported("stdin is not piped; start with stdin: piped".into())
        })?;
        let mut bytes = data;
        let byte_count = bytes.len();
        if newline {
            bytes.push(b'\n')
        }
        if !bytes.is_empty() {
            tx.try_send(Input::Write(bytes)).map_err(|e| match e {
                TrySendError::Full(_) => ioerr("PROCESS_STDIN_BACKPRESSURE: writer queue is full"),
                TrySendError::Disconnected(_) => {
                    ioerr("PROCESS_STDIN_CLOSED: child no longer accepts input")
                }
            })?
        }
        if close {
            g.running.stdin.take();
            event(&mut g.report, phase, index, "stdin_closed", None)
        } else {
            event(
                &mut g.report,
                phase,
                index,
                "stdin_write",
                Some(format!("{} bytes", byte_count)),
            )
        }
        Ok(done(Duration::ZERO))
    }
    pub fn observe(
        &mut self,
        name: &str,
        phase: &str,
        index: usize,
    ) -> Result<StepOutcome, StepError> {
        let g = self.active(name)?;
        refresh(g)?;
        let (o, ot, _, oc) = g
            .running
            .out
            .lock()
            .map_err(|_| ioerr("stdout lock poisoned"))?
            .snapshot(g.running.out_cursor);
        let (e, et, _, ec) = g
            .running
            .err
            .lock()
            .map_err(|_| ioerr("stderr lock poisoned"))?
            .snapshot(g.running.err_cursor);
        g.running.out_cursor = oc;
        g.running.err_cursor = ec;
        event(
            &mut g.report,
            phase,
            index,
            "observe",
            Some(format!("stdout {ot} bytes; stderr {et} bytes")),
        );
        Ok(StepOutcome {
            status: ExecutionStatus::Completed,
            exit_code: g.report.exit_code,
            signal: g.report.signal,
            stdout: o,
            stdout_total: ot,
            stderr: e,
            stderr_total: et,
            wall_time: Duration::ZERO,
            error: None,
        })
    }
    pub fn ready(
        &mut self,
        name: &str,
        cond: Readiness,
        timeout: Duration,
        deadline: Option<Duration>,
        phase: &str,
        index: usize,
    ) -> Result<StepOutcome, StepError> {
        let bound = deadline.map_or(timeout, |d| d.min(timeout));
        let start = Instant::now();
        let re = cond
            .regex
            .as_deref()
            .map(regex::bytes::Regex::new)
            .transpose()
            .map_err(|e| bad(format!("invalid readiness regex: {e}")))?;
        let g = self.active(name)?;
        // Generation-relative observation: this wait only observes stream bytes
        // appended after the previous successful readiness observation on the same
        // stream, so a stale marker cannot satisfy a new request. Every generation
        // starts with fresh cursors because `spawn` constructs a new `Running`.
        let mut truncated_seen = false;
        loop {
            if !refresh(g)? {
                return Err(ioerr(format!(
                    "PROCESS_EXITED_BEFORE_READY: `{name}` generation {} exited",
                    g.report.generation
                )));
            }
            let ready = if let Some(s) = &cond.contains {
                let cursor = if cond.stderr {
                    g.running.ready_err
                } else {
                    g.running.ready_out
                };
                let b = if cond.stderr {
                    &g.running.err
                } else {
                    &g.running.out
                };
                let (found, truncated) = {
                    let guard = b
                        .lock()
                        .map_err(|_| ioerr("readiness capture lock poisoned"))?;
                    let (found, _, truncated) = guard.contains_since(cursor, s.as_bytes());
                    (found, truncated)
                };
                truncated_seen |= truncated;
                if let Some(end) = found {
                    if cond.stderr {
                        g.running.ready_err = end;
                    } else {
                        g.running.ready_out = end;
                    }
                    true
                } else {
                    false
                }
            } else if let Some(r) = &re {
                let cursor = if cond.stderr {
                    g.running.ready_err
                } else {
                    g.running.ready_out
                };
                let b = if cond.stderr {
                    &g.running.err
                } else {
                    &g.running.out
                };
                let (found, truncated) = {
                    let guard = b
                        .lock()
                        .map_err(|_| ioerr("readiness capture lock poisoned"))?;
                    let (found, _, truncated) = guard.regex_since(cursor, r);
                    (found, truncated)
                };
                truncated_seen |= truncated;
                if let Some(end) = found {
                    if cond.stderr {
                        g.running.ready_err = end;
                    } else {
                        g.running.ready_out = end;
                    }
                    true
                } else {
                    false
                }
            } else if let Some(addr) = cond.tcp {
                TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok()
            } else {
                start.elapsed() >= Duration::from_millis(cond.alive_ms.unwrap_or(0))
            };
            if ready {
                event(&mut g.report, phase, index, "ready", Some(cond.description));
                return Ok(done(start.elapsed()));
            }
            if start.elapsed() >= bound {
                event(
                    &mut g.report,
                    phase,
                    index,
                    "readiness_timeout",
                    Some(cond.description),
                );
                return Ok(StepOutcome {
                    status: ExecutionStatus::TimedOut,
                    exit_code: None,
                    signal: None,
                    stdout: Vec::new(),
                    stdout_total: 0,
                    stderr: Vec::new(),
                    stderr_total: 0,
                    wall_time: start.elapsed(),
                    error: Some(if truncated_seen {
                        format!(
                            "PROCESS_READINESS_TIMEOUT: `{name}` was not ready within {} ms (output buffer truncated during this wait; the marker may have appeared in discarded bytes, so absence is not established)",
                            bound.as_millis()
                        )
                    } else {
                        format!(
                            "PROCESS_READINESS_TIMEOUT: `{name}` was not ready within {} ms",
                            bound.as_millis()
                        )
                    }),
                });
            }
            thread::sleep(POLL.min(bound.saturating_sub(start.elapsed())));
        }
    }
    pub fn terminate(
        &mut self,
        name: &str,
        phase: &str,
        index: usize,
    ) -> Result<StepOutcome, StepError> {
        let g = self.active(name)?;
        if !refresh(g)? {
            return Ok(done(Duration::ZERO));
        }
        #[cfg(unix)]
        {
            use command_group::{Signal, UnixChildExt};
            g.running
                .child
                .signal(Signal::SIGTERM)
                .map_err(|e| ioerr(format!("PROCESS_TERMINATE_FAILED: {e}")))?;
            event(
                &mut g.report,
                phase,
                index,
                "terminate_requested",
                Some("SIGTERM to owned process group".into()),
            );
            g.report.termination = Some("graceful_requested".into());
            Ok(done(Duration::ZERO))
        }
        #[cfg(not(unix))]
        {
            let _ = (phase, index);
            Err(StepError::Unsupported(
                "process.terminate unsupported on Windows; use process.kill".into(),
            ))
        }
    }
    pub fn kill(
        &mut self,
        name: &str,
        timeout: Duration,
        phase: &str,
        index: usize,
    ) -> Result<StepOutcome, StepError> {
        let g = self.active(name)?;
        let start = Instant::now();
        if refresh(g)? {
            g.running
                .child
                .kill()
                .map_err(|e| ioerr(format!("PROCESS_KILL_FAILED: {e}")))?;
            g.report.termination = Some("forced_kill_requested".into());
            event(&mut g.report, phase, index, "kill_requested", None)
        }
        loop {
            if !refresh(g)? {
                g.report.termination = Some("killed".into());
                return Ok(done(start.elapsed()));
            }
            if start.elapsed() >= timeout {
                g.report.termination = Some("survived_kill_deadline".into());
                event(&mut g.report, phase, index, "survivor_observed", None);
                return Err(ioerr(format!(
                    "PROCESS_TREE_SURVIVED: `{name}` remained alive after forced kill"
                )));
            }
            thread::sleep(POLL)
        }
    }
    pub fn wait(
        &mut self,
        name: &str,
        timeout: Duration,
        phase: &str,
        index: usize,
    ) -> Result<StepOutcome, StepError> {
        let start = Instant::now();
        let g = self.active(name)?;
        loop {
            if !refresh(g)? {
                event(&mut g.report, phase, index, "wait_exited", None);
                return Ok(StepOutcome {
                    status: ExecutionStatus::Completed,
                    exit_code: g.report.exit_code,
                    signal: g.report.signal,
                    stdout: Vec::new(),
                    stdout_total: 0,
                    stderr: Vec::new(),
                    stderr_total: 0,
                    wall_time: start.elapsed(),
                    error: None,
                });
            }
            if start.elapsed() >= timeout {
                event(&mut g.report, phase, index, "wait_timeout", None);
                return Ok(StepOutcome {
                    status: ExecutionStatus::TimedOut,
                    exit_code: None,
                    signal: None,
                    stdout: Vec::new(),
                    stdout_total: 0,
                    stderr: Vec::new(),
                    stderr_total: 0,
                    wall_time: start.elapsed(),
                    error: Some(format!("PROCESS_WAIT_TIMEOUT: `{name}` remains alive")),
                });
            }
            thread::sleep(POLL)
        }
    }
    fn active(&mut self, name: &str) -> Result<&mut Generation, StepError> {
        let keys = self.handles.keys().cloned().collect::<Vec<_>>();
        self.handles
            .get_mut(name)
            .ok_or_else(|| missing(name, &keys))?
            .generations
            .last_mut()
            .ok_or_else(|| bad("handle has no generation"))
    }
    pub fn cleanup(&mut self) -> &CleanupReport {
        if self.cleaned {
            return &self.cleanup;
        }
        self.cleaned = true;
        self.cleanup.attempted = true;
        // Owned-descendant observation happens BEFORE any termination signal:
        // afterwards the group may already be gone. Unix scans /proc for the
        // owned process-group id; other platforms record the root honestly.
        let mut seen = Vec::new();
        for (name, h) in &self.handles {
            if let Some(g) = h.generations.last() {
                let (method, pids) = observe_owned_descendants(g.report.pid);
                seen.push(DescendantObservation {
                    handle: name.clone(),
                    generation: g.report.generation,
                    root: g.report.process_identity.clone(),
                    root_pid: g.report.pid,
                    method: method.into(),
                    descendant_pids: pids,
                    cleanup_outcome: "pending".into(),
                });
            }
        }
        self.cleanup.descendants.extend(seen);
        let end = Instant::now() + Duration::from_secs(4);
        for (name, h) in &mut self.handles {
            if let Some(g) = h.generations.last_mut()
                && refresh(g).unwrap_or(true)
            {
                #[cfg(unix)]
                {
                    use command_group::{Signal, UnixChildExt};
                    if g.running.child.signal(Signal::SIGTERM).is_ok() {
                        g.report.termination = Some("cleanup_graceful_requested".into());
                        event(&mut g.report, "cleanup", 0, "terminate", None)
                    }
                }
            }
            let _ = name;
        }
        let grace = (Instant::now() + Duration::from_millis(500)).min(end);
        while Instant::now() < grace {
            if self.handles.values_mut().all(|h| {
                h.generations
                    .last_mut()
                    .is_none_or(|g| !refresh(g).unwrap_or(true))
            }) {
                break;
            }
            thread::sleep(POLL)
        }
        for (name, h) in &mut self.handles {
            if let Some(g) = h.generations.last_mut() {
                if refresh(g).unwrap_or(true) {
                    if g.running.child.kill().is_ok() {
                        self.cleanup.forced.push(g.report.process_identity.clone());
                        g.report.termination = Some("cleanup_forced_kill_requested".into());
                        event(&mut g.report, "cleanup", 0, "force_kill", None)
                    } else {
                        self.cleanup
                            .errors
                            .push(format!("{}: kill failed", g.report.process_identity))
                    }
                }
                let _ = name;
            }
        }
        while Instant::now() < end {
            if self.handles.values_mut().all(|h| {
                h.generations
                    .last_mut()
                    .is_none_or(|g| !refresh(g).unwrap_or(true))
            }) {
                break;
            }
            thread::sleep(POLL)
        }
        for (name, h) in &mut self.handles {
            if let Some(g) = h.generations.last_mut() {
                if refresh(g).unwrap_or(true) {
                    self.cleanup
                        .survivors
                        .push(g.report.process_identity.clone());
                    event(&mut g.report, "cleanup", 0, "survivor", Some(name.clone()))
                } else if g
                    .report
                    .termination
                    .as_deref()
                    .is_some_and(|s| s.starts_with("cleanup_"))
                {
                    g.report.termination = Some("cleanup_terminated".into());
                    self.cleanup
                        .terminated
                        .push(g.report.process_identity.clone())
                }
            }
        }
        for obs in &mut self.cleanup.descendants {
            if obs.cleanup_outcome != "pending" {
                continue;
            }
            obs.cleanup_outcome = if self.cleanup.survivors.iter().any(|s| s == &obs.root) {
                "root-survivor"
            } else if self.cleanup.terminated.iter().any(|t| t == &obs.root) {
                "root-terminated"
            } else {
                "root-exited"
            }
            .into();
        }
        &self.cleanup
    }
    pub fn report(&self) -> ProcessRuntimeReport {
        ProcessRuntimeReport {
            handles: self
                .handles
                .iter()
                .map(|(name, h)| HandleReport {
                    handle: name.clone(),
                    generations: h.generations.iter().map(|g| g.report.clone()).collect(),
                })
                .collect(),
            cleanup: self.cleanup.clone(),
        }
    }
}
impl Drop for ProcessRegistry {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.cleanup();
        }
    }
}
#[derive(Debug)]
pub struct Readiness {
    pub contains: Option<String>,
    pub regex: Option<String>,
    pub stderr: bool,
    pub alive_ms: Option<u64>,
    pub tcp: Option<SocketAddr>,
    pub description: String,
}
pub fn start_spec(p: &BTreeMap<String, Value>, root: &Path) -> Result<StartSpec, StepError> {
    let program = p
        .get("program")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("process.start requires string `program`"))?;
    let args = parse_strings(p.get("args"), "args")?
        .into_iter()
        .map(OsString::from)
        .collect();
    let cwd_label = p
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .to_string();
    let cwd = Some(if cwd_label == "." {
        root.to_path_buf()
    } else {
        resolve_in_worktree(root, &cwd_label).map_err(|e| StepError::Policy(e.to_string()))?
    });
    let env = match p.get("env") {
        None => Vec::new(),
        Some(value) => {
            let map = value
                .as_object()
                .ok_or_else(|| bad("`env` must be a mapping"))?;
            map.iter()
                .map(|(key, value)| {
                    crate::builtins::resolve_env_value(key, value).map(|v| (OsString::from(key), v))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    let stdin = match p.get("stdin").and_then(Value::as_str).unwrap_or("piped") {
        "piped" => true,
        "closed" | "null" => false,
        _ => return Err(bad("stdin must be `piped`, `closed`, or `null`")),
    };
    let limit = p
        .get("output_limit_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(OUT_MAX as u64) as usize;
    if limit == 0 || limit > OUT_LIMIT_MAX {
        return Err(bad(format!(
            "output_limit_bytes must be 1..={OUT_LIMIT_MAX}"
        )));
    }
    Ok(StartSpec {
        program: program.into(),
        args,
        cwd,
        cwd_label,
        env,
        stdin,
        output_limit: limit,
    })
}
pub fn millis(
    p: &BTreeMap<String, Value>,
    key: &str,
    required: bool,
) -> Result<Duration, StepError> {
    match p.get(key).and_then(Value::as_u64) {
        Some(n) if n <= 300_000 => Ok(Duration::from_millis(n)),
        Some(_) => Err(bad(format!("`{key}` must be <= 300000 milliseconds"))),
        None if required => Err(bad(format!("`{key}` must be an integer in milliseconds"))),
        None => Ok(Duration::from_secs(5)),
    }
}
pub fn parse_readiness(p: &BTreeMap<String, Value>) -> Result<Readiness, StepError> {
    let oc = p
        .get("stdout_contains")
        .and_then(Value::as_str)
        .map(str::to_string);
    let ec = p
        .get("stderr_contains")
        .and_then(Value::as_str)
        .map(str::to_string);
    let om = p
        .get("stdout_matches")
        .and_then(Value::as_str)
        .map(str::to_string);
    let em = p
        .get("stderr_matches")
        .and_then(Value::as_str)
        .map(str::to_string);
    let alive = p.get("alive_for_ms").and_then(Value::as_u64);
    let tcp = p
        .get("tcp")
        .map(|v| {
            let m = v
                .as_object()
                .ok_or_else(|| bad("tcp readiness must be a mapping"))?;
            let host = m
                .get("host")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("tcp.host must be an IP literal"))?;
            let port = m
                .get("port")
                .and_then(Value::as_u64)
                .filter(|x| *x > 0 && *x <= 65535)
                .ok_or_else(|| bad("tcp.port must be 1..65535"))?;
            let ip = host
                .parse::<IpAddr>()
                .map_err(|_| bad("tcp.host must be an IP literal; DNS is not used"))?;
            Ok(SocketAddr::new(ip, port as u16))
        })
        .transpose()?;
    let n = [
        oc.is_some(),
        ec.is_some(),
        om.is_some(),
        em.is_some(),
        alive.is_some(),
        tcp.is_some(),
    ]
    .into_iter()
    .filter(|x| *x)
    .count();
    if n != 1 {
        return Err(bad("wait_ready requires exactly one readiness condition"));
    }
    let stderr = ec.is_some() || em.is_some();
    let contains = oc.or(ec);
    let regex = om.or(em);
    let description = if let Some(s) = &contains {
        format!(
            "{} contains {s:?}",
            if stderr { "stderr" } else { "stdout" }
        )
    } else if let Some(s) = &regex {
        format!("{} matches {s:?}", if stderr { "stderr" } else { "stdout" })
    } else if let Some(ms) = alive {
        format!("alive for {ms} ms")
    } else {
        format!("TCP {} open", tcp.unwrap())
    };
    Ok(Readiness {
        contains,
        regex,
        stderr,
        alive_ms: alive,
        tcp,
        description,
    })
}
pub fn write_data(p: &BTreeMap<String, Value>) -> Result<(Vec<u8>, bool, bool), StepError> {
    let text = p.get("text").and_then(Value::as_str);
    let raw = p.get("bytes").and_then(Value::as_array);
    let bytes = match (text, raw) {
        (Some(s), None) => s.as_bytes().to_vec(),
        (None, Some(a)) => a
            .iter()
            .map(|v| {
                v.as_u64()
                    .filter(|n| *n <= 255)
                    .map(|n| n as u8)
                    .ok_or_else(|| bad("bytes entries must be integers 0..255"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        (None, None) => return Err(bad("write_stdin requires `text` or `bytes`")),
        _ => return Err(bad("write_stdin accepts text or bytes, not both")),
    };
    Ok((
        bytes,
        p.get("newline").and_then(Value::as_bool).unwrap_or(false),
        p.get("close").and_then(Value::as_bool).unwrap_or(false),
    ))
}
fn parse_strings(value: Option<&Value>, key: &str) -> Result<Vec<String>, StepError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| bad(format!("`{key}` must be a list of strings")))?;
    values
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| bad(format!("`{key}` entries must be strings")))
        })
        .collect()
}
/// Observe owned descendants of a supervised root process.
///
/// On Unix the supervisor starts each root as a process-group leader, so live
/// group members found via /proc (excluding the root itself) are owned
/// descendants. Elsewhere â€” notably Windows Job objects, which offer this crate
/// no stable enumeration API â€” only the root is reported, honestly labelled.
/// Escaped descendants outside the group/job boundary are UNSUPPORTED everywhere.
fn observe_owned_descendants(root_pid: Option<u32>) -> (&'static str, Vec<u32>) {
    match root_pid {
        None => ("root-pid-unknown", Vec::new()),
        Some(pid) => {
            #[cfg(unix)]
            {
                ("proc-pgrp-scan", owned_group_members(pid))
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                ("windows-job-membership-not-enumerated", Vec::new())
            }
        }
    }
}

/// Best-effort /proc scan for live members of the owned process group.
/// Reads only numeric /proc entries; silently skips vanished processes.
#[cfg(unix)]
fn owned_group_members(pgid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in dir.flatten() {
        let pid: u32 = match entry.file_name().to_str().and_then(|s| s.parse().ok()) {
            Some(pid) => pid,
            None => continue,
        };
        if pid == pgid {
            continue;
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        // `comm` may contain spaces or parens: fields start after the last `)`.
        // Layout there is ` state ppid pgrp ...`, so pgrp is the third field.
        let Some((_, after)) = stat.rsplit_once(')') else {
            continue;
        };
        let mut fields = after.split_whitespace();
        let _state = fields.next();
        let _ppid = fields.next();
        if fields
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .is_some_and(|pgrp| pgrp == pgid)
        {
            out.push(pid);
        }
    }
    out.sort_unstable();
    out
}

fn refresh(g: &mut Generation) -> Result<bool, StepError> {
    if g.report
        .termination
        .as_deref()
        .is_some_and(|s| ["exited", "killed", "cleanup_terminated"].contains(&s))
    {
        return Ok(false);
    }
    match g.running.child.try_wait() {
        Ok(Some(status)) => {
            g.report.exit_code = status.code();
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                g.report.signal = status.signal();
            }
            if g.report.termination.is_none()
                || g.report.termination.as_deref() == Some("graceful_requested")
            {
                g.report.termination = Some("exited".into());
            }
            g.running.stdin.take();
            if let Some(join) = g.running.stdin_join.take() {
                let _ = join.join();
            }
            if let Some(join) = g.running.out_join.take() {
                let _ = join.join();
            }
            if let Some(join) = g.running.err_join.take() {
                let _ = join.join();
            }
            Ok(false)
        }
        Ok(None) => Ok(true),
        Err(error) => Err(ioerr(format!("PROCESS_WAIT_FAILED: {error}"))),
    }
}
fn pump<R: Read + Send + 'static>(mut r: R, b: Arc<Mutex<Buffer>>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut bytes = [0u8; 8192];
        loop {
            match r.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut out) = b.lock() {
                        out.push(&bytes[..n])
                    }
                }
            }
        }
    })
}
fn input_loop(mut w: std::process::ChildStdin, rx: mpsc::Receiver<Input>) {
    while let Ok(Input::Write(b)) = rx.recv() {
        if w.write_all(&b).and_then(|_| w.flush()).is_err() {
            break;
        }
    }
}
fn event(g: &mut GenerationReport, phase: &str, index: usize, name: &str, detail: Option<String>) {
    if g.events.len() >= 2048 {
        g.events_truncated = true;
        return;
    }
    g.events.push(LifecycleEvent {
        phase: phase.into(),
        step_index: index,
        event: name.into(),
        at_unix_ms: now(),
        detail,
    })
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn redact(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    if [
        "token",
        "secret",
        "password",
        "passwd",
        "authorization",
        "api-key",
        "apikey",
        "private-key",
    ]
    .iter()
    .any(|n| lower.contains(n))
        && (s.starts_with('-') || s.contains('='))
    {
        "[REDACTED]".into()
    } else {
        s.into()
    }
}
fn missing(n: &str, keys: &[String]) -> StepError {
    bad(format!(
        "PROCESS_HANDLE_NOT_FOUND: no handle `{n}`. Available: {keys:?}. Start it with process.start first."
    ))
}
fn bad(s: impl Into<String>) -> StepError {
    StepError::Malformed(s.into())
}
fn ioerr(s: impl Into<String>) -> StepError {
    StepError::Io(s.into())
}
fn done(w: Duration) -> StepOutcome {
    StepOutcome {
        status: ExecutionStatus::Completed,
        exit_code: None,
        signal: None,
        stdout: vec![],
        stdout_total: 0,
        stderr: vec![],
        stderr_total: 0,
        wall_time: w,
        error: None,
    }
}
