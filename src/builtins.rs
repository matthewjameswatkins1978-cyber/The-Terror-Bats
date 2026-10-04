//! Built-in execution primitives (M3): `command`, `fs`, `git`.
//!
//! These are internal built-ins, not the M8 external adapter protocol.
//!
//! Honesty: built-in path validation is not a hostile-code sandbox. An
//! arbitrary child program spawned by `command.run` can still access host
//! paths; only Terror Bat's own built-in operations are confined.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::process_runtime::{self, ProcessRegistry};
use crate::supervisor::{
    CancelToken, ExecutionStatus, StdinPolicy, SupervisedCommand, SupervisedOutcome,
};

/// Why a built-in step could not be dispatched or execute. The runner maps
/// these to run statuses: `Policy` → PolicyDenied, `Malformed` → Invalid,
/// `Io` → InfrastructureError.
#[derive(Debug)]
pub enum StepError {
    /// A known-disallowed operation was refused (path escape, forbidden rev).
    Policy(String),
    /// The step payload is malformed for this adapter action.
    Malformed(String),
    /// Infrastructure failure (filesystem/OS error).
    Io(String),
    /// The requested operation is unavailable on this platform.
    Unsupported(String),
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepError::Policy(m) => write!(f, "{m}"),
            StepError::Malformed(m) => write!(f, "{m}"),
            StepError::Io(m) => write!(f, "{m}"),
            StepError::Unsupported(m) => write!(f, "{m}"),
        }
    }
}

pub type StepResult<T> = std::result::Result<T, StepError>;

/// Capability required by a built-in adapter action (declaration preflight).
pub fn required_capability(adapter: &str, action: &str) -> Option<&'static str> {
    match (adapter, action) {
        ("command", "run") => Some("process.spawn"),
        ("process", "start") => Some("process.start"),
        ("process", "wait_ready") => Some("process.readiness"),
        ("process", "write_stdin") => Some("process.stdin"),
        ("process", "observe") => Some("process.observe"),
        ("process", "terminate") => Some("process.terminate"),
        ("process", "kill") => Some("process.kill"),
        ("process", "wait") => Some("process.wait"),
        ("process", "restart") => Some("process.restart"),
        ("fs", "write") => Some("fs.write"),
        ("fs", "mkdir") => Some("fs.write"),
        ("fs", "remove") => Some("fs.write"),
        ("fs", "read") => Some("fs.read"),
        ("fs", "list") => Some("fs.read"),
        ("fs", "stat") => Some("fs.read"),
        ("fs", "digest") => Some("fs.read"),
        ("git", "status") => Some("git.inspect"),
        ("git", "diff") => Some("git.inspect"),
        ("git", "rev_parse") => Some("git.inspect"),
        ("git", "worktree.snapshot") => Some("git.inspect"),
        _ => None,
    }
}

