# Unfinished mission snapshot

Preserved for evaluation only. Unchecked criteria and historical failures remain; this is not a completion archive.

# E1P mission

tier: high | objective: Decompose settled order-flow projection into explicit pure algorithm stages while preserving exact outputs, deterministic ordering and measured cost.

Source: issue502 at campaign a6e554386b037bb2ec49fe29ff6494e4fef0460a. Independent source-first source/map/D4-D7 reconciliation completed; report SHA25649024916924FA7AED9E239723AD7977C6A0E005941B1C15D057B4A3663A09016. Baseline fixture/test preparation precedes runtime implementation.

Preparation freeze: C:/src/outside-eight-coordination/e1p-preparation-result.md records six additive test files, unchanged original source/index/manifests, exact W1/W2/W3 fixtures and output counts, toolchain, SHA256 source/binary/output manifests and separate baseline allocation observations. Complete baseline public-output bytes live in e1p-baseline-output; copied optimized baseline executables live in e1p-baseline-bin; frozen harness source is e1p-frozen-tests. No paired timing or runtime algorithm change occurred before this freeze. Private stage assertions and test-owner relocation remain implementation obligations.

## Ledger and original acceptance IDs

R1: Real algorithmic decomposition, not file-only movement: six cohesive stages, explicit immutable inputs/owned intermediates; project_settled below200 lines and no oversized replacement. Source Scope paragraphs1-3 and A1. -> A1.
R2: Preserve heat geometry and weighted slot semantics: one P99 quantity per drawn run, hidden heat cleared after reference and before drop/cap accounting, deterministic strongest-wall ties. Source stages1-3/A2. -> A2.
R3: Preserve aggression/reduction semantics: settled/live seam exactly-once evidence, correlation before filtering/refinement/folding, distinct session print/summary scales, pane budgets, exact volume, output order, gap clipping/sorting and all diagnostics. Source stages4-6/A3. -> A3.
R4: Existing signatures/outputs and engine cache cadence remain compatible; reuse fold/model/tiers/shared settled-live rules. No generic pipeline, fake I/O or giant replacement owner. Source Scope. -> A1,A3,A5.
R5: Existing golden/parity fixtures remain unchanged; add/move named owner tests proving load-bearing ordering/conservation, including seam/hidden heat/cross-grouping/bar boundaries. Source A4/assignment. -> A4.
R6: Immutable history; no new HashMap/history clone/extra traversal; paired exact-base dense-rebuild timing and allocations, existing live-half control, no unmeasured performance claim. Source A5. -> A5.
R7: Actual before/after responsibility and call coupling plus localized-change probe, files/lines/registration sites and moved headless tests; no automatic score or file-split credit. Source A6/assignment. -> A6.

A1-A6 remain exactly the issue's stable acceptance IDs and full semantics; evidence will live in a linked mission evidence artifact and the exact-head PR.

## Decisions and assumptions under campaign D1

D1: Original campaign user delegates all defaults except main/settings. Choose internal stage placement and conservative performance protocol autonomously; never relax existing acceptance, score or public/financial invariants.
D2: Work from reviewed campaign a6e55438 in feat/settled-projection-stages. External origin/main advanced to bcfb5f19 during fetch; do not adopt it into this child implicitly. Later synchronization is a separate reviewed campaign action.
D3: Prior MT5 and SER1 retry exhaustion blocks those tasks and feed registry, not this source-disjoint P2 task. No retry reset.
S1: New stage types remain private to projection; no renderer or engine cache behavior changes. Repository answers placement and naming, no user decision needed.
S2: Ordinary output-preserving private refactor has no intended visual behavior change; all caller/public outputs require exact differential evidence. If visible behavior actually changes, apply visual/UX gates before delivery.
S3: Root observed the existing dense benchmark calls whole project (settled+live), not settled alone, and neither benchmark measures allocations. Keep those fixtures unchanged; add matched instrumented coverage for settled and allocation proof before runtime edits. Do not mislabel existing elapsed-only output as allocations.

## Proposed boundaries for independent reconciliation

1. Resolve grouping/early exits/window/coverage and run sweep once.
2. Project grouped runs into owned drafts plus one quantity per drawn run; accumulate/consume SlotHeat in original order without additional history pass.
3. Consume drafts/reference quantities, calculate reference then hidden gate/cap/normalization; return heat output and drop count.
4. Cluster settled tier and partition liquidity reductions at original live boundary, preserving live_events order.
5. Correlate before filter; refine into marks, apply print/summary references, chart fold budget and exact counters; reuse existing shared helpers.
6. Clip/sort coverage gaps with original coverage/leading-absence rules, then assemble unchanged SettledProjection.

