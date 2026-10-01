# Reusery Bat Zero (M0)

Reusery is the **first real specimen** of Terror Bat. Bat Zero is not a toy demo: it is the existing Reusery A / B / C\* / C experiment re-expressed as a Terror Bat, run through the Terror Bat pipeline at milestone **M6.5**, and used to decide whether Terror Bat continues at all (the kill/continue gate, architecture.md §10–§11).

The purpose of Bat Zero is to **test Terror Bat itself**, using a prior real experiment whose conclusion is a **reference result to inspect — not a predetermined answer**.

> **Bat Zero validates Terror Bat against a prior real experiment, not against a predetermined answer. The prior Reusery conclusion is evidence to inspect, not truth to preserve.**

## 1. What Bat Zero is

The Reusery experiment investigates whether reuse discipline (preferring mature existing dependencies over newly introduced equivalent functionality) actually holds under adversarial pressure. Bat Zero wraps that experiment in the standard pipeline:

```text
CLAIM      reuse discipline holds in the target project/agent workflow
ATTACK     the Reusery A / B / C* / C procedure (shadow-dependency pressure)
EXECUTION  supervised run in a disposable worktree, declared capabilities
ORACLE     deterministic conditions over diffs, dependency facts, logs
EVIDENCE   content-addressed artifacts of the run
RECEIPT    verdict + provenance + reproduction instructions
```

The `dependency-shadow` example in [bat-spec-v0.md](bat-spec-v0.md) §10 is an early sketch in this direction, not the final Bat Zero Spec.

## 2. Facts, placeholders, assumptions

> **RESOLVED (Windows First Flight).** P1–P6 were inspected and resolved
> from the reusery.dev repository and its full history, and A1–A4 were
> tested. Full findings, receipts, replay proof and the kill/continue
> decision: [bat-zero-report.md](bat-zero-report.md). Headline: the
> A/B/C\*/C arm table exists only as a self-disavowed SYNTHETIC dry-run
> reference on an unmerged branch (`packet/13.6-live-kill-test` @
> `af5e170`); no empirical results ever landed on main. The prior
> conclusion is therefore disposed of as **evidence insufficient** for
> empirical claims — the disposition its own text licenses. Bat Zero ran
> five receipted arms against main @ `ed0243b` with zero engine changes;
> gate decision: **CONTINUE**.

The exact Reusery procedure must be recovered from the Reusery repository before the Bat Zero Spec is written (M6.5 prep). This document deliberately separates the three kinds of statement:

### Facts already known

- The Reusery experiment exists and has variant arms labelled **A**, **B**, **C\***, and **C**.
- The experiment concerns dependency-reuse discipline (reuse vs. shadow/reimplementation pressure).
- It produced a manual conclusion. **The fact is that a conclusion was reached and recorded**; whether that conclusion was correct remains testable. It serves as a reference result / comparison baseline for Bat Zero, not as ground truth.
- Terror Bat's own reuse-first discipline (Constitution 11) is thematically downstream of this experiment.

### Placeholders requiring Reusery inspection

- **[P1]** Exact definitions of arms A, B, C\*, C: initial conditions, interventions, and measurements per arm.
- **[P2]** The target repository(ies)/fixtures the arms operated on.
- **[P3]** What evidence the manual experiment actually recorded, and in what form.
- **[P4]** The prior manual conclusion and the reasoning chain that supported it — recorded as a *reference conclusion*, with its supporting evidence re-inspectable, not as an accepted truth.
- **[P5]** Which parts of the procedure are deterministic (mechanically checkable) and which involved human or model judgement.
- **[P6]** Environment facts the manual experiment implicitly relied on (tool versions, network availability, model identities if agents were involved).

### Assumptions (must be verified or replaced during inspection)

- **[A1]** The experiment can be expressed as one Bat per arm, sharing a claim family, rather than one monolithic Bat.
- **[A2]** The arms' outcomes are at least partly judgeable by deterministic oracles (dependency manifests, diffs, command exit status), not only by reading prose.
- **[A3]** The experiment ran against repository state that can be reconstructed at a pinned commit for reproducibility.
- **[A4]** Any AI involvement in the original experiment is re-expressible as an adapter step (`AI agent tasks`, architecture.md §6) with declared capabilities — and if it is not, that is itself a Bat Zero finding.

If inspection contradicts an assumption, the contradiction is recorded and the Bat Zero design adapts; assumptions are not silently upgraded to facts.

## 3. Bat Zero success criteria

Bat Zero must answer seven questions. These are the success criteria referenced by the M1 entry conditions and the kill/continue gate:

1. **Can Terror Bat express the existing experiment cleanly?** — the Reusery arms become readable Bat Specs without contortions.
2. **Can the experiment run through language-neutral primitives?** — setup/run steps go via adapters and generic capture, not Reusery-specific engine code.
3. **Can another machine reproduce the experiment from its receipt/evidence?** — a clean checkout plus the receipt's reproduction block re-runs Bat Zero to a comparable verdict.
4. **Does the receipt make the conclusion easier to inspect than the manual experiment?** — a fresh reader understands claim, attack, result, and limitations from the receipt alone.
5. **Does Terror Bat expose missing evidence or hidden assumptions in the existing experiment?** — placeholders P3–P6 get concrete answers; gaps in the original evidence become visible findings, not lost history.
6. **How much Terror-Bat-specific machinery was required?** — measured honestly: lines of adapter code, custom oracle conditions, engine changes. Large numbers trigger kill criterion 2.
7. **Did Terror Bat reduce ambiguity or simply add ceremony?** — the honest overall judgement, recorded in the Bat Zero report.

The **Bat Zero report** answers all seven explicitly, with evidence references. It must also state which comparison outcome against the prior Reusery conclusion applies. Bat Zero is explicitly allowed to conclude any of:

- Terror Bat **reproduces** the prior conclusion.
- Terror Bat **weakens** the prior conclusion.
- Terror Bat finds the prior evidence **insufficient**.
- Terror Bat **contradicts** the prior conclusion.
- The comparison is **inconclusive**.

A contradiction or weakening of the prior conclusion is a legitimate — potentially valuable — Bat Zero result, provided the receipts support it. What is *not* legitimate is tuning Terror Bat until it reproduces the prior answer.

The report is the input to the kill/continue decision (architecture.md §11). A "no" on 1, 3, or 4, or a damning answer on 6 or 7, is allowed — and expected — to stop the project.

## 4. Sequencing

```text
M6    receipts exist and are readable
M6.5  Bat Zero:
        a. Reusery repository inspection → resolve P1–P6, test A1–A4
        b. write Bat Specs for the arms
        c. run through the pipeline; produce receipts
        d. write the Bat Zero report (seven answers)
GATE  kill / continue
```

No Bat Zero work begins before receipts (M6) exist — a Bat Zero without receipts cannot answer questions 3 and 4.

## 5. Known open points (deferred, not hidden)

- Location and access path of the Reusery repository → resolved at M6.5 step (a).
- Whether arms involving AI agents are in scope for Bat Zero v1 or deferred to M9-informed re-runs → decided after P5/A4 inspection; direction: include at least one deterministic arm in the gate itself so the kill decision never rests solely on model-involved runs.
- How much of the original manual evidence should be imported into the evidence store vs. referenced externally → decided at M6.5; direction: import what is needed for receipt questions 4–5, reference the rest.
