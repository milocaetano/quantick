# Quantick outside score rubric v1.0

This file is the scoring authority for `outside-score`. It grades what an
architect from outside the project would grade: where code lives, how a large
crate is bounded inside, how big its units are, what the production binary
carries, and what a change costs an agent. It exists to disagree with
`quantick-score` wherever that rubric's proxies (file lines, crate edges,
process evidence) drift from the code. Increment the version when a criterion,
threshold, weight, anchor or formula changes; compare scores only within one
version.

## Independence

1. Score before reading anything that grades Quantick: `quantick-score`, its
   rubric and reports, campaign scorecards, PR self-assessments, review
   verdicts and goal archives. The evidence is the code at the target SHA, its
   CI results and the measured rows below.
2. After scoring, read the latest `quantick-score` result for the same tree, or
   the nearest one with its SHA named, and print a divergence table. A gap
   above 1.0 on the overall score is a finding: say which rubric misreads the
   code, and which criterion of it would need to change.
3. A score of 9.0 or above is provisional until an assessor from another model
   family, or a human, reproduces it within 0.5 at the same SHA and version.

## Procedure

Accept an optional revision, default `origin/main`. Fetch, resolve the full
SHA and print it with the date. Export the tree read-only and measure it:

```sh
git archive <sha> | tar -x -C <dir>
python tools/outside_score/measure.py <dir>
```

