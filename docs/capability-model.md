# Capability Model v0 (M0)

Terror Bat declares effects before execution (Constitution 5) and records honestly what was actually enforced (Constitution 19). A Bat may know less than the host knows, and may do less than the host can do (Constitution 6, 7).

## 1. Capabilities

Initial conceptual capability set:

```text
fs.read
fs.write
process.spawn
network.http
git.inspect
git.local.write
git.remote.write
host.config.read
host.config.write
credential.read
```

Shorthands used in Bat Specs: `network` covers `network.http` and any future network capability; `git.inspect` is read-only Git examination.

Capabilities may carry **scopes**:

```text
fs.read: repo/**
fs.write: fixture/**
git.local.write: worktree/**
```

A scoped grant is narrower than the unscoped capability. An unscoped declaration (e.g. bare `fs.read`) means "whole filesystem, subject to isolation reality" — the receipt must say so.

## 2. Capability states

Every capability relevant to a run passes through states. These are distinct and must never be conflated:

| State | Meaning |
|---|---|
| **DECLARED** | The Bat Spec asked for it (`requires`) or renounced it (`forbids`). |
| **GRANTED** | The supervisor agreed the run may have it. |
| **ENFORCED** | Machinery technically prevents use beyond the grant (e.g. network egress blocked at a real boundary, filesystem access confined by a sandbox). |
| **UNENFORCED** | Granted and merely advisory: the worker was *asked* not to exceed it, but nothing technically blocks it. |
| **DENIED** | Not granted. If the run cannot proceed without it, the execution status is `PolicyDenied`. |

The load-bearing rule:

> **Terror Bat must never claim a capability is technically blocked when it merely asked a worker not to use it.**

### Authority/enforcement vs reversibility/containment

These are separate properties and must be recorded separately:

**Authority/enforcement** — whether machinery prevents an operation outside the permitted scope. This is what the states above describe.

**Reversibility / containment** — properties such as:

```text
workspace mutations observable
workspace mutations reversible
host writes not contained
network not contained
```

> **Rollback is not enforcement.**

A disposable Git worktree provides a disposable workspace, easy observation of repository mutations, and rollback/reversibility for changes contained inside that workspace. It does **not** prevent a process from writing outside the worktree, reading unrelated host files, contacting the network, invoking Git credential helpers, or changing host configuration. Worktree-only isolation must therefore never classify scoped filesystem access as ENFORCED merely because mutations can later be discarded.

M0.1 deliberately does not introduce a type system for containment properties; receipts state them in plain words alongside capability states.

### Honesty under v0.1 worktree-only execution

Under worktree-only execution, capabilities such as `network`, host filesystem access, and `git.remote.write` are normally **UNENFORCED** unless a separate technical boundary genuinely prevents them. In particular, `git.remote.write` must not be claimed as DENIED merely because credentials were not deliberately provided: Git credential helpers or host configuration may still make credentials available. Receipts must state this honestly (receipt-v0.md §1, question 8).

## 3. Enforcement reality by isolation level

| Isolation | Plausibly ENFORCED | Typically UNENFORCED |
|---|---|---|
| Git worktree (v0.1) | **None by itself** — the worktree provides observability and reversibility of workspace mutations, not enforcement | `fs.*`, `network`, `host.config.*`, `credential.read`, `git.remote.write` |
| Container / VM (later, reused not built) | Network egress rules, filesystem mounts | Kernel-adjacent escapes |
| WASI sandbox (later) | Capability-based file/network grants by construction | Host resource exhaustion |

Terror Bat reuses external isolation systems rather than building its own (architecture.md §5). Each level upgrades which states are achievable; the DECLARED/GRANTED/ENFORCED/UNENFORCED/DENIED vocabulary stays the same.

## 4. Human rendering

Receipts render capabilities for humans:

```text
May:
✓ read repository
✓ modify disposable fixture (mutations reversible via worktree rollback —
  reversibility, not enforcement)
✓ run test processes

May not:
✗ push Git (advisory — credential helpers may still permit it)
✗ change host configuration (advisory — not technically blocked in v0.1)
✗ read credentials (advisory — not technically blocked in v0.1)
```

Every "may not" that is advisory only must be marked as such. Under worktree-only isolation, virtually all of them are.

## 5. Relationship to Tethers

Tethers may later provide stronger authority enforcement for capability decisions. Terror Bat must **not** depend on Tethers: the capability model, states, and honest receipt rendering are complete without it. Where Tethers is present, its decisions can move capabilities from UNENFORCED to ENFORCED with an auditable authority record; where it is absent, Terror Bat runs with worktree-level reality and says so.

## 6. Known open points (deferred, not hidden)

- The exact capability registry (names, scope grammar, defaults) is provisional until M3 exercises it against real worktree runs.
- How `process.spawn` scopes to specific adapters/commands is undecided; v0 treats spawn as coarse-grained and relies on adapter-level logging.
- Whether a DENIED-but-required capability fails fast at supervisor start or at first use → M2 (direction: fail fast, execution status `PolicyDenied`).
- The exact vocabulary/format for recording containment and reversibility properties in `receipt.json` → M6; M0.1 requires only that they exist, are separate from enforcement states, and are human-readable.
