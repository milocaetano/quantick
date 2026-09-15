# Quick-range owner and pinned main integration

**Tier:** high — state ownership, public annotation compatibility and per-frame input correctness.
Branch: feat/sync-main-chart-layout. Initial base: origin/campaign/outside-eight at 9ff57501.
Parent: https://github.com/milocaetano/quantick/issues/472; tasks: https://github.com/milocaetano/quantick/issues/501 and https://github.com/milocaetano/quantick/issues/496.
Objective: Transfer quick-range state, decisions and tests to a small headless MVU owner while preserving the complete volume-profile flow and resolving the four #447 findings against executable evidence.
Why: Reduce actual desktop coupling and the cost of adding a consumer; no architecture score or automatic undo/replay guarantee is claimed.

## Request ledger (source-map revision 4, combined D13 mission)

- R1 — D11 sections 1/4 and #501 Scope: move real quick-range lifecycle, availability, gesture/chrome and result rules below app; distinguish equivalent app ownership from direct root fields. → A1.
- R2 — D11 sections 1/2/4 and #501 Port: scoped Command/Event/Effect, one authoritative owner, plain inputs, correlated results, independent consumers; tested input/event reconciliation before view/effect stages as this owner leaves; no replacement god object. → A1,A2.
- R3 — D11 section 1, D12 and #501 shared-operation paragraph: UI/hotkey/admitted MCP share typed behavior, permissions, actor, idempotency, draft refusal, discovery and readback; preserve profile v1/v2, Fibonacci actions and honest future bar positions; compatible exact references extend existing future-aware input only if needed. → A3,A4.
- R4 — D11 section 3, #447 findings 1–4 and #501 A4: repair timeline drift, duplicate timestamp resolution and persistent chrome suppression; verify tape-lane eligibility and reconcile its claimed defect using #447's explicit non-defect disposition rule. Retain executable evidence and #447 as the ledger. → A4.
- R5 — D11 sections 3/6, D12 and #501 A5: preserve working flow, menu/ruler/Escape/profile/ticket/replay, #498 sharing toggle, #500 three actions/future coordinates and #485 pane-local layout strips/addressed rename/footer geometry. Reconcile stable pane and active layout before paint/convert. → A5.
- R6 — D11 section 2, D12 and #501 A6: default-off feature gates quick-range harness only; preserve enabled active/ready/1/future hook states and ordinary public capabilities/configuration. → A6.
- R7 — D11 sections 4–6 and #501 A7: bounded hot-path work and measured performance; preserve guard/frozen input/public and financial invariants. → A7.
- R8 — D11 section 5 and #501 A8: actual ownership/coupling/change-cost ledger and additive second-consumer probe; distinguish measured from estimated deltas. → A8.
- R9 — D11 sections 1–3 and #501 dependencies: first owner slice proceeds from reviewed campaign; MT5/registries remain scheduled after it; reconcile pinned MS2 and all five incoming PR behaviors in this same reviewed candidate per D13. → A1,A5,A8.

- R10 — #496 A1 and D13: integrate pinned main a6644bc602e203b17bb29a5581548b03c4898fc6 into reviewed campaign9ff preserving both histories and complete intended source behavior, with no discarded campaign repairs. → A9.

## Decisions and assumptions

Retained current D12/D13 and full #501/#496 source: [combined-sources.md](evidence/quick-range-mvu/combined-sources.md). Revision 2 and 3 preflight PASSes are historical; combined revision 4 preflight passed before merge or code, as recorded in G1. Only three old-base characterization tests were added before source drift, with no runtime/schema edits. Old worktree and failed evidence remain preserved.

