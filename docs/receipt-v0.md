# Receipt v0 (M0)

The receipt is the primary human-facing output of a Terror Bats run. A receipt that cannot be understood is a failed receipt, regardless of what the run proved (Constitution 14).

## 1. What a receipt must answer

Every receipt — JSON and human rendering alike — must answer:

1. **What was claimed?** — the claim text and `claim:sha256:...`.
2. **What attack was attempted?** — the Bat identity, resolved parameters, and attack summary.
3. **What actually happened?** — the execution status and step-by-step operation log references.
4. **How was it judged?** — the oracle definition identity, the oracle result, and which conditions matched or did not.
5. **What evidence exists?** — immutable evidence references (§4).
6. **What environment mattered?** — the *declared* relevant environment inputs, not a full machine dump.
7. **What capabilities were requested?** — the `requires` / `forbids` declarations.
8. **Which capabilities were actually enforced?** — per-capability state (ENFORCED vs UNENFORCED vs DENIED), honestly, with reversibility/containment properties recorded separately from enforcement (capability-model.md §2).
9. **What limitations remain?** — isolation guarantees and non-guarantees, known gaps, deferred judgements.
10. **How can the run be reproduced?** — run identity, target state (commit/worktree base), adapter versions, Terror Bats version.

## 2. Epistemic states

A run produces three distinct reports (architecture.md §4): the **execution status** (what mechanically happened), the **oracle result** (`Falsified` / `NotFalsified` / `Undetermined`), and the receipt's **epistemic verdict** (what may responsibly be claimed).

### Finding maturity

Findings progress through an epistemic lifecycle — not a process-execution status:

```text
SUSPECTED
A plausible problem has been identified but not reproducibly established.

REPRODUCED
The relevant behaviour has been recreated under stated conditions,
but deterministic evidence is still insufficient to establish the
claim violation.

PROVEN
Deterministic evidence establishes that the tested claim was
falsified under the stated conditions.
```

### Receipt verdicts

A receipt carries exactly one epistemic verdict about the claim:

```text
PROVEN          — finding maturity PROVEN: deterministic evidence establishes
                  the claim was falsified under the stated conditions.

REPRODUCED      — finding maturity REPRODUCED.

SUSPECTED       — finding maturity SUSPECTED.

NOT OBSERVED    — the attempted attack did not falsify the claim
                  (oracle result NotFalsified).

INCONCLUSIVE    — the experiment completed without enough information to
                  support or reject the claim (oracle result Undetermined).

INVALID         — the Bat or experiment was malformed.

INFRASTRUCTURE ERROR — Terror Bats or an external dependency failed in a way
                  that invalidated the experiment.
```

This is **not** PASS/FAIL and must never be reduced to it. `NOT OBSERVED` is not "the system is correct"; it is "this attack did not falsify the claim". Terror Bats never claims certification or trust status (Constitution 10).

### Mapping from execution status and oracle result

| Execution status | Oracle result | Receipt verdict |
|---|---|---|
| `Completed` | `Falsified` | `PROVEN` |
| `Completed` | `NotFalsified` | `NOT OBSERVED` |
| `Completed` | `Undetermined` | `INCONCLUSIVE`, or `REPRODUCED` / `SUSPECTED` where reproduction evidence exists but deterministic proof does not |
| `TimedOut`, `Crashed`, `Cancelled`, `PolicyDenied` | `Undetermined` (or unobtainable) | `INCONCLUSIVE` **by default** |
| `Invalid` | — | `INVALID` |
| `InfrastructureError` | — | `INFRASTRUCTURE ERROR` |

Hard rules:

- A crash does not prove claim failure. `Crashed` yields `INCONCLUSIVE` unless a deterministic oracle explicitly interprets the preserved crash artifacts against the claim and records that interpretation.
- A timeout does not prove claim failure; neither does a policy denial. These describe the run, not the system under test, and the receipt must say which.
- Model judgement alone can never produce `PROVEN`; `PROVEN` requires oracle result `Falsified` from deterministic machinery.
- `SUSPECTED` and `REPRODUCED` verdicts must record *why* deterministic machinery could not go further (missing oracle condition, advisory-only model judgement, etc.).
- `NOT OBSERVED` is not certification or correctness.

