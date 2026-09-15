# Combined sources

## D12

> ## D12 — Pin newly observed main for D11 compatibility, without changing priority
>
> Coordinator default under existing trader D1/D11 authority, 2026-09-15. Readback shows trader milocaetano merged PR #500 at 03:45:07Z (66ca483409943fc585404e288a075027fc8f6468) and #485 at 03:46:22Z (a6644bc602e203b17bb29a5581548b03c4898fc6). Pin MS2 source to a6644bc602e203b17bb29a5581548b03c4898fc6; campaign remains9ff57501249f51d8f75c21f52094ec3dc3c39af2. Do not chase subsequent source changes silently.
>
> #500 modifies the same quick-range family: three registered actions (Profile, Fibonacci Retracement, Fibonacci Projection), profile v2 with explicit bar_position/optional time and honest future coordinates, retained v1, future demo state, capability/scene/operability/retry/schema coverage. #485 adds pane-local layout strips, addressed rename state and pane footer geometry. These existing behaviors must survive MVU1 #501. They are compatibility obligations, not new product expansion or evidence of a campaign score.
>
> Amend #501's source/map before further implementation: reuse current future-aware typed operation where suitable; do not add a competing v1 exact-anchor API by default or drop v2/Fibonacci/future behavior. Exact current-bar identity must still prevent equal-timestamp mis-targeting and stale conversions. Distinguish an ineligible live-tape lane press from valid future chart coordinates. Reconcile each #447 finding against the pinned source and retain failing evidence; no automatic closure. Keep the first bounded headless state/command extraction and preserve all three actions through their existing gated adapters; no Fibonacci algorithm redesign.
>
> Old9ff characterization tests just added are retained as old-base evidence; source-map rev2 independent PASS was valid for earlier inputs, not new a664. Obtain a bounded source/map delta preflight before changed implementation. MS2 can prepare in parallel, separate worktree/target; reviewed campaign integration and current UI/contract verification remain required.
>
> Guard constraint: main500 adds a whole-file annotation exemption absent from the campaign. Do not automatically adopt a new waiver or raise a ceiling to make synchronization pass. Measure the actual combined source under prior and incoming guard inputs, preserve truthful counts, and reconcile with real extraction if needed. Original no-weaken tests/guards/financial rules/frozen seven remains authoritative. No main or settings mutation is authorized.
>

## D13

> ## D13 — One bounded MVU1 + MS2 integration mission, preserving the prior guard
>
> Coordinator default under D1/D11; no new trader approval needed for this scoped sequencing choice. Exact premerge tree843f294e from campaign9ff plus pinned maina664 counts 46,231 UI-free lines with main's new annotate.rs exemption, and 47,209 without it. The exemption hides 978 lines; without it the prior campaign ceiling46,911 is exceeded by298. Canonical guard report had no unreadable/undecodable/failed scan paths. This is raw guard evidence, not frozen outside-score measurement or a grade.
>
> A standalone unchanged MS2 cannot pass the campaign's no-weaken constraint. Do not solve it with a ceiling increase or whole-file exemption, and do not invent a cosmetic 298-line move. Combine the already-authorized first ownership extraction MVU1 #501 and required synchronization MS2 #496 into one bounded high-tier mission/PR on feat/sync-main-chart-layout, based on reviewed campaign9ff and preserving pinneda664 history. The mission must satisfy every existing #501 A1–A8 and #496 A1–A6, including all source UI/contract behavior, real headless state/rules/tests and actual guard proof. The amount extracted is measured, not promised. Both tasks integrate at the same reviewed green candidate, eliminating the artificial circular prerequisite; neither is complete merely because the other is.
>
> This remains a separate synchronization branch/PR into campaign/outside-eight, never main. One exclusive implementation writer and one source-preserving combined mission; root-only remote publication/serialized campaign merge. Fresh independent completeness on combined sources/map precedes merged-tree implementation. All ordered checks, architecture/AI/high delivery, exact-head CI, guard/freeze/safety/public/financial requirements hold. If actual extraction cannot meet the guard, continue substantive owner work within this first slice or identify the concrete remaining responsibility; no unproved success.
>
> Handoff: the former MS2 preparer stops/releases its worktree after bootstrap; MVU1 author takes exclusive ownership only after root readback. Existing feat/quick-range-mvu worktree, rev2/3 mission evidence and three old-base characterization tests remain retained; no destructive reset or erased failures/counters. Old preflight PASS is historical, not approval of new combined scope. Later MT5/registries remain behind this first reviewed outcome; other campaign work stays retained.
>