- D11 is the authenticated parent priority/continuation instruction quoted below. Campaign grants D1/D2/D6 remain authoritative; root owns remote writes, commits, PR and integration.
- S1 — Names and crate placement follow the explicit #501 design: quantick-chart-interaction::quick_range::QuickRangeModel. This is reversible implementation detail.
- S2 — Timeline rewrites explicitly mark the temporary range stale/unavailable; #501 A4 allows this instead of inventing a timestamp remap. No invalid ruler is painted over different bars.
- S3 — Reuse pagination_revision only after auditing every series mutation. Its documented lifecycle excludes live appends and includes rewrites. Exact reference checks also require stable pane ID, slot bounds and matching open timestamp.
- S4 — Superseded after D12 before runtime edits: the old v1 optional-field proposal is withdrawn. Preserve v1 unchanged; compatible optional exact identity may extend the existing profile-v2/Fibonacci future-aware input. Keep future bar_position and optional time honest; no released fixture changes or Fibonacci algorithm redesign.
- S5 — quick-range-harness is default off and gates only this family's demo setup. Legitimate config/capabilities stay in ordinary builds.
No unresolved question requires trader judgment; these choices are authorized, bounded and reversible.

## Acceptance criteria

Each A retains #501's ID. Evidence destination for all: .claude/evidence/quick-range-mvu/ and current-head PR reports, linked on #501.

- [ ] **A1** — Named state, transition/availability and conversion-result decisions leave app for the small headless owner; lifecycle tests move; no false root-field reduction claim. Evidence: source diff and ownership.md. (R1,R2,R9)
- [ ] **A2** — A second headless consumer/fake executor uses the same commands/events/effects without app; tests cover wrong owner, threshold, replacement, pane/tab removal, refusal and stale completion. Focused stage-order tests prove input/events reconcile before view and effect execution, including same-frame series/selection changes and conversion. Evidence: headless tests and extension-probe.md. (R2)
- [ ] **A3** — UI/hotkey/MCP share typed behavior and preserve permission denial, origin, idempotency, draft refusal, discovery, all three quick-range scene IDs and readback; retain profile v1/v2 and Fibonacci capability versions, future bar_position/optional time. Validate supplied pane/revision/slot/time/price facts before mutation; denial/refusal causes no unintended mutation. Evidence: integration/contract test logs. (R3)
- [ ] **A4** — All four #447 findings have evidence-backed final dispositions: failing-before regressions for the three actual repairs (rewrite stale/unavailable, duplicate-time exact slots, persistent selection releases chrome), and executable plus historical proof that tape-lane presses were already ineligible, under #447's original allowance for a reasoned non-defect disposition. Never claim a fourth repair or failing-before result. Evidence: regressions.md, tape-lane-baseline.md and linked #447 readback. (R3,R4)
- [ ] **A5** — Ordinary menus, ruler, paper/rail/confirmation Escape precedence, profile look, ticket/replay, #498 sharing toggle, #500 Profile/Fib Retracement/Fib Projection and valid future anchors, #485 pane-local strips/rename/footer remain correct. Stable pane and active layout reconcile before paint/convert; a tape lane is not future chart space. Include #493 context-menu actions/geometry, #489 stacked-pane resize/clamping, order-ticket guidance and replay/history; existing tests plus current-head visual/UX proof. Evidence: integration logs and screenshots. (R5,R9)
- [ ] **A6** — Default-off quick-range-harness feature isolates setup; QUANTICK_QUICK_RANGE_DEMO active/ready/1/future work when enabled; ordinary build lacks setup; feature CI/tests execute. Evidence: feature checks/CI and source inspection. (R6)
- [ ] **A7** — Idle/pressed per-frame work adds no allocation/locks/session-size work; paired dense-input measurements and all workspace/headless/cycle/UI-free guards pass without exemptions. Evidence: performance.md and validation logs. (R7)
- [ ] **A8** — Before/after ownership/change-cost ledger and fake extension probe demonstrate actual coupling change; report edited consumers/files/lines/registration sites, headless tests and limitations. Evidence: ownership.md and extension-probe.md. (R8,R9)

- [ ] **A9** — Pinned main and campaign ancestries survive the reviewed candidate, with complete intended incoming behavior and no discarded campaign changes. Evidence: exact source/base/head, merge ancestry and incoming-delta reconciliation. (R10)

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Operational gates

