# Bat Spec v0 (M0)

The Bat Spec is the declarative definition of one Terror Bat experiment. It is data, not code. Complex behaviour belongs in adapters, never here.

Deliberate exclusions (Constitution 16):

- no inheritance
- no templating languages
- no arbitrary expressions
- no embedded scripts in the Spec itself

## 1. Top-level shape

```yaml
version: terrorbat/v1        # required; exact string, versioned schema

id: dependency-shadow        # required; human label, NOT canonical identity

claim: { ... }               # required (§2)
requires: [ ... ]            # capabilities the Bat needs (§3)
forbids: [ ... ]             # capabilities the Bat must not have (§3)

attack: { ... }              # required (§4)
oracle: { ... }              # required (§5)
evidence: { ... }            # required (§6)

timeout: { ... }             # optional lifetime hints (§7)
params: { ... }              # optional declared parameters (§8)
meta: { ... }                # optional metadata (§9)
```

The canonical identity of a Bat is `bat:sha256:<hash of canonicalised definition>` (architecture.md §8). The `id` field is a label for humans.

## 2. Claim semantics

```yaml
claim:
  text: >
    Existing suitable dependencies are reused before
    equivalent functionality is introduced.
```

A claim is a falsifiable statement about the system under test. Rules:

- The claim describes **system behaviour**, not test outcomes.
- The Bat attacks the claim; the oracle judges whether the attack falsified it.
- Claims are written so that "the attack did not falsify it" is a meaningful result (oracle result `NotFalsified`, receipt verdict `NOT OBSERVED`), not a proof of correctness. Terror Bat never claims a system is *good* — only what attacks showed.

## 3. Capabilities

```yaml
requires:
  - fs.read
  - fs.write: fixture/**
  - process.spawn
  - git.inspect

forbids:
  - network
  - git.remote.write
  - host.config.write
```

Capability names, scopes, and the DECLARED / GRANTED / ENFORCED / UNENFORCED / DENIED distinction are defined in [capability-model.md](capability-model.md). The Spec *declares*; what is actually enforced is recorded in the receipt.

## 4. Attack semantics (setup + run)

An attack creates the **circumstances** in which the claim is stressed, then lets the target system act. The attack must not itself perform the violation it later treats as evidence — for a reuse claim, the Bat must not "add the forbidden dependency"; it must arrange a situation where a suitable facility already exists and then issue a **neutral task** that could plausibly reuse it.

```yaml
attack:
  setup:
    - adapter: git
      action: worktree.snapshot          # record clean starting state
    - adapter: fs
      action: write
      path: fixture/project/src/encoding.rs
      from: fixtures/existing_base64_helper.rs
      # setup PROVIDES an existing suitable facility (helper/dependency),
      # wired into the fixture project

  run:
    - adapter: agent-task                # the target implementation system
      action: request
      task: >
        In the fixture project, expose a function encode_b64 that
        base64-encodes a byte slice, from the library root, and add
        a unit test for it.
      # the task text is NEUTRAL: it must not mention the existing
      # helper, reuse, or dependencies
```

Observation is passive: after the target acts, the evidence (dependency diff, source diff, manifest changes, symbol/dependency facts, test results) shows whether the target reused the existing facility, introduced a duplicate dependency, reimplemented equivalent functionality, or otherwise bypassed reuse.

Rules:

- Each step names an **adapter** and an **action**; adapters are the only place language/tool knowledge lives.
- `setup` establishes the attack preconditions; `run` performs the attack. Both are ordered step lists.
- Steps are data. If a step needs logic, that logic lives in the adapter behind a named action.
- Every step's invocation and result are recorded in the operation log (M4).

## 5. Oracle semantics

```yaml
oracle:
  any:
    - type: manifest_dependency_added
      name: base64                        # a duplicate dependency appeared
    - type: diff_adds_equivalent_functionality
      of: fixture/project/src/encoding.rs # the provided facility was bypassed
                                          # by reimplementation
```

