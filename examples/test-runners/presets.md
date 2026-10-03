# Test-runner presets (data, not adapter implementations)

Each preset resolves to one ordinary `command.run` step plus an `exit_code`
oracle condition. If the runner emits a structured report (JUnit XML),
capture it with `fs.read` and judge it structurally; otherwise preserve raw
output rather than pretending to understand it.

## cargo

```yaml
attack:
  run:
    - adapter: command
      action: run
      program: cargo
      args: [test, --quiet]
oracle:
  type: exit_code
  step: run:0
  not_equals: 0
```

Requires: `process.spawn`. Optional JUnit: `cargo test -- --report-time`
has no stable XML; keep stdout/stderr as evidence.

## pytest

```yaml
attack:
  run:
    - adapter: command
      action: run
      program: python
      args: [-m, pytest, -q, --junitxml=junit.xml]
oracle:
  type: exit_code
  step: run:0
  not_equals: 0
```

JUnit at `junit.xml`: add an `fs.read` step and judge counts structurally.

## unittest

```yaml
attack:
  run:
    - adapter: command
      action: run
      program: python
      args: [-m, unittest]
oracle:
  type: exit_code
  step: run:0
  not_equals: 0
```

## npm / pnpm

```yaml
attack:
  run:
    - adapter: command
      action: run
      program: npm
      args: [test, --, --reporter=json]
oracle:
  type: exit_code
  step: run:0
  not_equals: 0
```

`pnpm test` substitutes directly.

## jest / vitest

```yaml
attack:
  run:
    - adapter: command
      action: run
      program: npx
      args: [jest, --json]
oracle:
  type: exit_code
  step: run:0
  not_equals: 0
```

`npx vitest run --reporter=json` substitutes directly.

## go

```yaml
attack:
  run:
    - adapter: command
      action: run
      program: go
      args: [test, ./...]
oracle:
  type: exit_code
  step: run:0
  not_equals: 0
```

## dotnet

```yaml
attack:
  run:
    - adapter: command
      action: run
      program: dotnet
      args: [test]
oracle:
  type: exit_code
  step: run:0
  not_equals: 0
```

TRX output (`--logger trx`) can be captured with `fs.read` when present.

## Maven / Gradle

```yaml
attack:
  run:
    - adapter: command
      action: run
      program: mvn
      args: [-q, test]
oracle:
  type: exit_code
  step: run:0
  not_equals: 0
```

`gradle test --quiet` substitutes directly. Surefire XML under
`target/surefire-reports` (Maven) or `build/test-results` (Gradle) can be
captured with `fs.read` when present.