Evidence: .claude/evidence/quick-range-mvu/validation.md, independent preflight/review reports and PR links.
- [x] G1 — Independent combined source-first completeness PASS by registry_design for revision 4 before merge/code, reviewed GOAL SHA256 EE80072C317525773C5C6AA79293EDD276D38885B5B91C1FAA38BE158613A3DE. Sources read first: D13 updated04:10:33Z, #501 04:11:49Z, #496 04:11:48Z, all2026-09-15. Earlier rev2/rev3 PASSes are historical only. Source: delivery/high mission/D13. Evidence: preflight.md.
- [x] G2 — Isolated sync worktree bootstrap completed by preparer before edits: local cargo build -p quantick-guards (10.98s exit0), cargo check -p quantick-app --all-targets (4m44s exit0); root verified clean9ff, then wrote private mission-base and high-tier marker. Child owns writes and C:/src/quantick-worktrees/feat-evidence-resources/target exclusively after handoff. This records root/preparer reported chronology, not child reruns. Source: mission/D13/root handoff.
- [ ] G3 — Ordered cargo fmt --all -- --check; cargo clippy --workspace --all-targets; cargo build --workspace; cargo test --workspace. Guards/English/frozen seven files/public-financial/feed-retry invariants preserved; cargo deny check bans licenses if lock moves. Sources: CLAUDE, #501, D11.
- [ ] G4 — Per-frame input/projection versus rare conversion/rewrite rate classes and paired dense-input baseline/candidate evidence before PR. Sources: new-extension/#501.
- [ ] G5 — Current-head UI harness, visual QA and trader UX including #498 contextbar behavior. Sources: mission/new-extension/#501.
- [ ] G6 — Act/read/discover, admitted contract/refusal/idempotency tests and generated/released schema compatibility. Sources: control contract/#501.
- [ ] G7 — D13 combines MS2 and MVU1 at one reviewed head, preserving pinned main a6644bc602e203b17bb29a5581548b03c4898fc6 and campaign9ff. Revalidate actual merged inputs; prior UI-free ceiling46911 and no annotation exemption. Do not silently chase later main. Source: D13/#501/#496.
- [ ] G8 — Preserve failures/counters and actual evidence; no score claim. Sources: D11/delivery/#501.

## Closing steps

- [ ] C1 — Publish draft PR to campaign/outside-eight. Root owns commits/remote writes.
- [ ] C2 — Current architecture/AI reports and zero required unresolved findings.
- [ ] C3 — High-tier delivery review last.
- [ ] C4 — Exact-head green CI, readiness and canonical final verifier PASS.
- [ ] C5 — Root-only authorized serialized campaign integration/readback for both issues, resulting campaign SHA and child closure/Project projection. Candidate/main score and final trader merge remain pending. No main merge.

## Implementation boundary and plan

### PR 511 bounded repair batch 1, 2026-09-15

Repair starts from 86e5c9d3460f867115d2e17142828976744bde8a against the
unchanged campaign base 9ff57501249f51d8f75c21f52094ec3dc3c39af2. The
coordinator reserved batch 1 and attempt 1 for MVU-V1, MVU-V2,
AR-STRIP-CONSTANT, AR-RESIZE-ACT and AR-RESIZE-DISCOVER before dispatch
(PR progress revision 2). This plan does not reset counters or close findings.
MVU-P1/P2 and their preliminary attempt counts remain preserved.

1. Characterize narrow status overlap and selected-bar/legend collision;
   preserve every status value, SIM honesty, live lane and legend. Use bounded
   responsive placement and the legend's actual measured rectangle carried
   by existing pane-frame geometry. Include parked bars and narrow bounds.
2. Name and document the layout strip scrollbar width at module scope.
3. Route pointer and admitted remote vertical resizing through one typed,
   addressed adjacent-pair operation, preserving neighbor boundaries, floors,
   horizontal share, permissions and actor semantics. Add truthful operation
   and divider discovery without changing released resize v1/v2 schemas.
4. Run focused regressions, guards, then the full ordered fmt/clippy/build/test
   loop and feature validation. Preserve seven frozen files and UI-free ceiling
   46911 without exemptions. Record current input hashes and actual outputs.
5. Return the uncommitted frozen diff to the coordinator for current-source
   visual/performance evidence and independent delta reviews. No completion,
   current review PASS or CI reuse is implied by implementation tests.

