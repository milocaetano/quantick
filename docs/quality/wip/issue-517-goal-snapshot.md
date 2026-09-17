# Unfinished mission snapshot

Preserved for evaluation only. Unchecked criteria and historical failures remain; this is not a completion archive.

# SER1: extract retained series and shared streaming fold

**Objective:** Extract the retained chart series lifecycle into a focused headless owner and share its streaming fold with production backtest while preserving outputs, provenance, execution ordering and retention policy.
**Why:** Deliver one real ownership boundary toward the user's whole-app modularity and additive-extension request, with executable regression proof. This child does not complete aggregate #478/#479 or establish a score improvement.
**Tier:** high — deterministic engine-adjacent lifecycle, hot paths, shared consumers and financial effect ordering.
**Status:** proposed pre-edit mission revision 1; all implementation, validation, review and closing criteria below remain unchecked.

## Identity and chronology

Issue: https://github.com/milocaetano/quantick/issues/517; stable campaign key SER1; parent https://github.com/milocaetano/quantick/issues/472; aggregate #478/#479.
Declared reviewed campaign base: a5c871e40bcab5253925b830bc09a746f40d3841 on origin/campaign/outside-eight. Read-only source inspection used feat-bar-selection-registry HEAD 75a8b8bd7cf25e36a03fb4837f23d4708b986bf8. Both trees were read back as 3a09773db1128882d1227ba0ff431d35b55a4656. Branch commit and integrated commit are distinct; no unmerged stacking.
Coordinator reports new child worktree C:/src/quantick-worktrees/feat-shared-series-owner, branch feat/shared-series-owner, created at a5c871. Bootstrap receipt: guards PASS 2026-09-16T18:02:45Z, app all-target check PASS 2026-09-16T18:06:00Z, session 4904 exit 0; private tier/base recorded after PASS, source still clean at a5c871/tree3a09773d. Coordinator claim: https://github.com/milocaetano/quantick/issues/517#issuecomment-5702196454. These are coordinator-reported actual readbacks, not executions by this planner. This planner has written no repository file, run no build and made no GitHub mutation.
Existing series-owner-issue.md, series-owner-journal.md, series-owner-result.md and checkpoint-55-manifest.md were inspected. They retain the earlier design and its historical independent-inspection claim. No standalone independent design report was found in the targeted scratch search. That historical claim is not the required current source-first completeness PASS.
Next: independent source-first pass reads the retained sources before this map, records hashes/revision and resolves gaps before code. Then mission persistence by the owning coordinator; then characterization tests before implementation. No past exception or retroactive test-first claim applies.

## Request ledger and source map

Stable IDs preserve the issue's existing R1-R7/A1-A7.
- **R1:** Move actual retained-series state, decisions and transitions to quantick-series, not merely files. Source: Context whole-app clarification; Scope paragraphs 1-2; issue A1. Includes named methods/borrowed reads, no reverse edge, no AppContext/universal bus/blanket forwarding/Deref.
- **R2:** Desktop RetainedSeries and production backtest SeriesFold share the streaming path without another retained tape; prove a retained headless second consumer. Source: Scope paragraph 1; issue A2. Runner effect ordering maps jointly to R5.
- **R3:** Preserve BarBuilder and registered BarConfiguration as the sole extension port; seventh fixture traverses both owners and lifecycle paths without identity dispatch. Source: Scope paragraph 1; issue A3; user purpose “facil de cdirar novas fetaures”.
- **R4:** Freeze deterministic domain behavior before changing it, moving domain tests with ownership. Source: Scope paragraphs 2-3 and issue A4. Covers all-family outputs, boundaries, deal readings, reset, footprints, seed/prepend, revisions, retention policy, wire/golden compatibility.
- **R5:** Preserve app-owned UI/effect behavior and financial ordering. Source: Scope paragraphs 2-3 and issue A5. Includes replay/reset/seek, drawing shifts/reanchor, workers/indicators/orderflow, feed scheduling, venue OHLCV prefix, paper/strategies and unchanged user actions.
- **R6:** Preserve hot-path and memory behavior with measurement. Source: issue A6; Scope paragraphs 1 and 3. No per-print event allocation layer, lock, registry lookup or tape copy; disabled footprint work/storage absent; reads borrow; deliberate retained-input rebuild.
- **R7:** Prove actual responsibility transfer under unchanged measurements without broad completion/score claims or weakening safeguards. Source: Context quantified limited estimate; issue A7; Scope exclusions; Attributed request plus closing bounded-child statement. #412/#318/#442/#471 remain separate.
Operational Execution/dependencies/Gates/Closing clauses map once to G1-G9/C1-C5 below; archive prohibition is retained verbatim and explicitly superseded by D2.

