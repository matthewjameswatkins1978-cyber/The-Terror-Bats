# \^v^/ The Terror Bats Framework

**Ordinary tests ask whether the system works. Terror Bats asks what could still be wrong.**

**Language-agnostic adversarial testing · Deterministic evidence · Replayable receipts · Windows + Linux x86-64**

[![Stateful process core CI](https://github.com/matthewjameswatkins1978-cyber/The-Terror-Bats/actions/workflows/stateful-process-core.yml/badge.svg)](https://github.com/matthewjameswatkins1978-cyber/The-Terror-Bats/actions/workflows/stateful-process-core.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![Status: Release Candidate](https://img.shields.io/badge/status-1.0.0--rc.1-orange)](CHANGELOG.md)

> **What could still be wrong while all the ordinary tests are green?**

Terror Bats is an **adversarial falsification framework**. Instead of asking a test to confirm that a feature behaves as expected, you give a **Bat** a claim to challenge. It runs a bounded experiment, gathers the evidence, applies a deterministic oracle, and issues a content-addressed **receipt** saying exactly what the experiment established.

It works with existing command-line tools and test runners. The core needs **no AI model, API key, cloud account, Docker, or Rust toolchain to run the prebuilt executable**.

**[Start with the ZIP](#your-first-flight-windows-zip-tutorial)** · **[Read the manual](docs/MANUAL.md)** · **[See what a Bat looks like](bats/command-exit.yaml)** · **[Report an issue](../../issues)**

> **Release availability:** The `v1.0.0-rc.1` Windows ZIP, Linux tarball and checksums have been built, but the GitHub release is currently a **draft**. They will appear under [Releases](../../releases) when published. Until then, follow [Build from source](#build-from-source). The published ZIP, when available, contains the executable and manuals, **not** the repository's `bats/` example directory.

---

## Why would I use this?

Imagine an agent reports “all tests pass”, but it silently rewrote a protected file, left a process running, introduced a duplicate dependency, or printed reassuring output while the real evidence tells a different story. Conventional tests may catch those situations *if you wrote the right assertions*. Terror Bats gives you a structured way to **attack the underlying claim**, record the conditions, and make the outcome inspectable.

| Good for | What Terror Bats brings |
| --- | --- |
| **Agent-generated code** | Test the result, not the agent's confidence; inspect changes and independently evaluate claims. |
| **Regression and safety invariants** | Attempt protected-path mutations, unexpected changes, forbidden effects, and other deliberately chosen violations. |
| **Processes and CLI programs** | Supervise child processes, readiness, timeouts, output, and owned-process cleanup. |
| **Hard-to-reproduce failures** | Pin the target Git commit, retain evidence, and replay the experiment with fresh execution identities. |
| **Auditable engineering** | Keep a receipt with what ran, what was observed, which oracle fired, and what was *not* proven. |
| **Different languages and stacks** | Use Git, commands, existing tests, and external adapters without rewriting everything in one language. |

A Bat can use a familiar tool, such as `cargo test`, `pytest`, or a purpose-built verifier, as one step of a larger experiment. Terror Bats is the **evidence and falsification layer around your existing tools**, not another assertion library.

## How one Bat works

```text
            THE CLAIM
   "This protected file does not change"
                 |
                 v
         THE ATTACK
  Try the change in a disposable worktree
                 |
                 v
       SUPERVISED EXECUTION
  Commands · processes · captured output
                 |
                 v
       DETERMINISTIC ORACLE
    Did the evidence falsify the claim?
                 |
                 v
        EVIDENCE + RECEIPT
    Verifiable identity · recorded context
                 |
                 v
     INSPECT  ·  COMPARE  ·  REPLAY
```

A **Bat** is a YAML experiment: *claim + attack + oracle + evidence requirements*. Terror Bats keeps three different questions separate:

- **What happened to the run?** Completed, timed out, crashed, or failed in some other way.
- **What did the oracle observe?** Falsified, not falsified, or undetermined.
- **What can the receipt responsibly claim?** A verdict grounded in evidence, not a guess.

| Verdict | What it means |
| --- | --- |
| `PROVEN` | The tested claim was **falsified under the recorded conditions**. Not a universal proof. |
| `NOT OBSERVED` | This particular attack **did not falsify** the claim. **Not** proof of correctness or safety. |
| `INCONCLUSIVE` | The evidence could not decide, including timeout or crash cases. |
| `INVALID` | The experiment was malformed; no useful test was made. |
| `INFRASTRUCTURE ERROR` | The framework or an external dependency failed. Not a target finding. |

The receipt schema also defines `REPRODUCED` and `SUSPECTED`, but RC1 does not manufacture those verdicts without an evidence path. Read [Receipt v0](docs/receipt-v0.md) for the full contract.

## Where it complements other testing tools

| Tool / approach | Its strength | Terror Bats' different angle |
| --- | --- | --- |
| **pytest, JUnit, Cargo test** | Precise unit and integration assertions, broad ecosystems | Cross-tool claims, isolated Git experiments, structured evidence receipts, and replay. |
| **AFL++, libFuzzer, property testing** | Finding unexpected inputs and cases at scale | You define the adversarial experiment and oracle; Terror Bats does **not** replace fuzz-input generation. |
| **Playwright and browser testing** | Real user workflows and browser behavior | Worktree-backed experiments and receipts across commands and repositories; not a browser automation replacement. |
| **CI systems** | Scheduling, pipelines, build gates, distribution | Explain **why** a particular claim was falsified and preserve the evidence; run Terror Bats from CI if desired. |

**The advantage is the contract, not a claim to be faster or “better at testing” in general.** The combination of declarative attacks, deterministic judgement, Git-worktree isolation, and content-addressed receipts is particularly helpful where ordinary green/red test output tells too little of the story. No comparative speed or bug-finding superiority benchmark is claimed.

---

## Your first flight: Windows ZIP tutorial

**About 10 minutes.** Windows x86-64, PowerShell 7, and Git are sufficient. No admin rights or Rust install required. This lesson creates its own **temporary** Git repository and does not touch your real projects.

### 1. Download and check the ZIP

When the release is published, open **[GitHub Releases](../../releases)** and download:

- `terrorbats-1.0.0-rc.1-windows-x86_64.zip`
- `SHA256SUMS.txt`

Keep both in your `Downloads` folder. In PowerShell:

```powershell
$zip = Join-Path $HOME 'Downloads\terrorbats-1.0.0-rc.1-windows-x86_64.zip'
Get-FileHash -LiteralPath $zip -Algorithm SHA256
Get-Content (Join-Path $HOME 'Downloads\SHA256SUMS.txt')
```

Compare the ZIP's SHA-256 hash with the matching line in `SHA256SUMS.txt`. Stop if they differ. The Windows RC binary is **not Authenticode-signed**; use only the official release source.

### 2. Extract, run, and check your machine

```powershell
$bundle = Join-Path $HOME 'Tools\TerrorBats-RC1'
Expand-Archive -LiteralPath $zip -DestinationPath $bundle
$tb = Join-Path $bundle 'terrorbats.exe'

& $tb --version
& $tb doctor
```

Expect `terrorbats 1.0.0-rc.1`. `doctor` checks core prerequisites, including Git, temporary worktrees, evidence storage, and process supervision. Some **optional** tools may be missing without causing a failure.

You can use `& $tb` throughout this lesson. There is **no PATH modification**. Later, the bundle's `install.ps1` can install it to a user-local directory; changing PATH is opt-in.

### 3. Create a harmless practice repository

Terror Bats uses a clean Git repository as its test target. This makes a new, tiny repository under your temporary folder:

```powershell
$lesson = Join-Path $env:TEMP ('terrorbats-lesson-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$target = Join-Path $lesson 'target'
New-Item -ItemType Directory -Path $target -Force | Out-Null

git -C $target init
Set-Content -Path (Join-Path $target 'README.md') -Value 'My first test target'
git -C $target add README.md
git -C $target -c user.name='Terror Bats Tutorial' -c user.email='tutorial@example.invalid' commit -m 'Initial demo'
```

The temporary target is now committed and clean. The Bat definition and evidence store will be **outside** this Git repository.

### 4. Write your very first Bat

Copy this whole block into the same PowerShell window. It creates `first-bat.yaml`:

```powershell
$bat = Join-Path $lesson 'first-bat.yaml'
@'
version: terrorbat/v1
id: first-flight
claim:
  text: Git's version command exits successfully.
requires: [process.spawn]
attack:
  run:
    - adapter: command
      action: run
      program: git
      args: [--version]
oracle:
  type: exit_code
  step: run:0
  not_equals: 0
evidence:
  capture: [stdout, stderr]
timeout:
  run: 60s
'@ | Set-Content -Path $bat -Encoding utf8
```

Read it almost as English: “I claim `git --version` succeeds. Run it, and look for evidence of a nonzero exit code.” The oracle is an **attempt to falsify the claim**, not a pass-condition disguised as a claim.

### 5. Check it, attack it, read the result

```powershell
$store = Join-Path $lesson 'evidence'

& $tb spec check $bat
& $tb run $bat --repo $target --store $store
```

On a normal machine, Git exits successfully, so expect **`NOT OBSERVED`**. That means this Bat found no violation. It doesn't certify Git or your repository.

Look at the terminal receipt and copy the `receipt:sha256:...` identifier it reports. Then inspect it:

```powershell
& $tb inspect 'receipt:sha256:PASTE_YOUR_REAL_ID_HERE' --store $store
```

Replace the example ID with the one you actually received. The receipt points to captured evidence and records the tested conditions. Receipts and evidence in the chosen store survive the test process.

### 6. Try a deliberate detection self-check

Want to see `PROVEN` in action? The repository includes [`bats/unexpected-change.yaml`](bats/unexpected-change.yaml), a demonstrator that deliberately alters a protected path in a disposable worktree and checks that the change is detected. Once you have cloned the repository, run:

```powershell
# In a checkout of The-Terror-Bats:
& $tb run .\bats\unexpected-change.yaml --repo $target --store $store
```

Here **`PROVEN` means the deliberate mutation was detected**, not that an unexpected vulnerability has been discovered in your practice repository. A proven finding may return process exit code `1` by design.

### 7. Replay a safe experiment, if you want to

```powershell
& $tb replay 'receipt:sha256:PASTE_YOUR_REAL_ID_HERE' --store $store
```

For this tutorial, use the receipt from **step 5** (the harmless Git version check). CLI replay creates a fresh execution and a new receipt; it never rewrites the original. Real-world replays may execute programs again, so inspect what you are replaying first.

**Where is everything?** `$target` is the disposable practice Git repository, `$bat` is your Bat specification, and `$store` is the evidence store. Your actual projects remain untouched.

> **Prefer a shorter lesson?** See [the five-minute quickstart](docs/QUICKSTART.md). It assumes you already have a repository checkout, with the sample `bats/` files available.

---

## Linux and building from source

**Linux x86-64:** Once published, download `terrorbats-1.0.0-rc.1-linux-x86_64.tar.gz` and `SHA256SUMS.txt` from [Releases](../../releases). Verify the SHA-256 checksum, extract, run the bundled `install.sh --prefix "$HOME/.local"` or use the executable directly, then run `terrorbats doctor`. The [manual](docs/MANUAL.md) documents Linux paths and limitations.

**Build from source (including while the release is a draft):** requires Git and a working stable Rust toolchain.

```powershell
git clone https://github.com/matthewjameswatkins1978-cyber/The-Terror-Bats.git
Set-Location .\The-Terror-Bats
cargo build --release --bin terrorbats
.\target\release\terrorbats.exe --version
.\target\release\terrorbats.exe doctor
.\target\release\terrorbats.exe run .\bats\command-exit.yaml --repo 'D:\path\to\your\clean-git-repo'
```

The source checkout includes the sample Bats and packs. Build from the [`v1.0.0-rc.1` tag](../../tree/v1.0.0-rc.1) for the packaged RC1 baseline; `main` also includes newer work not necessarily present in the ZIP.

## When you want to go further

**Bats, packs, and campaigns.** Compose ordered attacks and repeat them under recorded conditions. Every child execution retains its own receipt; `--stop-on-proven` stops after an evidenced finding. See [packs/first-flight.yaml](packs/first-flight.yaml) and the [manual](docs/MANUAL.md).

**Adapters.** The core includes command, filesystem, and Git primitives, plus a **partial** stateful process runtime, compositions, and an external JSONL protocol. Explore with `terrorbats adapters` and `terrorbats adapter inspect process`.

**AI and MCP.** AI may help propose what to test, but an AI opinion **never** turns a verdict into `PROVEN`. The separately packaged, source-tracked [MCP bridge](adapters/mcp/README.md) offers structured tools for compatible agent clients. It is **not part of the RC1 ZIP**. It starts read-only, and **MCP replay is unconditionally disabled in bridge v0.1.0**. A local protocol handshake alone does not establish a ChatGPT or Codex client connection.

## The important boundaries

- A disposable **Git worktree is not a security sandbox**. Tests may still access host files, credentials, the network, or remotes through spawned programs. Run only experiments you trust.
- **Captured child stdout/stderr is preserved verbatim.** A program printing secrets can put them in evidence; avoid sensitive environments and review receipts before sharing.
- A `PROVEN` receipt means a **particular claim was falsified under recorded conditions**, not that a system is universally broken. `NOT OBSERVED` never means “certified safe.”
- Process ownership and cleanup have platform limits; escaped descendants are not guaranteed contained. The process adapter remains `builtin-partial`.
- Evidence is content-addressed and checked for corruption, but local storage is not tamper-proof or guaranteed to survive power loss. Replay needs the original target and commit to remain available.

## Documentation

| Read | For |
| --- | --- |
| [User manual](docs/MANUAL.md) | Full install, authoring, adapters, oracles, receipts, replay and troubleshooting |
| [Quickstart](docs/QUICKSTART.md) | First receipt when you already have the repository |
| [First Flight guide](docs/first-flight.md) | More Windows examples |
| [Bat specification](docs/bat-spec-v0.md) | YAML schema, capabilities and deterministic oracle rules |
| [Architecture](docs/architecture.md) | Execution pipeline, isolation and process supervision |
| [Receipt format](docs/receipt-v0.md) | The epistemic contract and evidence identities |
| [Security](SECURITY.md) | Threat boundaries and private vulnerability reporting |
| [Changelog](CHANGELOG.md) | What the release candidate actually contains |

**Licensing:** [MIT](LICENSE-MIT) **OR** [Apache-2.0](LICENSE-APACHE).

**Terror Bats' promise is deliberately modest:** it won't tell you that software is safe. It will help you ask sharper questions, run repeatable attacks, and keep the evidence when the answer matters.

**Freedom to reason. Boundaries for consequences. Evidence for success.**