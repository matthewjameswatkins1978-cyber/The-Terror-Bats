# Universal Adapters

The normal first question is: **"Can one of the universal adapters already
reach this thing?"** A specialist adapter is the exception, not the rule.

```powershell
terrorbat adapters
terrorbat adapter inspect filesystem
terrorbat adapter inspect http --json
```

Adapters describe **how Terror Bat interacts with a target**. They never
encode project-specific opinions about correctness. Universal adapters
collect observations; Bat Specs and deterministic oracles decide what those
observations mean.

## Status vocabulary

Discovery is honest, not aspirational:

```text
builtin-stable      implemented, tested, covered by the local gate
builtin-partial     implemented subset; limits stated per operation
composition         no dedicated runtime; composed from other adapters
external-protocol   via terrorbat-adapter/v1 (see external-adapter-v1.md)
planned             not yet implemented; use the stated alternative
```

A `planned` adapter is never silently emulated.

## The catalogue

| Adapter     | Status            | What it reaches                              |
| ----------- | ----------------- | -------------------------------------------- |
| `command`   | builtin-stable    | Any executable: cargo, pytest, node, pwsh…   |
| `filesystem`| builtin-stable    | Files/dirs: read, write, list, stat, digest  |
| `git`       | builtin-stable    | Repo state: status, diff, rev-parse, snapshot|
| `process`   | builtin-partial   | Long-running programs (supervised subset)    |
| `stdio`     | composition       | stdin/stdout workers via `command.run`       |
| `json`      | composition       | Structural JSON via oracle + `fs`            |
| `http`      | planned           | Via `curl` steps today; native later         |
| `tcp`       | planned           | Via explicit clients today; native later     |
| `test-runner`| composition      | Presets below: command → outcome → evidence  |
| `database`  | composition       | Native CLIs (`sqlite3`, `psql`) via command  |
| `snapshot`  | composition       | `fs.digest` + git captures + oracle          |
| `external`  | external-protocol | Specialist escape hatch (JSONL stdio)        |

## Stateful process lifecycle

The process adapter owns a handle for one Bat execution. A handle keeps every
generation; restart requires the previous generation to be terminal and starts
the next generation from the original argv/cwd/environment settings. Replay
repeats the declarative attack with new PIDs and timestamps. Those instance
values are receipt evidence, not Bat identity.

Readiness is condition-based and bounded. `wait_ready` supports stdout or stderr
text/regex, a literal IP and TCP port, or an alive-for probe. A timeout means
the condition was not observed in time; it does not establish a target failure.
`observe` returns bytes since the previous observation and preserves honest byte
totals when the bounded tail has truncated output.

```yaml
claim:
  text: A worker drops requests sent after it announces readiness.
requires: [process.start, process.readiness, process.stdin, process.observe, process.kill]
attack:
  run:
    - adapter: process
      action: start
      handle: worker
      program: python
      args: [-u, worker.py]
      cwd: .
      stdin: piped
    - adapter: process
      action: wait_ready
      handle: worker
      stdout_contains: READY
      timeout_ms: 5000
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
  all:
    - type: text_contains
      step: run:4
      stream: stdout
      substring: ECHO:ping
```

The receipt records handle/generation, root PID and process identity, argv,
cwd label, lifecycle events, exit/termination state, bounded output counts,
and cleanup. Root-process identity is instance-specific; generation order,
events and exit/termination outcomes are semantic replay evidence. Cleanup
attempts graceful termination before forced termination and records root
survivors. It does not enumerate each descendant or detect a descendant that
escapes its owned process group/job, and this runtime is not a hostile-code
sandbox.

### Secret references and the disclosure boundary

`process.start` and `command.run` accept runtime-only `{$secret: NAME}`
environment references. The supported guarantee is narrow: the declarative
configuration (Bat source, canonical spec) stores the reference, not the
resolved value; receipt invocation payloads record env values as
`[REDACTED]`; replay resolves the reference again; a missing variable fails
closed with `SECRET_NOT_AVAILABLE`.

The explicit limitation: captured child stdout/stderr and files read back
as evidence are byte-verbatim. A target process that prints its secret — to
stdout, stderr, its own logs, or files Terror Bat later reads — discloses
that secret into the evidence store. Arbitrary literal credentials in Bat
source, argv, stdin, or other authored fields are not protected by the
environment-reference mechanism, and the heuristic argv redaction is not a
comprehensive secret-safety guarantee. Authors must keep secrets out of
child-observable output; there is no automatic redaction, by design for RC1.

