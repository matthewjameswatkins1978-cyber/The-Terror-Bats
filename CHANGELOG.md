# Changelog

Significant user-visible changes only. No commit-message dumps.

## 0.2.0-rc.1

First public release candidate (Windows + Linux x86-64).

- Executable and crate renamed to `terrorbats`; version `0.2.0-rc.1`.
- Stateful process adapter: supervised handles with generations —
  start, condition-based readiness with per-stream cursors, stdin,
  bounded observe, wait, Unix SIGTERM terminate, forced kill, restart.
- Runtime `$secret` environment references: configuration keeps the
  reference, receipts record `[REDACTED]`, replay re-resolves, missing
  fails closed. Captured child output is byte-verbatim by design.
- Bounded supervision: root exit and pipe EOF distinguished, owned
  process-group/Job cleanup with survivor reporting.
- Linux path semantics: drive/UNC/ADS rules are Windows-only.
- CLI: `doctor` (human + JSON), `completions` (PowerShell/bash/zsh/fish),
  `man` from the live definition, exact `--version`.
- Docs: canonical `docs/MANUAL.md`, `docs/QUICKSTART.md`, refreshed README.
- Receipts carry immutable verdicts (`PROVEN` … `INFRASTRUCTURE ERROR`)
  with replay across fresh OS identities.

Known RC boundaries: `process` stays `builtin-partial`; escaped
descendants unsupported; no output sanitisation; `http`/`tcp` via
explicit clients; worktrees are not hostile-code containment.
