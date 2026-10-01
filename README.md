# Terror Bat

Terror Bat is a language-agnostic falsification and assurance framework. It attempts to **disprove claims** about systems, isolates the attack, records what happened, and produces reproducible evidence. It orchestrates existing machinery (test runners, fuzzers, static analysis, Git) around a single pipeline: claim → attack → execution → oracle → evidence → receipt. It does not replace Cargo test, pytest, Playwright, or any other testing system.

> **What could still be wrong while all the ordinary tests are green?**

> **Imagine with AI. Attack deliberately. Prove mechanically. Preserve the evidence.**

## Current status

**Windows First Flight (0.1).** The full core pipeline works end to end on Windows: Bat Spec identity (M1/M1.1/M1.2), supervised process-tree execution (M2), disposable Git-worktree runs with built-in `command`/`fs`/`git` primitives (M3), content-addressed evidence store + operation log (M4), deterministic oracle engine (M5), receipts + inspect/evidence/replay CLI (M6), and `terrorbat doctor` (W1). Start with the **[First Flight guide](docs/first-flight.md)**. The core requires no cloud, API key, model, Docker, Python or Node. Caching, external adapters and AI discovery are deliberately not implemented yet.

## Documents

- [First Flight guide](docs/first-flight.md) — install, run, inspect, replay, Bat authoring, exit codes, limitations
- [Architecture](docs/architecture.md) — pipeline, supervision, isolation, adapters, AI boundary
- [Bat Spec v0](docs/bat-spec-v0.md) — the declarative Bat model
- [Receipt v0](docs/receipt-v0.md) — epistemic states and the primary human-facing output
- [Capability model](docs/capability-model.md) — declared vs enforced effects
- [Reusery Bat Zero](docs/reusery-bat-zero.md) — the first real specimen, and how it validates Terror Bat itself

Roadmap, kill criteria, and M1 entry criteria are in [architecture.md](docs/architecture.md).
