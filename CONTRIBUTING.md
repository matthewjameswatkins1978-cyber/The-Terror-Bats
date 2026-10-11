# Contributing

Release branch discipline: `release/1.0.0-rc.1-version` is version-only
convergence based on reviewed main `655c8e7`. It may update package
metadata, release-facing labels, docs, and tag/version mismatch checks.
Unrelated features, new adapters, GUIs, and platform ports wait for after RC1.

## Gates (run before every push)

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
```

Hosted Windows + Linux CI runs the same three gates. No merge on red.

## Evidence rules for contributors

- Prove fixes with adversarial regressions, including negative controls
  where practical (the test must fail on the old defect).
- Security claims must describe what the implementation guarantees —
  never what we wish it guaranteed.
- Documented commands must be executed by the docs smoke
  (`tests/cli_release.rs`) or syntactically checked against the CLI.
- Preserve `%LOCALAPPDATA%\TerrorBat` evidence compatibility and the
  `terrorbat/v1` / `terrorbat-adapter/v1` protocol identifiers. Those
  never churn cosmetically.
- CRLF line endings are preserved in the repository; let Git handle
  conversion, never bulk-rewrite files.
