# Receipt v0 (M0)

The receipt is the primary human-facing output of a Terror Bat run. A receipt that cannot be understood is a failed receipt, regardless of what the run proved (Constitution 14).

## 1. What a receipt must answer

Every receipt — JSON and human rendering alike — must answer:

1. **What was claimed?** — the claim text and `claim:sha256:...`.
2. **What attack was attempted?** — the Bat identity, resolved parameters, and attack summary.
3. **What actually happened?** — the execution outcome and step-by-step operation log references.
4. **How was it judged?** — the oracle definition identity and which conditions matched or did not.
5. **What evidence exists?** — immutable evidence references (§4).
6. **What environment mattered?** — the *declared* relevant environment inputs, not a full machine dump.
7. **What capabilities were requested?** — the `requires` / `forbids` declarations.
8. **Which capabilities were actually enforced?** — per-capability state (ENFORCED vs UNENFORCED vs DENIED), honestly (capability-model.md §2).
9. **What limitations remain?** — isolation guarantees and non-guarantees, known gaps, deferred judgements.
10. **How can the run be reproduced?** — run identity, target state (commit/worktree base), adapter versions, Terror Bat version.

## 2. Epistemic states

A receipt carries exactly one epistemic verdict about the claim:

```text
PROVEN

Deterministic evidence demonstrates that the tested claim
was falsified under the stated conditions.


REPRODUCED

The suspicious behaviour was reproduced, but available
machinery cannot fully prove the interpretation.


SUSPECTED

Evidence suggests a problem but is insufficient to reproduce
or prove it.


NOT OBSERVED

The attempted attack did not expose the claimed failure.


INCONCLUSIVE

The experiment completed without enough information to
support or reject the claim.


INVALID

The Bat or experiment was malformed.


INFRASTRUCTURE ERROR

Terror Bat or an external dependency failed in a way that
invalidated the experiment.
```

This is **not** PASS/FAIL and must never be reduced to it. `NOT OBSERVED` is not "the system is correct"; it is "this attack did not falsify the claim". Terror Bat never claims certification or trust status (Constitution 10).

### Mapping from execution outcomes

The execution outcome vocabulary (architecture.md §4) has eleven values; the verdict set above has seven. Mapping rules:

| Execution outcome | Verdict |
|---|---|
| `Proven` | `PROVEN` |
| `Reproduced` | `REPRODUCED` |
| `Suspected` | `SUSPECTED` |
| `NotObserved` | `NOT OBSERVED` |
| `Invalid` | `INVALID` |
| `InfrastructureError` | `INFRASTRUCTURE ERROR` |
| `TimedOut`, `Crashed`, `Cancelled`, `PolicyDenied` | `INCONCLUSIVE` **by default** |
| `Inconclusive` | `INCONCLUSIVE` |

Hard rules:

- A crash is **not** automatically evidence that the claim failed. `Crashed` yields `INCONCLUSIVE` unless a deterministic oracle explicitly interprets the crash artifacts against the claim and records that interpretation.
- `TimedOut`, `Cancelled`, and `PolicyDenied` describe the run, not the system under test. The receipt must say which.
- `SUSPECTED` verdicts must record *why* deterministic machinery could not go further (missing oracle condition, advisory-only model judgement, etc.). Model judgement alone can never produce `PROVEN`.

## 3. Judgement record

The receipt records how the verdict was reached:

- `oracle:sha256:...` — identity of the oracle definition used.
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
  "execution_outcome": "Proven",
  "verdict": "PROVEN",
  "judgement": { "oracle_sha": "oracle:sha256:...", "conditions": [] },
  "evidence": [ { "kind": "git_diff", "ref": "evidence:sha256:..." } ],
  "environment": { "declared_inputs": {} },
  "capabilities": { "requested": [], "enforced": [], "unenforced": [], "denied": [] },
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
Attack: introduced base64 0.22 into a project already vendoring
        base64 encoding; built; inspected dependency resolution.
Result: shadow dependency accepted without reuse check.
        oracle: 2/2 conditions matched (file_contains, dependency_exists)
Evidence: git_diff 9f2c… · stdout 41ab… · stderr c77d… · log 0be3…
Isolation: disposable git worktree (NOT hostile-code containment)
May:     read repo, write fixture/**, spawn processes
May not: network (unenforced — advisory), git push (denied)
Reproduce: run 71fc @ commit e4d2…, adapters {cargo 0.3, git 0.2}, tb 0.1.0
Limits: crash-free run; oracle covers declaration only, not intent.
```

## 7. Known open points (deferred, not hidden)

- Exact `receipt.json` schema and field-level canonicalisation → M6.
- Whether advisory AI judgements get their own evidence kind or a `judgement.advisory` sub-record → M6, informed by M9.
- Rendering format (Markdown vs terminal) → M6; both may share one data source.
