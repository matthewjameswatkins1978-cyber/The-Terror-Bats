# Reusery Bat Zero Report — Windows First Flight

**Date:** 2026-10-01 · **Terror Bat:** 0.1.0 First Flight candidate · **Platform:** Windows x86-64

Bat Zero is the product test: a real experiment, run through the real
pipeline, judged against the seven M0 questions and the kill gate. The prior
Reusery conclusion is a **reference result to inspect, not truth to
preserve** ([reusery-bat-zero.md](reusery-bat-zero.md)).

---

## 1. Inspection findings (placeholders P1–P6, resolved from evidence)

Target: `https://github.com/matthewjameswatkins1978-cyber/reusery.dev`,
main @ `ed0243be63316ebca636bdf01512b90422be5f2a` (clean, 52 commits).

| # | Question | Finding (with repository evidence) |
|---|---|---|
| P1 | Arm definitions | **Resolved.** Five arms in `benchmarks/results/report.md` @ `af5e170d93c60ab6a0a038161c0b42f1f7ac7960`: `arm_a` Raw Strong Agent (Unaided), `arm_b1` Agent + Prompt Discipline, `arm_b2` Hostile Realistic Baseline, `arm_c_star` Agent + Evidence/Receipts Only, `arm_c` Full Reusery. 15 frozen tasks (10 mechanism, 3 naturalistic, 2 sequential). Harness commits: `e424fdc` (13.5), `7ec09de`/`4367f42` (13.6). |
| P2 | Targets/fixtures | **Resolved.** Frozen task set inside the harness branches; system under test = the Reusery resolver (REUSE/ADAPT/DEPEND/REFERENCE/BUILD routes). |
| P3 | Original evidence | **Resolved.** Synthetic dry-run reference traces only: report header states `Execution Mode: dry_run`, `Total Runs Evaluated: 75`, and every arm count is labelled "harness check, not a measurement". **No empirical run was ever committed.** The results file exists only on unmerged branch `packet/13.6-live-kill-test`; `git merge-base --is-ancestor af5e170 HEAD` → false; main's `benchmarks/` holds only README.md, schema-v1.yaml, `internal/benchmark/*`. |
| P4 | Reference conclusion | **Resolved.** The report self-disavows empirical readings: "must NOT be cited as empirical frontier model findings", "support no product, architectural, or purchasing conclusion", "No further interpretation is licensed in dry-run mode." `benchmarks/README.md` (main): "does not make any claim that Reusery saved time, tokens or money"; "Packet 16 owns release-level validation claims." |
| P5 | Deterministic vs judgement | **Resolved.** Harness scoring is deterministic over frozen traces; the arm runs themselves are agent-in-the-loop (model-dependent). No live-model execution evidence exists in the repository. |
| P6 | Environment/tool/model facts | **Resolved from the report header:** date 2026-10-01 00:23:26 UTC, mode `dry_run`, evaluation model `gpt-6-luna`, 75 runs evaluated. Main requires Go 1.27.1; this machine has `go1.27.1 windows/amd64` (exact match). |

**Assumption scores (M0 §2):** A2 confirmed (the expressed slice is
deterministically judgeable). A3 confirmed (pinned-commit reconstruction
works; historical objects reachable). A1 partially contradicted and A4
contradicted for First Flight: the original arms are agent-in-the-loop runs;
re-scoring them requires AI execution (M9 — deliberately not built). Bat
Zero was adapted per M0's rule ("assumptions are not silently upgraded to
facts"): five Bats over the shared claim family *benchmark evidence state
and reuse discipline in reusery.dev*, all judged by deterministic machinery.

## 2. The five arms (executed, pinned, receipted)

All arms ran against main @ `ed0243be6331` in disposable worktrees; source
clone verified unchanged afterwards. Registry conditions and one `go test`
command verifier only — **zero Reusery-specific engine code**.

| Arm | Claim attacked | Oracle | Result | Verdict | Receipt |
|---|---|---|---|---|---|
| BZ-1 | Empirical benchmark results are committed on main | `file_absent benchmarks/results/report.md` | Falsified | **PROVEN** | `receipt:sha256:dddd6e2a4b2faefe6d84d86ed1cc3d5f761fb7e1ffbf870880ea897199236065` |
| BZ-2 | Recording machinery is committed on main | `any` of three `file_absent` | NotFalsified | **NOT OBSERVED** | `receipt:sha256:e6589f1f53923adee3aeded02be8a4071a0b935c3b940f771fcf9a049215f94f` |
| BZ-3 | The dry-run reference report is unrecoverable from history | `command` verifier: `git cat-file -e af5e170…:benchmarks/results/report.md`, expect_exit 1 → exited 0 | Falsified | **PROVEN** | `receipt:sha256:8d1a97df83fe64aa5b175138b157b05092da434a607c125ab3bd70c427838b25` |
| BZ-4 | go.mod reuses mature deps (HTTP/Postgres/migrations/MCP) instead of reimplementing | `any` of four `not: text_contains go.mod …` | NotFalsified | **NOT OBSERVED** | `receipt:sha256:94134d66c4f9f4f0eb8cf20bffec2cfdc9763676035e3019faf904d7d8255ef2` |
| BZ-5 | The recording machinery's own tests pass at the pin | `command` verifier: `go test ./internal/benchmark -count=1`, expect_exit 0 → exited 0 | NotFalsified | **NOT OBSERVED** | `receipt:sha256:6538d68850b77158301b02af444c4558786723739f633fef042d8fec8d8dd70a` |

