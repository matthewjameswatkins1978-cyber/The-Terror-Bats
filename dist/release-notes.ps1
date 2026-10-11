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

Language-agnostic adversarial falsification with deterministic evidence.

> What could still be wrong while all the ordinary tests are green?

## Why RC1 exists

This release candidate is suitable for serious public evaluation on
supported Windows and Linux x86-64 systems. It is not certification,
not a sandbox, and not formally verified.

## Epistemic contract

PROVEN means the tested claim was falsified under recorded conditions.

NOT OBSERVED is not certification.

## Capabilities

- Bat Spec identity, disposable-worktree runs, deterministic oracles
- Stateful process runtime (readiness cursors, stdin/observe/wait/kill/restart)
- Runtime secret references with an explicit verbatim-capture boundary
- Packs and serial Campaigns, external JSONL adapter protocol
- Evidence store, immutable receipts, replay, CLI (doctor, completions, man)

## Installation

Windows: extract the ZIP, run `install.ps1` (user-local, no admin).
Linux: extract the tarball, run `install.sh --prefix $HOME/.local`.
Verify: `terrorbats --version` then `terrorbats doctor`.
Full guide: MANUAL.md inside the bundle.

## Checksums and provenance

Verify with `sha256sum -c SHA256SUMS.txt` (Linux) or
`Get-FileHash -Algorithm SHA256` (Windows). Build provenance is
attested on this release. Windows RC1 binary is not
Authenticode-signed.

## Known limitations

- `process` adapter stays `builtin-partial`; escaped descendants unsupported.
- Captured child output is byte-verbatim (no sanitisation).
- Worktrees are not hostile-code containment.
- See MANUAL.md §27 and CHANGELOG.md for the full list.

## Reporting problems

Open an issue with `terrorbats --version`, OS/architecture, the Bat
reproducer, and the receipt id. See SECURITY.md for sensitive reports.
'@