/// Validate a worktree-relative path and resolve it inside the worktree.
///
/// Rejects absolute/drive paths, UNC paths, rooted paths, `..` traversal
/// escaping the worktree, and (via canonicalisation of the existing prefix)
/// junction/symlink escapes. Built-in validation only — see module docs.
pub fn resolve_in_worktree(worktree: &Path, rel: &str) -> StepResult<PathBuf> {
    let policy = |m: String| StepError::Policy(m);
    if rel.is_empty() {
        return Err(policy(
            "empty path is not a valid worktree-relative path".into(),
        ));
    }
    let p = Path::new(rel);
    if p.has_root() || p.is_absolute() || rel.starts_with("\\\\") || rel.starts_with("//") {
        return Err(policy(format!(
            "path `{rel}` is absolute, rooted, or UNC; worktree-relative paths only"
        )));
    }
    if rel.contains(':') {
        return Err(policy(format!(
            "path `{rel}` contains a drive or alternate-data-stream separator"
        )));
    }
    // Lexical normalisation with `..` escape rejection.
    let mut normalized = PathBuf::new();
    let mut depth: isize = 0;
    for component in p.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(policy(format!(
                        "path `{rel}` escapes the worktree via `..` traversal"
                    )));
                }
                normalized.pop();
            }
            Component::Normal(part) => {
                depth += 1;
                normalized.push(part);
            }
            other => {
                return Err(policy(format!(
                    "path `{rel}` contains an unsupported component ({other:?})"
                )));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(policy(format!(
            "path `{rel}` resolves to the worktree root; not an actionable target"
        )));
    }
    // Junction/symlink escape guard: canonicalise the deepest existing prefix
    // and require containment within the canonical worktree.
    let canonical_root = worktree.canonicalize().map_err(|e| {
        StepError::Io(format!(
            "cannot canonicalise worktree root `{}`: {e}",
            worktree.display()
        ))
    })?;
    let parts: Vec<_> = normalized.components().collect();
    let mut probe = canonical_root.clone();
    let mut existing_len = 0;
    for (i, part) in parts.iter().enumerate() {
        probe.push(part);
        if probe.exists() {
            existing_len = i + 1;
        } else {
            break;
        }
    }
    let mut existing = canonical_root.clone();
    for part in &parts[..existing_len] {
        existing.push(part);
    }
    if existing_len > 0 {
        let canonical_existing = existing.canonicalize().map_err(|e| {
            StepError::Io(format!(
                "cannot canonicalise existing path prefix `{}` (junction/symlink?): {e}",
                existing.display()
            ))
        })?;
        if !canonical_existing.starts_with(&canonical_root) {
            return Err(policy(format!(
                "path `{rel}` resolves through a junction/symlink to `{}` which is outside \
                 the worktree; refusing (built-in validation only, not a sandbox)",
                canonical_existing.display()
            )));
        }
        existing = canonical_existing;
    }
    let mut final_path = existing;
    for part in &parts[existing_len..] {
        final_path.push(part);
    }
    Ok(final_path)
}

/// Mechanical outcome of one built-in step.
#[derive(Debug, Clone)]
pub struct StepOutcome {
    pub status: ExecutionStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stdout_total: u64,
    pub stderr: Vec<u8>,
    pub stderr_total: u64,
    pub wall_time: Duration,
    pub error: Option<String>,
}

impl StepOutcome {
    fn from_supervised(out: SupervisedOutcome) -> StepOutcome {
        StepOutcome {
            status: out.status,
            exit_code: out.exit_code,
            signal: out.signal,
            stdout: out.stdout.bytes,
            stdout_total: out.stdout.total_bytes,
            stderr: out.stderr.bytes,
            stderr_total: out.stderr.total_bytes,
            wall_time: out.wall_time,
            error: out.error,
        }
    }
}

/// Execution context handed to built-in steps.
pub struct StepCtx<'a> {
    /// Disposable worktree root; all built-in relative paths resolve here.
    pub worktree: &'a Path,
    /// Directory of the Bat Spec file (for `from:` fixture reads).
    pub spec_dir: &'a Path,
    /// Remaining wall-clock budget for this step, if any.
    pub deadline: Option<Duration>,
    pub processes: Option<&'a std::cell::RefCell<ProcessRegistry>>,
    pub phase: &'a str,
    pub step_index: usize,
}

fn payload_string(payload: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_string)
}

fn malformed(msg: impl Into<String>) -> StepError {
    StepError::Malformed(msg.into())
}

