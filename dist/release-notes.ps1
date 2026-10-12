<#
.SYNOPSIS
  Render the Terror Bats release notes for a given version (no publish).

.DESCRIPTION
  Single controlled interpolation: only the title line expands $Version.
  The Markdown body is a single-quoted here-string, so backticks, $HOME
  and shell examples survive byte-for-byte — an unquoted shell heredoc
  would execute the `code spans` as commands. Used by the tag-triggered
  release workflow and by the release-notes dry-run regression test.

.EXAMPLE
  pwsh -NoProfile -File dist/release-notes.ps1 -Version 1.0.0-rc.1
#>
param(
    [Parameter(Mandatory = $true)][string]$Version
)

$ErrorActionPreference = 'Stop'

Write-Output "# The Terror Bats Framework $Version"
Write-Output @'

**Find what ordinary green tests can miss.**

The Terror Bats Framework is a language-agnostic **adversarial falsification** tool. Give it a claim, attempt to disprove that claim in a disposable Git worktree, evaluate real evidence with a deterministic oracle, and keep a content-addressed receipt. It reuses Git and your existing command-line test tools instead of replacing them.

> What could still be wrong while all the ordinary tests are green?

## Why try Terror Bats?

- **Challenge promises instead of trusting a green dashboard.** Test invariants such as protected files remaining unchanged, process cleanup, and command behavior.
- **Keep evidence, not just a pass/fail string.** Receipts tie an attack to a Git commit, execution status, oracle decision, and captured artifacts.
- **Replay a particular experiment.** The original evidence remains intact while a new run records what changed.
- **Bring your own stack.** Run existing commands, verifiers and tools; no AI, API key, Docker, or cloud account is required for core execution.
- **Be precise about uncertainty.** PROVEN means the tested claim was falsified under recorded conditions. `NOT OBSERVED` does not mean safe or correct; a timeout is `INCONCLUSIVE`.

**Compared with unit tests:** Terror Bats operates one layer above individual assertions, arranging attacks and recording what those attacks establish. **Compared with fuzzers:** you define the attack and oracle rather than relying on built-in input generation. **Compared with CI:** it supplies the structured evidence and receipt that a CI job can run and archive. These tools complement one another; no general performance or bug-detection superiority is claimed.

## What's in this release candidate?

- Windows x86-64 ZIP and Linux x86-64 tarball
- `terrorbats` CLI, installer helpers, manuals and checksums
- Declarative Bat Specs, Git worktree-backed runs and deterministic oracles
- Supervised command and partial stateful `process` adapters
- Evidence objects, content-addressed receipts and local replay
- Packs, serial campaigns and an external JSONL adapter protocol
- `doctor`, shell completions and generated command reference

This is a **release candidate for evaluation**, not a certification, a security sandbox, or formally verified software.

## First flight from the Windows ZIP

1. Download the Windows ZIP **and** `SHA256SUMS.txt` from this release.
2. Check the ZIP hash with `Get-FileHash -Algorithm SHA256` against the checksum file:

   ```powershell
   $zip = Get-ChildItem . -Filter 'terrorbats-*-windows-x86_64.zip' | Select-Object -First 1
   Get-FileHash $zip.FullName -Algorithm SHA256
   Get-Content .\SHA256SUMS.txt
   ```

3. Extract the ZIP. The bundled `install.ps1` optionally installs user-locally without admin rights. From the extracted directory, run:

   ```powershell
   .\terrorbats.exe --version
   .\terrorbats.exe doctor
   ```

4. For a **zero-Rust, zero-admin** first lesson that creates a harmless Git repo and a Bat, follow the [README's Windows ZIP tutorial](https://github.com/matthewjameswatkins1978-cyber/The-Terror-Bats#your-first-flight-windows-zip-tutorial). The binary bundle contains the manual but **not** the repository's `bats/` sample directory.

**Linux:** Verify with `sha256sum -c SHA256SUMS.txt`, extract the tarball, optionally use `install.sh --prefix $HOME/.local`, then run `terrorbats --version` and `terrorbats doctor`.

The Windows release-candidate binary is **not Authenticode-signed**. The release provenance and SHA-256 checksums should be checked before use.

## What this release is not

- **Not hostile-code containment:** Git worktrees isolate changes made by built-in steps, but spawned programs can still reach host files, network services, credentials and remotes.
- **Not automatic secret redaction:** captured child output is byte-verbatim. Inspect artifacts before sharing.
- **Not a universal proof of correctness:** `PROVEN` falsifies one claim under recorded conditions; `NOT OBSERVED` is not certification.
- **Not a general-purpose fuzzer or browser runner:** use established specialist tools where those jobs fit.
- **Not full process containment:** the process adapter stays `builtin-partial`; escaped descendants are unsupported.

The source repository's separately maintained MCP adapter is **not included in these core binary archives**. MCP replay is disabled in its initial bridge release.

## Learn more and help improve it

- [README: how it works, comparisons, first tutorial](https://github.com/matthewjameswatkins1978-cyber/The-Terror-Bats)
- [User manual](https://github.com/matthewjameswatkins1978-cyber/The-Terror-Bats/blob/main/docs/MANUAL.md)
- [Report bugs and ideas](https://github.com/matthewjameswatkins1978-cyber/The-Terror-Bats/issues)
- [Security reporting policy](https://github.com/matthewjameswatkins1978-cyber/The-Terror-Bats/blob/main/SECURITY.md)

When reporting a bug, include OS/architecture, `terrorbats --version`, the Bat or minimal reproducer, and a receipt identifier **only if it is safe to share**.

**Freedom to reason. Boundaries for consequences. Evidence for success.**
'@
