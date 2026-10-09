# The Terror Bats Framework — User Manual (0.2.0-rc.1)

> A Bat is one falsification experiment.

This manual is the canonical long-form guide for RC1. It describes the
software as it actually behaves. Where reality has sharp edges, they are
stated, not sanded off.

---

## 1. What The Terror Bats Framework is

The Terror Bats Framework (Terror Bats after first mention) is a
language-agnostic adversarial falsification framework. It attempts to
**disprove claims** about systems: it isolates the attack, records what
happened, and produces reproducible evidence.

It orchestrates existing machinery — test runners, fuzzers, static
analysis, Git — around a single pipeline. It does not replace Cargo test,
pytest, Playwright, or any other testing system.

RC1 runs on Windows x86-64 and Linux x86-64. Execution needs no AI, no
cloud, no model, and no Docker.

---

## 2. The central question

```text
What could still be wrong while all the ordinary tests are green?
```

Every Bat is an attempt to answer that question for one specific claim.
A Bat that finds nothing has not certified the system — it has only
failed to falsify one claim, once, under recorded conditions.

---

## 3. Mental model

```text
claim → attack → execution → oracle → evidence → receipt
```

- **Claim**: a falsifiable statement about the target system.
- **Attack**: declarative steps (adapters) executed against a disposable
  copy of the target.
- **Execution**: supervised run in a disposable Git worktree; every step
  outcome captured.
- **Oracle**: deterministic judgement over the captured evidence
  (Falsified / NotFalsified / Undetermined — never a guess).
- **Evidence**: content-addressed bytes (stdout, diffs, digests, logs).
- **Receipt**: the signed-by-hashing verdict document. The receipt is the
  product surface: human-readable, replayable, immutable.

---

## 4. Installation

### Windows

```powershell
cargo build --release --bin terrorbats
.\install\install.ps1 -InstallDir "$HOME\bin"
```

The helper copies `target\release\terrorbats.exe` into your directory. It
never modifies PATH unless `-AddToUserPath` is passed, and reports exactly
what changed when it does.

Requirements: Rust stable toolchain, Git on PATH.

### Linux

```sh
cargo build --release --bin terrorbats
install -m 755 target/release/terrorbats ~/.local/bin/terrorbats
```

Requirements: Rust stable toolchain, Git on PATH. The Linux bundle also
ships `share/man/man1/terrorbats.1` (generated from the live CLI: run
`terrorbats man` to regenerate it).

Verify either platform with:

```text
terrorbats --version
terrorbats doctor
```

`--version` prints exactly `terrorbats 0.2.0-rc.1`. `doctor` checks Git,
the evidence store, worktree capability, and process supervision, and
reports optional tools without scary failures for absent ones.

---

## 5. First flight

Run a supplied Bat against any clean Git repository:

```text
terrorbats run bats/command-exit.yaml --repo D:\some-project
```

This Bat runs `git --version` through the supervised command adapter and
judges it purely by exit code. On a healthy machine it yields
**NOT OBSERVED** — which is not a correctness certificate, only "this
attack did not falsify the claim".

The target repository must be clean (no uncommitted changes). All
mutation happens in a disposable worktree; your source tree is protected
from Terror Bats' own built-in operations.

---

## 6. Understanding results

A receipt ends in exactly one verdict:

| Verdict | Meaning |
|---|---|
| `PROVEN` | Deterministic evidence falsified the claim under the recorded conditions. |
| `REPRODUCED` | The behaviour was recreated, but evidence is insufficient to establish a violation. |
| `SUSPECTED` | A plausible problem was identified but not reproducibly established. (No First Flight evidence path produces this; it is never manufactured.) |
| `NOT OBSERVED` | This attack did not falsify the claim. **Not** a correctness certificate. |
| `INCONCLUSIVE` | Not enough information to support or reject the claim (timeout, crash, missing evidence). |
| `INVALID` | The Bat or experiment was malformed; nothing was tested. |
| `INFRASTRUCTURE ERROR` | Terror Bats or a dependency failed. Never a finding about the target. |