/// Dispatch one built-in step. Unknown adapters/actions are malformed input
/// (the runner also rejects them earlier during capability preflight).
fn process_dispatch(
    action: &str,
    payload: &BTreeMap<String, Value>,
    ctx: &StepCtx<'_>,
) -> StepResult<StepOutcome> {
    let handle = payload_string(payload, "handle")
        .ok_or_else(|| malformed(format!("process.{action} requires string `handle`")))?;
    let mut registry = ctx
        .processes
        .ok_or_else(|| StepError::Io("process registry unavailable in this phase".into()))?
        .borrow_mut();
    match action {
        "start" => registry.start(
            &handle,
            process_runtime::start_spec(payload, ctx.worktree)?,
            ctx.phase,
            ctx.step_index,
        ),
        "wait_ready" => registry.ready(
            &handle,
            process_runtime::parse_readiness(payload)?,
            process_runtime::millis(payload, "timeout_ms", true)?,
            ctx.deadline,
            ctx.phase,
            ctx.step_index,
        ),
        "write_stdin" => {
            let (b, n, c) = process_runtime::write_data(payload)?;
            registry.write(&handle, b, n, c, ctx.phase, ctx.step_index)
        }
        "observe" => registry.observe(&handle, ctx.phase, ctx.step_index),
        "terminate" => registry.terminate(&handle, ctx.phase, ctx.step_index),
        "kill" => registry.kill(
            &handle,
            process_runtime::millis(payload, "timeout_ms", false)?,
            ctx.phase,
            ctx.step_index,
        ),
        "wait" => registry.wait(
            &handle,
            process_runtime::millis(payload, "timeout_ms", true)?,
            ctx.phase,
            ctx.step_index,
        ),
        "restart" => registry.restart(&handle, ctx.phase, ctx.step_index),
        _ => Err(malformed(format!("unknown process action `{action}`"))),
    }
}

pub fn dispatch(
    adapter: &str,
    action: &str,
    payload: &BTreeMap<String, Value>,
    ctx: &StepCtx<'_>,
) -> StepResult<StepOutcome> {
    match (adapter, action) {
        ("command", "run") => command_run(payload, ctx),
        ("process", action) => process_dispatch(action, payload, ctx),
        ("fs", "write") => fs_write(payload, ctx),
        ("fs", "mkdir") => fs_mkdir(payload, ctx),
        ("fs", "read") => fs_read(payload, ctx),
        ("fs", "list") => fs_list(payload, ctx),
        ("fs", "stat") => fs_stat(payload, ctx),
        ("fs", "digest") => fs_digest(payload, ctx),
        ("fs", "remove") => fs_remove(payload, ctx),
        ("git", "status") => git_capture(ctx, &["status", "--porcelain"]),
        ("git", "diff") => git_capture(ctx, &["diff", "HEAD"]),
        ("git", "rev_parse") => git_rev_parse(payload, ctx),
        ("git", "worktree.snapshot") => git_snapshot(ctx),
        _ => Err(malformed(format!(
            "unknown built-in adapter action `{adapter}.{action}`"
        ))),
    }
}

/// `command.run`: direct process spawn through M2. No implicit shell: the
/// program is executed as given; a shell requires explicit intent
/// (`program: pwsh`, ...). A nonzero exit is Completed, not a step failure —
/// the oracle decides what it means.
fn command_run(payload: &BTreeMap<String, Value>, ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let program = payload_string(payload, "program")
        .ok_or_else(|| malformed("command.run requires a string `program` field"))?;
    let mut cmd = SupervisedCommand::new(OsString::from(&program));
    if let Some(args) = payload.get("args") {
        let list = args
            .as_array()
            .ok_or_else(|| malformed("command.run `args` must be a list of strings"))?;
        for arg in list {
            let s = arg
                .as_str()
                .ok_or_else(|| malformed("command.run `args` entries must be strings"))?;
            cmd.args.push(OsString::from(s));
        }
    }
    cmd.cwd = match payload_string(payload, "cwd") {
        Some(rel) => Some(resolve_in_worktree(ctx.worktree, &rel)?),
        None => Some(ctx.worktree.to_path_buf()),
    };
    if let Some(stdin) = payload_string(payload, "stdin") {
        cmd.stdin = StdinPolicy::Bytes(stdin.into_bytes());
    }
    if let Some(env) = payload.get("env") {
        let map = env
            .as_object()
            .ok_or_else(|| malformed("command.run `env` must be a mapping of strings"))?;
        for (k, v) in map {
            let value = v
                .as_str()
                .ok_or_else(|| malformed(format!("env value for `{k}` must be a string")))?;
            cmd.env.set.push((OsString::from(k), OsString::from(value)));
        }
    }
    cmd.deadline = ctx.deadline;
    let out = crate::supervisor::run(&cmd, &CancelToken::new());
    Ok(StepOutcome::from_supervised(out))
}

