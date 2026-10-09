//! Universal adapter capability discovery.
//!
//! Terror Bat's normal first question is: *"Can one of the universal
//! adapters already reach this thing?"* This module answers it without
//! folklore: every adapter reports its own operations, inputs, outputs,
//! constraints, examples, and platform limitations.
//!
//! Principle: adapters describe **how Terror Bat interacts with a target**.
//! They never encode project-specific opinions about correctness. Adapters
//! collect observations; Bat Specs and deterministic oracles decide what
//! those observations mean.
//!
//! Status vocabulary (honest, not aspirational):
//!
//! ```text
//! builtin-stable      — implemented, tested, covered by the local gate
//! builtin-partial     — implemented subset; limits stated per operation
//! composition         — no dedicated runtime; composed from other adapters
//! external-protocol   — via terrorbat-adapter/v1 (docs/external-adapter-v1.md)
//! planned             — not yet implemented; use the stated alternative
//! ```
//!
//! A `planned` adapter is never silently emulated: Bats that require it
//! must use the documented alternative or wait for the implementation.

use serde::Serialize;

/// One adapter operation (e.g. `command.run`, `fs.read`).
#[derive(Debug, Clone, Serialize)]
pub struct OperationInfo {
    pub action: String,
    pub description: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    /// Capability the Bat must declare in `requires` to use this action.
    pub capability: String,
    pub example: String,
}

/// One universal adapter.
#[derive(Debug, Clone, Serialize)]
pub struct AdapterInfo {
    pub name: String,
    pub title: String,
    pub status: String,
    pub description: String,
    pub operations: Vec<OperationInfo>,
    pub capabilities: Vec<String>,
    pub constraints: Vec<String>,
    pub examples: Vec<String>,
    pub platform_notes: Vec<String>,
}

fn op(
    action: &str,
    description: &str,
    inputs: &[&str],
    outputs: &[&str],
    capability: &str,
    example: &str,
) -> OperationInfo {
    OperationInfo {
        action: action.to_string(),
        description: description.to_string(),
        inputs: inputs.iter().map(|s| s.to_string()).collect(),
        outputs: outputs.iter().map(|s| s.to_string()).collect(),
        capability: capability.to_string(),
        example: example.to_string(),
    }
}

/// Constructor for the static catalogue below: one call site per adapter, so
/// bundling the nine fields into a struct would only move them, not remove them.
#[allow(clippy::too_many_arguments)]
fn adapter(
    name: &str,
    title: &str,
    status: &str,
    description: &str,
    operations: Vec<OperationInfo>,
    capabilities: &[&str],
    constraints: &[&str],
    examples: &[&str],
    platform_notes: &[&str],
) -> AdapterInfo {
    AdapterInfo {
        name: name.to_string(),
        title: title.to_string(),
        status: status.to_string(),
        description: description.to_string(),
        operations,
        capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
        constraints: constraints.iter().map(|s| s.to_string()).collect(),
        examples: examples.iter().map(|s| s.to_string()).collect(),
        platform_notes: platform_notes.iter().map(|s| s.to_string()).collect(),
    }
}

