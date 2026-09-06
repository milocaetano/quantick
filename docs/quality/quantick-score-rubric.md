# Quantick score rubric v1.0
This file is the scoring authority for `quantick-score`. A version change is a
measurement change: increment the version when criteria, weights, maturity
anchors, A+ gates or formulas change.

## Assessment procedure
Accept an optional git revision and default to `origin/main`. Fetch when
network access is available, resolve the target to a full SHA and print it with
the assessment date. Inspect that revision with read-only repository and GitHub
operations. Do not edit files, mutate GitHub or build the project while
scoring; use current CI and committed execution evidence. Prefer evidence in
this order:

1. executable behavior and current CI results;
2. tests, guards, schemas and generated contracts;
3. implementation cited by `file:line`;
4. architecture records and operational documentation;
5. prose claims, which prove intent only.

An open issue is a lead rather than proof of a current defect. Confirm it in
the target or label the claim unverified. Use previous reviews only as
supporting evidence; `ai-review` grades a diff and is not a numeric project
score.

For every criterion report the score, strongest evidence, what prevents the
next point and one observable condition that earns it. Rank at most three next
moves by points unlocked, then risk reduced, then effort. Prefer a smaller
enforceable change when it earns the same point as a rewrite. Say whether a
move fits an existing issue or needs a new one, without changing GitHub.

Report, in order: `Quantick score v1.0` with target, full SHA and date; the
overall number and grade; a dimension table with score, weight and points; each
A+ gate as `PASS` or `BLOCKED`; the full criterion ledger; the ranked next
moves; and unavailable, stale or unverified evidence. The ledger columns are
ID, score, evidence, reason the next point was not earned and its observable
next-point condition.

## Calculation
There are 20 criteria worth five points each, for 100 points total.

| Dimension | Criterion count | Maximum points | Weight |
| --- | ---: | ---: | ---: |
| Sustainable engineering | 10 | 50 | 50% |
| Agentic development | 5 | 25 | 25% |
| AI-operable product | 5 | 25 | 25% |

For each dimension:

```text
dimension score = earned points / maximum points × 10
```

For the overall score:

```text
overall score = total earned points / 10
```

Round only displayed dimension and overall scores to one decimal place. Use
unrounded points for all calculations.

## Shared maturity anchors
Apply these anchors to every criterion. The criterion's target below defines
what is being matured.

| Score | Required state |
| ---: | --- |
| 0 | Absent, contradicted by the target, or currently harmful. |
| 1 | Intent or isolated experiment exists; the normal path does not rely on it. |
| 2 | Partially implemented or manually followed; important paths or evidence are missing. |
| 3 | Implemented on the normal path with direct current evidence. |
| 4 | Enforced or tested, including a relevant failure or boundary path. |
| 5 | Independently reproducible at the target revision, maintained against drift, and demonstrated on the next realistic variant or operating condition. |

Evidence rules narrow the anchors:

- Plans, issue text and design prose can score at most 1 for an implementation
  criterion unless current code or execution evidence confirms them.
- Current implementation without a test, guard, CI result or measured artifact
  can score at most 3.
- Evidence without a target SHA or from a different revision is stale and can
  score at most 2 until reconfirmed.
- Unavailable evidence earns no assumed points. Record the limitation.
- One artifact may support several criteria, but each score must explain the
  distinct property it proves. Do not deduct twice for the same consequence.