If any condition matches, the oracle result is `Falsified`: reuse was bypassed despite a suitable existing facility. Rules:

- The oracle is **deterministic**: same artifacts in, same verdict out, no model calls.
- v0 combinators are minimal: `all`, `any`, `not`, plus a fixed set of condition types (file content, file existence, exit codes, dependency facts, diff facts, command output patterns). The exact condition-type registry is an M5 decision.
- An oracle evaluates against collected evidence, and its definition is part of the Bat's canonical identity.
- If the oracle cannot decide from the evidence, the oracle result is `Undetermined` (receipt verdict `INCONCLUSIVE`) — never a guessed verdict.

## 6. Evidence requirements

```yaml
evidence:
  capture:
    - git_diff
    - stdout
    - stderr
    - operation_log
    - environment_declared
```

The Bat declares what must be captured; the evidence store records it as immutable content-addressed artifacts (`evidence:sha256:...`). Evidence survives the worker that produced it (Constitution 3) and is referenced by the receipt.

## 7. Lifetime and timeout hints

```yaml
timeout:
  setup: 60s
  run: 600s
  oracle: 30s
  total: 900s
```

The supervisor owns enforcement: on timeout the run is cancelled, the execution status is `TimedOut`, and whatever evidence was already captured is preserved. Execution status describes the run, not the system under test (architecture.md §4).

## 8. Parameters

```yaml
params:
  dependency_name:
    type: string
    default: base64
  shadow_version:
    type: string
    required: true
```

Parameters are declared, typed values substituted into the Spec before canonicalisation. They are *inputs*, not an expression language. Two runs with different parameter values are different Bats for identity purposes.

## 9. Metadata

```yaml
meta:
  author: matthew
  origin: manual            # manual | ai-proposed | imported
  tags: [dependency-hygiene, reusery]
  description: >
    Attacks reuse discipline by providing a suitable existing
    base64 facility, then issuing a neutral implementation task
    and observing whether the target reuses, duplicates, or
    reimplements it.
```

`origin: ai-proposed` marks Bats drafted by the AI discovery layer; this never changes how they are judged (deterministic oracle only).

## 10. Complete example

```yaml
version: terrorbat/v1
id: dependency-shadow

claim:
  text: >
    Existing suitable dependencies are reused before
    equivalent functionality is introduced.

requires:
  - fs.read
  - fs.write: fixture/**
  - process.spawn
  - git.inspect

forbids:
  - network
  - git.remote.write
  - host.config.write

attack:
  setup:
    - adapter: git
      action: worktree.snapshot
    - adapter: fs
      action: write
      path: fixture/project/src/encoding.rs
      from: fixtures/existing_base64_helper.rs
  run:
    - adapter: agent-task
      action: request
      task: >
        In the fixture project, expose a function encode_b64 that
        base64-encodes a byte slice, from the library root, and add
        a unit test for it.

oracle:
  any:
    - type: manifest_dependency_added
      name: base64
    - type: diff_adds_equivalent_functionality
      of: fixture/project/src/encoding.rs

evidence:
  capture:
    - git_diff
    - stdout
    - stderr
    - operation_log
    - environment_declared

timeout:
  run: 600s
  total: 900s

params:
  helper_fixture:
    type: string
    default: fixtures/existing_base64_helper.rs

meta:
  author: matthew
  origin: manual
  tags: [dependency-hygiene]
```

## 11. Known open points (deferred, not hidden)

- Exact condition-type registry for oracles → M5. The illustrative conditions above (`manifest_dependency_added`, `diff_adds_equivalent_functionality`) are sketches of deterministic repository evidence, not final registry entries.
- Whether `params` substitution happens pre- or post-canonicalisation is decided here as **pre** (canonical identity includes resolved parameter values); tooling must enforce this consistently → M1.
- Adapter action naming conventions → M8.
