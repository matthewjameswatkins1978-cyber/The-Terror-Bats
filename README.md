# The Terror Bats Framework

Language-agnostic adversarial falsification with deterministic evidence.

> What could still be wrong while all the ordinary tests are green?

Windows • Linux • Rust • No AI required for execution

## Install

```powershell
cargo build --release --bin terrorbats
.\install\install.ps1 -InstallDir "$HOME\bin"
```

## First Bat

```text
terrorbats run bats/command-exit.yaml --repo D:\some-project
```

On a healthy machine: `Completed` / `NOT OBSERVED` — the attack did not
falsify the claim (not a correctness certificate).

## Receipt example

```text
terrorbats run bats/command-exit.yaml --repo D:\some-project --json
terrorbats replay <receipt:sha256:...> --store <path>
```

Every run ends in one verdict: `PROVEN`, `REPRODUCED`, `SUSPECTED`,
`NOT OBSERVED`, `INCONCLUSIVE`, `INVALID`, or `INFRASTRUCTURE ERROR`.
The receipt is immutable; replay repeats the attack with fresh OS
identities.

## Manual

The canonical guide: [`docs/MANUAL.md`](docs/MANUAL.md). Five-minute
version: [`docs/QUICKSTART.md`](docs/QUICKSTART.md).

## Adapters

Built-in: `command`, `filesystem`, `git`, stateful `process`
(partial), plus compositions (`stdio`, `json`, `test-runner`,
`database`, `snapshot`) and the external JSONL protocol:

```text
terrorbats adapters
terrorbats adapter inspect process
```

## Limitations

- Disposable worktrees are **not** hostile-code containment.
- Captured child output is byte-verbatim — a target that prints its
  secret discloses it into evidence. No automatic redaction.
- Escaped/out-of-group descendants unsupported; `process` stays
  `builtin-partial`.
- `http`/`tcp` have no native builtins yet (use `curl`/explicit clients).
- This is 0.2.0-rc.1 for serious evaluation — not certification, not a
  sandbox, not formally verified.

## Documents

- [Manual](docs/MANUAL.md) — the canonical long-form guide
- [Quickstart](docs/QUICKSTART.md) — five minutes to first receipt
- [First Flight guide](docs/first-flight.md) — install, run, inspect, replay
- [Architecture](docs/architecture.md) — pipeline, supervision, isolation
- [Bat Spec v0](docs/bat-spec-v0.md) — the declarative Bat model
- [Receipt v0](docs/receipt-v0.md) — verdicts and the human-facing output
- [Capability model](docs/capability-model.md) — declared vs enforced effects
- [Changelog](CHANGELOG.md) — user-visible release history
- [Security](SECURITY.md) — what runs with your authority, and how to report

## License

MIT OR Apache-2.0 — see `LICENSE-MIT` and `LICENSE-APACHE`.