## Sustainable engineering — 50 points
| ID | Criterion | Five-point target |
| --- | --- | --- |
| SE1 | Architecture boundaries | Dependency direction and ownership boundaries are explicit, compiler-visible where possible, mechanically guarded and proven by a second implementation or consumer. |
| SE2 | Modular change surface | Responsibilities have named owners; representative features land mainly as new focused units plus bounded registration edits; size and blast-radius ratchets prevent renewed concentration. |
| SE3 | Extension seams | The next feed, bar type, indicator, tool or workspace owner docks through a stable port and registry without editing unrelated variants. |
| SE4 | Correctness and determinism | Domain behavior has independent fixtures, error-path tests and deterministic replay; chart, backtest and bot share the same engine behavior. |
| SE5 | Test strategy | Tests sit at the lowest useful layer, exercise relevant failures and boundaries, avoid implementation-derived expectations, and remain runnable alone. |
| SE6 | Hot-path complexity | Per-trade, per-depth and per-frame work is independent of session length, measured on representative dense fixtures and guarded against regression. |
| SE7 | Bounded state and pressure | Queues, journals, caches and retained market history have stated caps, observable overflow behavior and no silent loss under supported bursts. |
| SE8 | Safety, errors and data honesty | Invalid input fails closed with typed context; security boundaries are tested; inferred, stale, dropped or incomplete market data is surfaced accurately. |
| SE9 | Runtime diagnosis | Health, latency, backlog, degradation and recovery are observable enough to separate a data problem from rendering, transport or domain failure. |
| SE10 | Delivery and dependency health | A clean pinned environment can format, lint, build and test the workspace; default-branch CI and dependency policy are current and enforced. |

## Agentic development — 25 points

| ID | Criterion | Five-point target |
| --- | --- | --- |
| AD1 | Repository legibility | A fresh agent can find authoritative architecture, ownership, commands and constraints without reconstructing them from conversation history. |
| AD2 | Objective-to-PR workflow | Issues, missions, acceptance evidence, isolated worktrees, reviews and delivery form one documented and enforced path with durable state. |
| AD3 | Headless verification | Important behavior can be driven and inspected without manual UI input through deterministic fixtures, fakes and standalone tests below the app shell. |
| AD4 | Enforced review gates | Correctness, architecture, completeness and relevant visual or trader reviews cover the exact shipped diff and stale approvals are rejected. |
| AD5 | Unattended continuity | An agent can resume after context loss, recover from tool or CI failure, track multi-PR dependencies and stop at explicit authority boundaries without relying on repeated human prompts. |

## AI-operable product — 25 points

| ID | Criterion | Five-point target |
| --- | --- | --- |
| AP1 | Discovery and reachability | Every supported user behavior has a stable named capability discoverable from a versioned registry rather than existing only behind a click. |
| AP2 | Typed contracts | Requests, results, errors, permissions, schemas and compatibility rules are machine-readable, versioned and guarded against drift. |
| AP3 | Authority and safety | Authentication, least privilege, sensitive capability defaults, consent, limits and audit evidence are enforced at the product boundary and tested against bypass. |
| AP4 | Retry, recovery and observation | Mutable calls have explicit retry semantics and deduplication or safe readback; agents can wait for change, diagnose partial failure and reconcile state after interruption. |
| AP5 | Semantic product state | Agents can inspect stable semantic state, correlate it with visible output and receive bounded evidence that distinguishes known, inferred, stale and unavailable data. |

## Grade bands and A+ gates

| Numeric score | Grade |
| ---: | --- |
| 9.0–10.0 | A+ when every A+ gate passes; otherwise `A (A+ blocked)` |
| 8.0–8.9 | A |
| 7.0–7.9 | B |
| 6.0–6.9 | C |
| 0.0–5.9 | D |

All A+ gates must pass:

1. Overall numeric score is at least 9.0.
2. Every dimension score is at least 8.0.
3. Default-branch CI is green at the assessed revision.
4. No confirmed reachable critical defect permits unauthorized real trading,
   corrupts persisted or financial state, materially falsifies market data, or
   bypasses an authority boundary.
5. Every product capability reachable by an agent either supports safe retry
   and deduplication or declares a non-retryable contract with enough readback
   to reconcile an uncertain outcome.
6. Scalability claims for supported live workloads have current measurements,
   stated rates and bounded-state evidence at the assessed revision.

An assessment must print every gate as `PASS` or `BLOCKED` with evidence. It
must not infer a pass from the absence of a known issue.

## Comparisons and target gaps

Compare scores only when both reports use this rubric version. Show point
deltas by criterion; do not claim improvement from a rounded overall number.

The next-point condition must be observable and bounded. Good conditions name
a test, guard, registry entry, measured threshold or public artifact. "Improve
the architecture", "add more tests" and similar directions are not target
conditions.