These are proposed algorithm boundaries, not a framework or blanket context object. Preserve Decimal operations/order, stable sorting tie behavior and original range iterators.

## Gates and closing obligations

G1 English, determinism, one engine, data honesty, dependency/headless/size/context/UI-free guards and frozen seven measurement files.
G2 Test-first deterministic fixtures and baseline before implementation; all original tests unchanged.
G3 Per-rebuild hot path measured flat-or-better against identical exact-base fixture/build with allocation counts and live-half control; freeze protocol before measurement.
G4 Full ordered fmt/clippy/build/test before code commits; affected extra checks; exact-head final CI.
G5 Full current architecture and AI review; independent source-first completeness then full delivery review; no markers without canonical producers.
G-AI1..4 exact required block and D1..D8 what-done-means will be carried verbatim into the live mission.
C1 Archive and draft PR with source/evidence before reviews; current durable reports, final readiness and canonical verifier; reviewed campaign-only head-pinned merge, explicit issue/Project reconciliation. No main merge.

## PF2-PF4 explicit owner and gate completion

Proposed private modules and typed results (names are part of this plan, no public API change):
- projection/window.rs: resolve_window -> ResolvedWindow/early empty result; sweep_window -> WindowLiquidity containing the existing coverage vector and GroupedLiquidity. Own tests window_tests.rs for early returns, both-pane depth gate, retention/known-book endpoint and single sweep.
- projection/heat.rs: project_geometry -> HeatDrafts (DraftCell, SlotHeat stay private here; drafts and one-per-drawn-run quantities); finish_heat -> ProjectedHeat (cells, reference, dropped count). Own heat_tests.rs: one run across bars/lane, weighted spans/generation, fixed/P99 references, hidden-before-cap and stable tie order.
- projection/settled_flow.rs: cluster_settled -> SettledFlow (TierClusters, settled events, live events and seam); finish_settled -> SettledMarks (aggressions, event primitives, scale values and counters). Own settled_flow_tests.rs: seam-1/seam/seam+1 stable event partition and removed quantity, correlation-before-filter, independent scales and fold conservation.
- projection/gaps.rs: project_gaps -> existing Vec<GapPrimitive>; own gaps_tests.rs for leading/open/clipped/no-coverage/hidden/sort rules.
- Shared settled/live helpers remain one implementation. If sibling stage calls would create a parent-child dependency cycle, move the exact shared correlation/filter/event placement into projection/evidence.rs, consumed by both paths, with existing semantics and owner tests; never copy helper implementations. Reuse tiers/fold/model and existing grouping rules, do not reimplement them.
- projection/tests retains integration/public parity and the unchanged original seam/cross-grouping/bar-boundary cases. Move cohesive existing tests to their owner with explicit import-only relocation accounting, not fixture/assertion rewriting.

Proof destinations: docs/quality/settled-projection-stages-evidence.md (before/after owner/caller/probe inventory, source/test identities and exact commands); source-first and measurement raw artifacts externally at C:/src/outside-eight-coordination/e1p-* with durable child/PR links; private stage tests beside owners. A5 exact output proof compares every SettledProjection, LiveMarks and joined HeatmapProjection field/vector order, scales/events/diagnostics from baseline/candidate; no new PartialEq public API is required solely for this harness.

G6 inherited preservation: unchanged safety fixes, order-ticket/replay, public/financial contracts and engine cache semantics are verified by full declared-base diff review, unchanged relevant source identities and applicable existing full-workspace regressions. If touched or uncertain, targeted affected tests are required; no blanket evidence reuse. Frozen seven measurement files must retain their charter blob IDs. Parent E1 aggregate <=15 oversized functions/100k remains a campaign obligation at integrated outcome, not a child score claim.
A6 evidence explicitly counts root/app-owned decisions (expected no new ones), concrete engine and app render consumers, cross-owner access sites and localized probe files/lines/tests/registrations; actual measured result may show no improvement for a selected probe.
G7 UI classification follows preimplementation decision D7: headless public+joined parity and existing engine projection/cache regressions demonstrate unchanged observable outputs; no new user action/control/hook. This is applicability, not visual PASS. If the actual diff changes a visible surface, run UI-harness/visual-QA/trader-UX gates before acceptance.

## Verbatim issue request

<!-- campaign-task:milocaetano/quantick#472/E1P -->
## Context

E1 #482 child. Closed #375 moved files unchanged and expressly did not decompose bodies; it does not discharge this algorithmic outcome. project_settled in crates/orderflow/src/projection.rs remains a large multi-stage algorithm. This is not Sans-IO: there is no socket/clock responsibility to separate.

## Scope

Equivalent non-app responsibility: settled projection rebuild currently owns resolved viewport/gates/coverage, grouped liquidity, cell drafts/references/caps, aggression clusters, settled/live seam reduction ownership, evidence correlation, folded marks and gap assembly in one function.

