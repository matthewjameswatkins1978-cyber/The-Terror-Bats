# Terror Bat Architecture v0.1 (M0)

Status: architectural contract only. No implementation exists. Planned implementation language: Rust.

## 1. Core pipeline

Every Terror Bat experiment follows one pipeline:

```text
CLAIM
  ↓
ATTACK
  ↓
EXECUTION
  ↓
ORACLE
  ↓
EVIDENCE
  ↓
RECEIPT
```

- **CLAIM** — a falsifiable statement about the system under test (e.g. "suitable dependencies are reused before equivalent functionality is introduced").
- **ATTACK** — a deliberate attempt to falsify the claim, expressed declaratively in a Bat Spec.
- **EXECUTION** — running the attack under a supervisor, inside an isolation boundary, with declared capabilities.
- **ORACLE** — deterministic machinery that judges what happened against the claim.
- **EVIDENCE** — immutable, content-addressed artifacts that outlive the run (diffs, stdout/stderr, logs, environment facts).
- **RECEIPT** — the primary human-facing output: what was claimed, what was attempted, what happened, how it was judged, and how to reproduce it.

Terror Bat answers one question: **what could still be wrong while all the ordinary tests are green?** It does not replace test suites, fuzzers, property testing, mutation testing, static analysis, security scanners, or model evals. It orchestrates and combines them.

## 2. Conceptual components

| Component | Role |
|---|---|
| **Bat Spec** | Declarative definition of one experiment (see [bat-spec-v0.md](bat-spec-v0.md)). |
| **Bat supervisor** | Owns the lifetime of one Bat execution: start, timeout, cancellation, crash handling. |
| **Runner** | Executes the attack steps; delegates language-specific work to adapters. |
| **Adapters** | Language/tool bridges that run `cargo test`, `pytest`, HTTP requests, Git inspection, etc. |
| **Isolation boundary** | The containment the run actually gets (v0.1: disposable Git worktree; explicitly *not* hostile-code containment). |
| **Deterministic oracle engine** | Evaluates oracle conditions against collected artifacts without model judgement. |
| **Evidence store** | Content-addressed, append-only storage of run artifacts. |
| **Receipt generator** | Produces `receipt.json` plus a human-readable rendering. |
| **AI discovery layer** (optional) | Proposes candidate Bats and drafts Bat Specs. Never required for execution. |

## 3. Constitution

These principles bind all later milestones. Wording may be polished; meaning may not drift.

1. Everything executable has an owner.
2. Failure is expected, isolated and explicit.
3. Evidence survives the process that created it.
4. Every meaningful immutable artifact can have a stable content identity.
5. Effects and capabilities are declared before execution.
6. A Bat may know less than the host knows.
7. A Bat may do less than the host can do.
8. AI may propose attacks; deterministic machinery proves findings wherever reasonably possible.
9. `suspected`, `reproduced`, and `proven` are distinct epistemic states.
10. Terror Bat never claims certification, approval, security or trust status that an external authority has not actually granted.
11. Existing testing systems are reused rather than unnecessarily replaced.
12. Human-readable names are labels; canonical identity may be content-derived.
13. Cached evidence may only be reused when all declared relevant inputs match.
14. Important failures must be explainable to a human.
15. The framework must not become harder to understand than the system it is testing.
16. Configuration must remain declarative. Do not smuggle a programming language into YAML/TOML.
17. AI is optional for execution. Core Bats must run without an API key.
18. Context is a resource. AI receives the smallest useful projection and asks for additional context when needed.
19. Isolation claims must state exactly what is and is not enforced.
20. Evidence is more important than the framework's opinion.

## 4. Supervision (OTP-inspired)

Terror Bat steals OTP *semantics*, not the BEAM runtime:

- Everything executable has an owner; supervisors own cancellation and timeout.
- Workers have explicit lifetimes; failures are expected and isolated.
- Child failure must not corrupt unrelated work.
- Restart behaviour is explicit per worker kind, not implicit.
- Evidence must outlive the worker that produced it (Constitution 3).

Conceptual tree:

```text
Colony
│
├── BatSupervisor
│   ├── Setup
│   ├── Attack
│   ├── Oracle
│   └── EvidenceCollector
│
└── ReceiptWriter
```