## Current issue #501

Updated: 2026-09-15T04:11:49Z

> <!-- campaign-task:milocaetano/quantick#472/MVU1 -->
> ## Context
>
> First D11 implementation slice, contributing to A1 #478 and A2 #479. PR #433 already shipped quick-range volume profiles; this task changes ownership, not reimplements that feature. Existing #447 remains the single ledger for its four defects. No direct quick-range field exists on QuantickApp: the equivalent app-crate state is DrawingChromeSurface.quick_range. Root-owned conversion/result and Escape integration decisions must shrink, and this distinction must be measured honestly.
>
> ## Scope
>
> Move Selection.owner/anchors/ready, Idle/Pressed/Selected lifecycle, press threshold, release/replacement/dismissal, owner reconciliation, conversion availability and success/refusal/stale-result policy from surfaces/drawing_chrome/quick_range.rs and app/drawing_input.rs into quantick-chart-interaction::quick_range::QuickRangeModel. Move gesture eligibility and temporary-versus-persistent chrome ownership policy out of pane/quick_range.rs and drawing_chrome/mod.rs. Keep geometry, egui painting, persistent drawing insertion, transport admission and execution of effects in narrow app adapters.
>
> Port: typed update(Command, RangeContext), observe(Event, RangeContext), view() -> RangeView with scoped Effect output. Commands/events cover gesture/release/dismiss/convert, owner/pane removal, series prepend/rebuild, selection changes and correlated conversion completion/refusal. Inputs carry plain coordinates and stable pane/series facts; no QuantickApp reference, egui, JSON, network, async or clock in the headless owner. One authoritative state, no shadow copies. Scope enums to this family, never create AppCommand for every capability.
>
> UI and existing hotkeys emit these operations; admitted MCP and local profile conversion converge on one typed profile operation without bypassing capability admission. Preserve annotate.fixed_range_profile.create identity/version behavior, provenance, idempotency and structured readback. Exact range identity includes validated pane, series revision, slot and timestamp; do not assume timestamps are unique or repurpose a generation without checking its complete lifecycle. Any necessary public field/schema extension must be explicitly compatible/versioned and pass released-contract checks.
>
> ## Acceptance criteria
>
> - [ ] A1: Named state, transition/availability and conversion-result decisions leave app for the small headless owner; relevant lifecycle tests move with them. No root-field reduction is claimed where none occurred.
> - [ ] A2: A second headless consumer/fake executor drives the same commands/events/effects without constructing app; tests cover wrong owner, threshold boundaries, replacement, pane/tab removal, refused conversion and late completion not clearing a new selection.
> - [ ] A3: Existing UI/hotkey/MCP operation paths share typed behavior while preserving permission denial, origin, idempotency, draft refusal, discovery, scene ID quick_range.fixed_range_profile and readback. Denial/refusal causes no unintended mutation.
> - [ ] A4: Resolve #447 findings with failing-before regression evidence: prepend/rebin preserves exact anchors or explicitly reports stale/unavailable state; equal-millisecond slots remain exact; selecting a persistent drawing in another pane/indicator band releases temporary chrome ownership; tape-lane presses cannot form eligible history ranges. Record each disposition on #447, never close it merely for a move.
> - [ ] A5: Preserve ordinary right-click menus, persistent ruler, paper/rail/confirmation Escape precedence, profile appearance, ticket/replay and main PR #498 all-charts toggle. Existing integration assertions remain; visual/UX proof uses current head.
> - [ ] A6: Quick-range harness setup is isolated behind an explicit default-off Cargo feature; retain the existing QUANTICK_QUICK_RANGE_DEMO names and states in that build, with ordinary-build absence and feature-enabled CI/tests. Legitimate user config and public capabilities remain active normally; do not hide measurement input.
> - [ ] A7: Idle/pressed per-frame path adds no allocations/locks or work proportional to session length; paired dense input baseline/candidate measurements prove the touched path. Workspace/headless/cycle/UI-free guards include the new crate without exemptions.
> - [ ] A8: Before/after ownership and change-cost ledger plus a fake second-consumer/extension probe demonstrates reduced coupling, not automatic score/undo/replay gains.
>
> Dependencies: reviewed F1/R1/MS1 integrations are satisfied on campaign9ff57501. MS2 #496 must reconcile main ed7531ea including #493/#489/#498 before final shell integration acceptance; source-disjoint headless modeling, characterization and owner implementation may start on current reviewed campaign without using unmerged code. Rebase/revalidate when MS2 lands. Do not wait for aggregate A1/A2 completion (this is their first bounded child).
>
> Priority: P0, first implementation. Native parent A1 #478; crosslinks A2 #479, A3 #480, A4 #481, defects #447. Later MT5/registry implementation is scheduled after this slice's reviewed campaign integration.
>
> ## Campaign assignment
>
> Campaign: https://github.com/milocaetano/quantick/issues/472. Authority/priority: D11 https://github.com/milocaetano/quantick/issues/472#issuecomment-5674457925. Owner class: autonomous. Tier: high (state, boundary and hot-path correctness). One isolated mission/PR from reviewed origin/campaign/outside-eight; no main/settings writes. Full source-first independent mission preflight, moved/additive tests, ordered fmt/clippy/build/test, applicable UI/UX/performance checks, exact-diff architecture/AI/delivery reviews and exact-head green CI. Preserve all CLAUDE.md invariants, original safety fixes, ticket/replay, public/financial contracts, guards and frozen seven measurement files. No score awarded by this task.
>
> Evidence: this issue, head-pinned PR reports/CI and parent checkpoint. Issue/Project IDs, owner, branch/worktree, PR/head/base: null until claim/readback. Initial operation/repair counters: 0/0, thereafter cumulative. Report raw before/after coupling, app/root-owned decisions, moved headless tests, affected consumers, and additive probe files/lines/registration sites; distinguish measured deltas from estimates. No new god object or universal dispatcher, renamed roots or file-only moves as acceptance.
>
> ## D12 compatibility refinement (supersedes older source/API assumptions)
>
> Source: https://github.com/milocaetano/quantick/issues/472#issuecomment-5674579200. Pin main a6644bc602e203b17bb29a5581548b03c4898fc6 through MS2. In addition to prior #493/#489/#498, preserve #500's Profile/Fib Retracement/Fib Projection actions, profile v2 bar_position plus optional time, v1 compatibility, future anchors/readback and QUANTICK_QUICK_RANGE_DEMO=future. Preserve #485 pane-local layout strips, addressed rename and footer geometry. These are existing-feature nonregressions under A3/A5/A6, not a request to redesign Fibonacci.
>
> A1/A2 owner now represents the shared quick-range lifecycle and scoped three-action request identity; actual drawing construction stays with existing tool owners. A3 uses existing v2/Fibonacci typed chart-anchor operation where suitable, v1 unchanged. Source inspection finds time-bearing v2 anchors still resolve by timestamp, so A4 equal-millisecond identity remains required. If adding exact refs, use compatible optional validation on the existing future-aware input, never overwrite future bar_position or fabricate market time. Validate all supplied identity/time/price facts before any mutation. Keep future actions available on valid ranges; stale rewritten ranges may explicitly refuse without painting misleading anchors. Live tape lane is not future chart space.
>
> A2/A5 additionally pin stable pane identity and active pane-local layout reconciliation before paint/convert. Explicit quick-range frame stages and tested order are part of this slice, as required by D11 item6 and aggregate #480. A4 still requires individual #447 source/current findings and failing-before or reasoned dispositions; older9ff tests are historical until rerun on combined inputs. No implementation based on stale preflight/API assumptions: source-map delta requires independent completeness review.
>
> ## D13 combined delivery boundary
>
> https://github.com/milocaetano/quantick/issues/472#issuecomment-5674602257 authorizes one bounded high-tier combined MVU1+MS2 mission on feat/sync-main-chart-layout. All #501 A1–A8 and #496 A1–A6 remain required at the same reviewed candidate. This replaces the artificial separate-integration prerequisite: source synchronization and real headless owner extraction may be validated/integrated together, preserving both ancestries and prior guard policy. No new annotation exemption, increased ceiling, test/contract waiver or automatic closure. Original feat/quick-range-mvu test/map artifacts remain retained. Exclusive author mvu433_design after MS2 preparer bootstrap/release; root owns remote delivery and serialized campaign-only merge. First-slice priority unchanged.
>