Use small concrete owners/functions and typed intermediate values for: (1) resolve window/grouping/coverage and sweep runs; (2) project run geometry and weighted bar summaries into DraftCell and exactly one reference per drawn run; (3) resolve reference/apply visibility-cap/normalize; (4) cluster aggression and partition reductions at seam; (5) correlate evidence before filter/refine/fold budgets; (6) clip/sort gaps and assemble existing SettledProjection. Reuse fold/model/tiers and existing settled/live shared rules. No generic pipeline framework, fake I/O abstraction or replacement giant helper.

Interface: pure stages with immutable history inputs and explicit owned/borrowed intermediate values; existing project_settled/project_live signatures and outputs remain compatible. Engine cache-validity/rebuild cadence stays in orderflow/engine.rs; no new cache owner or history traversal.

## Acceptance criteria

- [ ] A1: project_settled is below 200 lines through algorithmic boundaries, with no extracted oversized replacement; decisions/intermediates/tests assigned to named owners.
- [ ] A2: Preserve P99 one quantity per drawn run; hidden heat cleared after reference calculation and before cap accounting; exact weighted slot averages and deterministic strongest-wall ties.
- [ ] A3: Preserve correlation-before-visibility, exactly-once settled/live reduction evidence, separate session-print/summary scales, pane folds, volume conservation, output order, gap clipping/sorting and every diagnostic counter.
- [ ] A4: Existing goldens/parity fixtures unchanged; add stage tests for load-bearing ordering and conservation. Existing seam, hidden-heat reference, cross-grouping and bar-boundary cases remain.
- [ ] A5: No input mutation, new HashMap, intermediary history clone or extra traversal. Paired existing dense rebuild benchmark reports timing/allocations against exact base; live-half benchmark remains a control. No performance claimed before measurement.
- [ ] A6: Report actual responsibility/call-coupling and localized-change probe before/after; no file-split or automatic rubric credit.

Dependencies: E1 #482 parent; source-disjoint from MT5. Scheduling priority P2 after D11 MVU first and MT5/registry sequence; no false technical dependency on sockets. Frame stage work may consume the existing projection output without waiting for this extraction. Original E1 aggregate criterion remains mandatory.

## Campaign assignment

Campaign: https://github.com/milocaetano/quantick/issues/472. Authority/priority: D11 https://github.com/milocaetano/quantick/issues/472#issuecomment-5674457925. Owner class: autonomous. Tier: high (state, boundary and hot-path correctness). One isolated mission/PR from reviewed origin/campaign/outside-eight; no main/settings writes. Full source-first independent mission preflight, moved/additive tests, ordered fmt/clippy/build/test, applicable UI/UX/performance checks, exact-diff architecture/AI/delivery reviews and exact-head green CI. Preserve all CLAUDE.md invariants, original safety fixes, ticket/replay, public/financial contracts, guards and frozen seven measurement files. No score awarded by this task.

Evidence: this issue, head-pinned PR reports/CI and parent checkpoint. Issue/Project IDs, owner, branch/worktree, PR/head/base: null until claim/readback. Initial operation/repair counters: 0/0, thereafter cumulative. Report raw before/after coupling, app/root-owned decisions, moved headless tests, affected consumers, and additive probe files/lines/registration sites; distinguish measured deltas from estimates. No new god object or universal dispatcher, renamed roots or file-only moves as acceptance.

## E1P pre-implementation defaults under campaign D1

These defaults answer the independent source-first questions before runtime edits or measurements. Full issue502 A1-A6 and campaign criteria remain unchanged.

D4 — Performance protocol: retain both existing ignored benchmarks and their exact fixtures/loops (whole projection: 3 warmups/30 calls; live-half: prebuilt settled,3 warmups/60 calls). Label whole projection honestly as settled+live+compose. Add matched settled-only and allocation observations outside timing. Freeze the same instrumentation and fixture code in exact-base and candidate builds. Fifteen alternating base/candidate measured pairs after three warmup pairs, no sample removal. Report raw times, per-pair ratios, median, nearest-rank p95 and per-side population CV; validity requires each CV <=5%, flat-within-measurement tolerance requires median <=1.05 and p95 <=1.10. This is a tolerance, not proof of zero overhead or a speedup. Report paired medians and tails separately. Allocation count and bytes per operation must not increase; setup, test harness, logging and destruction boundaries must be explicit. Allocation observations are separate from timed rounds. No automatic rerun on failed/invalid output.

