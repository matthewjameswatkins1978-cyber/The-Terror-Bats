# Lantern under attack

`terrorbat_lantern.py` is a one-shot `terrorbat-adapter/v1` adapter, named `lantern`.
It invokes the actual Lantern ledger repository through a narrowly scoped test
binary, the actual application bridge contract binary, and the existing
`lantern_git_export.py` exporter against a bounded loopback HTTP fixture.
It does not contact a production Lantern service or a remote GitHub repository.
Python 3 and Git must be installed. All adapter commands and arguments in the
bindings file should use absolute paths. Invoke Python with `-B` so importing
the target exporter does not leave bytecode files in its source tree.

## Target and fixture

Product baseline: `3afaeaa09a108848f6fb6625077598c7380ba777` (`master`).
The separately committed Lantern fixture adds only factual test exports:
`crates/lighting-store-surreal/tests/hostile_ledger.rs` and
"bridge_github::tests::hostile_authority_history" and a restarted offline
runner history fixture. The existing crash seam test now drops and reloads its
real bridge state before replay. These changes touch tests only.
Use a clean disposable checkout of the fixture branch. Build there:

```powershell
cargo test -p lighting-store-surreal --test hostile_ledger --no-run --message-format=json
cargo test -p lighting --bin lighting --no-run --message-format=json
```

Obtain the exact test executables from Cargo's `compiler-artifact` JSON entries.
Bind the fixture HEAD and its product ancestor explicitly. The adapter rejects
any Terror Bat `target_commit` that differs from the configured fixture HEAD.
Its ordinary logical output identifies both revisions, target executable/script
SHA-256, the operation, and execution ID. The adapter/checker source digest is part of its description identity, so changed checker semantics invalidate replay and Campaign expectations. Raw histories are preserved as ordinary
step stdout evidence and child receipts in the selected store.

## Actions and checker

| Action | Actual surface | Payload |
|---|---|---|
| `ledger_history` | `SurrealLedgerRepository::ingest/list`, embedded SurrealKV | `scenario: race` or `sequential`, `writers: 1..32` |
| `bridge_contract` | allowlisted actual `lighting` bridge tests | named scenario in `CONTRACTS` |
| `mirror_probe` | actual exporter plus temporary Git mirror | `scenario: normal`, `crlf`, or `atomic` |

Capabilities are explicit: `process.spawn`, `fs.read`, `fs.write`, `git.inspect`,
and `network` (bounded loopback fixture traffic). Each Bat declares setup/run/total
budgets; inner fixtures also have bounded subprocess waits. Fixture databases,
Git repositories, bridge state, HTTP servers and identities are disposable.

The ledger fixture launches N barrier-synchronised real repository calls,
records every Stored/Duplicate/Error outcome, drops every storage handle,
reopens the same file-backed store, and independently lists durable records.
The checker requires a complete history and close/reopen evidence. It checks
exactly one Stored winner, one durable row, equivalent duplicate records, and
zero competitor errors. Seven sensitivity controls distinguish violated
invariants from incomplete or malformed mechanical evidence. Those synthetic
controls never become Lantern findings.

The oracle is an ordinary `json_value_equals` detector on `/checks/violation`.
The adapter never emits a Terror Bat verdict. Unsupported setup, missing test
filters, crashed fixtures, timeouts, missing histories and failed legacy unit
tests are returned as `infrastructure_error`, not healthy observations.

The factual authority fixture counts the mutation endpoint independently:
ALLOW is the positive control; ASK, DENY and unreachable authority each must add
zero calls. Legacy bridge tests preserve raw assertion output. Their passing
result establishes only the exact contract asserted by that test, not broader
production storage or transport promises.

## Inventory

Fifteen Bats cover two and sixteen simultaneous same-key writers,
sequential replay with close/reopen, rewritten ancestry, forbidden committed
intent edits, oldest-first order, authority denial/no-write, observed hash
conflict, crash-seam replay and tampered payload, checkpoint reload, applied
state reload, mirror exact byte hashes, CRLF checkout hostility, and failed
staged export preserving the old mirror.

`packs/lantern-hostile.yaml` contains thirteen fast contracts.
`packs/lantern-stress.yaml` contains fifty explicit serial entries: 25 two-writer
races, 10 sixteen-writer races, 10 checkpoint reloads and 5 mirror checks. Pack
concurrency remains serial; each race itself launches target-level concurrency.
The manifest intentionally repeats entries because Pack v1 has no per-entry
iteration field. `--runs` can repeat the complete bounded manifest.

Ledger same-key/different-payload is not treated as a new conflict promise:
Lantern's ledger contract explicitly returns the existing event for an existing
key. Modified bridge intents are covered by the real tampered-replay contract.

## Execution

Copy the example bindings, replace all placeholder paths/revisions, then:

```powershell
terrorbats run bats/lantern/same-key-race.yaml --repo TARGET --adapters adapters.yaml --store EVIDENCE --json
terrorbats replay RECEIPT_ID --adapters adapters.yaml --store EVIDENCE --json
terrorbats pack run packs/lantern-hostile.yaml --repo TARGET --adapters adapters.yaml --store EVIDENCE --json
terrorbats pack run packs/lantern-stress.yaml --repo TARGET --adapters adapters.yaml --store EVIDENCE --json
python -B -m unittest discover -s adapters/lantern -p test_checker.py -v
```

Retain EVIDENCE outside the disposable target and source worktrees. A replay
runs a new race; it cannot promise the identical interleaving or verdict.

## Deliberate limits

The actual BridgeRunner fixture commits three intents, uses an owned local bare
Git remote (push disabled), persists its checkpoint, destroys each runner,
restarts twice, and checks zero repeated mutation calls plus complete receipts,
state reload, oldest-first mutation order and status. It performs no cloud Git
operation. Crash replay drops/reopens the real bridge state before re-execution.
The observed-hash contract uses a mock HTTP memory response and does not qualify
real-store durable no-mutation. That stronger storage assertion remains
inconclusive; passing the conflict receipt test does not broaden its meaning.

The headless Windows runtime is introduced by open PR #12, not this product
baseline; an owned-child interruption Bat is excluded until that exact runtime
revision is selected. Open PR #12 (`700eef220b47377de9f1a69287610cf74281516c`)
changes bridge durable idempotency and SurrealDB to stable 3.3.0. Open PR #11
(`e4b28083980bffd26ad4c1d0391ba497fe33111e`) changes public demo deployment.
Neither PR revision inherits conclusions from these baseline experiments.
No product repairs, cloud deployment, Nebius access, production memory stores,
or live GitHub bridge writes are part of this package.