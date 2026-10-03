# Terror Bat 0.1 — First Flight Guide (Windows)

This guide gets a technical user from zero to a real, evidence-backed result
without reading source code.

## What Terror Bat is

A language-agnostic falsification and assurance framework. It attacks claims
about systems, isolates the attack in a disposable Git worktree, captures
what happened as content-addressed evidence, judges it with deterministic
oracles, and produces a receipt.

> **What could still be wrong while all the ordinary tests are green?**

A **Bat** is one declarative experiment (YAML): a claim, an attack, an
oracle, and evidence requirements. The pipeline:

```text
CLAIM → ATTACK → EXECUTION → ORACLE → EVIDENCE → RECEIPT
```

## What Terror Bat is not (First Flight)

- Not a sandbox. The disposable Git worktree gives **reversibility and
  observation**, not containment. Host filesystem, network, credentials and
  Git remotes remain **UNENFORCED** (advisory declarations only).
- Not a test runner replacement. It reuses Git, Cargo, pytest, etc. via
  `command.run` or command-verifier oracles.
- Not an AI tool. The core requires no API key, no cloud, no model. AI
  discovery is a later milestone; `PROVEN` is always deterministic.

## Install (portable)

The release artifact is a single executable:

```powershell
cargo build --release --bin terrorbat
# -> target\release\terrorbat.exe
```

Copy `terrorbat.exe` wherever you like (helper: `install\install.ps1`).
Terror Bat never modifies PATH silently. Git must be on PATH.

Check readiness:

```powershell
terrorbat doctor
```

Doctor verifies Git, the evidence store, worktree create/remove, temp
writability and M2 process supervision; it reports optional tools
(cargo/rustc/pwsh/python/node) without requiring them.

## Run a Bat

```powershell
terrorbat run bats\unexpected-change.yaml --repo D:\Projects\some-project
```

Rules:

- The target repository **must be clean** (no tracked/staged/untracked
  changes). Dirty targets are refused — Terror Bat never silently includes
  or discards your local work.
- The exact HEAD commit is pinned; the attack runs in a disposable worktree
  under `%TEMP%\terrorbat\<execution-id>\`, never in your working tree.
- Useful flags: `--param name=value` (repeatable), `--store <path>`,
  `--json`.

Output ends with a verdict and a receipt id (human presentation by Sartorial; `--json` stays bare schema):

```text
\^v^/  BAT RECEIPT  \^v^/
> VERDICT  PROVEN
Deterministic evidence established that the tested claim was falsified
under the recorded conditions. This is not a universal proof...

RECEIPT:   receipt:sha256:...
```

### What PROVEN means — and does not mean

`PROVEN` = deterministic evidence established the claim was **falsified
under the recorded conditions** (this repo, this commit, this machine, this
Terror Bat version). It is **not** a universal proof, not a certification,
and not a security statement.

`NOT OBSERVED` = this attack did not falsify the claim. It is **not** a
correctness certificate.

Verdict layers (never flattened):

```text
Execution status : Completed | TimedOut | Crashed | Cancelled |
                   PolicyDenied | Invalid | InfrastructureError
Oracle result    : Falsified | NotFalsified | Undetermined
Verdict          : PROVEN | REPRODUCED | SUSPECTED | NOT OBSERVED |
                   INCONCLUSIVE | INVALID | INFRASTRUCTURE ERROR
```

A crash, timeout, cancellation or policy denial is **never** evidence that
the claim failed; such runs yield `INCONCLUSIVE` (or `INVALID` /
`INFRASTRUCTURE ERROR`).

## Inspect, evidence, replay

```powershell
terrorbat inspect <execution-id | receipt:sha256:...>
terrorbat evidence show evidence:sha256:...        # text printed; digest verified on read
terrorbat evidence show evidence:sha256:... --out file.bin   # binary extraction
terrorbat replay <execution-id | receipt:sha256:...>
```

Replay creates a **new execution** from the stored Bat source and the pinned
commit (local only; it never fetches). The original receipt is immutable.
The comparison report lists status/oracle/verdict equality **and per-kind
evidence identity** — a shared verdict alone never counts as equivalence.

## Where evidence lives (and how to remove it)

Default store: `%LOCALAPPDATA%\TerrorBat\`

```text
TerrorBat\
  objects\sha256\ab\cdef...   content-addressed evidence (deduplicated)
  runs\<execution-id>\        manifest.json, operations.jsonl, receipt.json
  temp\