D5 — Supplementary dense depth coverage: add a deterministic matched-base/candidate fixture with book snapshots, changing quantities across many levels, reductions, coverage interruption/resynchronization, bars and live seam, with enabled depth/aggression/gaps. Include depth geometry, reference/cap and gap stages actually bypassed by the existing bubbles-only workload. Freeze exact fixture dimensions/output checks before runtime edits and retain no-depth cases. Public settled output and joined output parity includes every diagnostic/scale/event field, not just mark count. An additive benchmark does not replace either existing required benchmark.

D6 — Cost/probe: record physical function span separately from frozen long-function metric; public signatures/callers, responsibilities, private typed fields passed, cross-owner call sites, allocations/traversals, moved tests and per-owner file/line counts. Use an external scratch-only deterministic strongest-wall tie-policy modification as an actual before/after localized-change probe, count changed owners/files/lines/tests/registration sites, and never ship the probe behavior. Equal probe cost is reported honestly; no required artificial improvement or automatic grade.

D7 — UI applicability: this high-tier task changes no intended user-visible output/control. Existing exact public/combined projection parity and headless fixture results are primary output evidence; no new action or UI surface means no new hook requirement. Reviewers must inspect actual delta for a visible change. Any changed visible semantics require applicable UI-harness/visual-QA/trader-UX proof before delivery; no visual PASS is preclaimed. Existing engine cache cadence, renderer registry and public signatures are unchanged constraints.

No new user question is needed: these are engineering defaults delegated by campaign D1. Missing proof stays unmet. No main/settings change.



## Executable acceptance checklist

**Tier:** high — deterministic hot-path algorithm and evidence/volume boundaries.
Evidence destination for every item: docs/quality/settled-projection-stages-evidence.md and head-pinned child PR reports.

- [ ] **A1** — project_settled below200 lines, no oversized replacement, six real algorithm boundaries with concrete owners/intermediates/test ownership; compatible public interfaces/cache owner. *(R1,R4)*
- [ ] **A2** — One reference quantity per drawn run; exact weighted slots, hidden heat reference-before-hide-before-cap, deterministic wall ties and normalized output unchanged. *(R2)*
- [ ] **A3** — Exact correlation/visibility/seam/scales/folds/conservation/output/gaps/diagnostics and public joined output preserved. *(R3,R4)*
- [ ] **A4** — Existing goldens/parity fixtures unchanged; additive/moved owner tests prove ordering and conservation, seam/hidden heat/cross-grouping/bar boundaries. *(R5)*
- [ ] **A5** — No input mutation/new HashMap/history clone/extra traversal; matched exact-base dense whole/settled/depth timing+allocation and existing live control satisfy frozen D4-D5. *(R4,R6)*
- [ ] **A6** — Actual before/after responsibilities/call coupling, app/root decisions, consumers, moved tests and scratch-only localized probe counts reported honestly. *(R7)*
- [ ] **G1** — English, deterministic/headless/dependency/data-honesty/guard/frozen-measurement invariants verified.
- [ ] **G2** — Pre-edit deterministic baseline/fixtures, preserved old assertions, source-first preflight linked.
- [ ] **G3** — Per-rebuild hot path performance measured under D4-D5; live control no regression; no unmeasured speed claim.
- [ ] **G4** — Full ordered fmt/clippy/build/test and exact-head final CI green; applicable extra checks complete.
- [ ] **G5** — Current architecture review resolved; independent source-first and full delivery reviews complete under canonical producers.
- [ ] **G6** — Inherited safety/ticket/replay/public-financial/cache obligations proven by actual scope review and applicable regressions; E1 aggregate explicitly remains parent-owned.
- [ ] **G7** — Actual-delta UI applicability checked; exact public/joined parity and cache regressions, plus UI/visual/UX evidence if visible behavior changes.
<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

G-AI evidence destinations: current child PR durable report, shared thread-list result and validated private projection.

## Closing steps
- [ ] **C1** — Draft PR and reviewed mission archive/source/evidence published.
- [ ] **C2** — Full independent delivery PASS at current key after architecture/AI.
- [ ] **C3** — Exact-head green CI, readiness and canonical mission_ship_gate PASS.
- [ ] **C4** — Head-pinned reviewed merge into campaign/outside-eight only, ancestry/issue/Project proof; return control to campaign.

## What done means
<!-- what-done-means:v1 -->
- **D1** — The open PR is non-draft and matches branch, head and base.
- **D2** — Every registered CI check is green at that head.
- **D3** — Architecture has a current projection and durable PASS report.
- **D4** — Delivery has both when applicable; only bounded `small` is exempt.
- **D5** — AI has current completion, a durable report and zero listed threads.
- **D6** — The PR body carries current-head evidence and report URLs.
- **D7** — The final verifier publishes and verifies this literal reconciliation.
- **D8** — Only the user merges to `main`.
<!-- end what-done-means:v1 -->