Rate classes: status/bar placement is bounded per-frame geometry, without
session scans or paint-shape rescans; resizing runs only on drag/action events;
capability discovery and schema generation are rare. The existing mission,
source map and acceptance criteria remain authoritative. No new mission or PR.

### Draft-delivery evidence checkpoint, 2026-09-15

Runtime source 1b4bad7ebb257d0c0a016db6994338592b367bda, tree
8f29157022721601d731bf8d902e48768c2740ab. Both pinned parents are retained.
All 87 candidate-inputs-v4 entries were rehashed unchanged before archival.
Only evidence/mission prose changes after that runtime commit are reused;
current review verdicts and exact-head CI are not reused or implied.

- A1/A2/A8: ownership.md and extension-probe.md identify actual transferred
  state, scoped transitions, dependency edge and the 26-line fake consumer.
  Fourteen headless tests pass; no direct root-field reduction is claimed.
- A3/A4/G6: ordered-validation-v4.md contains current workspace PASS,
  including admitted operations, legacy/exact/future anchors, permissions,
  authorship and no-partial-mutation regressions. regressions.md and
  tape-lane-baseline.md retain three real failing-before fixes and the
  independently disproved fourth finding, linked back to #447 and #501.
- G3/G7/A9: ordered fmt/clippy/build/test PASS at the exact v4 inputs;
  app 2075 passed, headless 14 passed. Prior guardrails 277 and cargo-deny
  PASS are input-bound reuse, not new execution. The evidence-only update
  also ran cargo run -p quantick-guards -- --report and cargo test -p
  quantick-guards successfully. UI-free is 46907/46911; no new exemption.
- A6: feature-enabled build and 2076 app tests passed at runtime source;
  feature CI at the eventual PR head remains pending.
- A7/G4: five paired ABBA blocks/state are measured in performance.md.
  Active median average CPU is -0.086%; other central deltas are small and
  mixed, tails noisy. Unconditional flat-or-better acceptance is NOT claimed.
- A5/G5: current-source captures and independent visual/UX review are
  partial. Narrow status-bar overlap and selected-profile context-bar
  occlusion require explicit disposition; missing matrix cells remain
  unproven. A draft PR does not waive either gate.
- C1-C5 and G-AI1-G-AI4: pending current PR publication, independent
  reviews, any necessary repairs, exact-head CI, final verifier and
  authorized campaign-only integration. This archive is a reviewable
  mission record, not a completion declaration.

The original checklist is retained unchecked where the complete criterion
has not yet been independently reconciled. No architecture/outside score,
quantick-score improvement or main delivery follows from local tests.