The `ReceiptWriter` sits outside `BatSupervisor` so that a crashed Bat still yields a receipt describing the crash.

### Execution status, oracle result, finding maturity

Terror Bat keeps a strict separation between three conceptual layers:

```text
WHAT HAPPENED TO THE RUN?
        ↓
WHAT DID THE ORACLE DETERMINE?
        ↓
WHAT MAY WE RESPONSIBLY CLAIM?
```

**Execution status** — what mechanically happened to the run:

```text
Completed
TimedOut
Crashed
Cancelled
PolicyDenied
Invalid
InfrastructureError
```

**Oracle result** — the deterministic oracle's report on whether the available evidence falsified the claim:

```text
Falsified
NotFalsified
Undetermined
```

`NotFalsified` means only that *this attack* did not falsify the claim. It never means the claim is universally true. Exact Rust representations of these layers are an M5 implementation decision; the semantic separation is binding now.

**Finding maturity** — the epistemic lifecycle of a finding, not a process-execution status:

```text
SUSPECTED → REPRODUCED → PROVEN
```

A receipt additionally reports `NOT OBSERVED`, `INCONCLUSIVE`, `INVALID`, or `INFRASTRUCTURE ERROR` where appropriate. Semantics and the mapping from (execution status, oracle result) to receipt verdict are defined in [receipt-v0.md](receipt-v0.md).

Hard rules:

- A crash does not prove claim failure.
- A timeout does not prove claim failure.
- A policy denial does not prove claim failure.
- Model judgement cannot create `PROVEN`.
- `NOT OBSERVED` is not certification or correctness.
- `TimedOut`, `Cancelled`, and `PolicyDenied` describe the run, not the system under test, and must never be silently mapped to a claim verdict.

## 5. Isolation (v0.1)

```text
repository
→ disposable Git worktree
→ Bat execution
→ collect evidence
→ destroy worktree
```

**A Git worktree is not hostile-code containment.** It provides a disposable workspace, easy observation of repository mutations, and rollback for changes contained inside that workspace. It does not stop a process from writing outside the worktree, reading unrelated host files, contacting the network, invoking Git credential helpers, or changing host configuration.

**Rollback is not enforcement.** The reversibility of workspace mutations never makes a capability ENFORCED; enforcement and reversibility are recorded separately ([capability-model.md](capability-model.md) §2).

Per Constitution 19, every receipt must state exactly what isolation was and was not enforced.

Future stronger isolation should reuse external systems — containers, WASI, OpenShell, VMs — rather than building a Terror Bat container runtime.

## 6. Adapter principle

Terror Bat must not need to understand every programming language. Adapters (built-in or external) execute:

```text
cargo test
pytest
go test ./...
npm test
HTTP requests
Git inspection
AI agent tasks
```

Initial external adapter protocol direction: **JSON / JSONL over stdin/stdout**. This is provisional in M0; the protocol is specified no earlier than M8.

Complex behaviour belongs in adapters, never in the Bat Spec (Constitution 16).

## 7. AI boundary

AI roles may eventually include:

- discovering candidate Bats,
- generating Bat Specs,
- advisory semantic judgement (clearly labelled as advisory).

Hard rules:

- AI must not silently promote its own suspicion to a deterministic proof.
- Core Bats run without an API key (Constitution 17).
- AI output entering the pipeline is a *proposal*; only deterministic oracle results set epistemic states.

## 8. Content identity (direction only)

Important artifacts receive content identities (Unison-inspired identity model, not its language):

```text
bat:sha256:...
claim:sha256:...
attack:sha256:...
oracle:sha256:...
evidence:sha256:...
receipt:sha256:...
```

Human names remain labels; content identity is canonical where appropriate (Constitution 12).

**M1 implementation status.** `bat:`, `claim:`, `attack:`, and `oracle:` identities are implemented: each is SHA-256 over RFC 8785 canonical JSON of the resolved semantic component (see [bat-spec-v0.md](bat-spec-v0.md) §13). `environment.relevant` declarations are part of Bat identity; actual machine values are never inspected or hashed. Run, evidence, and receipt identities, and cache lookup, are later milestones.

Conceptual run identity:

```text
hash(
    canonical_bat_definition,
    target_state,
    relevant_environment,
    adapter_versions,
    oracle_definition,
    terrorbat_version
)
```

Binding rules for M1+:

