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
| **ENFORCED** | Machinery technically prevents use beyond the grant (e.g. worktree confinement for a scoped `fs.write`, network denied at a real boundary). |
| **UNENFORCED** | Granted and merely advisory: the worker was *asked* not to exceed it, but nothing technically blocks it. |
| **DENIED** | Not granted. If the run cannot proceed without it, the outcome is `PolicyDenied`. |

The load-bearing rule:

> **Terror Bat must never claim a capability is technically blocked when it merely asked a worker not to use it.**

In v0.1 (Git-worktree isolation), most `forbids` entries are **UNENFORCED**: the worktree does not stop a determined process from touching the network or the host filesystem. The receipt records each capability's actual state (receipt-v0.md §1, question 8).

## 3. Enforcement reality by isolation level

| Isolation | Plausibly ENFORCED | Typically UNENFORCED |
|---|---|---|
| Git worktree (v0.1) | Scoped `fs.write` rollback via worktree destruction; `git.remote.write` absent if no credentials configured in the worktree environment | `network`, `host.config.*`, `credential.read`, unscoped `fs.*` |
| Container / VM (later, reused not built) | Network egress rules, filesystem mounts | Kernel-adjacent escapes |
| WASI sandbox (later) | Capability-based file/network grants by construction | Host resource exhaustion |

Terror Bat reuses external isolation systems rather than building its own (architecture.md §5). Each level upgrades which states are achievable; the DECLARED/GRANTED/ENFORCED/UNENFORCED/DENIED vocabulary stays the same.

## 4. Human rendering

Receipts render capabilities for humans:

```text
May:
✓ read repository
✓ modify disposable fixture
✓ run test processes

May not:
✗ push Git
✗ change host configuration
✗ read credentials
```

Where a "may not" is advisory only, the rendering must mark it, e.g. `✗ network (advisory — not technically blocked in v0.1)`.

## 5. Relationship to Tethers

Tethers may later provide stronger authority enforcement for capability decisions. Terror Bat must **not** depend on Tethers: the capability model, states, and honest receipt rendering are complete without it. Where Tethers is present, its decisions can move capabilities from UNENFORCED to ENFORCED with an auditable authority record; where it is absent, Terror Bat runs with worktree-level reality and says so.

## 6. Known open points (deferred, not hidden)

- The exact capability registry (names, scope grammar, defaults) is provisional until M3 exercises it against real worktree runs.
- How `process.spawn` scopes to specific adapters/commands is undecided; v0 treats spawn as coarse-grained and relies on adapter-level logging.
- Whether a DENIED-but-required capability fails fast at supervisor start or at first use → M2 (direction: fail fast, outcome `PolicyDenied`).