/// `fs.write`: exactly one of `text` / `from`. `from` resolves against the
/// Bat Spec directory (host-side read-only fixture source); `path` is
/// worktree-relative. Creates parent directories as needed.
fn fs_write(payload: &BTreeMap<String, Value>, ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let rel = payload_string(payload, "path")
        .ok_or_else(|| malformed("fs.write requires a string `path`"))?;
    let text = payload.get("text").and_then(Value::as_str);
    let from = payload.get("from").and_then(Value::as_str);
    let bytes = match (text, from) {
        (Some(t), None) => t.as_bytes().to_vec(),
        (None, Some(f)) => {
            let source = ctx.spec_dir.join(f);
            std::fs::read(&source).map_err(|e| {
                StepError::Io(format!(
                    "cannot read fixture `from: {f}` relative to the Bat Spec directory `{}`: {e}",
                    ctx.spec_dir.display()
                ))
            })?
        }
        (Some(_), Some(_)) => {
            return Err(malformed(
                "fs.write accepts exactly one of `text` or `from`, not both",
            ));
        }
        (None, None) => {
            return Err(malformed(
                "fs.write requires exactly one of `text` or `from`",
            ));
        }
    };
    let target = resolve_in_worktree(ctx.worktree, &rel)?;
    let start = std::time::Instant::now();
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            StepError::Io(format!(
                "cannot create parent directories for `{}`: {e}",
                parent.display()
            ))
        })?;
    }
    std::fs::write(&target, &bytes)
        .map_err(|e| StepError::Io(format!("cannot write `{}`: {e}", target.display())))?;
    Ok(StepOutcome {
        status: ExecutionStatus::Completed,
        exit_code: Some(0),
        signal: None,
        stdout: Vec::new(),
        stdout_total: 0,
        stderr: Vec::new(),
        stderr_total: 0,
        wall_time: start.elapsed(),
        error: None,
    })
}

/// `fs.mkdir`.
fn fs_mkdir(payload: &BTreeMap<String, Value>, ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let rel = payload_string(payload, "path")
        .ok_or_else(|| malformed("fs.mkdir requires a string `path`"))?;
    let target = resolve_in_worktree(ctx.worktree, &rel)?;
    let start = std::time::Instant::now();
    std::fs::create_dir_all(&target).map_err(|e| {
        StepError::Io(format!(
            "cannot create directory `{}`: {e}",
            target.display()
        ))
    })?;
    Ok(StepOutcome {
        status: ExecutionStatus::Completed,
        exit_code: Some(0),
        signal: None,
        stdout: Vec::new(),
        stdout_total: 0,
        stderr: Vec::new(),
        stderr_total: 0,
        wall_time: start.elapsed(),
        error: None,
    })
}

/// Retained-bytes cap for filesystem reads and digests: totals are always
/// honest, only retention is bounded (same 1 MiB policy as M2 captures).
const FS_RETAIN_LIMIT: usize = 1_048_576;

fn retained(bytes: Vec<u8>) -> (Vec<u8>, u64) {
    let total = bytes.len() as u64;
    if bytes.len() > FS_RETAIN_LIMIT {
        (bytes[..FS_RETAIN_LIMIT].to_vec(), total)
    } else {
        (bytes, total)
    }
}

fn ok_stdout(full: Vec<u8>, wall_time: std::time::Duration) -> StepOutcome {
    let (stdout, total) = retained(full);
    ok_stdout_retained(stdout, total, wall_time)
}

fn ok_stdout_retained(stdout: Vec<u8>, total: u64, wall_time: std::time::Duration) -> StepOutcome {
    StepOutcome {
        status: ExecutionStatus::Completed,
        exit_code: Some(0),
        signal: None,
        stdout,
        stdout_total: total,
        stderr: Vec::new(),
        stderr_total: 0,
        wall_time,
        error: None,
    }
}

