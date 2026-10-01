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

environment: { ... }         # optional relevant-environment declarations (§12)

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

Scoped capability entries may be written either as a single-key YAML mapping or as a quoted string, because M0 writes them unquoted:

```yaml
requires:
  - fs.read
  - fs.write: fixture/**        # YAML mapping form: {fs.write: fixture/**}
  - "fs.read: repo/**"          # quoted string form
```

Both forms normalize to the same canonical string (`name: scope`), so the choice never affects Bat identity. A scoped entry with a non-string scope, or a mapping with more than one key, is rejected.

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
- Until then the oracle is parsed as an **opaque structured mapping**: M1 validates that it is a mapping (so `$param` resolution and canonicalisation work uniformly) but does not interpret its contents.
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

Bat Spec v1 accepts timeout values in one strict representation:

```text
<unsigned integer>s
```

Examples: `30s`, `60s`, `600s`, `900s`. Ambiguous aliases (`10m`, `0.5h`, `600000ms`) are rejected, as are bare numbers and non-string values. Internally, timeouts canonicalise to integer seconds, so `60s` and `060s` are the same semantic value and share identity.

```yaml
timeout:
  setup: 60s
  run: 600s
  oracle: 30s
  total: 900s
```

All four keys are optional; absent keys (or an absent `timeout` section) simply contribute nothing to the semantic projection — no hidden defaults are invented.

The supervisor owns enforcement: on timeout the run is cancelled, the execution status is `TimedOut`, and whatever evidence was already captured is preserved. Execution status describes the run, not the system under test (architecture.md §4).

## 8. Parameters

Parameters are explicit typed values, resolved **before** canonicalisation. They are not a template language: no expressions, no environment-variable interpolation, no `${foo}` substitution inside strings.

Declared types: `string`, `bool`, `integer` (no floats).

Integers — declared defaults, CLI overrides, and resolved values alike — must fit the JCS-safe range `−9007199254740991..=9007199254740991` (±(2⁵³ − 1)), enforced through the same shared boundary as all other spec integers (see §13). Out-of-range values are rejected before canonicalisation.

```yaml
params:
  helper_fixture:
    type: string
    default: fixtures/existing_base64_helper.rs
  parallelism:
    type: integer
    required: true
```

A parameter reference is an explicit data node — a mapping containing exactly one key, `$param`:

```yaml
attack:
  setup:
    - adapter: fs
      action: write
      path: fixture/project/src/encoding.rs
      from:
        $param: helper_fixture
```

References may appear anywhere in the Spec's data. Resolution rules:

```text
CLI override (--param name=value)
    ↓
declared default
    ↓
missing required parameter = ERROR
```

Rejected: undeclared overrides, duplicate overrides, type mismatches (override or default vs declared type), missing required values, `$param` references to undeclared names, and `$param` mappings containing additional keys.

After resolution, references are replaced by their values and the `params` declaration block itself is **excluded from Bat identity**: two source Specs that resolve to the same experiment share an identity even if one value came from a default and the other from an explicit override. That is intentional.

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
- Adapter action naming conventions → M8.
- ~~`params` substitution pre- vs post-canonicalisation~~ — resolved by M1: substitution happens **pre**-canonicalisation, the `params` block is excluded from identity, and the behaviour is covered by identity tests.
- ~~Timeout value representation~~ — resolved by M1: strict `<unsigned integer>s`, canonicalised to integer seconds (§7).

## 12. Environment relevance declarations (M1)

An optional declarative section names the environment facts that may matter to reproducibility:

```yaml
environment:
  relevant:
    - os.name
    - os.arch
    - rustc.version
```

These are symbolic names only. M1 does **not** inspect the machine and does **not** capture their values: the declaration states *these facts may matter*. Rules:

- `environment.relevant` is optional; entries are strings, sorted/deduplicated during semantic normalisation.
- Actual values are **not** part of Bat identity; the declarations **are**.
- Future run identity will combine these declarations with actual captured values. No secret values belong here.

## 13. Semantic projection and content identity (M1)

Before hashing, the resolved Bat is converted to a semantic projection:

Included: `version`, `claim`, `requires`, `forbids`, `environment.relevant`, `attack`, `oracle`, `evidence`, `timeout`.

Excluded: `id`, `meta`, `params` declarations, source filename, comments, YAML formatting.

Set-like collections (`requires`, `forbids`, `environment.relevant`, `evidence.capture`) are sorted and deduplicated; order is preserved where it can be meaningful (`attack.setup`, `attack.run`, oracle child arrays). No logical-equivalence reasoning is attempted on oracle expressions.

Source-level strictness: Bat Specs are JSON-shaped structured data, so **all YAML mapping keys must be strings** — at the top level and inside opaque adapter payloads, oracle structures, and `meta`. Non-string keys (`1:`, `true:`, `null:`, compound keys) are rejected rather than silently stringified, so `1: value` can never be identical to `"1": value`. Note the parser resolves unquoted scalars with YAML 1.1-style rules: unquoted `y`, `n`, `yes`, `no`, `on`, `off` as keys resolve to booleans and are therefore rejected — quote them (`"on": ...`) to use them as strings.

### Integers

Exact integers in semantic data:

```text
-9007199254740991..=9007199254740991
```

Larger exact values must be strings. This applies everywhere integers can reach the parsed document: opaque adapter payloads, oracle structures, nested mappings and sequences, timeout values, and parameter defaults/overrides/resolved values. (The top-level `params` declaration block is checked by parameter validation with identical bounds and parameter-aware errors; the boundary itself is defined once, in one shared location.)

### Floating-point values

Finite IEEE-754/JCS values may exist in opaque structured payloads. `NaN` and infinities are rejected (by the YAML parser itself).

Opaque numeric data follows JCS / IEEE-754 semantics. Exact integers larger than ±(2^53−1) must be represented as strings.

### Parameters

Remain:

```text
string
bool
integer
```

No float parameter type.

The projection is serialised to **RFC 8785 canonical JSON** (inspectable via `terrorbat spec canonical`) and hashed with **SHA-256**, producing identities of the form `<kind>:sha256:<lowercase hex>`:

```text
bat:sha256:...
claim:sha256:...
attack:sha256:...
oracle:sha256:...
```

Each sub-identity uses the same canonicalisation rules over the corresponding resolved semantic component. Canonical JSON bytes are part of the M1 contract and are pinned by golden-vector tests; changing the canonical form later requires an explicit identity-version decision.