```

Override per-command with `--store <path>`. To remove old runs: delete
their `runs\<execution-id>` directories; to reclaim evidence bytes, delete
objects no longer referenced by any run you care about (or the whole store —
it is derived data; receipts you still hold remain the record). Objects are
digest-verified on every read; a corrupted object fails loudly
(`TB-EVIDENCE-CORRUPT`) instead of being served.

## Writing a Bat

Minimal shape (full reference: [bat-spec-v0.md](bat-spec-v0.md)):

```yaml
version: terrorbat/v1
id: my-bat
claim:
  text: A falsifiable statement about the target repository.
requires: [fs.write, process.spawn, git.inspect]   # declare effects first
attack:
  setup:
    - adapter: fs
      action: mkdir
      path: generated
  run:
    - adapter: command
      action: run
      program: cargo          # no implicit shell — pwsh/cmd must be explicit
      args: [test, --quiet]
oracle:
  any:
    - type: exit_code
      step: run:0
      not_equals: 0
    - type: git_diff_contains
      substring: "TODO(fixme)"
evidence:
  capture: [git_diff, stdout, stderr]
timeout:
  run: 600s                   # strict <unsigned integer>s
```

Built-in adapters: `command.run`, `fs.write`, `fs.mkdir`, `git.status`,
`git.diff`, `git.rev_parse`, `git.worktree.snapshot`. Oracle conditions:
`file_exists`, `file_absent`, `text_contains`, `text_matches`,
`git_diff_contains`, `git_diff_matches`, `path_changed`, `path_unchanged`,
`exit_code`, `evidence_present`, `json_value_equals`, `command`
(deterministic verifier). Combinators: `all`, `any`, `not` only.

Conditions are **falsification detectors**: when a condition fires, the
claim is falsified. Missing evidence yields `Undetermined` — the engine
never guesses.

## Exit codes

```text
0  completed; no proven falsification (also: NOT OBSERVED)
1  claim falsified — PROVEN finding
2  invalid request / spec / policy issue (INVALID, PolicyDenied, bad input)
3  infrastructure failure / inconclusive
```

The receipt remains authoritative; the exit code is a summary.

## Shipped Bats

- `bats/command-exit.yaml` — verifier command succeeds? (exit-code oracle)
- `bats/unexpected-change.yaml` — protected-path mutation is detected (PROVEN demo)
- `bats/orphan-process.yaml` — timeout vs. a process tree (needs `--param probe_program=...`)
- `bats/false-success.yaml` — green-looking output contradicted by diff evidence (Windows/cmd, explicit)
- `bats/example.yaml` — the dependency-shadow identity example (M1 docs)

## Packs and campaigns (W2)

First Flight is complete: the Bats above run, the self-attack passes, and receipts verify. The in-repo self-attack is explicitly **self-attack lite** — a fixed demonstrator over known Bats, not an open-ended assault on Terror Bat itself (that remains M10).

W2 adds repeated falsification on top of the same machinery. A **pack** names an ordered list of Bats (paths resolve relative to the pack file); a **campaign** runs a pack N times serially, each child a first-class ordinary run with its own receipt:

```powershell
terrorbat pack check packs\first-flight.yaml
terrorbat pack run packs\first-flight.yaml --repo D:\Projects\some-project --runs 2
terrorbat pack run packs\first-flight.yaml --repo D:\Projects\some-project --stop-on-proven
```

A retry re-runs *until something works*; a campaign re-runs *to see what holds* — later results never overwrite earlier ones, and `stop-on-proven` halts only after a durably persisted PROVEN receipt. Campaign receipts (`campaign:sha256:…`) reference child receipts without copying evidence. The shipped `packs\first-flight.yaml` is a demonstrator only and carries no assurance-profile claims. Full evidence reuse/caching (Constitution 13) and the external adapter protocol come later; W2 deliberately adds neither.

## Known limitations (First Flight)

- Worktree mode is not hostile-code containment (see above); run only Bats
  whose commands you would run yourself.
- Evidence durability covers supervised child failure — not power loss.
- Replay is same-machine/local-repo oriented; the pinned commit must still
  exist locally.
- Only Windows is proven in this release; Linux/macOS are untested.
- `environment.relevant` captures a small known-facts registry
  (os.name, os.arch, git.version, terrorbat.version); other declared names
  are honestly recorded as not captured.