/// `fs.read`: stream a worktree-relative file into step stdout, byte-preserving.
/// Large files keep honest totals with bounded retention. A missing file is an
/// infrastructure error; the oracle then yields Undetermined, never a finding.
fn read_bounded(path: &Path) -> StepResult<(Vec<u8>, u64)> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| StepError::Io(format!("cannot read `{}`: {e}", path.display())))?;
    let mut retained = Vec::with_capacity(FS_RETAIN_LIMIT);
    let mut total = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|e| StepError::Io(format!("cannot read `{}`: {e}", path.display())))?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        let keep = (FS_RETAIN_LIMIT - retained.len()).min(count);
        retained.extend_from_slice(&buffer[..keep]);
    }
    Ok((retained, total))
}

fn fs_read(payload: &BTreeMap<String, Value>, ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let rel = payload_string(payload, "path")
        .ok_or_else(|| malformed("fs.read requires a string `path`"))?;
    let target = resolve_in_worktree(ctx.worktree, &rel)?;
    let start = std::time::Instant::now();
    let (bytes, total) = read_bounded(&target)?;
    Ok(ok_stdout_retained(bytes, total, start.elapsed()))
}

/// `fs.list`: sorted directory listing on stdout, one entry per line —
/// directories suffixed with `/`, symlinks with `@` (never followed).
/// `.` lists the worktree root itself.
fn fs_list(payload: &BTreeMap<String, Value>, ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let rel = payload_string(payload, "path")
        .ok_or_else(|| malformed("fs.list requires a string `path`"))?;
    let target = if rel == "." || rel == "./" || rel.is_empty() {
        ctx.worktree.to_path_buf()
    } else {
        resolve_in_worktree(ctx.worktree, &rel)?
    };
    let start = std::time::Instant::now();
    let meta = std::fs::symlink_metadata(&target)
        .map_err(|e| StepError::Io(format!("cannot list `{rel}`: {e}")))?;
    if meta.file_type().is_symlink() {
        return Err(malformed("fs.list does not follow symlinks"));
    }
    if !meta.is_dir() {
        return Err(malformed(format!(
            "fs.list target `{rel}` is not a directory"
        )));
    }
    let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&target)
        .map_err(|e| StepError::Io(format!("cannot list `{rel}`: {e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StepError::Io(format!("cannot list `{rel}`: {e}")))?;
    let mut names: Vec<String> = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ft = entry
            .file_type()
            .map_err(|e| StepError::Io(format!("cannot stat `{rel}/{name}`: {e}")))?;
        if ft.is_dir() {
            names.push(format!("{name}/"));
        } else if ft.is_symlink() {
            names.push(format!("{name}@"));
        } else {
            names.push(name);
        }
    }
    names.sort();
    let text = if names.is_empty() {
        String::new()
    } else {
        names.join("\n") + "\n"
    };
    Ok(ok_stdout(text.into_bytes(), start.elapsed()))
}

/// `fs.stat`: JSON metadata on stdout (`path`, `kind`, `size`, `sha256`).
/// Missing files report `kind: "missing"` as data (Completed) — the adapter
/// reports reality, the oracle decides what it means.
fn fs_stat(payload: &BTreeMap<String, Value>, ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let rel = payload_string(payload, "path")
        .ok_or_else(|| malformed("fs.stat requires a string `path`"))?;
    let target = resolve_in_worktree(ctx.worktree, &rel)?;
    let start = std::time::Instant::now();
    let norm = rel.replace('\\', "/");
    let body = match std::fs::symlink_metadata(&target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            serde_json::json!({"path": norm, "kind": "missing", "size": null, "sha256": null})
        }
        Err(error) => {
            return Err(StepError::Io(format!(
                "cannot stat `{}`: {error}",
                target.display()
            )));
        }
        Ok(m) => {
            let ft = m.file_type();
            if ft.is_symlink() {
                let link = std::fs::read_link(&target).map_err(|e| {
                    StepError::Io(format!("cannot read link `{}`: {e}", target.display()))
                })?;
                serde_json::json!({"path": norm, "kind": "symlink", "size": null, "sha256": null, "target": link.to_string_lossy()})
            } else if ft.is_dir() {
                serde_json::json!({"path": norm, "kind": "dir", "size": null, "sha256": null})
            } else if ft.is_file() {
                let bytes = std::fs::read(&target).map_err(|e| {
                    StepError::Io(format!("cannot read `{}`: {e}", target.display()))
                })?;
                serde_json::json!({"path": norm, "kind": "file", "size": bytes.len(), "sha256": crate::evidence::sha256_hex(&bytes)})
            } else {
                serde_json::json!({"path": norm, "kind": "other", "size": null, "sha256": null})
            }
        }
    };
    let bytes = serde_json::to_vec(&body)
        .map_err(|e| StepError::Io(format!("cannot encode stat JSON: {e}")))?;
    Ok(ok_stdout(bytes, start.elapsed()))
}

