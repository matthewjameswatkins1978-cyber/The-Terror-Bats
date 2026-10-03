//! M8 external adapter process protocol. Adapters are supervised one-shot JSONL programs.
use crate::{
    evidence::sha256_hex,
    runner::RunStatus,
    spec::Capability,
    supervisor::{CancelToken, ExecutionStatus, StdinPolicy, SupervisedCommand, SupervisedOutcome},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

pub const PROTOCOL: &str = "terrorbat-adapter/v1";
pub const BINDINGS_VERSION: &str = "terrorbat-adapters/v1";
const OUTPUT_LIMIT: u64 = 1_048_576;
const DESCRIBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingsFile {
    pub version: String,
    pub adapters: BTreeMap<String, Binding>,
    #[serde(skip)]
    directories: BTreeMap<String, PathBuf>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterProvenance {
    pub protocol: String,
    pub name: String,
    pub version: String,
    pub description_id: String,
    pub program: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub describe_stderr: Option<crate::evidence::EvidenceRef>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDescription {
    pub requires: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Description {
    pub protocol: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    pub actions: BTreeMap<String, ActionDescription>,
}
#[derive(Serialize)]
struct DescribeRequest<'a> {
    protocol: &'a str,
    kind: &'a str,
}
#[derive(Serialize)]
struct ExecuteRequest<'a> {
    protocol: &'a str,
    kind: &'a str,
    request_id: &'a str,
    action: &'a str,
    payload: &'a BTreeMap<String, Value>,
    context: ExecuteContext<'a>,
}
#[derive(Serialize)]
struct ExecuteContext<'a> {
    worktree: &'a str,
    target_commit: &'a str,
    execution_id: &'a str,
    phase: &'a str,
    step_index: usize,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteResponse {
    protocol: String,
    kind: String,
    request_id: String,
    status: String,
    exit_code: i32,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedAdapter {
    pub provenance: AdapterProvenance,
    program: OsString,
    args: Vec<OsString>,
    pub description: Description,
    pub describe_stderr: Vec<u8>,
}
#[derive(Debug, Clone, Default)]
pub struct ResolvedAdapters {
    pub by_name: BTreeMap<String, ResolvedAdapter>,
}
#[derive(Debug)]
pub struct PreflightFailure {
    pub status: RunStatus,
    pub message: String,
}
#[derive(Debug)]
pub struct AdapterExecution {
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub protocol_stdout: Vec<u8>,
    pub protocol_stderr: Vec<u8>,
    pub protocol_stdout_total_bytes: u64,
    pub protocol_stdout_truncated: bool,
    pub protocol_stderr_total_bytes: u64,
    pub protocol_stderr_truncated: bool,
    pub wall_time: Duration,
    pub error: Option<String>,
    pub provenance: AdapterProvenance,
}

fn failure(status: RunStatus, message: impl Into<String>) -> PreflightFailure {
    PreflightFailure {
        status,
        message: message.into(),
    }
}
pub fn is_builtin(name: &str) -> bool {
    matches!(name, "command" | "fs" | "git")
}

pub fn load_bindings(path: Option<&Path>) -> Result<Option<BindingsFile>, String> {
    let Some(path) = path else { return Ok(None) };
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read adapter bindings `{}`: {e}", path.display()))?;
    let mut file: BindingsFile = serde_saphyr::from_str(&text)
        .map_err(|e| format!("invalid adapter bindings `{}`: {e}", path.display()))?;
    if file.version != BINDINGS_VERSION {
        return Err(format!(
            "adapter bindings version must be `{BINDINGS_VERSION}`"
        ));
    }
    for (name, binding) in &file.adapters {
        if name.trim().is_empty() || is_builtin(name) {
            return Err(format!("adapter name `{name}` is empty or reserved"));
        }
        if binding.program.trim().is_empty() {
            return Err(format!("adapter `{name}` has an empty program"));
        }
        let p = Path::new(&binding.program);
        if p.is_relative() && p.components().count() > 1 {
            file.directories.insert(
                name.clone(),
                path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            );
        }
    }
    Ok(Some(file))
}

fn command(
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<&Path>,
    input: Vec<u8>,
    deadline: Option<Duration>,
) -> SupervisedCommand {
    let mut cmd = SupervisedCommand::new(program);
    cmd.args = args;
    cmd.cwd = cwd.map(Path::to_path_buf);
    cmd.stdin = StdinPolicy::Bytes(input);
    cmd.deadline = deadline;
    cmd.capture_limit = OUTPUT_LIMIT;
    cmd.env.clear = true;
    // Minimal OS process-startup/runtime search environment; never forward credentials or arbitrary variables.
    for key in ["PATH", "SYSTEMROOT", "WINDIR", "TEMP", "TMP"] {
        if let Some(v) = std::env::var_os(key) {
            cmd.env.set.push((OsString::from(key), v));
        }
    }
    cmd
}
fn binding_program(file: &BindingsFile, name: &str, binding: &Binding) -> PathBuf {
    let p = Path::new(&binding.program);
    if p.is_relative() && p.components().count() > 1 {
        file.directories
            .get(name)
            .cloned()
            .unwrap_or_default()
            .join(p)
    } else {
        p.to_path_buf()
    }
}
fn json_line(bytes: &[u8]) -> Result<&str, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|e| format!("protocol stdout is not UTF-8: {e}"))?;
    let line = text.strip_suffix('\n').ok_or_else(|| {
        "protocol stdout must contain one newline-terminated JSON line".to_string()
    })?;
    let line = line.strip_suffix('\r').unwrap_or(line);
    if line.is_empty() || line.contains(['\r', '\n']) {
        return Err(
            "protocol stdout must contain exactly one JSON line and no extra output".into(),
        );
    }
    Ok(line)
}
fn request<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let mut b = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    b.push(b'\n');
    Ok(b)
}
fn process_failure(out: &SupervisedOutcome, stage: &str) -> Option<PreflightFailure> {
    if out.status != ExecutionStatus::Completed {
        let status = if stage == "describe" {
            RunStatus::InfrastructureError
        } else {
            out.status.into()
        };
        return Some(failure(
            status,
            out.error
                .clone()
                .unwrap_or_else(|| format!("adapter {stage} process ended as {:?}", out.status)),
        ));
    }
    if out.exit_code != Some(0) {
        return Some(failure(
            RunStatus::InfrastructureError,
            format!(
                "adapter {stage} process exited with OS status {:?}; protocol processes must exit 0",
                out.exit_code
            ),
        ));
    }
    if out.stdout.truncated() || out.stderr.truncated() {
        return Some(failure(
            RunStatus::InfrastructureError,
            format!("adapter {stage} output exceeded the {OUTPUT_LIMIT}-byte limit"),
        ));
    }
    None
}