Exit codes mirror the outcome: `0` completed with no proven falsification,
`1` falsified (PROVEN finding), `2` invalid request/spec/policy issue,
`3` infrastructure failure or inconclusive execution.

---

## 7. What PROVEN does and does not mean

PROVEN means: deterministic evidence established that the tested claim was
falsified under the recorded conditions (exact commit, exact Bat, exact
environment declarations).

It does **not** mean: the system is broken everywhere, the finding
generalises, anyone is certified, or anything is production-safe. A
receipt proves one falsification, not a universal truth.

---

## 8. Doctor

```text
terrorbats doctor
terrorbats doctor --json
```

Reports: Terror Bats version, OS/architecture, Git availability, evidence
store availability, temp/worktree capability, process-supervision
capability (a live M2 probe), optional tool discovery, and platform
limitations. Optional tools are reported, never failed. `--json` emits the
same report machine-readably.

---

## 9. Writing a Bat

A Bat is declarative YAML data, not code. Complex behaviour belongs in
adapters, never in the spec:

```yaml
version: terrorbat/v1
id: my-first-bat
claim:
  text: The build verifier exits successfully.
requires: [process.spawn]
attack:
  run:
    - adapter: command
      action: run
      program: git
      args: [--version]
oracle:
  type: exit_code
  step: run:0
  not_equals: 1
evidence:
  capture: [stdout, stderr]
timeout:
  run: 60s
```

Validate without executing:

```text
terrorbats spec check my-bat.yaml
terrorbats spec id my-bat.yaml
```

`requires` declares capabilities the Bat needs; `forbids` declares what it
refuses. A step needing an undeclared capability is `INVALID`; a step
needing a forbidden one is `POLICY DENIED`.

---

## 10. Bat Spec reference overview

- `version`: must be exactly `terrorbat/v1` (a protocol identifier, not
  marketing — it never churns cosmetically).
- `id`, `claim.text`, `requires`, `forbids`, `environment.relevant`.
- `attack.setup[]` then `attack.run[]` in strict order, bounded budgets.
- `oracle`: deterministic conditions (exit codes, text contains, JSON
  values, digests, diffs).
- `evidence.capture`: which streams and captures persist.
- `timeout.run` / `timeout.total`: every operation is bounded.
- Content identity: the canonical semantic JSON (RFC 8785) is hashed
  (SHA-256) into `bat:sha256:…`, `claim:sha256:…`, and friends. Formatting
  changes never alter meaning.

Full field reference: `docs/bat-spec-v0.md`.

---

## 11. Built-in adapters

```text
terrorbats adapters
terrorbats adapter inspect process
```

- `command` (stable): any executable, argv-based, no implicit shell.
- `filesystem` (stable): read/write/list/stat/digest/remove, strictly
  worktree-confined.
- `git` (stable): status/diff/rev-parse/snapshot, read-only plus the
  disposable worktree.
- `process` (partial): stateful supervised processes (see §12).
- `stdio`, `json`, `test-runner`, `database`, `snapshot` (composition:
  built from the above, no new runtime).
- `http`, `tcp` (planned: use `curl`/explicit clients via `command.run`
  today; localhost-first).
- `external` (protocol): specialist escape hatch over JSONL stdio.

---

## 12. Stateful process adapter

One handle spans Bat steps: `start`, `wait_ready` (stdout/stderr contains
or regex, TCP listener, alive-for), `write_stdin`, `observe` (bounded
output since last read), `wait`, `terminate` (Unix SIGTERM only),
`kill`, `restart` (next generation, history preserved).

- Readiness uses per-stream generation-relative cursors: a stale marker
  can never satisfy a new wait.
- Root exit and pipe EOF are distinct facts. If output drains are still
  open when the root exits, the receipt records `output_drain_incomplete`
  — incompleteness reported, never manufactured complete.
- Owned process-group/Job cleanup is bounded and truthful. Escaped
  descendants (outside the owned boundary) are UNSUPPORTED.
- The adapter stays `builtin-partial`: graceful terminate is Unix-only,
  and Windows Job membership is not enumerated.