On Linux, `terminate` requests SIGTERM to the owned process group. On Windows,
graceful termination is explicitly unsupported; use `kill` for Job-based forced
termination. Both platforms support bounded wait, readiness, stdin,
observation and restart.

## Composition

The important part is that adapters compose. A Bat can do:

```text
filesystem.snapshot (fs.digest in setup)
process.start       (argv-based, supervised)
process.wait_ready  (condition-based readiness)
process.write_stdin (same live handle)
process.observe     (bounded output since last read)
process.kill        (owned process group/job termination)
filesystem.snapshot (fs.digest again)
oracle.evaluate     (path_changed, text_contains, json_value_equals)
```

without a special `my-weird-web-service-adapter`. Another Bat might do:

```text
git.snapshot
command.execute
filesystem.inspect
git.inspect
command.execute
oracle.evaluate
```

Project-specific knowledge belongs in the **Bat**, not the adapter.

## The snapshot pattern (false-success in one shape)

```yaml
attack:
  setup:
    - adapter: fs
      action: digest
      path: .
  run:
    - adapter: command
      action: run
      program: cargo
      args: [test, --quiet]
oracle:
  all:
    - type: text_contains
      step: run:0
      stream: stdout
      substring: ALL TESTS PASSED
    - type: path_changed
      path: tb-should-not-exist.txt
```

Invoke → success claimed → snapshot → compare against the promised
invariant → prove the success was false. See
`bats/filesystem-snapshot-detects-change.yaml` (positive) and
`bats/filesystem-snapshot-quiet-control.yaml` (control).

## Test-runner presets (data, not code)

Presets resolve to ordinary `command.run` steps. Capture exit/output/
duration; consume JUnit XML when present; otherwise preserve raw output
rather than pretending to understand it.

| Ecosystem | Command                                  |
| --------- | ---------------------------------------- |
| cargo     | `cargo test`                             |
| pytest    | `python -m pytest -q`                    |
| unittest  | `python -m unittest`                     |
| npm       | `npm test -- --reporter=json`            |
| pnpm      | `pnpm test`                              |
| jest      | `npx jest --json`                        |
| vitest    | `npx vitest run --reporter=json`         |
| go        | `go test ./...`                          |
| dotnet    | `dotnet test`                            |
| Maven     | `mvn -q test`                            |
| Gradle    | `gradle test --quiet`                    |

Judge with the `exit_code` condition; when JUnit XML exists, capture the
file with `fs.read` and judge structurally. Copy-paste fragments live in
`examples/test-runners/presets.md`.

## Databases without drivers

Command-driven: `sqlite3 test.db 'SELECT …;'`, `psql … -c '…'`,
`mysql … -e '…'` as ordinary steps against local/disposable instances.
HTTP-exposed databases use the HTTP pattern. Durability is judged by
commit → close → reopen → inspect sequences, never by in-memory success.
A dedicated SQL adapter arrives only if repeated dogfooding justifies it.

## Platform differences

Universal does NOT mean pretending Windows and Linux are identical:

- Windows: junctions, job-object termination, no graceful signals.
- Linux: symlinks, `SIGTERM` grace period, Unix permissions.

A Bat may require a capability such as `filesystem.symlink`. When the
platform cannot provide it, the run reports **UNSUPPORTED** (mechanical
status; verdict `INCONCLUSIVE` with an `UNSUPPORTED` note) — never `FAIL`,
and certainly never `PROVEN`. Missing privilege surfaces as
infrastructure reality, not as a target finding.

## Escalation to a specialist adapter

Write a dedicated adapter only when at least one of these is true:

1. The universal adapters cannot expose necessary state.
2. The target uses a genuine protocol requiring semantic understanding.
3. Correct evidence collection requires ecosystem-specific knowledge.
4. Universal composition would become dangerously fragile.
5. The specialist adapter provides substantial repeated value.

Before creating one, record why:

```text
Universal adapters insufficient because:
required durable transaction identity is not externally observable.
```

That explanation is evidence for why the abstraction needs expanding.