pub fn resolve(
    file: Option<&BindingsFile>,
    steps: &[(&str, &str, &str)],
    requires: &[Capability],
    forbids: &[Capability],
) -> Result<ResolvedAdapters, PreflightFailure> {
    let mut resolved = ResolvedAdapters::default();
    let names: BTreeSet<String> = steps
        .iter()
        .map(|(_, name, _)| (*name).to_string())
        .filter(|n| !is_builtin(n))
        .collect();
    for name in names {
        let Some(binding_file) = file else {
            return Err(failure(
                RunStatus::Invalid,
                format!("external adapter `{name}` requires an explicit --adapters binding file"),
            ));
        };
        let Some(binding) = binding_file.adapters.get(&name) else {
            return Err(failure(
                RunStatus::Invalid,
                format!("external adapter `{name}` has no binding"),
            ));
        };
        let program = binding_program(binding_file, &name, binding);
        let args: Vec<OsString> = binding.args.iter().map(OsString::from).collect();
        let describe = request(&DescribeRequest {
            protocol: PROTOCOL,
            kind: "describe",
        })
        .map_err(|e| failure(RunStatus::InfrastructureError, e))?;
        let out = crate::supervisor::run(
            &command(
                program.as_os_str().to_owned(),
                args.clone(),
                None,
                describe,
                Some(DESCRIBE_TIMEOUT),
            ),
            &CancelToken::new(),
        );
        if let Some(e) = process_failure(&out, "describe") {
            return Err(e);
        }
        let line =
            json_line(&out.stdout.bytes).map_err(|e| failure(RunStatus::InfrastructureError, e))?;
        let desc: Description = serde_json::from_str(line).map_err(|e| {
            failure(
                RunStatus::InfrastructureError,
                format!("malformed adapter description: {e}"),
            )
        })?;
        if desc.protocol != PROTOCOL {
            return Err(failure(
                RunStatus::InfrastructureError,
                format!(
                    "adapter `{name}` reported unsupported protocol `{}`",
                    desc.protocol
                ),
            ));
        }
        if desc.kind != "description" || desc.name != name || desc.version.trim().is_empty() {
            return Err(failure(
                RunStatus::InfrastructureError,
                format!("adapter description kind/name/version is invalid for `{name}`"),
            ));
        }
        for (action, detail) in &desc.actions {
            if action.trim().is_empty()
                || detail
                    .requires
                    .iter()
                    .any(|c| c.trim().is_empty() || c.contains(':'))
            {
                return Err(failure(
                    RunStatus::InfrastructureError,
                    format!("adapter `{name}` advertises invalid action/capability names"),
                ));
            }
        }
        let value = serde_json::to_value(&desc)
            .map_err(|e| failure(RunStatus::InfrastructureError, e.to_string()))?;
        let identity = crate::canonical::canonical_json(&value)
            .map_err(|e| failure(RunStatus::InfrastructureError, e.to_string()))?;
        let provenance = AdapterProvenance {
            protocol: PROTOCOL.into(),
            name: name.clone(),
            version: desc.version.clone(),
            description_id: format!("adapter:sha256:{}", sha256_hex(&identity)),
            program: binding.program.clone(),
            args: binding.args.clone(),
            describe_stderr: None,
        };
        resolved.by_name.insert(
            name,
            ResolvedAdapter {
                provenance,
                program: program.into_os_string(),
                args,
                description: desc,
                describe_stderr: out.stderr.bytes,
            },
        );
    }
    let declared: BTreeSet<String> = requires
        .iter()
        .map(|c| c.0.split(':').next().unwrap_or(&c.0).trim().to_string())
        .collect();
    let forbidden: BTreeSet<String> = forbids
        .iter()
        .map(|c| c.0.split(':').next().unwrap_or(&c.0).trim().to_string())
        .collect();
    for (phase, name, action) in steps {
        if is_builtin(name) {
            continue;
        }
        let adapter = &resolved.by_name[*name];
        let Some(desc) = adapter.description.actions.get(*action) else {
            return Err(failure(
                RunStatus::Invalid,
                format!("{phase}: adapter `{name}` does not advertise action `{action}`"),
            ));
        };
        for cap in desc
            .requires
            .iter()
            .map(String::as_str)
            .chain(std::iter::once("process.spawn"))
        {
            if forbidden.contains(cap) {
                return Err(failure(
                    RunStatus::PolicyDenied,
                    format!("{phase} (`{name}.{action}`) requires forbidden capability `{cap}`"),
                ));
            }
            if !declared.contains(cap) {
                return Err(failure(
                    RunStatus::Invalid,
                    format!("{phase} (`{name}.{action}`) requires undeclared capability `{cap}`"),
                ));
            }
        }
    }
    Ok(resolved)
}