---

## 13. Packs

```text
terrorbats pack check packs/first-flight.yaml
terrorbats pack run packs/first-flight.yaml --repo D:\target --runs 2
```

A Pack is an ordered list of Bats run serially as a campaign: every child
is a first-class ordinary run, later results never overwrite earlier ones,
and `--stop-on-proven` halts only after a durably persisted PROVEN
receipt. Campaign receipts reference child receipts without copying
evidence.

---

## 14. Campaigns

Campaigns stay serial: child order is part of the evidence. The target
HEAD is pinned at campaign start and re-checked before every child. A
campaign is not a retry mechanism — it re-runs *to see what holds*, and a
retry re-runs *until something works*.

---

## 15. External adapters

Specialist escape hatch (`terrorbat-adapter/v1`, JSONL over stdio):
handshake exposes protocol/identity/capabilities/schemas/requirements;
`discover`, `prepare`, `execute`, `inspect`, collect evidence, optional
cleanup. Adapter output is evidence input — adapter opinion is never proof
(a lying adapter cannot forge PROVEN).

```text
terrorbats run bat.yaml --repo D:\Target --adapters adapters.yaml
```

Replay requires the same `--adapters` file; description identities are
compared before any worktree is created.

---

## 16. Evidence store

Content-addressed under one root (`objects/`, `runs/`, `campaigns/`):

- Default: `%LOCALAPPDATA%\TerrorBat` on Windows,
  `~/.local/share/TerrorBat` on Linux. Override any invocation with
  `--store <path>`. Existing receipts are never silently moved or
  deleted; the default is preserved through RC1.
- Objects are stored by SHA-256 and verified on read; a mismatch is
  corruption and an error, never silently overwritten.
- Step stdout/stderr are stored byte-verbatim (bounded 1 MiB retained per
  stream; totals always counted).

---

## 17. Receipts

The receipt is the primary human-facing output. It carries execution
status, the oracle result, the verdict plus its one-sentence meaning,
step records with evidence references, process lifecycle reports, and
replay instructions. Invocation `env` values are stored as `[REDACTED]`.

```text
terrorbats run bats/command-exit.yaml --repo D:\target --json
```

`--json` prints the full receipt machine-readably.

---

## 18. Inspect

```text
terrorbats inspect <execution-id | receipt:sha256:...>
terrorbats evidence show evidence:sha256:...        # text printed; digest verified on read
terrorbats evidence show evidence:sha256:... --out file.bin   # binary extraction
```

---

## 19. Replay

```text
terrorbats replay <execution-id | receipt:sha256:...>
```

Replay repeats the declarative attack as a new execution with fresh
operating-system identities (PIDs, timestamps). The original receipt stays
immutable. `$secret` references resolve again from the environment — replay
without the secret fails closed. Generation order, events, and exit
outcomes are the semantic evidence compared across runs.

---

## 20. Capabilities and authority

Bats declare `requires`/`forbids`; built-in action capabilities are checked
against declarations at preflight (undeclared = `INVALID`, forbidden =
`POLICY DENIED`). Other host-level access (network, filesystem outside the
worktree, git remote writes) is advisory-declared and honestly reported as
UNENFORCED — never silently blocked, never pretended contained.

---

## 21. Isolation boundary

```text
Disposable Git worktree mode is NOT hostile-code containment.
```

Host filesystem, network, and credential access are UNENFORCED. Child
programs are NOT path-confined; only Terror Bats' own built-in operations
are worktree-confined. Run only Bats and adapters you are willing to
execute with your own OS authority.

---

## 22. Secrets

- `process.start` and `command.run` accept runtime-only `$secret`
  environment references. The declarative configuration stores the
  reference, never the value; receipts record env as `[REDACTED]`; replay
  re-resolves; a missing variable fails closed.
- Explicit limitation: captured child output and files read as evidence
  are byte-verbatim. A target that prints its secret discloses it into
  the evidence store. There is no automatic redaction, by design for RC1.
