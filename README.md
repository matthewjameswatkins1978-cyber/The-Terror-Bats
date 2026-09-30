# Terror Bat

Terror Bat is a language-agnostic falsification and assurance framework. It attempts to **disprove claims** about systems, isolates the attack, records what happened, and produces reproducible evidence. It orchestrates existing machinery (test runners, fuzzers, static analysis, Git) around a single pipeline: claim → attack → execution → oracle → evidence → receipt. It does not replace Cargo test, pytest, Playwright, or any other testing system.

> **What could still be wrong while all the ordinary tests are green?**

> **Imagine with AI. Attack deliberately. Prove mechanically. Preserve the evidence.**

## Current status

**M1 — Bat Spec parsing + canonical identity, implemented in Rust.** `terrorbat spec check|id|canonical` parses YAML Bat Specs, resolves `$param` parameters, canonicalises to RFC 8785 JSON, and produces `bat`/`claim`/`attack`/`oracle` content identities. Golden-vector tests pin the canonical form. There is still **no execution**: no runners, no worktrees, no oracles that run, no receipts.

## M0 documents

- [Architecture](docs/architecture.md) — pipeline, supervision, isolation, adapters, AI boundary
- [Bat Spec v0](docs/bat-spec-v0.md) — the declarative Bat model
- [Receipt v0](docs/receipt-v0.md) — epistemic states and the primary human-facing output
- [Capability model](docs/capability-model.md) — declared vs enforced effects
- [Reusery Bat Zero](docs/reusery-bat-zero.md) — the first real specimen, and how it validates Terror Bat itself

Roadmap, kill criteria, and M1 entry criteria are in [architecture.md](docs/architecture.md).