## Current issue #496

Updated: 2026-09-15T04:11:48Z

> ﻿<!-- campaign-task:milocaetano/quantick#472/MS2 -->
> ## Context
>
> Parent: https://github.com/milocaetano/quantick/issues/472. Stable key: MS2.
> The trader account milocaetano merged PR #493 at 2026-09-15T00:46:12Z (merge 7a6b79e39eedfeaf4c609880a72fc2cb570181e1) and PR #489 at 00:47:27Z (merge aaae6f32943ea559df8b5e53da1c4ecb0183be01). Current campaign remains 4b13b64a0f4556470cbf6b7a5e1c7f1b000d0f50 after reviewed MS1 #491 / PR #494. This is observed external main work, not an agent main merge.
>
> The incoming delta from the previously synchronized main1429941f changes 24 paths, including chart-context-menu actions, paper-ticket entry guidance, replay/history actions, harness geometry/tests and stacked-pane layout. A read-only git merge-tree at these exact tips returned f7bb546169b185b0ef12dec297d7424bb0d44980 with exit0: no textual conflicts, NOT integration correctness or validation proof. All seven frozen measurement blobs match the initial baseline at both tips.
>
> ## Scope
>
> - Incorporate the observed main commits through one isolated synchronization branch and a reviewed PR into campaign/outside-eight, preserving both histories and all already delivered campaign repairs.
> - Inspect the complete incoming delta and the actual combined tree; prove menu/harness geometry, order-ticket/replay and stacked-pane behavior through appropriate targeted tests and existing visual evidence where its exact inputs remain valid. Run new UI evidence when affected inputs or conflicts require it.
> - Revalidate actual source/campaign tips before starting; record any additional main delta rather than silently expanding scope or trusting this initial conflict probe.
> - Preserve baseline, frozen seven blobs, public and financial rules, determinism, single engine, one-way/headless dependencies, English and all ratchets. Do not replace campaign code with main wholesale or weaken guards to make the merge pass.
> - Respect finish-first scheduling: A1R/C2 validation and required repairs continue at their frozen campaign inputs. This queued task does not authorize another competing app build, stale review restamping, or ownership of external worktrees.
>
> Out of scope: main merges, GitHub settings, score/rubric changes, new product features, destructive reset/force push, or declaring outside-eight achieved.
>
> ## Acceptance criteria
>
> - [ ] A1: Record exact source main and campaign SHAs; the reviewed synchronization head preserves both ancestries and contains the complete intended incoming behavior without discarded campaign changes.
> - [ ] A2: Targeted executable proof covers affected context-menu actions and geometry, stacked-pane resize/clamping, order-ticket and replay/history regressions. Carry forward prior evidence only with the delivery contract's complete unchanged-input proof; otherwise run the applicable checks.
> - [ ] A3: F1/R1 feed-integrity and retry/idempotency behavior remains intact; all frozen seven blobs equal the initial manifest and every CLAUDE.md invariant/guard remains intact or stronger.
> - [ ] A4: An isolated high-tier mission retains source requests and stable criteria/gates, passes independent source-first completeness before edits, and records actual full ordered fmt/clippy/build/test results at the merged inputs. No check or failure is relabelled.
> - [ ] A5: Current full architecture/AI/high-tier delivery reviews, zero unresolved required findings, exact-head green CI and canonical ship proof precede a root-only head-pinned merge into campaign/outside-eight.
> - [ ] A6: Read back merge ancestry/target and child closure/Project projection; publish the resulting campaign SHA. Keep candidate/main score and final trader merge criteria pending.
>
> ## Campaign assignment
>
> Owner class: autonomous. Priority: 2 integration correctness. Tier: high (combined UI/replay/ticket regression surface; no authority widening).
> Dependency: MS1 #491 integrated_campaign_with_green_ci, proven by https://github.com/milocaetano/quantick/issues/491#issuecomment-5672935909.
> Scheduling: ready for preparation; implementation only when finish-first work and disjoint ownership/build capacity permit. Never interrupt a live validation handle solely to refresh the base.
> Planned branch: feat/sync-main-chart-layout. Branch/worktree/owner/PR/head: null until created and read back.
> Executor: strongest gpt-6-astra at actual dispatch, with a durable executor line then.
> Operation/repair counters: 0/0. No product implementation started.
>
> Validation/evidence: this issue, committed mission/evidence archive, exact-key review reports, CI and parent checkpoints. Sources: parent D1/D2/D6 and docs/campaign/integration.md, docs/workflow/delivery.md. Main/settings remain exclusively the trader's action.
>
> ## D11 source-tip refresh and priority support
>
> Decision https://github.com/milocaetano/quantick/issues/472#issuecomment-5674457925. This is the existing synchronization issue, not a duplicate. Source main is now ed7531ea8388e5dd686f9acf2cd34d8d82d185be, including PR #498 all-charts drawing shortcut in addition to #493 chart menu and #489 pane resizing. Earlier aaae6f32 source reference is historical; resolve main again before execution. Preserve complete histories, all three behaviors and campaign safety changes/frozen seven.
>
> Priority P0 support to #501 shell integration. #501's source-disjoint headless modeling/characterization may start from reviewed campaign9ff without stacking unmerged code; final shell acceptance requires this reviewed sync and revalidation. Equivalent responsibility is integration reconciliation, not a new domain owner: no state/rules move in this sync. Interface remains existing source/target contracts; tests cover shared context bars/show-on-all-charts, menus, panes, profile flow, ticket/replay and full exact-head CI. Every merge remains campaign-only after independent exact-diff review.
>
> ## D12 pinned source and guard preservation
>
> Source: https://github.com/milocaetano/quantick/issues/472#issuecomment-5674579200. Pinned source main is now a6644bc602e203b17bb29a5581548b03c4898fc6, including #500 right-drag three actions/profilev2/future coordinates and #485 pane-local layout strips, beyond earlier #493/#489/#498. Earlier source references remain historical. Include all five PR behaviors in A1/A2 and preserve campaign9ff changes. No silent chase of subsequent main tips.
>
> A3/G2 explicitly prohibit adopting new guard waivers or raising ceilings merely to synchronize: inspect main500's whole annotate.rs UI-free exemption against prior campaign measurement, report actual combined counts and use genuine extraction if needed. No score/frozen-rule change. Keep #501 source/map in sync; root revalidates exact inputs before campaign merge.
>
> ## D13 combined delivery boundary
>
> https://github.com/milocaetano/quantick/issues/472#issuecomment-5674602257 authorizes one bounded high-tier combined MVU1+MS2 mission on feat/sync-main-chart-layout. All #501 A1–A8 and #496 A1–A6 remain required at the same reviewed candidate. This replaces the artificial separate-integration prerequisite: source synchronization and real headless owner extraction may be validated/integrated together, preserving both ancestries and prior guard policy. No new annotation exemption, increased ceiling, test/contract waiver or automatic closure. Original feat/quick-range-mvu test/map artifacts remain retained. Exclusive author mvu433_design after MS2 preparer bootstrap/release; root owns remote delivery and serialized campaign-only merge. First-slice priority unchanged.
>