/// `fs.digest`: canonical digest of a worktree-relative tree — sorted
/// `sha256  rel` lines for files, `dir  rel/` markers, `symlink  rel -> target`
/// records. `.` digests the whole worktree. The before/after snapshot primitive:
/// capture in setup, capture after run, compare with path_changed/text conditions.
fn fs_digest(payload: &BTreeMap<String, Value>, ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let rel = payload_string(payload, "path")
        .ok_or_else(|| malformed("fs.digest requires a string `path`"))?;
    let (target, base_rel) = if rel == "." || rel == "./" || rel.is_empty() {
        (ctx.worktree.to_path_buf(), String::new())
    } else {
        let t = resolve_in_worktree(ctx.worktree, &rel)?;
        (t, rel.replace('\\', "/"))
    };
    let start = std::time::Instant::now();
    let io = |what: &str, e: std::io::Error| StepError::Io(format!("cannot digest `{what}`: {e}"));
    let meta = std::fs::symlink_metadata(&target).map_err(|e| io(&rel, e))?;
    let mut lines: Vec<String> = Vec::new();
    if meta.file_type().is_symlink() {
        let link = std::fs::read_link(&target).map_err(|e| io(&rel, e))?;
        lines.push(format!("symlink  {base_rel} -> {}", link.to_string_lossy()));
    } else if meta.is_file() {
        let bytes = std::fs::read(&target).map_err(|e| io(&rel, e))?;
        lines.push(format!(
            "{}  {base_rel}",
            crate::evidence::sha256_hex(&bytes)
        ));
    } else if meta.is_dir() {
        let mut stack: Vec<(PathBuf, String)> = vec![(target, base_rel)];
        while let Some((dir, prefix)) = stack.pop() {
            let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&dir)
                .map_err(|e| io(&prefix, e))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| io(&prefix, e))?;
            let mut ordered = entries;
            ordered.sort_by_key(|e| e.file_name());
            for entry in ordered {
                if lines.len() > 200_000 {
                    return Err(StepError::Io(
                        "cannot digest: tree exceeds 200,000 entries".to_string(),
                    ));
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let entry_rel = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                let ft = entry.file_type().map_err(|e| io(&entry_rel, e))?;
                if ft.is_symlink() {
                    let link = std::fs::read_link(entry.path()).map_err(|e| io(&entry_rel, e))?;
                    lines.push(format!(
                        "symlink  {entry_rel} -> {}",
                        link.to_string_lossy()
                    ));
                } else if ft.is_dir() {
                    lines.push(format!("dir  {entry_rel}/"));
                    stack.push((entry.path(), entry_rel));
                } else if ft.is_file() {
                    let bytes = std::fs::read(entry.path()).map_err(|e| io(&entry_rel, e))?;
                    lines.push(format!(
                        "{}  {entry_rel}",
                        crate::evidence::sha256_hex(&bytes)
                    ));
                } else {
                    lines.push(format!("other  {entry_rel}"));
                }
            }
        }
    } else {
        lines.push(format!("other  {base_rel}"));
    }
    lines.sort();
    let text = if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    };
    Ok(ok_stdout(text.into_bytes(), start.elapsed()))
}

