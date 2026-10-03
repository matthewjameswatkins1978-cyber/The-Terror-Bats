# Terror Bat

Terror Bat is a language-agnostic falsification and assurance framework. It attempts to **disprove claims** about systems, isolates the attack, records what happened, and produces reproducible evidence. It orchestrates existing machinery (test runners, fuzzers, static analysis, Git) around a single pipeline: claim → attack → execution → oracle → evidence → receipt. It does not replace Cargo test, pytest, Playwright, or any other testing system.

> **What could still be wrong while all the ordinary tests are green?**

> **Imagine with AI. Attack deliberately. Prove mechanically. Preserve the evidence.**

## Current status

**First Flight + M8 external adapters.** The core pipeline runs on Windows: Bat Spec identity (M1/M1.1/M1.2), M2 process-tree supervision, disposable worktree runs, durable evidence, deterministic oracles, receipts/replay, Packs and serial Campaigns. M8 adds the language-neutral `terrorbat-adapter/v1` stdio protocol; see the [external adapter guide](docs/external-adapter-v1.md). Start with the [First Flight guide](docs/first-flight.md). Terror Bat requires no cloud, model, Docker, Python, or Node for its built-ins. Evidence reuse/caching and AI discovery remain future work.

## Documents

- [First Flight guide](docs/first-flight.md) — install, run, inspect, replay, Bat authoring, exit codes, limitations
- [Architecture](docs/architecture.md) — pipeline, supervision, isolation, adapters, AI boundary
- [External adapter protocol v1](docs/external-adapter-v1.md) — bindings, describe/execute messages, capabilities, provenance, and replay
- [Bat Spec v0](docs/bat-spec-v0.md) — the declarative Bat model
- [Receipt v0](docs/receipt-v0.md) — epistemic states and the primary human-facing output
- [Capability model](docs/capability-model.md) — declared vs enforced effects
- [Reusery Bat Zero](docs/reusery-bat-zero.md) — the first real specimen, and how it validates Terror Bat itself

Roadmap, kill criteria, and M1 entry criteria are in [architecture.md](docs/architecture.md).