1. Source-first independent completeness review of combined original sources then this map; only after PASS and root confirmation merge pinned a664 with --no-commit, retain MERGE_HEAD for ancestry and resolve the actual guard-baseline conflict under prior policy.
2. Baseline ownership/performance and characterization; tests before state/defect changes.
3. Std-only headless owner and fixed-size transition outputs; fake independent consumer.
4. Typed exact-reference validation behind existing registered admission, all anchors checked before insertion.
5. Narrow pane/surface/hotkey adapters; default-off quick-range harness.
6. Validate complete combined incoming UI (#493/#489/#498/#500/#485), contracts, financial/replay and guard regressions at this candidate.
7. Full checks, raw measurements, archive mission before independent current-key reviews, and root handoff.

No direct quick-range field exists on QuantickApp. Equivalent state leaves DrawingChromeSurface.quick_range; root conversion-result decisions shrink. Geometry, egui painting, drawing insertion, authorization/transport and effect execution remain in app.
No MT5/registry/project_settled implementation in this child; D11 retains them as later campaign work. Python/MT5-specific checks are N/A unless scope actually changes. No new engine aggregation logic.

## Combined criterion reconciliation

#496 A1 → R10/A9; A2 → R5/A5/G5; A3 → R7/A7/G3/G7; A4 → G1/G2/G3; A5 → G-AI1–4/C2/C3/C4/C5; A6 → C5. #501 A1–A8 retain their IDs verbatim in meaning. All evidence/reviews/full ordered checks use the same final merged inputs. No separate MS2 prerequisite or second completion claim remains.

## Retained delegated request

> Root bootstrap complete before firstedit: session16524 cargo build guards7.49s and check quantick-app --all-targets26.94s exit0 using R1target; local worktree guardbuild7.49s exit0 at04:01-ish. WT feat-quick-range-mvu HEAD9ff clean; root wrote private mission-base and mission-tier high afterward viaapply_patch. You now own WT writes and exclusive borrowed CARGO_TARGET_DIR=C:/src/quantick-worktrees/fix-mutation-retry-truth/target (oldE1M paused). Persist source-preserving highGOAL viaapply_patch including full userverbatim/D11/issue501 delegatedsource, map you proposed, literalAIgates, evidencepaths and commandsbootstrap actual chronology. Registry_design assigned independent source-first completeness and will requestmap; send it yourmap/GOAL after it reads sources. No code until itsPASS. AfterPASS implement bounded firstslice test-first per501, preserveallrequirements. Root owns remote writes/commits/PR and final reviews; stop beforecommit, return changedtree/tests/evidence. No othercrate/harness scope creep. Buildfocus firstheadless crate; sharedtargetonlyyou afterthishandoff. Use committed rules inWT notdirtymain. Report progresswithin10min, source/designdoubts toroot.

## Attributed original request and D11

Source: authenticated trader request retained verbatim by parent comment 5674457925; quotation is repository-language exemption.
> ## D11 — Highest-priority owner decomposition and explicit resumption
>
> Actor: authenticated trader in the current Codex conversation, 2026-09-15. This replaces the temporary pause and changes scheduling, not the frozen success criteria or merge authority.
>
> 1. First implement the #433 quick-range volume-profile flow as a narrow headless MVU owner with feature-scoped Command/Event/Effect shared by UI, hotkeys and permission-gated MCP. Transfer real state, rules and tests; QuantickApp retains composition, rendering and effect execution. The existing state partly lives in drawing chrome/panes, so evidence must distinguish root fields from equivalent app-crate ownership.
> 2. Next implement true MT5 Sans-IO and complete feed/bar/layer registries. Extract tested frame stages incrementally as their owners leave; do not wait for an all-or-nothing A1→A2→A3 chain. Keep per-feature harness hooks behind explicit features. project_settled needs algorithmic decomposition, not Sans-IO.
> 3. Retain paused C2 #477/PR #499, F2 #495 and E1M #397 work and all failed evidence/counters. They do not preempt this priority. Reuse #397 rather than pretend its current async helper extraction proves Sans-IO. Reconcile overlapping #447 defects without duplicate issues. Preserve the first slice's full working UI and main's new drawing behavior.
> 4. No replacement god object, universal dispatcher, file-split/rename credit, automatic undo/replay guarantee or score claim. Each issue names state, decisions, owner, interface, dependencies and executable acceptance; outside app it names the equivalent responsibility.
> 5. Measure before/after fields and decisions still owned by the root, cross-owner reads, dependency edges, number of consumers requiring edits, tests runnable without app, and an additive extension probe (files/lines edited plus registration sites). Run frozen raw measurements separately. Only independent assessors award scores; architecture patterns do not guarantee a higher grade.
> 6. D1/D2/D6 authority remains: up to three isolated independent implementation missions; serialize reviewed green campaign-only merges; main/settings remain trader-only. Existing safety fixes and blind gates 4/5, quantick-score non-regression and K1–K6/M1–M3 remain mandatory.
>
> ### Attributed verbatim user request
>
> > Incorpore à campanha atual uma prioridade explícita: desmontar o QuantickApp como god object. Crie e vincule as issues faltantes, confira duplicações e ajuste a ordem de execução da campanha.
> >
> > Não basta dividir impl em arquivos nem renomear QuantickApp para Workspace. Transfira estado, regras e testes para donos menores e independentes. QuantickApp deve ficar restrito à composição, UI e execução
> >   de efeitos.
> >
> > Aplique:
> >   1. Functional Core / Imperative Shell como diretriz geral.
> >   2. MVU com donos de estado headless.
> >   3. Command/Event/Effect: UI, hotkeys e MCP compartilham operações, preservando permissões; hooks de harness isolados por feature.
> >   4. Sans-IO no MT5: máquina de protocolo separada de socket e relógio.
> >   5. Registries completos por família: feeds, barras e layers.
> >   6. Etapas explícitas no frame, com dependências de ordem testadas.
> >
> > Priorize a decomposição do QuantickApp com MVU + comandos. Use o fluxo de perfil de volume da #433 como primeiro recorte. Depois avance para MT5 e registries; extraia etapas do frame conforme os donos
> >   saírem. project_settled exige decomposição algorítmica, não Sans-IO.
> >
> > Cada issue precisa nomear o estado e as decisões que saem de QuantickApp, seu novo dono, interface, dependências e testes de aceite. Nas issues fora do app, identifique a responsabilidade equivalente.
> >
> > Não crie outro god object ou dispatcher gigante. Não prometa undo, replay ou pontos no rubric automaticamente. Meça a redução de acoplamento e do custo de adicionar funcionalidades.
> >
> > Atualize a campanha e prossiga na execução dentro da autorização existente.
> >
> > Obs: coloque isso como piroridade e continue o restante depois. A implmentação do desisn pattern vai elevar a nossa nota de arquitetura, então pririze ela.
>

## Retained issue #501

Source: GitHub issue body read before implementation, 2026-09-15.
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

## Retained defect ledger #447

Source: GitHub issue body read before implementation, 2026-09-15.
> Found by campaign #367 sync X3 (PR #445).
>
> Step 0 (code-review at medium) round three on PR #445 read #433's quick-range code (right-drag measure, then convert to a fixed-range volume profile) and returned four findings that were deferred rather than repaired in the synchronization. They are #433's design and are present on `main` too. Coordinator decision D28 kept the four earlier repairs in #445; these four are this issue.
>
> ## Findings
>
> 1. **The temporary range does not follow a history prepend or a bar-size change** — Should-fix.
>    - Evidence: `crates/app/src/surfaces/drawing_chrome/quick_range.rs:51` (`Selection::anchors` holds bar numbers). Persistent drawings are shifted when older history is installed (`crates/app/src/pane/series.rs:237`, `self.drawings.shift_bars(delta)`, beside `shift_right_edge`); the quick range is not.
>    - Effect: after "load older" or a backfill while a range is up, the ruler covers bars `delta` slots away from what was measured, and its readout is wrong; converting then places the profile by timestamp on the originally measured bars, not on the ruler shown.
> 2. **Conversion goes through timestamps only** — Should-fix.
>    - Evidence: `crates/app/src/control/annotate.rs:344` (`fixed_range_profile_input` sends `time_unix_ms` per anchor) and `:441` (`resolve_slot` picks the newest bar opened at or before each time).
>    - Effect: on tick/volume charts where several bars open in the same millisecond (bursts; MetaTrader's folded B3 deals), a range starting at bar 80 can place a profile starting at bar 81 or 82. A profile drawn with the toolbox over the same range keeps the exact bars, so the two ways of making one object disagree.
> 3. **A ready range hides the selected drawing's context bar** — Consider.
>    - Evidence: `crates/app/src/surfaces/drawing_chrome/mod.rs:951` (while a range is ready, `draw_floating` does not call `context_bar::draw`); only a primary click inside the owning pane's price band dismisses the range.
>    - Effect: selecting a drawing in an indicator band or the other pane of a split leaves the range up and the drawing's style/lock/delete bar missing until Escape or a click on the price area.
> 4. **A right-drag can start inside the live tape lane** — Consider.
>    - Evidence: `crates/app/src/pane/quick_range.rs:48` (the press checks only that it lies inside the price band, which spans the lane; later drag positions are clamped to the lane edge, the press is not).
>    - Effect: the first anchor lands in the lane; on a time chart it gets a future timestamp (button enabled, conversion snaps back to the newest bar); on a tick chart it has no timestamp and the button stays disabled.
>
> ## Acceptance criteria
>
> - [ ] Each finding is fixed, or closed with the reason it is not a defect, with a test that fails before the fix where a fix lands.
>