The script owns every measured row's definition. Where `quantick-guards
--report` carries the same quantity, the report row wins and the script
cross-checks it. Do not edit files or build while scoring; use current CI.

Report, in order: `Quantick outside score v1.0`, target, full SHA, date; the
overall score and grade; a dimension table with score, weight and points; the
ledger (ID, score, measured value or evidence, what blocks the next point, and
one observable next-point condition); the divergence table; at most three next
moves ranked by overall points unlocked, then risk, then effort; unavailable or
unverified evidence. Commit a report meant for reuse as
`docs/quality/outside-score/<short-sha>.md`.

## Scale and calculation

Every criterion scores 0–5 on one shared meaning, so that a criterion's score
times two reads on the grade bands below:

| Score | Meaning |
| ---: | --- |
| 5 | Reference quality: what mature, long-maintained software does. |
| 4 | Very good: better than most commercial codebases. |
| 3 | Typical of a working production system. |
| 2 | Below typical: the weakness costs time on ordinary changes. |
| 1 | Poor: the weakness shapes most changes. |
| 0 | Absent, contradicted by the code, or harmful. |

A measured criterion takes the band its row falls in. A judged criterion
takes the highest anchor whose every condition the evidence meets.

```text
dimension score = Σ(score × weight) / (5 × Σ weight) × 10
overall score   = 0.45 × architecture + 0.40 × engineering + 0.15 × change cost
```

Round only displayed numbers, to one decimal.

| Overall | Grade |
| ---: | --- |
| 9.0–10.0 | A+: reference quality |
| 8.0–8.9 | A: very good |
| 7.0–7.9 | B: good, above a typical production system |
| 5.0–6.9 | C: typical production system |
| 0.0–4.9 | D: poor |

Caps: the overall score is at most 8.9 while any criterion scores 2 or less,
and at most 6.9 while default-branch CI is red at the target SHA.

## Architecture — 45%

| ID | Weight | Criterion | Score |
| --- | ---: | --- | --- |
| A1 | 25 | **Crate layering.** Dependency direction is compiler-visible and guarded; the domain is headless and deterministic; each domain crate has a second consumer in the workspace. | 5: all three, guarded. 4: guarded direction and a headless domain; some domain crate has a single consumer. 3: direction documented, not guarded. 2: a UI or I/O dependency inside the domain. 1: cycles between crates. |
| A2 | 20 | **Code in the right crate.** `ui_crate.ui_free_share_percent`: production lines of UI-crate files that never name the UI toolkit, over the UI crate's production lines. | 5: ≤10. 4: ≤25. 3: ≤45. 2: ≤65. 1: >65. |
| A3 | 20 | **Boundaries inside large crates.** The largest `impl_spread` (files one type's inherent `impl` blocks span) among crates over 20,000 production lines. `crate_visibility_per_kloc` is reported, not graded: it rewards spelling crate-private items `pub`. | 5: ≤4. 4: ≤8. 3: ≤20. 2: ≤35. 1: >35. |
| A4 | 10 | **Explicit composition.** Per-frame and per-event pipelines state their order in structure, not prose. | 5: registered stages with declared dependencies, and a test fails on a reorder. 4: stages in one declared list; the known hazards pinned by tests. 3: one sequence function, order documented at each step, at least one hazard pinned by a test. 2: order documented only. 1: order implicit. |
| A5 | 10 | **Harness out of production.** `harness_hooks` (distinct `QUANTICK_*` names production code reads, in any crate), and where capture, demo and automation hooks run. | 5: every hook compiles only under a cargo feature or `cfg(test)`. 4: ≤20 names, read in one launch module, none checked on a per-trade or per-frame path. 3: hooks live in dedicated modules and number ≤150. 2: hooks spread into domain-adjacent code, or >150. 1: a hook alters feed, order or persisted data in a release build without an explicit opt-in. |
| A6 | 15 | **Extension seams.** A new feed, bar type, indicator, layer or capability docks as a new file plus one registration line. | 5: guarded, and the last three additions docked that way. 4: ports and a guard exist; one family still edits a central match. 3: ports for most families, unguarded. 2: most additions edit central code. 1: no ports. |

## Software engineering — 40%

| ID | Weight | Criterion | Score |
| --- | ---: | --- | --- |
| E1 | 25 | **Unit size.** `fns.over_200.per_100k`: production functions over 200 lines per 100,000 production lines. | 5: ≤5. 4: ≤15. 3: ≤35. 2: ≤60. 1: >60. |
| E2 | 25 | **Tests.** `tests.per_production_line`, and flaky failures among the last 30 default-branch CI runs. | 5: ratio ≥0.6, failure paths and injected clocks, no flaky failure. 4: ratio ≥0.6, at most 2 flaky failures. 3: ratio ≥0.25, at most 5. 2: ratio <0.25 or more than 5. 1: tests do not gate merges. |
| E3 | 15 | **Failure discipline.** `panic_sites.per_kloc`: `unwrap()` and `panic!`, `unreachable!`, `todo!`, `unimplemented!` per 1,000 production lines; `expect_sites` reported beside it. | 5: ≤1. 4: ≤3. 3: ≤8. 2: ≤15. 1: >15. |
| E4 | 15 | **Performance evidence.** Hot paths carry declared budgets on dense fixtures. | 5: budgets checked in the ordinary test run and measured at a recent SHA. 4: measured and current, not gated. 3: measured once. 2: claimed without numbers. 1: none. |
| E5 | 20 | **Delivery health.** Pinned toolchain; format, lint, build and test on every PR on two operating systems; dependency policy enforced; default branch green at the SHA. | 5: all. 4: one missing. 3: two missing. 2: CI exists but does not gate merges. 1: no CI. |

## Agent change cost — 15%

The cost an agent pays to make a correct change: what it must read, and how
long it waits to learn whether the change compiles and passes.

| ID | Weight | Criterion | Score |
| --- | ---: | --- | --- |
| C1 | 60 | **Read cost per change.** Production lines of the files a change touches plus the in-crate modules those files reference, per merged PR. | 5: recorded per PR by CI, and flat or falling over the last 20 PRs while production lines grew. 4: recorded per PR by CI. 3: a committed script computes it for any PR. 2: only partial or ad hoc measures, such as instruction weight or tokens noted in PR bodies. 1: intent only. |
| C2 | 40 | **Edit-loop time.** Incremental `cargo test -p <crate>` time after touching one file in each of the three largest crates, on a stated host. | 5: budgets checked on a schedule. 4: measured and committed at a SHA under 30 days old. 3: measured and committed once. 2: noted only in PR bodies or conversation. 1: none. |

## Changing this rubric

A threshold moves only with a version bump and a stated reason, never inside a
campaign that is scored by it. Calibrate a measured band on outside projects
with `measure.py`, not on Quantick: band 3 should hold well-kept large
projects, band 5 reference-quality ones. Version 1.0 was calibrated on
ripgrep, nushell and rerun; the rows and the bands they fall in are in
`docs/quality/outside-score/d3d4b23d.md`.
