# Terror Bats Quickstart — five minutes to first receipt

```text
cargo build --release --bin terrorbats
terrorbats --version
```

Expect exactly `terrorbats 0.2.0-rc.1`.

```text
terrorbats doctor
```

All core checks should pass. Optional tools may be absent — that never
fails doctor.

```text
terrorbats run bats/command-exit.yaml --repo D:\some-project
```

The target repository must be clean. Expect `Completed` /
`NOT OBSERVED`: the attack did not falsify the claim.

```text
terrorbats replay <receipt:sha256:...> --store <path>
```

Same attack, fresh OS identities, new receipt. The original never changes.

Next: [`MANUAL.md`](MANUAL.md) for Bat authoring, adapters, secrets,
replay semantics, and limitations. Every command above is executed by
the automated docs smoke (`tests/cli_release.rs`), so this page cannot
rot silently.
