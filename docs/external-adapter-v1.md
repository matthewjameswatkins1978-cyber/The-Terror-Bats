# External Adapter Protocol v1

Terror Bat invokes an external adapter as a supervised, one-shot process. The Bat Spec remains v1: each step still contains `adapter`, `action`, and an opaque structured payload. Terror Bat does not load third-party code into its process and does not interpret the payload.

## Bindings

Supply a local bindings file explicitly to each command that can execute a Bat:

```powershell
terrorbat run bat.yaml --repo D:\Target --adapters adapters.yaml
terrorbat pack run pack.yaml --repo D:\Target --adapters adapters.yaml
terrorbat replay <receipt> --adapters adapters.yaml
```

Schema version: `terrorbat-adapters/v1`.

```yaml
version: terrorbat-adapters/v1
adapters:
  example-python:
    program: python
    args:
      - D:\Adapters\example.py
```

Each binding contains only `program` and ordered string `args`. Unknown fields, duplicate names, and attempts to bind built-in names (`command`, `fs`, `git`) are rejected. Relative executable paths containing a directory are resolved from the bindings file directory. Bare program names are resolved through the process PATH. The bindings file is execution configuration; it is not Bat or Pack identity.

The adapter executable is trusted user-selected code. Describe and execute calls run as the user. Terror Bat supplies a minimal process environment (`PATH`, Windows startup variables, and temporary-directory variables), not arbitrary host variables or credentials. The disposable worktree is not a sandbox.

## Transport

Protocol version: `terrorbat-adapter/v1`.

Each process receives one UTF-8 JSON object followed by a newline on stdin, writes exactly one UTF-8 JSON object followed by a newline on stdout, and exits. stderr is reserved for diagnostics. A successful protocol process exits with OS status 0. Terror Bat supervises both describe and execute processes through M2, bounds retained protocol output, and terminates the process tree on timeout.

A valid exchange with stderr diagnostics remains valid. Protocol stdout containing malformed JSON, additional lines or other text fails as infrastructure error. Describe has a five-second supervisor timeout; execute uses the remaining setup/run and total Bat timeout budgets.

## Describe

Terror Bat describes each referenced external adapter before creating the worktree or executing any attack step:

```json
{"protocol":"terrorbat-adapter/v1","kind":"describe"}
```

The response has this shape:

```json
{
  "protocol":"terrorbat-adapter/v1",
  "kind":"description",
  "name":"example-python",
  "version":"1.2.3",
  "actions":{
    "run-tests":{"requires":["process.spawn"]}
  }
}
```

Every action must provide a list of ordinary Terror Bat capability base names. Terror Bat also requires `process.spawn` for every external action. A missing binding/action or undeclared required capability is `INVALID`; a required capability explicitly forbidden by the Bat is `POLICY DENIED`. Spawn errors, timeouts, crashes, malformed output, unsupported protocol versions, and name mismatches are infrastructure errors.

The description identity is `adapter:sha256:<hex>` over Terror Bat's canonical JSON representation of protocol, name, implementation version, actions, and requirements. It identifies the advertised interface; it does not establish binary equivalence. Binding paths and formatting are excluded.

## Execute

Terror Bat sends:

```json
{
  "protocol":"terrorbat-adapter/v1",
  "kind":"execute",
  "request_id":"unique-request-id",
  "action":"run-tests",
  "payload":{"task":"opaque Bat step data"},
  "context":{
    "worktree":"D:\\...\\worktree",
    "target_commit":"...",
    "execution_id":"...",
    "phase":"run",
    "step_index":0
  }
}
```

The adapter interprets `payload`. `context` identifies the disposable experiment and does not contain credentials or a copy of the host environment.

A response is:

```json
{
  "protocol":"terrorbat-adapter/v1",
  "kind":"result",
  "request_id":"unique-request-id",
  "status":"completed",
  "exit_code":17,
  "stdout":"logical operation output",
  "stderr":"logical operation error output"
}
```

Supported statuses are `completed`, `invalid`, `policy_denied`, and `infrastructure_error`. `exit_code` is the logical operation's exit code; it does not by itself falsify a claim. Only the deterministic oracle judges the captured evidence. Logical stdout/stderr are stored separately from the adapter process's protocol stdout/stderr diagnostics. Adapters can create larger evidence files inside the worktree for the existing post-state capture to observe.

An adapter never returns `PROVEN`. Process timeout, crash, cancellation, and protocol corruption are mechanical failures and cannot become claim falsification.

## Receipts, Campaigns, and replay

Each executed external step records adapter name, implementation version, protocol version, description identity, configured program and ordered arguments. Describe diagnostics and execute-process diagnostics are separate evidence from logical step streams. Built-in steps omit all M8-only fields so built-in receipt serialization and content identities remain unchanged.

Execute protocol streams record their observed byte totals and whether capture was truncated. Retained protocol evidence carries the same truncation flag, and receipt limitations state the retained and observed counts. Valid response stdout is parsed into logical streams rather than duplicated as protocol evidence; malformed or failed response stdout is retained. Exceeding either per-stream capture limit remains an infrastructure error and never proves a claim.

Campaign children remain ordinary runs. Their receipts retain adapter provenance; the Campaign refuses to write its own receipt if an adapter description identity changes for an adapter already used during that Campaign. Pack identity remains about the Bat definitions.

Replay of any Bat that references an external adapter requires an explicit `--adapters` file. Before creating a worktree, Terror Bat compares freshly advertised description identities with those recorded in the original receipt and refuses mismatches. It never executes a program path recovered from a receipt on its own.

## Fixture and demonstration

`src/bin/tb_adapter_fixture.rs` is a small Rust protocol fixture for tests and dogfood. It is not a production adapter. Build it and Terror Bat, then use [the demo Bat](../examples/m8/demo.yaml), [its binding](../examples/m8/adapters.yaml), and [a one-entry Pack](../packs/m8-external.yaml) against a clean target repository.