pub struct AdapterInvocation<'a> {
    pub adapter: &'a ResolvedAdapter,
    pub action: &'a str,
    pub payload: &'a BTreeMap<String, Value>,
    pub worktree: &'a Path,
    pub target_commit: &'a str,
    pub execution_id: &'a str,
    pub phase: &'a str,
    pub step_index: usize,
    pub deadline: Option<Duration>,
}

pub fn execute(invocation: AdapterInvocation<'_>) -> AdapterExecution {
    let AdapterInvocation {
        adapter,
        action,
        payload,
        worktree,
        target_commit,
        execution_id,
        phase,
        step_index,
        deadline,
    } = invocation;
    let request_id = uuid::Uuid::new_v4().to_string();
    let worktree_text = worktree.to_string_lossy();
    let input = match request(&ExecuteRequest {
        protocol: PROTOCOL,
        kind: "execute",
        request_id: &request_id,
        action,
        payload,
        context: ExecuteContext {
            worktree: &worktree_text,
            target_commit,
            execution_id,
            phase,
            step_index,
        },
    }) {
        Ok(v) => v,
        Err(e) => return failed(RunStatus::InfrastructureError, e, adapter, None),
    };
    let out = crate::supervisor::run(
        &command(
            adapter.program.clone(),
            adapter.args.clone(),
            Some(worktree),
            input,
            deadline,
        ),
        &CancelToken::new(),
    );
    if let Some(e) = process_failure(&out, "execute") {
        return failed(e.status, e.message, adapter, Some(out));
    }
    let response = match json_line(&out.stdout.bytes).and_then(|s| {
        serde_json::from_str::<ExecuteResponse>(s)
            .map_err(|e| format!("malformed adapter response JSON: {e}"))
    }) {
        Ok(r) => r,
        Err(e) => return failed(RunStatus::InfrastructureError, e, adapter, Some(out)),
    };
    if response.protocol != PROTOCOL
        || response.kind != "result"
        || response.request_id != request_id
    {
        return failed(
            RunStatus::InfrastructureError,
            "adapter response protocol, kind, or request id mismatch".into(),
            adapter,
            Some(out),
        );
    }
    let status = match response.status.as_str() {
        "completed" => RunStatus::Completed,
        "invalid" => RunStatus::Invalid,
        "policy_denied" => RunStatus::PolicyDenied,
        "infrastructure_error" => RunStatus::InfrastructureError,
        _ => {
            return failed(
                RunStatus::InfrastructureError,
                format!("unsupported adapter result status `{}`", response.status),
                adapter,
                Some(out),
            );
        }
    };
    AdapterExecution {
        status,
        exit_code: Some(response.exit_code),
        signal: None,
        stdout: response.stdout.into_bytes(),
        stderr: response.stderr.into_bytes(),
        protocol_stdout: Vec::new(),
        protocol_stdout_total_bytes: out.stdout.total_bytes,
        protocol_stdout_truncated: false,
        protocol_stderr_total_bytes: out.stderr.total_bytes,
        protocol_stderr_truncated: out.stderr.truncated(),
        protocol_stderr: out.stderr.bytes,
        wall_time: out.wall_time,
        error: (status != RunStatus::Completed).then(|| format!("adapter reported {status:?}")),
        provenance: adapter.provenance.clone(),
    }
}
fn failed(
    status: RunStatus,
    message: String,
    adapter: &ResolvedAdapter,
    out: Option<SupervisedOutcome>,
) -> AdapterExecution {
    let protocol_stdout_total_bytes = out.as_ref().map_or(0, |o| o.stdout.total_bytes);
    let protocol_stderr_total_bytes = out.as_ref().map_or(0, |o| o.stderr.total_bytes);
    let protocol_stdout_truncated = out.as_ref().is_some_and(|o| o.stdout.truncated());
    let protocol_stderr_truncated = out.as_ref().is_some_and(|o| o.stderr.truncated());
    let (process_status, exit_code, signal, stdout, stderr, wall_time) = match out {
        Some(o) => (
            o.status,
            o.exit_code,
            o.signal,
            o.stdout.bytes,
            o.stderr.bytes,
            o.wall_time,
        ),
        None => (
            ExecutionStatus::InfrastructureError,
            None,
            None,
            Vec::new(),
            Vec::new(),
            Duration::ZERO,
        ),
    };
    let mapped = if matches!(
        status,
        RunStatus::TimedOut | RunStatus::Cancelled | RunStatus::Crashed
    ) {
        RunStatus::from(process_status)
    } else {
        status
    };
    AdapterExecution {
        status: mapped,
        exit_code,
        signal,
        stdout: Vec::new(),
        stderr: Vec::new(),
        protocol_stdout: stdout,
        protocol_stderr: stderr,
        protocol_stdout_total_bytes,
        protocol_stdout_truncated,
        protocol_stderr_total_bytes,
        protocol_stderr_truncated,
        wall_time,
        error: Some(message),
        provenance: adapter.provenance.clone(),
    }
}
