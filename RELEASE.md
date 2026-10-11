# Release process (RC1)

Authoritative order (see the RC1 convergence packet, GIT SHAPE):

1. Repair on feature branches, independent review, merge to `main`.
2. Version-convergence work on `release/1.0.0-rc.1-version`, based on verified main `655c8e7`. No protocol or feature churn.
3. Open a narrowly scoped release-version PR against `main`: hosted
   Windows/Linux verification, independent review and merge.
4. Tag the exact merged main: `v1.0.0-rc.1`.
5. Tag-triggered CI builds both bundles, checksums and provenance and
   creates a **draft pre-release** titled `The Terror Bats Framework 1.0.0-rc.1`.
6. Verify both draft assets, checksums, provenance and the First Flight
   loop before separately authorising public publication.

## Rules

- Never tag or publish from an unverified head. The tag points at merged,
  reviewed, CI-green `main` — never at a release branch directly.
- Checksums (SHA-256) ship beside every asset; provenance notes record
  which commit and workflow run produced each bundle.
- No package-manager publishing for RC1 (no MSI/winget/deb/rpm/brew):
  ZIP + PowerShell installer and tar.gz + shell installer only.
- A PASS on one SHA never transfers to a moving head. Any new commit
  restarts verification for that head.