/// `fs.remove`: remove one worktree-relative file, symlink, or *empty*
/// directory. Never recursive, never the worktree root (root-resolving paths
/// are already refused by worktree confinement).
fn fs_remove(payload: &BTreeMap<String, Value>, ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let rel = payload_string(payload, "path")
        .ok_or_else(|| malformed("fs.remove requires a string `path`"))?;
    let target = resolve_in_worktree(ctx.worktree, &rel)?;
    let start = std::time::Instant::now();
    let meta = std::fs::symlink_metadata(&target)
        .map_err(|e| StepError::Io(format!("cannot remove `{rel}`: {e}")))?;
    if meta.file_type().is_symlink() || meta.is_file() {
        std::fs::remove_file(&target)
            .map_err(|e| StepError::Io(format!("cannot remove `{rel}`: {e}")))?;
    } else if meta.is_dir() {
        std::fs::remove_dir(&target).map_err(|e| {
            StepError::Io(format!(
                "cannot remove `{rel}` (only empty directories): {e}"
            ))
        })?;
    } else {
        return Err(StepError::Io(format!(
            "cannot remove `{rel}`: unsupported file kind"
        )));
    }
    Ok(StepOutcome {
        status: ExecutionStatus::Completed,
        exit_code: Some(0),
        signal: None,
        stdout: Vec::new(),
        stdout_total: 0,
        stderr: Vec::new(),
        stderr_total: 0,
        wall_time: start.elapsed(),
        error: None,
    })
}

fn git_capture(ctx: &StepCtx<'_>, args: &[&str]) -> StepResult<StepOutcome> {
    let mut cmd = SupervisedCommand::new(OsString::from("git"));
    cmd.args = args.iter().map(OsString::from).collect();
    cmd.cwd = Some(ctx.worktree.to_path_buf());
    cmd.deadline = ctx.deadline;
    let out = crate::supervisor::run(&cmd, &CancelToken::new());
    Ok(StepOutcome::from_supervised(out))
}

fn git_rev_parse(payload: &BTreeMap<String, Value>, ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let rev = payload_string(payload, "rev").unwrap_or_else(|| "HEAD".to_string());
    // Constrained local revision inspection: reject anything that looks like
    // a remote operation or option injection.
    if rev.starts_with('-') || rev.contains("..") || rev.contains(':') {
        return Err(StepError::Policy(format!(
            "git.rev_parse `rev` must be a plain local revision, got `{rev}`"
        )));
    }
    git_capture(ctx, &["rev-parse", &rev])
}

/// `git.worktree.snapshot`: HEAD + status in stable porcelain form, as two
/// separate supervised Git calls merged deterministically (no shell chaining).
fn git_snapshot(ctx: &StepCtx<'_>) -> StepResult<StepOutcome> {
    let head = git_capture(ctx, &["rev-parse", "HEAD"])?;
    let status = git_capture(ctx, &["status", "--porcelain"])?;
    let mut stdout = head.stdout.clone();
    if head.status == ExecutionStatus::Completed && head.exit_code == Some(0) {
        stdout.extend_from_slice(b"--- status --porcelain ---\n");
        stdout.extend_from_slice(&status.stdout);
    }
    let failed = head.status != ExecutionStatus::Completed
        || head.exit_code != Some(0)
        || status.status != ExecutionStatus::Completed
        || status.exit_code != Some(0);
    let stdout_total = stdout.len() as u64;
    Ok(StepOutcome {
        status: if failed {
            ExecutionStatus::InfrastructureError
        } else {
            ExecutionStatus::Completed
        },
        exit_code: if failed { None } else { Some(0) },
        signal: None,
        stdout,
        stdout_total,
        stderr: status.stderr.clone(),
        stderr_total: status.stderr_total,
        wall_time: head.wall_time + status.wall_time,
        error: if failed {
            head.error
                .clone()
                .or(status.error.clone())
                .or_else(|| Some("git.worktree.snapshot: underlying git command failed".into()))
        } else {
            None
        },
    })
}