- Canonicalisation must be deterministic (stable field order, stable encoding).
- Secrets must not accidentally become hash inputs that leak information.
- Environment inputs must be *declared*, not indiscriminately hashed from the whole machine.
- Cache hits need explainable provenance; cache misses should eventually explain what changed.
- Content addressing is **not** semantic equivalence. Identical work may be reused only if all declared relevant inputs match (Constitution 13). Terror Bat does not attempt semantic equivalence of arbitrary source code.

## 9. Context economy (architectural requirement)

AI-assisted Terror Bat operations are designed around **progressive disclosure**. Never dump an entire repository into model context by default.

Future logical context handles:

```text
repo:structure
symbol:path::normalize
history:path::normalize
tests:path
dependency:foo
run:71fc
```

An AI receives initially: goal, claim, relevant project map, known evidence, capabilities, available context handles — and requests more as needed (Constitution 18).

Potential future metrics (not implemented in M0):

```text
input tokens
output tokens
objects retrieved
files opened
symbols inspected
tool definitions loaded
model cost
verified findings per context token
```

## 10. v0.1 build roadmap

```text
M0    Constitution + architecture contracts            ← this document set
M1    Bat Spec parsing + canonical identity
M2    OTP-style supervised process execution
M3    Disposable Git worktree execution
M4    Content-addressed evidence store + operation log
M5    Deterministic oracle engine
M6    Human + JSON receipts
M6.5  Reusery Bat Zero

--------------------------
KILL / CONTINUE GATE
--------------------------

M7    Evidence reuse and explainable caching
M8    External adapter protocol
M9    AI discovery/generation
M10   Terror Bat attacks Terror Bat

Later WASI / stronger sandboxes, assurance profiles, distributed Colony
```

## 11. Kill criteria (mandatory)

Terror Bat should be **stopped or substantially redesigned** if Bat Zero shows that:

1. It is merely a complicated shell-script runner.
2. Expressing the Reusery experiment requires large amounts of Terror-Bat-specific code.
3. Receipts are less understandable than the underlying experiment.
4. Language independence requires Terror Bat to deeply understand each language ecosystem.
5. Evidence cannot be made meaningfully more reproducible than ordinary logs.
6. Isolation creates more operational risk than it removes.
7. The framework adds significant context/tooling overhead without producing better assurance.
8. Deterministic proof repeatedly collapses into model judgement.
9. The architecture requires recreating existing mature runtimes, test systems, sandboxes or package managers.
10. Terror Bat becomes a platform before proving a useful Bat.

A good experiment is allowed to kill the project.

## 12. M1 entry criteria

Implementation may begin only when M0 clearly defines:

- Bat identity — §8, plus [bat-spec-v0.md](bat-spec-v0.md)
- claim semantics — §1, bat-spec-v0 §2
- attack semantics — bat-spec-v0 §4
- oracle semantics — bat-spec-v0 §5, receipt-v0 §3
- evidence semantics — receipt-v0 §4
- receipt states — receipt-v0 §2
- capability states — [capability-model.md](capability-model.md) §2
- isolation limitations — §5
- canonicalisation direction — §8
- Bat Zero success criteria — [reusery-bat-zero.md](reusery-bat-zero.md) §3
- kill criteria — §11

Known ambiguities are recorded explicitly rather than hidden:

- The oracle condition grammar is intentionally minimal in v0; its exact form is an M5 decision.
- The adapter protocol is a direction, not a contract (M8).
- Run-identity environment inputs: which facts are "relevant" is per-Bat declared via `environment.relevant` (declaration mechanism resolved by M1 — see [bat-spec-v0.md](bat-spec-v0.md) §12); combining declarations with captured values for run identity is still future work.
- Mapping rules from (execution status, oracle result) to receipt verdicts beyond the hard rules in §4 are specified in receipt-v0 §2 but will need case-law from Bat Zero.

## 13. Non-goals for v0.1

v0.1 does **not** build:

- a programming language
- a distributed scheduler
- a cloud platform
- a GUI
- a web dashboard
- a Colony marketplace
- semantic equivalence for arbitrary code
- a container runtime
- an operating system sandbox
- a package manager
- a giant plugin framework
- a new test runner
- an agent swarm
- certification infrastructure

Future integrations are allowed. Premature infrastructure is not.