## Decisions under standing delegation

- **D1:** Campaign D1 authorizes deciding defaults and implementation; D6 permits independent parallel missions (coordinator default up to three, serialized merges). Use the issue's two-owner design, existing engine registry, focused crate and borrowed API. No new user question: these are settled scope or reversible implementation choices. Strongest executor gpt-6-astra per issue; independent reviewers do not implement their reviewed patch.
- **D2:** The source's “No new tracked execution evidence/GOAL archive” is historical and superseded by authenticated user grant https://github.com/milocaetano/quantick/issues/472#issuecomment-5700791807 (milocaetano; 2026-09-16T16:19:09Z). Commit the required mission archive as last implementation/evidence commit before reviews under current workflow. Keep raw logs external and concise results/report links on PR. #507 is no prerequisite solely for that old conflict. No tests/reviews/authority waiver.
- **D3:** Retention stays exactly current: no limits, eviction, new persistence or #412 policy. No indicator projection/control decomposition or financial product feature. Keep UI float-to-positive-Decimal conversion in app because drawing-price callers use it.
- **D4:** Performance gate uses the deterministic dense fixture protocol below; 5% is a predeclared noise ceiling for median paired elapsed ratios, not a regression allowance. Demonstrated regression, repeated positive signal or unexplained memory growth requires repair. No post-result threshold adjustment.
- **D5:** Port-carving is an explicit first slice: SeriesFold owns streaming builder and forming ladder; RetainedSeries owns retained lifecycle and composes it. Existing consumer edits are necessary integration, measured openly; claim registration-only addition for a future bar implementation, not for this extraction.
- **D6:** On future campaign-base movement (including #505), reconcile composition/Cargo/guard graph overlap and invalidate affected comparisons/review keys. Freeze/review latest fetched declared base before integration; never main merge/settings changes.

## Assumptions

Operational amendment D7 was authorized and published before guard edits:
https://github.com/milocaetano/quantick/issues/517#issuecomment-5702819975.
The extension boundary follows the actual RetainedSeries, SeriesFold and
FormingFootprint owners across app/src and series/src. Their exact caps may
sum to no more than the replaced ChartState cap of 571; QuantickApp remains
capped at 10210, with no cross-root growth transfer. Existing exact app shapes,
alias/macro/exclusion protections and fail-closed scans remain. Independent
architecture, AI and delivery review must examine this contract amendment.
Original source/ledger and R1-R7/A1-A7 are unchanged; original map hash is retained
in the coordinator's preflight evidence. Direct RetainedSeries consumer naming
avoids any new alias exception.

Operational repair amendment D8 was published before the baseline adjustment:
https://github.com/milocaetano/quantick/issues/517#issuecomment-5703451382.
SER1-PERF-001 batch 1 / attempt 1 restores single-query borrowed batch seeding
in retained reset/rebuild/refold. Exact caps are RetainedSeries418,
SeriesFold94 and FormingFootprint59, totaling571: explicitly +5 from the
intermediate566 and equal to the original ChartState ceiling. QuantickApp10210
and all shapes/scanner protections remain unchanged; aggregate10781. Six
actual lifecycle regressions failed [0,4] versus expected [1,1] before the fix,
then passed unchanged, including empty-batch query and supported reading order.
Reservation: https://github.com/milocaetano/quantick/issues/472#issuecomment-5703378792;
finding: https://github.com/milocaetano/quantick/issues/517#issuecomment-5703387807.
All43 timing medians over the declared ceiling remain failed evidence; this
bounded repair is not timing PASS or an assumption that other bulk/tails are fixed.

Operational allocation amendment D9 was published before its baseline edit:
https://github.com/milocaetano/quantick/issues/517#issuecomment-5703750812.
The batch 2 inline refinement measures RetainedSeries419, SeriesFold94 and
FormingFootprint57: total570, one below D8/original571; QuantickApp10210 and
all scanner/shape protections remain unchanged, with aggregate10780 and no
headroom. The private retained hint adds one charged line; the idiomatic
let-Some-else reduces footprint charge by two without removing fallback
comments or operations. The prior573/571 guard failure is preserved. Final
bounded footprint inline refinement is reserved at
https://github.com/milocaetano/quantick/issues/472#issuecomment-5703782095.
Codegen inspection, fresh full validation, timing acceptance and independent
final review remain separate gates, not implied by this allocation.

- **S1:** Files proposed below are reversible organization choices; final names may change with a recorded map delta, not altered requirements. No new framework or traits for speculative owners.
- **S2:** Performance fixtures use deterministic generated trades in an external driver, fixed seed-free formula and output hashes. Exact fixture sizes and protocol below are chosen before measurements, under D1.
- **S3:** No new visible feature/action is intended; existing hooks and control calls should suffice for unchanged surfaces. Indirect chart regression QA remains mandatory. A newly exposed action/surface would trigger the full act/read/discover and hook obligations before delivery.

## Ownership and interface design

Dependencies: app -> series -> engine; backtest -> series -> engine. series may use existing rust_decimal primitives; no UI/network/async/clock, and no dependence on app, feed, indicators, strategy, sim or paper. Engine keeps bar definitions and builders, not series lifecycle.

Proposed files: crates/series/Cargo.toml; src/lib.rs public exports; src/fold.rs streaming owner; src/footprint.rs forming-ladder boundary logic; src/retained.rs retained owner; src/readings.rs stable deal merging if required to keep files focused; tests/fixtures and integration tests. Register workspace member/dependencies in root Cargo.toml and app/backtest Cargo.toml. Adjust Cargo.lock and explicit guards graph/headless registration for the new lawful edges; no exemption/ceiling increase. Update AGENTS map and relevant docs for truthful ownership.

Current state migration:
- ChartState builder -> SeriesFold.builder (Box<dyn BarBuilder> created by BarConfiguration once at construction/reconfiguration).
- FootprintSeries builder/base_group/pending -> private optional forming-footprint state in SeriesFold; closed ladders -> RetainedSeries owned Vec<BarFootprint>.
- ChartState spec, trades (TradeTape), backfill_trade_count, backfill_done, bars, backfill_boundary, price_grid, tape_reference_price, deal_samples, readings_held, timeline_revision, series_revision -> RetainedSeries.
- partial -> borrowed SeriesFold builder partial; retain a cache only if a characterized contract demands it, never mirrored mutable domain truth.
- footprint_enabled -> optional forming-footprint presence plus retained configuration necessary for re-enable/regroup; keep the disabled group setting without allocating any ladder.
The existing historical count of 16+4 is not a promise of exactly 20 final fields: report moved, composed and eliminated fields separately.

SeriesFold public named operations: new(BarConfiguration), push(&Trade) -> Option<ClosedSeriesBar> (owned Bar plus optional owned BarFootprint), observe_deals(DealSample), partial() -> Option<&Bar>, partial_footprint() -> Option<&BarFootprint>, diagnostics(), progress(); explicit footprint configuration/reinitialization used on rare lifecycle changes. No retained tape, closed-output Vec, event Vec, callback bus or clone of every output. Caller immediately consumes a close by value. Internal fold preserves counted/uncounted diagnostic delta, rollover-close-without-print, and pending versus closed.trade_count ownership of boundary prints. No bar-family switch.

RetainedSeries operations preserve existing names where meaningful: new, ingest_live, ingest_backfill, prepend_history -> net bar increase, set_spec, reset_series (keeps readings), rebuild_bars, observe_deals, observe_deals_batch, set_footprint_enabled, set_footprint_group, seed_from borrowed tape/count/readings. Read APIs: bars, partial, bar_footprints, partial_footprint, trades, deal_samples, spec, backfill_boundary/count, revisions, tape grid/reference price, temporal slot lookup and progress. Lifecycle rebuild creates a fresh fold from the same registered configuration, seeds readings first, iterates retained tape without a tape copy, recomputes provenance and aligned outputs. Stable equal-millisecond reading order/dedup and deferred-reading behavior stay in the retained owner.

App composition: replace ChartState's domain implementation with direct consumption of RetainedSeries (an import alias is acceptable compatibility plumbing, not forwarding methods or mirrored fields). Keep dec_from_f64 and UI-facing reexports locally if callers need them. Remove app FootprintSeries production owner. Pane::seed_from delegates domain seeding once, then retains notifications.
App-owned paths: crates/app/src/pane/series.rs (viewport/drawing shifts, history prefix, indicators/worker commands, lane resets, orderflow notifications); crates/app/src/tab/feed.rs (paper then panes then strategy); crates/app/src/app/replay_and_history.rs (feed/replay orchestration); pane composition/main module declarations and imports. Backtest crates/backtest/src/run.rs substitutes SeriesFold for local builder while preserving the exact simulator/event reactions -> fold -> indicators -> on_bar -> queued commands ordering. No changes to strategy/simulator ownership or Session storage.

Extension proof uses crates/engine/tests/support/seventh_bar.rs and BarRegistry::new with the existing catalog plus that definition, resolving BarConfiguration through the production registry. Series does not create a second registry/factory. Tests run the same fixture through fold and retained live/backfill/prepend/rebuild/footprint flows; backtest's production run_session continues to accept the resulting configuration. Test-only registration is one definition/entry, no series/pane/runner identity cases.

## Test-first characterization sequence

1. Before production edits, add fixed trades + explicit expected bars/ladders/provenance/revision values against the current app/runner boundary. Run and retain exact successful source tree, tests and hashes; failing fixture discovery is recorded and resolved before using it as baseline. Do not derive expected values from candidate output.
2. Move existing domain tests from state.rs, state/tape_identity_tests.rs, state/bar_spec_parity_tests.rs and footprint_series.rs with their owner. Add headless external-consumer tests under series/tests. Preserve named coverage such as a_lower_reading_at_the_same_millisecond_is_a_late_poll_live_and_rebuilt, a_rollover_on_an_uncounted_print_keeps_the_ladders_aligned, a_refold_with_a_held_reading_rebuilds_the_bars_too, a_series_reset_keeps_the_readings, timeline_revision_tracks_ingest_prepend_and_rebuilds, series_revision_ignores_prints_and_closes_and_tracks_rebuilds, the_first_live_print_after_a_backfill_copies_nothing.
3. Characterize every shipped family including deal-count and all imbalance units; time-boundary final-print inclusion; prints before first reading; held/duplicate/same-ms dip/rollover and batch-vs-live readings; reset keeping readings versus app market reset dropping them (current reset_series also resets footprint enable/group to new defaults; preserve it); footprint off/on/off/group changes and held-readings refold; first-seen grid/reference price; empty/nonempty seed and prepend, split clamping and original live provenance; unchanged parameter no-op; revision identity semantics (live append bumps timeline only, backfill/rebuild bumps series and timeline).
4. Keep app-specific tests at app boundaries: pane/tests/lane_transport_tests.rs, drawing/replay/paper tests under app/src/app/tests, tab/feed.rs ordering boundary; backtest/tests/harness.rs and paper_account.rs. Add a concrete next-print-only order fixture proving a bar-triggered order cannot fill on its triggering print and reset/seek preserves intended paper journal/disarm behavior. Domain extraction must not move these effects below app.
5. Run existing engine golden/wire/control bar serialization compatibility and seventh-family fixture unchanged plus new traversal. Retain expected serialized bytes; no schema regeneration to conceal incompatibility.

## Rate classes and performance protocol

| Paths / operation | Rate | Budget and proof |
| --- | --- | --- |
| fold.rs/footprint.rs push; retained.rs ingest_live/grid/revisions; pane/series.rs live composition; backtest/run.rs loop; tab/feed.rs unchanged ordering | Per-trade | No added allocation layer, lock, catalog lookup, retained tape copy; fixed dispatch and counted-print logic. Existing TradeTape chunk growth, bars Vec growth, enabled footprint BTreeMap growth and underlying builders must be counted honestly, not described as zero total allocations. |
| Retained live deal observation | Per-reading (treat as hot per-trade class) | Preserve append fast path and existing held insertion costs; no new rebuild per late reading. Existing deal builders may retain required readings; do not claim constant memory for them. |
| Borrowed getters and app projections | Per-frame | Borrow stable data, no cloned tape/closed vectors or stable-ladder rebuild at frame rate; use existing cache/revision invalidation. |
| Construction/config/reset/backfill/prepend/rebuild/seed/regroup/batch readings | Rare bulk lifecycle | Explicit O(retained inputs) refold; existing sort/dedup cost retained, no new limits or silent eviction. Registry resolution here only. |
| Cargo/guards/docs/tests | Build/rare | No runtime cost. |
| Depth events | Per-depth N/A | Series consumes prints/deal evidence, not depth; no depth path change. |

Before implementation capture baseline through a deterministic external harness at the exact integrated-base tree; do not mutate the shared read-only source tree. Coordinator chooses isolated baseline build resource and records compiler/profile/target/environment/fixture hash. Candidate same release options and same host, serial execution without competing builds.
Fixture: 1,000,000 trades with deterministic 300 ms timestamps, repeating prices/quantities/buy-sell pattern and stable deal readings; 100,000-trade small case for scaling; all registered shipped families plus seventh test configuration where driver supports registration. Per-family live and backfill+rebuild+prepend workloads, footprints off and on, output digest equality first. Warm up each executable twice; collect 20 paired runs alternating base/candidate and candidate/base; record every sample, median ratio, p95 and dispersion. Accept only no demonstrated regression: paired median candidate/base <=1.05, investigate any repeatable positive timing change and any tail shift; inconclusive noisy evidence needs diagnosis, not PASS. Separately report setup/ingest/rebuild so startup or fixture generation cannot hide event costs.
Runner memory: exercise production run_session with no-op strategy/no indicators over preallocated 100k, 1m and 2m sessions, keeping session creation outside measured interval. Record allocator peak/current live bytes during run plus baseline/candidate process memory as supplementary data; control existing report/simulator allocations. The incremental fold-retention component must be independent of print count for print-only supported families with footprints disabled, zero retained TradeTape/closed-history storage, and zero ladder allocation. A counting allocator/structural source proof disambiguates RSS from retained input Session memory. SeriesFold enabled-ladder tests may grow only current capped ladder, never all closed ladders.
Evidence destinations: external ser1/performance/{identity,fixture,base,candidate,samples,memory,analysis}; PR body links exact numbers and limitations. Re-run affected comparisons after tested inputs/base/environment change. No new performance score.

## Acceptance criteria

- [ ] **A1** — Actual ChartState/FootprintSeries domain state and transitions leave app for the composed series owner with lawful dependencies and no mirrors. *Evidence:* state/rule before-after inventory, exact diff, graph/headless tests and independent architecture report -> external ser1/ownership.md and PR architecture report. *(R1)*
- [ ] **A2** — Desktop and production runner use the same streaming fold, runner adds no retained tape, and a retained consumer fixture runs without app with preserved ordering. *Evidence:* series integration tests, production run_session fixture and memory report -> external ser1/tests.md, ser1/performance/memory and PR. *(R2)*
- [ ] **A3** — Registered seventh-family definition traverses both owners and retained lifecycle/footprints without new identity dispatch or factory. *Evidence:* named fixture results and registration/edit inventory -> external ser1/extension.md and PR blast-radius section. *(R3)*
- [ ] **A4** — Pre-implementation expected fixtures and moved tests prove every listed deterministic/domain/wire behavior and unchanged retention policy. *Evidence:* timestamped baseline tree/test patch before production edit, golden outputs and current tests -> external ser1/characterization.md and PR test links. *(R4)*
- [ ] **A5** — Replay/reset/seek, drawing reanchor/shifts, lane reset, prefix semantics, paper journal and next-print-only fills preserve explicit app/runner effect ordering. *Evidence:* boundary regressions plus visual/trader report -> external ser1/effects.md, ser1/visual and PR. *(R5)*
- [ ] **A6** — No added per-print allocation layer/lock/registry lookup/tape copy, no disabled ladder cost, borrowed frame reads, measured timing and runner retention meet protocol; bulk rebuild stays deliberate. *Evidence:* paired measurements, allocations/source audit -> external ser1/performance/analysis and PR performance section. *(R6)*
- [ ] **A7** — Actual fields/rules/tests transferred and consumer edit costs plus app production/UI-free delta are reported under frozen metrics with no aggregate/score claim, dilution or weakened guard/contract. *Evidence:* unchanged measurement blob checks, base/candidate report and diff inventory -> external ser1/measurements.md and PR. *(R7)*

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

G-AI evidence destinations declared now: current-head durable PR AI report, canonical unresolved-thread readback and producer-created private projection, linked from PR body and final verifier. None has run for SER1.

## Injected gates

- [ ] **G1** — Independent source-first high-tier map reconciliation PASS before code; recorded source/map hashes and report. Authority: delivery contract “Reconcile requirements before implementation”, mission step 2/4. Destination external ser1/preflight.md, then PR.
- [ ] **G2** — Isolated branch/worktree from verified campaign base, ownership checked, guards built plus cargo check -p quantick-app --all-targets before first edit; mission-base and mission-tier high recorded by coordinator; live goal persisted before characterization. Authority: mission steps 5/6; new-task; integration contract. Destination bootstrap receipt and external ser1/chronology.md.
- [ ] **G3** — English, determinism, headless/lawful dependency/size/context/cycle/UI-free guards and frozen measurement contracts preserved. Authority: CLAUDE.md and campaign K4. Destination guards/diff/blob logs and PR.
- [ ] **G4** — Full ordered cargo fmt --all -- --check; cargo clippy --workspace --all-targets; cargo build --workspace; cargo test --workspace before code commits, plus targeted owner/runner/app regressions. cargo deny check bans licenses when Cargo.lock changes; affected supplemental suites, including guardrails suite if operational hooks change. Authority: CLAUDE.md verification and issue gates. Destination external ser1/validation exact-tree logs and PR.
- [ ] **G5** — Hot-path budget/protocol declared before implementation and measured before PR as A6; additive docking/fake registration and defaults proven as A3. Authority: mission gates; new-extension sections 1-6. Destination performance/extension reports and PR.
- [ ] **G6** — Indirect chart surfaces receive ui-harness regression evidence, visual-qa PASS or explicitly authorized defects, trader-ux-review without unresolved Blocker; new/changed surfaces get hooks in same change if any. Authority: mission user-visible row, issue explicit proportional visual/trader proof. Destination external ser1/visual and PR reports.
- [ ] **G7** — Full architecture review including medium-effort step-0 bug pass and complete shape pass; every Blocker/Should-fix resolved or validly deferred in PR. Authority: mission high tier, CLAUDE.md. Destination producer's current-key durable PR report/projection.
- [ ] **G8** — Archive actual goal as last commit before architecture/AI/delivery reviews; truthful evidence/counters and current review keys after repairs. Authority: mission step 8, delivery delta/retry rules, D2 amendment. Destination tracked .claude/GOAL-archive-shared-series-owner.md; PR progress records preserve attempts (new SER1 starts 0; never import/reset siblings).
- [ ] **G9** — Campaign authority/base/dependencies read back; no main/settings action, no safety/product/score waiver, same frozen blobs; serialized exact-head campaign integration only after gate. Authority: campaign D1/D6 and integration contract. Destination PR body, operation journal and integration readback.

## Non-applicable gates

No new trader action/capability or UI surface is planned; a new act/read/discover registry entry is not required solely for an internal owner extraction. Existing operations remain controllable and observable; G6 still proves regression. No feed/depth/MT5/Python runtime change planned: their conditional supplemental suites do not apply unless touched. No deployment/spend/main/settings authority. No prose-only verification exemption: this is code. No historical process exception inherited. No scorecard acceptance at child level: unchanged measurement reporting applies, final assessment remains campaign work.

## Closing steps

- [ ] **C1** — Draft PR opened against campaign/outside-eight with source mapping, complete A/G evidence, performance/blast-radius report and architecture then AI then full delivery-review PASS at current review key; independent delivery reconciles sources before map. Source: mission step 8 and delivery contract. Destination PR reports/body.
- [ ] **C2** — Exact final-head registered CI checks all green; PR non-draft with branch/head/base matching; canonical mission_ship_gate.sh mission <actual-pr> PASS including literal reconciliation. Source: mission completion and integration contract. Destination PR final-verifier report.
- [ ] **C3** — Coordinator rereads grant/current base, validates head-pinned authorized merge through gate, reads merge actor/time/SHA and ancestry/tree plus green integration evidence. Source: integration contract steps 4-5. Destination campaign operation result; state integrated_campaign only.
- [ ] **C4** — Explicit child issue and Project readback reconciled after proven campaign integration; retained operation/repair counters and histories. Source: issue Closing and campaign workflow. Destination issue #517/campaign checkpoint; main completion remains pending.
- [ ] **C5** — Return control to coordinator for remaining #478/#479 work and campaign final fresh assessment/reviews; no per-child user-pasted goal. Source: mission campaign completion override. Destination coordinator handoff.

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

These reserved D labels are literal verifier clauses, separate from mission Decisions D1-D6 above.

## Full retained source — attributed verbatim issue #517

The entire issue body read on 2026-09-16 follows. Its no-archive clause is retained without alteration; D2 above records the superseding user grant.

```text
<!-- campaign-task:milocaetano/quantick#472/SER1 -->
## Context

Parent campaign: https://github.com/milocaetano/quantick/issues/472. Aggregate owner: #478, coordinated with #479. Stable key SER1. Authenticated user clarification: the entire app crate, not merely QuantickApp, must become modular and extensible through elegant boundaries with low regression risk. File relocation and additional crate names alone do not meet that request.

Independent source inspection at campaign-tree-equivalent97ed and bar registry4be2 identifies a coherent domain still in app: ChartState owns16 series fields, FootprintSeries another4. Approximately440 existing nonblank/non-comment production source lines own retention, rebuild, provenance, revisions and ladder alignment; this is an explicitly limited measurement, not the frozen score metric or aggregate A1 completion. A new owner is estimated at500-700 such lines plus tests. No broad app-shrink claim.

## Scope

Extract one focused headless quantick-series crate, depending only on engine plus necessary existing third-party primitives. Compose a streaming SeriesFold (builder, optional forming footprint, closed output returned by value) with RetainedSeries (trade/deal evidence, closed outputs, backfill provenance, configuration, grid and revisions). Desktop consumes RetainedSeries; production backtest consumes SeriesFold without retaining another tape. Reuse the existing BarBuilder and #504 BarConfiguration registry; do not create another factory or grow engine into the series/application lifecycle owner.

Move ingestion/backfill/prepend/rebuild, deal-sample ordering/deduplication/deferred readings, footprint enable/regroup/refold/alignment and domain seeding with their tests. Named methods and borrowed reads are the contract; no AppContext, universal command bus, blanket forwarding or Deref. App retains rendering, viewport/drawing shifts, worker channels, indicator/orderflow notifications, feed scheduling and authorized paper/strategy effects. Preserve paper-before-panes-before-strategy and simulator-before-fold-before-strategy ordering.

Preserve the current retention policy: #412 is separate and no new limit, silent eviction or policy decision is introduced here. #318 indicator projection and #442/#471 control decomposition are not absorbed. Venue OHLCV prefix semantics, public wire formats, user features and financial rules are unchanged.

## Acceptance criteria

- [ ] A1 (R1): ChartState/FootprintSeries domain state and transitions genuinely leave app for the series owner; app retains only a composed owner and UI/effect orchestration, with no mirrored domain state or reverse edge.
- [ ] A2 (R2): Production desktop and backtest use the same streaming fold; the runner does not acquire retained-history storage or reorder simulator/strategy execution. A second-consumer retained-series fixture runs without app.
- [ ] A3 (R3): The existing registered seventh-bar fixture traverses streaming and retained live/backfill/rebuild/footprint paths with no new series/pane/runner identity branch; no duplicate registry/factory. Name the extension contract and registration edits.
- [ ] A4 (R4): Tests written before implementation freeze all-family output, time-boundary/uncounted-deal prints, held/duplicate/rollover readings, reset semantics, footprint on/off/regroup alignment, seed/prepend provenance, timeline versus series revisions and existing wire/golden behavior.
- [ ] A5 (R5): App replay/reset/seek, drawing reanchoring, indicator-lane reset and paper journal/next-print-only-fill behavior remain covered; effect ordering is explicit and tested at the owning boundary.
- [ ] A6 (R6): No new per-print event allocation layer, lock, registry lookup or tape copy; disabled footprints add no ladder storage/work. Frame reads borrow. Measure dense-tape base/candidate timing and runner retained-memory behavior; report existing chunk/vector/BTreeMap allocations honestly. Rebuild remains deliberate O(retained inputs).
- [ ] A7 (R7): Report actual state/rules/tests transferred, consumer edits and app production/UI-free delta under unchanged metrics. No denominator padding, generic dumping crate, guard/rubric weakening or claim that this child completes A1/A2 or improves a score by itself.

## Execution and dependencies

Class autonomous; priority P0 architecture owner following current registry delivery; tier high (determinism/hot path/data lifecycle). Executor: gpt-6-astra — strongest for new crate boundary and data/financial ordering. Main/settings exclusively user-owned.

Implementation base: latest reviewed origin/campaign/outside-eight after REGB #504 is integrated with green checks and required reviews. No stacking on unmerged4be2. Read-only design/characterization planning may proceed now; implementation ownership/worktree/PR/head remain null until coordinator claim. REGL #505 overlaps composition/graph only and must be reconciled before integration if its base has moved.

Gates: independent source-first map reconciliation before code; guards armed and app all-target check before first edit; engine-adjacent test-first; full ordered fmt/clippy/build/test and affected guard suites before commit; exact-diff architecture/AI/full delivery; exact-head CI; proportional visual/trader regression proof for indirect chart changes. Existing historical exceptions for other children do not apply here. No new tracked execution evidence/GOAL archive; current final-gate conflict remains an integration dependency, never bypassed.

Closing: reviewed current-head PR, authorized campaign integration only, retained counters and child/Project readback. Operation/repair counters start0 for this new child; other children's histories unchanged. Raw execution artifacts outside Git, concise review results on PR. Board/issue/worktree IDs and evidence are filled from actual readbacks, not guessed.

## Attributed request

> Mas nao basta mover, tem que manter o desenho bonito com design pattern elegante. Que permite crescer o codigo. Deixar ele extensivo, facil de cdirar novas fetaures. Com risco baixo de quebrar

This is one bounded child of the broader request, not a narrowing of the campaign objective. Remaining app ownership remains open under #478/#479.
```

## Retained campaign authority — attributed verbatim

Delegated preparation request from the coordinator (historical preparation scope; subsequent worktree/bootstrap readbacks are recorded above):

> Prepare bounded SER1 #517 mission design/ledger ONLY, no repository edits/build/worktree/GitHub writes. Read C:/src/quantick/.claude/skills/mission/SKILL.md, new-extension/SKILL.md, docs/workflow/delivery.md and CLAUDE.md fully. Read actual issue517 plus campaign472 D1/D6 and archive grant5700791807: issue517 last no-archive clause is historical superseded; retain verbatim source and explicit decision amendment. Current reviewed campaign base a5c871e40bcab5253925b830bc09a746f40d3841 (bar75a tree-equivalent). Source tree can be inspected via clean C:/src/quantick-worktrees/feat-bar-selection-registry at75a8b8bd7cf25e36a03fb4837f23d4708b986bf8, noting branch commit vs integrated SHA. Existing read-only series design under outside-eight-coordination/series-owner-*; locate linked independent design rather than redo entire audit. Produce external scratch complete proposed high-tier mission with original source verbatim, stable R1-R7/A1-A7 matching issue, explicit performance protocol/rate classes, named ownership/interface/fields/paths, test-first characterization plan and full gates/closing/literal G-AI, D1 delegation choices and truthful no-code-yet chronology. Source and map will receive separate independent pre-edit completeness pass after your output. No new user question needed for settled D1 defaults; flag substantive contradiction. New worktree not created yet. No subagents. Report file path and minimal decisions.

Campaign #472 D1:
> Main merges and GitHub settings belong to the trader: prepare settings changes as one-line human tasks on the parent issue and never block other work on them. Decide every other default yourself, and never wait on the trader for anything but the merge to main.

Campaign #472 D6:
> esta permitido criar muitos pr em paralelo para nao esperar outros ficarem pronto

Archive decision https://github.com/milocaetano/quantick/issues/472#issuecomment-5700791807:
> The user explicitly permits retaining evidence in the existing workflow format and defers changing that workflow until later. This supersedes the earlier prohibition on tracked execution evidence and mission archives for this campaign. The required mission archive may therefore be committed with each child under the current delivery contract.