Wall times 1.8–3.2 s per arm including worktree create, captures, oracle and
cleanup (BZ-5 includes a real Go compile+test). NOT OBSERVED verdicts are
not correctness certificates; PROVEN verdicts are scoped to the recorded
conditions (this repo, this commit, this machine, this version).

Specs: [`batzero/`](../batzero) in this repository — rerunnable by anyone:

```powershell
terrorbats run batzero\bz1-benchmark-evidence-absent.yaml --repo <reusery.dev clone>
```

### Replay proof (BZ-1)

`terrorbats replay receipt:sha256:dddd6e2a…` → new execution
`d95f6279-2881-4753-81b5-1328357ef6a7`, new receipt
`receipt:sha256:657291213881d7230c86d58a7878a1874b2c8ecb2588850fdef569f275dc6d27`;
execution status / oracle result / verdict **same**; `base_snapshot`,
`git_status`, `git_diff` evidence identities **same**; original receipt
byte-identical after replay.

## 3. Comparison to the prior reference result

Allowed dispositions: reproduced / weakened / **evidence insufficient** /
contradicted / inconclusive.

**Disposition: evidence insufficient** — for any empirical arm-superiority
claim. This agrees with the prior report's own licensing text; Terror Bat
did not need to take it on faith, it proved the current state
mechanically:

- the celebrated arm table (33.3% → 100%) is synthetic dry-run harness
  verification, by its own header and per-count labels (P3/P4);
- it never landed on the canonical branch (BZ-1 **PROVEN**);
- it remains fully recoverable from an unmerged branch, so nothing was
  hidden or lost (BZ-3 **PROVEN**);
- the machinery for honest future measurement persists on main (BZ-2), and
  its own tests pass at the pin (BZ-5);
- the product practises manifest-level reuse discipline for its own
  infrastructure (BZ-4).

No Terror Bat output was tuned toward any prior answer; the two PROVEN
findings are properties of the repository, not of the reference report's
conclusions.

## 4. The seven M0 questions

1. **Express cleanly?** Yes for the evidence-state/verifier claim family —
   five Bats of ~35 lines each, registry conditions only. Live agent-arm
   re-scoring is NOT expressible in First Flight (requires AI execution,
   M9); recorded as a finding, not forced.
2. **Language-neutral primitives?** Yes. A Go repository was judged from
   manifest facts, file presence, Git plumbing and one `go test` exit code.
   The engine contains zero Go knowledge.
3. **Reproducible from receipt/evidence?** Yes — BZ-1 replayed to identical
   verdict and identical evidence identities on the same machine; receipts
   pin repo, commit, Git and Terror Bat versions.
4. **Receipts easier to inspect than the manual experiment?** Yes.
   Establishing these facts manually took branch archaeology
   (`for-each-ref`, `merge-base --is-ancestor`, `cat-file`, filtered
   `log`); each receipt states claim → attack → oracle → verdict →
   evidence in one screen, digest-verifiable.
5. **Missing evidence / hidden assumptions exposed?** Yes — this is the
   headline finding: the empirical-evidence gap (synthetic-only results,
   unmerged branch, none on main) is now mechanically proven rather than
   folklore.
6. **Terror-Bat-specific machinery required?** **Zero engine lines.** Five
   YAML specs + this report. (The only Bat bug found anywhere in First
   Flight — a missing `fs.mkdir` setup in the shipped `orphan-process`
   demo — was caught by the self-attack suite before Bat Zero.)
7. **Reduced ambiguity or added ceremony?** Reduced. One-line verdicts,
   receipt-backed, replayable; ceremony is five small YAML files.

## 5. Kill / continue gate

**Decision: CONTINUE.** No kill condition (architecture §11 / packet §O)
tripped:

- not an ornate shell runner — pinning, worktree isolation, content-addressed
  evidence, operation logs, receipts and replay all did real work;
- no special-case Reusery engine code (0 lines);
- receipts beat the archaeology they replace;
- language independence held on a real Go repository;
- evidence is meaningfully stronger than ordinary logs (deduplicated,
  digest-verified, immutable, replayable);
- isolation added no operational risk (source clone verified unchanged);
- deterministic judgement never leaned on model opinion (no AI involved);
- nothing rebuilt that mature tools already provide (Git, Go, M2 supervisor);
- ceremony < assurance; no platform creep.

## 6. Limitations of this Bat Zero

- The agent-in-the-loop arms (the original experiment's core) remain
  unrescored: that needs AI execution (M9) and, for live-model numbers,
  Reusery-side Packet 16 work. Terror Bat's gap, honestly recorded — not
  papered over.
- BZ-5's Go verifier used the warm module cache; a cold machine would
  exercise the network (declared, UNENFORCED — the receipt says so).
- All runs are same-machine; cross-machine replay is later work (M7+).
