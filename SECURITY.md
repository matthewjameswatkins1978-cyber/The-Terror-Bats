# Security

## What runs with your authority

Terror Bats executes programs. A Bat's steps run with **your** OS
authority: child processes are not sandboxed, disposable Git worktrees
are not hostile-code containment, and adapters (built-in or external)
may invoke any tool you could invoke yourself.

Only run Bats and adapters you are willing to execute.

## Secrets discipline

- Prefer runtime `$secret` environment references over literals.
- Captured child stdout/stderr and files read as evidence are
  byte-verbatim: a target that prints a secret discloses it into the
  evidence store. There is no automatic redaction.
- Receipt invocation payloads record environment values as `[REDACTED]`,
  but evidence objects, logs you copy, and terminal scrollback are your
  responsibility.

## Reporting

Do not file public issues for live vulnerabilities. Report privately via
the repository's configured private channel (Security tab) with: exact
`terrorbats --version`, OS/architecture, a minimal reproducer, and the
receipt id if one exists. No separate security mailbox exists for this
project — use the private channel the forge provides.
