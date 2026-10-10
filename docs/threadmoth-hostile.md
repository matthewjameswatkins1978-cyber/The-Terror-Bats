# Threadmoth hostile pack (`terrorbat-adapter/v1` + `packs/threadmoth-hostile.yaml`)

A thin production adapter (`adapters/threadmoth/adapter.py`) drives the **real**
Threadmoth 1.10.0 CLI, plus fifteen deliberately aggressive Bats
(`bats/threadmoth/*.yaml`) that attack Threadmoth's core promise:

> previewed, authorised edits land exactly as certified, within declared
> boundaries, or Threadmoth refuses honestly without modifying anything.

Findings are captured, never repaired here.

## Adapter

`adapters/threadmoth/adapter.py` speaks `terrorbat-adapter/v1` (`describe` /
`execute`, one JSON line on stdin/stdout, OS exit 0). Actions:

```text
capabilities, preview, mutate, plan, apply_plan, transact_preview, transact
```

(`recover_inspect` is omitted: no Bat needs it, and only needed actions are
exposed.) The adapter validates its Terror Bats payload strictly (unknown or
irrelevant fields are `invalid`, exit 2; worktree escapes are `invalid`;
missing executables and non-JSON Threadmoth output are
`infrastructure_error`), builds the real Threadmoth CLI invocation, parses the
certificate/plan JSON verbatim, optionally stores it at `output_path`, and
returns it as logical stdout with Threadmoth's own exit code
(0 applied/no-change, 2 refused, 3 runtime failure). Refusal reason codes are
preserved. The adapter never judges the claim; the deterministic oracle does.

`--threadmoth` selects the Threadmoth executable (default: `threadmoth` on
PATH). `capabilities` takes no input path; every other action takes exactly
its relevant `request_path` (`preview`, `mutate`, `plan`, `transact_preview`,
`transact`) or `plan_path` (`apply_plan`), plus an optional `output_path`
artifact inside the worktree.

## Fixtures and oracles

`adapters/threadmoth/fixtures.py` prepares disposable fixtures, performs the
intervening attack step, and independently measures the outcome (`sha256`,
file bytes, exit codes, certificate values). Each Bat's oracle asserts the
checker's `violation` flag is `true` — i.e. the Bat is FALSIFIED only when
Threadmoth actually breaks the invariant. `NOT OBSERVED` (no violation) is a
legitimate result, never a defect.

`tests/threadmoth_adapter.py` (Python unittest) covers the adapter contract
against the real CLI plus synthetic oracle-direction controls (a tampered
certificate must flip the verdict to PROVEN; a missing artifact must yield
INCONCLUSIVE), proving the checker is sensitive without ever mistaking a
synthetic defect for a Threadmoth finding.

## Running

```powershell
# validate the pack and every effective Bat
terrorbats pack check packs/threadmoth-hostile.yaml

# run the fifteen hostile Bats against a disposable target
Copy-Item examples/threadmoth/adapters.yaml $env:TEMP\tm-adapters.yaml
# (edit the two absolute paths inside first)
terrorbats pack run packs/threadmoth-hostile.yaml --repo D:\CleanTarget `
    --adapters $env:TEMP\tm-adapters.yaml

# repeat the four deterministic attacks ten times (forty ordinary children)
terrorbats pack run packs/threadmoth-stress.yaml --repo D:\CleanTarget `
    --adapters $env:TEMP\tm-adapters.yaml --runs 10

# single Bat / replay (receipts are first-class ordinary runs)
terrorbats run bats/threadmoth/stale-preview.yaml --repo D:\CleanTarget `
    --adapters $env:TEMP\tm-adapters.yaml --json
terrorbats replay <receipt> --adapters $env:TEMP\tm-adapters.yaml
```

The hostile pack operates against disposable fixture worktrees only. The
Threadmoth source checkout is never modified by these Bats.

## Bat inventory

| Bat | Attacks |
| --- | --- |
| `stale-preview` | mutate with a stale `pre_hash` must refuse (`STALE_IDENTITY`), no relocation |
| `stale-plan` | applying a plan after its source changed must refuse (`PLAN_STALE`) |
| `tampered-plan` | plan content edited without legitimate identity must refuse (`PLAN_INVALID`) |
| `path-prefix-escape` | target outside `allowed_path_prefixes`, incl. `..` lexical variants, must refuse |
| `junction-symlink-escape` | edit through an in-tree link pointing outside must refuse or behave as documented; unsupported setup is INCONCLUSIVE, never PROVEN |
| `ambiguous-exact-match` | repeated identical text with `exactly_one` must not pick one (`TARGET_AMBIGUOUS`) |
| `candidate-guard-staleness` | stale candidate reuse must refuse, not relocate (`STALE_IDENTITY` / `CANDIDATE_SELECTION_INVALID`) |
| `hard-budget-overrun` | authorising fewer bytes than required must refuse without silent enlargement |
| `crlf-preservation` | CRLF input: bounded structured edit, no whole-file line-ending rewrite |
| `unicode-bom` | UTF-8/BOM files: exact post-byte hash, preservation facts, no lossy transcoding |
| `transaction-rollback` | late-member failure must leave all members' bytes untouched (atomicity) |
| `certificate-byte-truth` | certificate `post_hash` must equal the independently hashed bytes; changed ranges must cover every changed byte |
| `replayed-identical-request` | identical safe request again must be `NO_CHANGE`, never a second mutation |
| `unknown-fields-strictness` | unknown fields at request/operation/budget depths must be rejected |
| `malformed-structured-no-fallback` | unsatisfiable structured request must not silently downgrade to text mutation |