## 3. Judgement record

The receipt records how the verdict was reached:

- `oracle:sha256:...` — identity of the oracle definition used.
- The oracle result (`Falsified` / `NotFalsified` / `Undetermined`).
- Each oracle condition, its inputs (evidence references), and its individual result.
- Whether any advisory AI judgement was consulted, clearly separated from the deterministic result and never allowed to change it.

## 4. Evidence references

Evidence is immutable and content-addressed:

```text
evidence:sha256:9f2c...   git_diff
evidence:sha256:41ab...   stdout (run step 2)
evidence:sha256:c77d...   stderr (run step 2)
evidence:sha256:0be3...   operation_log
evidence:sha256:aa19...   environment_declared
```

The receipt references evidence by hash; it does not need to embed it. Evidence outlives the process that produced it (Constitution 3). Reuse of cached evidence is permitted only when all declared relevant inputs match (Constitution 13), and a cache hit must show explainable provenance: which earlier run produced it, under what run identity.

## 5. `receipt.json`

Conceptual shape (exact schema fixed at M6):

```json
{
  "version": "terrorbat/receipt/v0",
  "receipt_id": "receipt:sha256:...",
  "run_id": "...",
  "bat": { "id": "dependency-shadow", "bat_sha": "bat:sha256:...", "params": {} },
  "claim": { "text": "...", "claim_sha": "claim:sha256:..." },
  "attack_summary": "...",
  "execution_status": "Completed",
  "oracle": { "oracle_sha": "oracle:sha256:...", "result": "Falsified", "conditions": [] },
  "verdict": "PROVEN",
  "judgement": { "advisory_consulted": false },
  "evidence": [ { "kind": "git_diff", "ref": "evidence:sha256:..." } ],
  "environment": { "declared_inputs": {} },
  "capabilities": { "requested": [], "enforced": [], "unenforced": [], "denied": [], "containment": [] },
  "isolation": { "mode": "git-worktree", "guarantees": [], "non_guarantees": [] },
  "limitations": [],
  "reproduction": { "target_state": "...", "adapter_versions": {}, "terrorbat_version": "..." }
}
```

The receipt itself receives `receipt:sha256:...` over its canonicalised content.

## 6. Human rendering

Every receipt gets a human-readable rendering generated from the same data. Sketch:

```text
TERROR BAT RECEIPT — dependency-shadow            verdict: PROVEN
Claim:  Existing suitable dependencies are reused before
        equivalent functionality is introduced.
Attack: provided a suitable existing base64 helper, then issued a neutral
        implementation task to the target system; observed what it built.
Run:    completed · oracle: Falsified (2/2 conditions matched:
        manifest_dependency_added, diff_adds_equivalent_functionality)
Result: target bypassed the existing facility instead of reusing it.
Evidence: git_diff 9f2c… · stdout 41ab… · stderr c77d… · log 0be3…
Isolation: disposable git worktree (NOT hostile-code containment;
        workspace mutations reversible, host/network NOT contained)
May:     read repo, write fixture/**, spawn processes
May not: network (advisory — not enforced), git push (advisory — not enforced)
Reproduce: run 71fc @ commit e4d2…, adapters {agent-task 0.1, git 0.2}, tb 0.1.0
Limits: crash-free run; oracle covers observable repository effects only,
        not the target's intent.
```

## 7. Known open points (deferred, not hidden)

- Exact `receipt.json` schema and field-level canonicalisation → M6.
- Whether advisory AI judgements get their own evidence kind or a `judgement.advisory` sub-record → M6, informed by M9.
- Rendering format (Markdown vs terminal) → M6; both may share one data source.