- Literal credentials in Bat source, argv, stdin, or other authored fields
  are not protected. Keep secrets out of child-observable output.

---

## 23. Windows behaviour

First-class target. Process supervision uses Job objects with forced
termination; graceful `process.terminate` is explicitly unsupported (use
`kill`). Job membership is not enumerated — termination is applied and
reported, not censused. CRLF/LF and `.gitattributes` transforms are
first-class hostile cases, never normalised away. Junction/symlink escapes
are refused; missing symlink privilege surfaces as InfrastructureError.

---

## 24. Linux behaviour

Identical adapter contracts. Process groups with SIGTERM graceful
termination and SIGKILL forced kill; owned members observed via
`/proc` group scan (zombies excluded from kill decisions). Drive/UNC/ADS
path rules do not apply; Unix backslash filenames resolve normally.
Line-ending probes compare blob-vs-worktree bytes.

---

## 25. Exit codes

```text
0  framework operation completed, no proven falsification
1  claim falsified / PROVEN finding
2  invalid request / spec / policy issue
3  infrastructure failure / inconclusive execution
```

The run manifest/receipt is authoritative; the exit code is a summary.

---

## 26. Troubleshooting

- **Dirty target refused**: commit or stash; Terror Bats never silently
  includes uncommitted changes.
- **`INVALID` preflight**: a step needs a capability the Bat did not
  declare — add it to `requires`.
- **`POLICY DENIED`**: the Bat forbids something a step needs — reconcile
  the declaration, not the engine.
- **`SECRET_NOT_AVAILABLE`**: set the referenced environment variable and
  re-run (replay included).
- **`PROCESS_WAIT_TIMEOUT` / readiness timeout**: the target was not ready
  in time; a timeout describes the run, not the target. (If output was
  truncated mid-wait, absence of a marker is not established.)
- **`INFRASTRUCTURE ERROR`**: read the step error; missing runners and
  refused paths are environment reality, never target findings.
- **Survivors in cleanup**: an owned process outlived termination — the
  receipt names it; investigate the target, not the supervisor.

---

## 27. Known limitations

- `process` adapter is `builtin-partial` (see §12, §23, §24).
- Escaped/out-of-group descendants are unsupported and never enumerated.
- Captured output is not sanitised (see §22).
- `http`/`tcp` have no native builtins yet (see §11).
- Evidence durability covers supervised child-process failure, not
  power loss or disk corruption.
- No macOS support; x86-64 Windows and Linux only.
- This RC is for serious public evaluation, not certification: no
  security certification, no hostile-code sandbox, no formal verification.

---

## 28. Version/protocol compatibility

- Product: `terrorbats 0.2.0-rc.1` (`terrorbats --version`).
- Bat Spec: `terrorbat/v1`. External adapter protocol:
  `terrorbat-adapter/v1`. These identifiers never churn cosmetically; if
  they ever change, the protocol versions, history is not rewritten.
- Receipts embed their version; replay compares semantic evidence, not
  PIDs or timestamps, across executions.

---

## 29. Glossary

- **Bat**: one falsification experiment (declarative YAML).
- **Claim**: the falsifiable statement under test.
- **Attack**: the declarative steps executed against the target.
- **Oracle**: deterministic judgement (Falsified / NotFalsified /
  Undetermined).
- **Evidence**: content-addressed captured bytes.
- **Receipt**: the verdict document; immutable once written.
- **Replay**: a fresh execution of a recorded attack.
- **Pack**: an ordered serial list of Bats.
- **Campaign**: a Pack execution; children stay first-class runs.
- **Adapter**: how Terror Bats interacts with a target (built-in,
  composed, or external).
- **Worktree**: the disposable Git copy the attack actually touches.
- **PROVEN**: deterministic evidence falsified the claim. Nothing more.

---

## 30. Where to report issues

RC1 issues go to the repository issue tracker with: the exact
`terrorbats --version`, OS/architecture, the Bat (or minimal reproducer),
the receipt id, and the evidence-store failure excerpt. Security-sensitive
reports: see `SECURITY.md` — do not file public issues for live
vulnerabilities.
