# Release process (RC1)

Authoritative order (see the RC1 convergence packet, GIT SHAPE):

1. Repair on feature branches, independent review, merge to `main`.
2. Coherent release commits on `release/0.2.0-rc.1`.
3. Open the RC convergence PR (release → main): hosted Windows/Linux
   verification, independent release review, merge.
4. Tag the exact merged main: `v0.2.0-rc.1`.
5. Build the two platform bundles and publish the GitHub Release as
   **Pre-release** titled `The Terror Bats Framework 0.2.0-rc.1`.
6. Post-publication verification: download each asset, verify checksums,
   rerun the First Flight loop from the bundle.

## Rules

- Never tag or publish from an unverified head. The tag points at merged,
  reviewed, CI-green `main` — never at a release branch directly.
- Checksums (SHA-256) ship beside every asset; provenance notes record
  which commit and workflow run produced each bundle.
- No package-manager publishing for RC1 (no MSI/winget/deb/rpm/brew):
  ZIP + PowerShell installer and tar.gz + shell installer only.
- A PASS on one SHA never transfers to a moving head. Any new commit
  restarts verification for that head.