/// The full universal adapter catalogue in stable order.
pub fn all() -> Vec<AdapterInfo> {
    vec![
        adapter(
            "command",
            "Universal Command Adapter",
            "builtin-stable",
            "Anything callable from a shell is potentially testable: executable, args, \
             working directory, environment, stdin, stdout, stderr, exit code, timeout, \
             process tree, and files created/changed. No ecosystem-specific adapter required.",
            vec![op(
                "run",
                "Direct process spawn through the M2 supervisor. No implicit shell: the program \
                 is executed as given; a shell requires explicit intent (program: pwsh/cmd/sh). \
                 A nonzero exit is Completed, not a step failure — the oracle decides what it means.",
                &[
                    "program: string (required)",
                    "args: list of strings",
                    "cwd: worktree-relative path",
                    "stdin: string",
                    "env: mapping of strings or {$secret: NAME} references (resolved at run time, never persisted)",
                ],
                &[
                    "exit code",
                    "stdout bytes",
                    "stderr bytes",
                    "process-tree termination",
                    "wall time",
                    "timeout/crash classification",
                ],
                "process.spawn",
                "adapter: command, action: run, program: cargo, args: [test, --quiet]",
            )],
            &["process.spawn"],
            &[
                "Child programs are NOT path-confined; only Terror Bat's own built-in operations are worktree-confined.",
                "Output capture is bounded (1 MiB retained per stream; totals always counted).",
                "Secrets must come from environment/configuration, never from receipts. `env` accepts runtime-only `{$secret: NAME}` references: resolved from the execution environment at start, never written to Bat source, receipts, or the evidence store; replay requires the secret again, and a missing variable fails closed with SECRET_NOT_AVAILABLE.",
            ],
            &[
                "program: cargo, args: [test]",
                "program: python, args: [cli.py, --flag]",
                "program: node, args: [cli.js]",
                "program: pwsh, args: [-File, script.ps1]",
            ],
            &[
                "Windows and Linux: identical contract.",
                "Windows has no graceful-signal step; shutdown proceeds to job termination (still bounded).",
            ],
        ),
        adapter(
            "filesystem",
            "Universal Filesystem Adapter",
            "builtin-stable",
            "Operate against files and directories without knowing the target's language. \
             The adapter reports reality (bytes, listings, digests); it never decides whether \
             a changed file represents a defect.",
            vec![
                op(
                    "write",
                    "Write text or a spec-dir fixture (`from:`) to a worktree-relative path; parents created.",
                    &[
                        "path: worktree-relative (required)",
                        "text: string | from: spec-relative fixture",
                    ],
                    &["empty stdout; exit 0"],
                    "fs.write",
                    "adapter: fs, action: write, path: input/data.json, text: '{\"k\":1}'",
                ),
                op(
                    "mkdir",
                    "Create a directory tree inside the worktree.",
                    &["path: worktree-relative (required)"],
                    &["empty stdout; exit 0"],
                    "fs.write",
                    "adapter: fs, action: mkdir, path: generated",
                ),
                op(
                    "read",
                    "Read a worktree-relative file into step stdout (byte-preserving; large files truncated with honest totals).",
                    &["path: worktree-relative (required)"],
                    &["file bytes on stdout", "truncation totals"],
                    "fs.read",
                    "adapter: fs, action: read, path: output/result.json",
                ),
                op(
                    "list",
                    "List a worktree-relative directory (sorted entries, one per line, directories suffixed with /).",
                    &["path: worktree-relative (required)"],
                    &["sorted listing on stdout"],
                    "fs.read",
                    "adapter: fs, action: list, path: output",
                ),
                op(
                    "stat",
                    "Report size, kind (file/dir/missing), and sha256 (files only) as JSON on stdout.",
                    &["path: worktree-relative (required)"],
                    &["JSON metadata on stdout"],
                    "fs.read",
                    "adapter: fs, action: stat, path: output/result.json",
                ),
                op(
                    "digest",
                    "Digest a worktree-relative tree: sorted `sha256  relpath` lines for files plus directory markers. The before/after snapshot primitive.",
                    &["path: worktree-relative (required, '.' for whole worktree)"],
                    &["canonical digest listing on stdout"],
                    "fs.read",
                    "adapter: fs, action: digest, path: output",
                ),
                op(
                    "remove",
                    "Remove a worktree-relative file or empty directory (never recursive; never the worktree root).",
                    &["path: worktree-relative (required)"],
                    &["empty stdout; exit 0"],
                    "fs.write",
                    "adapter: fs, action: remove, path: stale/cache.tmp",
                ),
            ],
            &["fs.write", "fs.read"],
            &[
                "All paths are worktree-relative; absolute/drive/UNC/..-escape rejected as policy.",
                "Junction/symlink escapes refused via canonical-prefix containment.",
                "Hostile inputs (missing/readonly/empty/corrupt/unicode/nested paths) are reported, never normalised away.",
            ],
            &[
                "missing file -> honest step error (oracle sees Undetermined, never a forged finding)",
                "readonly file -> OS error surfaced as InfrastructureError",
                "unicode/nested paths -> handled as opaque bytes",
            ],
            &[
                "Windows: junctions refused on escape; symlinks may need privilege — missing privilege surfaces as InfrastructureError, never as a target finding.",
                "Linux: symlinks, Unix permissions honoured; escape refused identically.",
            ],
        ),
        adapter(
            "git",
            "Universal Git Adapter",
            "builtin-stable",
            "Repository state is part of many modern systems. Read-only inspection plus \
             disposable-worktree isolation; hostile cases (dirty tree, CRLF transforms, \
             stale HEAD, rewritten history, detached HEAD, missing objects) are surfaced, \
             never silently canonicalised unless the contract defines canonical bytes.",
            vec![
                op(
                    "status",
                    "Porcelain status of the disposable worktree.",
                    &[],
                    &["porcelain status on stdout"],
                    "git.inspect",
                    "adapter: git, action: status",
                ),
                op(
                    "diff",
                    "Diff HEAD in the disposable worktree (after intent-to-add, so new files appear).",
                    &[],
                    &["unified diff on stdout"],
                    "git.inspect",
                    "adapter: git, action: diff",
                ),
                op(
                    "rev_parse",
                    "Resolve a plain local revision (no ranges, no remotes, no option injection).",
                    &["rev: plain local revision (default HEAD)"],
                    &["commit hash on stdout"],
                    "git.inspect",
                    "adapter: git, action: rev_parse, rev: HEAD",
                ),
                op(
                    "worktree.snapshot",
                    "HEAD plus porcelain status as one deterministic snapshot.",
                    &[],
                    &["snapshot bytes on stdout"],
                    "git.inspect",
                    "adapter: git, action: worktree.snapshot",
                ),
            ],
            &["git.inspect"],
            &[
                "Target repository must be clean before the run; dirty targets are refused.",
                "All mutation happens in the disposable worktree; hostile tests prefer disposable clones/worktrees.",
                "Never change a user's repository destructively without explicit authority.",
            ],
            &["git.snapshot, command.execute, filesystem.inspect, git.inspect, oracle.evaluate"],
            &[
                "Windows: CRLF/LF and .gitattributes transforms are first-class hostile cases.",
                "Linux: identical contract; line-ending probe via blob-vs-worktree bytes.",
            ],
        ),
        adapter(
            "process",
            "Universal Process Adapter",
            "builtin-partial",
            "Stateful process generations can span Bat steps: start, wait for stdout/stderr/TCP/alive readiness, write stdin, observe bounded output, wait, terminate, kill, and restart. Handles are execution-local; replay creates fresh operating-system identities. The adapter remains partial because graceful terminate is Unix-only and descendant observation covers owned process-group/job members only (escaped descendants unsupported).",
            vec![
                op(
                    "start",
                    "Start an argv-based process generation under the process-group/job supervisor.",
                    &["handle, program, args, cwd, env, stdin, output_limit_bytes"],
                    &["generation, PID, root identity, start event"],
                    "process.start",
                    "adapter: process, action: start, handle: worker, program: python, args: [-u, worker.py], stdin: piped",
                ),
                op(
                    "wait_ready",
                    "Wait for stdout/stderr contains or regex, a literal-IP TCP listener, or an alive-for probe; returns on condition or bounded timeout.",
                    &["handle, one readiness condition, timeout_ms"],
                    &["ready or timed-out execution status; probe event"],
                    "process.readiness",
                    "adapter: process, action: wait_ready, handle: worker, stdout_contains: READY, timeout_ms: 5000",
                ),
                op(
                    "write_stdin",
                    "Queue bounded bytes/text to the same live process; optionally append newline or close stdin.",
                    &["handle, text or bytes, newline, close"],
                    &["input lifecycle event"],
                    "process.stdin",
                    "adapter: process, action: write_stdin, handle: worker, text: ping, newline: true",
                ),
                op(
                    "observe",
                    "Return output since the previous observation with actual byte totals and bounded-tail truncation honesty.",
                    &["handle"],
                    &["stdout/stderr bytes, totals, truncation flags, current root exit state"],
                    "process.observe",
                    "adapter: process, action: observe, handle: worker",
                ),
                op(
                    "terminate",
                    "Request graceful termination of the owned process group using SIGTERM.",
                    &["handle"],
                    &["termination request lifecycle event"],
                    "process.terminate",
                    "adapter: process, action: terminate, handle: worker",
                ),
                op(
                    "kill",
                    "Force termination of the owned process group/job and wait up to the supplied bound.",
                    &["handle, timeout_ms"],
                    &["termination, exit state, timeout or survivor error"],
                    "process.kill",
                    "adapter: process, action: kill, handle: worker, timeout_ms: 2000",
                ),
                op(
                    "wait",
                    "Wait for a generation to exit up to a bounded timeout.",
                    &["handle, timeout_ms"],
                    &["exit code/signal or timeout"],
                    "process.wait",
                    "adapter: process, action: wait, handle: worker, timeout_ms: 2000",
                ),
                op(
                    "restart",
                    "Start the next generation from the original start specification after the current generation is terminal.",
                    &["handle"],
                    &["new generation and preserved prior history"],
                    "process.restart",
                    "adapter: process, action: restart, handle: worker",
                ),
            ],
            &[
                "process.start",
                "process.readiness",
                "process.stdin",
                "process.observe",
                "process.terminate",
                "process.kill",
                "process.wait",
                "process.restart",
            ],
            &[
                "Handles live for one Bat execution only. Restart preserves the previous generation and assigns a new generation number and process identity.",
                "Cleanup makes a bounded graceful-then-force attempt and reports root-process survivors. Owned process-group/job descendants observed at cleanup start are recorded per handle and generation (Unix: /proc group scan; Windows: root only, job membership not enumerated). Escaped descendants are UNSUPPORTED.",
                "Output is a bounded tail. Totals and truncation are explicit; readiness timeout is not evidence that the target is ready or defective.",
            ],
            &["servers, daemons, workers, local databases, language servers, background services"],
            &[
                "Windows: process.start, readiness, stdin, wait, restart, and forced Job-based kill are supported; graceful terminate is explicitly unsupported.",
                "Linux: process-group SIGTERM terminate and SIGKILL forced kill are supported.",
            ],
        ),
        adapter(
            "stdio",
            "Universal StdIO / Pipe Adapter",
            "composition",
            "Machine interfaces over stdin/stdout (line protocol, raw bytes, JSON, JSONL, \
             request/response). Today: command.run with `stdin:` covers one-shot interactions \
             and scripted sessions (heredoc-style inputs, JSONL payloads). Persistent \
              multi-turn process conversations with a live handle use the stateful process adapter.",
            vec![op(
                "run with stdin (one-shot session)",
                "Feed bytes/JSON/JSONL on stdin; capture stdout/stderr; judge structurally with the JSON oracle and text conditions.",
                &["program, args", "stdin: string (raw/line/JSON/JSONL bytes)"],
                &["stdout/stderr bytes", "exit code"],
                "process.spawn",
                "adapter: command, action: run, program: python, args: [worker.py], stdin: '{\"op\":\"ping\"}\n'",
            )],
            &["process.spawn"],
            &[
                "One-shot command input only; process handles and multi-turn conversations are provided by the process adapter.",
                "For JSONL workers, one stdin document per line; responses judged from captured stdout.",
            ],
            &[
                "compiler tools, language servers (one-shot mode), MCP-style tools, agent harnesses, Unix filters",
            ],
            &[
                "Identical contract on Windows and Linux; line endings observed as bytes, never normalised.",
            ],
        ),
        adapter(
            "json",
            "Universal JSON Adapter",
            "composition",
            "JSON is common enough for first-class structural interaction — without inventing \
             a second programming language. Today: fs.read/command stdout feeds bytes; the \
             deterministic oracle provides parse, shape validation, JSON Pointer extraction \
             (RFC 6901), equality, and null-vs-missing distinction; mutations are declarative \
             (fs.write a mutated document, remove/corrupt/reorder fields as data). No JSONPath, \
             no scripting.",
            vec![op(
                "observe + oracle (no dedicated runtime)",
                "Parse, validate shape, extract via JSON Pointer, compare, and judge boundary numbers, type corruptions, and null/missing distinctions — all in the oracle.",
                &[
                    "any step stdout / worktree file / evidence object",
                    "oracle: json_value_equals with pointer",
                ],
                &["Falsified / NotFalsified / Undetermined (missing evidence never guesses)"],
                "fs.read (to observe) — no new capability",
                "oracle: { type: json_value_equals, step: run:0, pointer: /status, value: ok }",
            )],
            &["fs.read"],
            &[
                "Keep transformations declarative (data files), never a JSON DSL.",
                "Duplicate/reorder judged only where the contract defines semantics.",
                "Invalid JSON in the source yields Undetermined, never a finding.",
            ],
            &["mutate fields, remove fields, corrupt types, boundary numbers, null vs missing"],
            &["Platform-independent: byte-level JSON handling."],
        ),
        adapter(
            "http",
            "Universal HTTP Adapter",
            "planned",
            "Any HTTP-accessible system should be attackable without custom integration \
             (GET/POST/PUT/PATCH/DELETE/HEAD, headers, query, body/JSON/form, timeout; capture \
             status/headers/exact bytes/parsed JSON/timing/failures/redirects; sequences like \
             create → query → mutate → query → delete → query). NOT YET A NATIVE BUILTIN: \
             today use command.run with curl (Windows: curl.exe ships with modern Windows) or \
             the external adapter protocol. Secrets by environment reference, never in receipts.",
            vec![op(
                "request (via command.run today)",
                "Invoke curl (or another explicit client) as an ordinary supervised step; judge status/bytes/JSON with text and json_value_equals conditions.",
                &["method, URL, headers, query, body/JSON/form, timeout (as client args)"],
                &[
                    "status, headers, exact response bytes, timing, connection failure, redirects (as client output)",
                ],
                "process.spawn",
                "adapter: command, action: run, program: curl, args: [-sS, -X, GET, http://127.0.0.1:PORT/health]",
            )],
            &["process.spawn", "network (advisory: UNENFORCED)"],
            &[
                "No native TLS/client in v0.2; the client program owns TLS behaviour and its output is evidence.",
                "Network containment is UNENFORCED (worktree isolation honesty).",
                "Connection failures are evidence (Undetermined or NotFalsified per oracle), never automatic findings.",
            ],
            &["REST APIs, local services, web apps, agent gateways, test servers"],
            &[
                "Windows: prefer curl.exe; localhost-first for v0.2.",
                "Linux: curl/python-node clients identical via command.run.",
            ],
        ),
        adapter(
            "tcp",
            "Universal TCP Adapter",
            "planned",
            "Raw TCP for things that are not conveniently HTTP (connect, send/receive bytes, \
             disconnect/reconnect, timeout, half-close, deliberately malformed frames) without \
             implementing every protocol in Terror Bat. NOT YET A NATIVE BUILTIN: today use \
             command.run with an explicit client or the external adapter protocol.",
            vec![op(
                "exchange (via command.run today)",
                "Drive a scripted byte exchange through an explicit client program; malformed frames are data, not syntax errors.",
                &["host, port, send bytes, receive budget, timeout"],
                &["received bytes, timing, connection failure"],
                "process.spawn",
                "adapter: command, action: run, program: python, args: [tcp_probe.py, 127.0.0.1, 9999]",
            )],
            &["process.spawn", "network (advisory: UNENFORCED)"],
            &[
                "No protocol semantics in Terror Bat; bytes in, bytes out, oracle judges.",
                "Half-close where the platform supports it; otherwise documented as unavailable.",
            ],
            &["custom daemons, protocol fuzzing seams, readiness probes"],
            &[
                "Windows/Linux: TCP semantics identical at this layer; half-close support probed, not assumed.",
            ],
        ),
        adapter(
            "test-runner",
            "Universal Test-Runner Adapter",
            "composition",
            "No separate adapters for pytest/Cargo/npm/Go-test unless needed: one generic \
             runner driven by configuration understands only command → outcome → structured \
             evidence. Presets are DATA (example Bat fragments), not separate implementations. \
             Structured reports (JUnit XML) are consumed when present; otherwise raw output is \
             preserved rather than pretended about.",
            vec![op(
                "run suite (via command.run)",
                "Execute the ecosystem's own runner as an ordinary step; capture exit/output/duration; optionally detect counts; consume JUnit XML as text evidence.",
                &["runner command + args (see presets)", "cwd, env, timeout"],
                &["exit status, output, duration, optional test counts, optional JUnit XML bytes"],
                "process.spawn",
                "adapter: command, action: run, program: cargo, args: [test, --quiet]",
            )],
            &["process.spawn"],
            &[
                "Presets never become execution paths: a preset resolves to an ordinary command.run step.",
                "If no structured report exists, keep raw output; do not invent test counts.",
            ],
            &[
                "cargo test, pytest, unittest, npm/pnpm test, jest, vitest, go test, dotnet test, Maven, Gradle (see docs/universal-adapters.md)",
            ],
            &[
                "Runner availability is environment reality (see `terrorbat doctor` optional tools); missing runners are InfrastructureError, never findings.",
            ],
        ),
        adapter(
            "database",
            "Universal Database Adapter",
            "composition",
            "No giant database abstraction for v0.2: composition over drivers. Command-driven \
             databases use native clients through the command adapter (psql, sqlite3, mysql); \
             HTTP-exposed databases use the HTTP pattern. A small SQL adapter arrives later \
             only if repeated dogfooding justifies it.",
            vec![op(
                "query via native client (via command.run)",
                "Run the database's own CLI as an ordinary step against a disposable/local instance; judge rows/bytes with text/JSON conditions.",
                &[
                    "client program + connection args (local/disposable only)",
                    "query text or file",
                ],
                &["result bytes, exit status"],
                "process.spawn",
                "adapter: command, action: run, program: sqlite3, args: [test.db, 'SELECT 1;']",
            )],
            &["process.spawn"],
            &[
                " v0.2: local/disposable instances only; no production credentials in Bats or receipts.",
                "Durability judged by commit → close → reopen → inspect sequences, never by in-memory success.",
            ],
            &["sqlite3 via CLI, postgres via psql, HTTP databases via the HTTP pattern"],
            &[
                "Identical composition on both platforms; client availability varies (doctor reports).",
            ],
        ),
        adapter(
            "snapshot",
            "Universal Snapshot Adapter",
            "composition",
            "Capture state before and after an attack, then assert deterministically over change \
             (unchanged/changed/created/removed/equals/contains/digest-equals/count-changed/structural \
             difference). The false-success pattern in one line: invoke → success claimed → \
             snapshot → compare against the promised invariant → prove the success was false.",
            vec![op(
                "snapshot + compare (via fs.digest + git.snapshot + oracle)",
                "Digest the tree (fs.digest) and/or snapshot Git state before and after; judge with path_changed/path_unchanged, text_contains, and git_diff_contains.",
                &[
                    "fs.digest path=. before/after",
                    "git.worktree.snapshot / git.diff captures",
                ],
                &["digest listings, diff/status evidence, oracle change verdicts"],
                "fs.read + git.inspect",
                "setup: fs.digest path=output; run: command...; oracle: path_changed output/result.dat",
            )],
            &["fs.read", "git.inspect"],
            &[
                "Snapshots are evidence, not verdicts; the oracle decides.",
                "Digest form is canonical (sorted sha256+relpath); formatting changes never alter meaning.",
            ],
            &[
                "false-success: stdout claims success while digest/diff proves mutation",
                "durability: commit → close → reopen → digest compare",
            ],
            &[
                "Platform-independent digest; line-ending sensitivity is a hostile case, not a normalisation.",
            ],
        ),
        adapter(
            "external",
            "External Adapter Protocol",
            "external-protocol",
            "Specialist escape hatch (terrorbat-adapter/v1, JSONL over stdio): handshake exposes \
             protocol/identity/capabilities/schemas/requirements; operations cover discover, prepare, \
             execute, inspect, collect evidence, optional cleanup. Adapter output is evidence input — \
             adapter opinion is never proof (a lying adapter cannot forge PROVEN). See docs/external-adapter-v1.md.",
            vec![op(
                "execute (supervised one-shot JSONL)",
                "Terror Bat supervises the adapter process; describe/execute are bounded, schema-checked, and content-addressed into provenance.",
                &[
                    "bindings file (--adapters)",
                    "declared requires/forbids covering adapter needs + process.spawn",
                ],
                &[
                    "adapter stdout/stderr evidence",
                    "describe-Stderr diagnostics",
                    "description-id provenance",
                ],
                "per-adapter requires + process.spawn",
                "terrorbat run bat.yaml --repo . --adapters adapters.yaml",
            )],
            &["process.spawn"],
            &[
                "Adapter crash/malformed/slow/false-success/unsupported are infrastructure or Invalid — never target findings.",
                "Conformance harness covers valid/crashing/lying/malformed/false-success/slow/unsupported adapters.",
            ],
            &[
                "Lantern Keeper and Threadmoth thin adapters (adapters/lantern, adapters/threadmoth)",
            ],
            &[
                "Windows/Linux: stdio JSONL identical; program resolution honours the bindings file directory.",
            ],
        ),
    ]
}

/// Look up one adapter by name.
pub fn find(name: &str) -> Option<AdapterInfo> {
    all().into_iter().find(|a| a.name == name)
}

/// Stable adapter names in catalogue order.
pub fn names() -> Vec<String> {
    all().into_iter().map(|a| a.name).collect()
}
