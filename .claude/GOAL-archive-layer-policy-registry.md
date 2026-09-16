# Complete chart-layer ownership and registration

**Tier:** high; layer state, control authority, and per-frame ordering.

Move chart-layer policy out of the desktop root and dock rendering contributions
through typed packages, so the complete family has cohesive headless ownership
and additive extension. This is a retrospective archive of the existing mission,
prepared after implementation; it does not establish missing pre-edit chronology.

Issue: [#505](https://github.com/milocaetano/quantick/issues/505).
PR: [#515](https://github.com/milocaetano/quantick/pull/515).
Parent: [#472](https://github.com/milocaetano/quantick/issues/472).
Branch: `feat/layer-policy-registry`.
Declared base: `origin/campaign/outside-eight`.

## Request ledger

Stable R1-R7 and A1-A7 are retained from the live mission and prior independent
delivery review. Source spans refer to the complete delegated request quoted
below. Repeated execution instructions map to G/C, not additional product asks.

- **R1** — Transfer complete-family visibility policy/state to a focused headless
  owner; retain external feature authority and avoid an omnibus context or
  central feature dispatcher. Source: Scope paragraphs 1-3, acceptance bullet 1.
  Coverage: A1, A4, A5.
- **R2** — Prove additive descriptor and renderer registration through production
  discovery/state/operation/render paths without root surgery. Source:
  acceptance bullet 2 and final Evidence paragraph. Coverage: A2, A7.
- **R3** — Share UI/hotkey/admitted-control operations and readback while
  preserving targeting and permissions. Source: acceptance bullet 3.
  Coverage: A3.
- **R4** — Preserve requested/effective visibility, tape-off choice, reasons,
  pane/shared-grid scope, preset LaneMarks, drawing undo, indicator persistence,
  rejected unknown operations and compatible unknown persisted IDs. Source:
  Scope paragraph 2 and acceptance bullet 4. Coverage: A4.
- **R5** — Register ordered switch and non-switch rendering contributions,
  preserving clipping, hit testing, axis claims, projection demand and same-frame
  effects. Coordinate #318 without taking over its owner. Source: Scope
  paragraphs 1-3 and acceptance bullet 5. Coverage: A5.
- **R6** — Move pure tests and prove a UI-free second consumer, ordinary/harness
  integration and real visual/trader flows, including #493/#489/#498
  compatibility. Source: acceptance bullets 5-6. Coverage: A6.
- **R7** — Measure dispatch/root-policy/mutable-owner counts, additive-probe
  edits and paired frame overhead; distinguish measurements from estimates and
  make no cosmetic-completion or score claim. Source: acceptance bullet 7 and
  final Evidence paragraph. Coverage: A7.

## Decisions and amendments

Parent decision IDs retain their parent meaning; they are not local renumbering.

- **D14** — The attributed user architecture request below and the delegated
  #505 scope require complete ownership/registration, not a menu-only table or
  file split. MS2/MVU1 integration prerequisites remain required.
- **D16** — Only the historical independent pre-edit source/map chronology is
  accepted retrospectively under the standing D1 delegation. See Deferred.
- **D17** — Only the specific inherited narrow-window overlap is accepted for
  this extraction. See Deferred; it remains geometry FAIL/inherited.
- **Archive authorization, 2026-09-16** — The authenticated user decision
  [5700791807](https://github.com/milocaetano/quantick/issues/472#issuecomment-5700791807)
  supersedes the earlier no-tracked-archive instruction, including that clause
  in the historical D16 record. Existing workflow archives may now be committed.
  This does not waive any current gate, adopt unmerged PR #507, or grant main merge.
- **A3 amendment retained** — Snapshot is capped at 64 panes with explicit
  omissions. Correlated `layers.visibility.set` journal events reconcile
  admitted changes, including no-op and omitted-pane targets, through the
  existing `events.read` cursor/retention contract. No new read capability or
  trader-tab mutation.

## Assumptions

- **S1** — A domain-specific `layers` leaf owns policy. This follows the
  requested headless ownership boundary and one-way dependency graph.
- **S2** — Preserve existing failed-save stamp behavior. Persistence redesign
  was not requested; external feature ownership remains explicit.
- **S3** — LayerState owns local switches only; external feature switches are
  read live and receive typed effects. This implements the source's
  no-duplicated-authority constraint.
- **S4** — Retain u32 masks with explicit registration validation. The existing
  bounded representation is sufficient; collision/overflow tests prove failure.

These assumptions are the retained design choices, not a claim that a new
pre-edit interrogation or independent preflight occurred.

## Acceptance criteria and evidence

The checked product lines record the prior independently reviewed implementation
at `4c54d82257436caa2054b12cd29f2f18bbf8db31`, tree
`fad705a8c78504479356f894e8a6575caf94484f`, base
`c9138e6100921bf70f8126a4cf308c47bbe8b183`, review key
`293de974a6da3536e6bd5b5e08bf6ac88f23cc23`. They do not claim the combined
candidate on the changed campaign base has received final review. Fresh local
validation is recorded below; new full current-key reviews, current visual/
performance applicability and final-head CI remain pending.

Scratch root below is the readable absolute directory
`C:/src/quantick-worktrees/outside-eight-coordination/`. It is outside Git;
named artifacts are retained evidence, not newly executed tests.

- [x] **A1** — All 21 switchable members have registry-driven discovery,
  visibility/scope/availability/inheritance/persistence policy under a headless
  owner, preserving existing authoritative switches.
  *Evidence:* `crates/layers/src/builtins.rs`, `state.rs`, and
  `crates/layers/tests/policy.rs`; independent assertion/diff review at
  scratch `delivery-515-4c54d822/criteria.md`.
  → [prior full delivery report](https://github.com/milocaetano/quantick/pull/515#issuecomment-5692655278). *(R1)*
- [x] **A2** — A fake descriptor/package traverses production discovery,
  state, operation and actual rendering without root/giant-dispatcher surgery;
  bounded registration rejects collisions and overflow.
  *Evidence:* `crates/layers/tests/extensions.rs`,
  `crates/app/src/pane/render_registry/probe.rs`,
  `crates/app/src/app/tests/layer_control_tests.rs`; scratch
  `delivery-515-4c54d822/criteria.md`.
  → prior full delivery report above. *(R2)*
- [x] **A3** — UI, hotkeys and admitted control share operations/readback with
  existing cockpit.layout admission, stable targets and reasons, retry/
  idempotency behavior, bounded snapshot and correlated journal reconciliation.
  *Evidence:* `crates/app/src/control/layers.rs` and
  `crates/app/src/app/tests/layer_control_tests.rs` assert permission denial,
  stale/unknown targets, scope, omitted-pane reconciliation and no-op/retry.
  → scratch `delivery-515-4c54d822/criteria.md` and prior full delivery report. *(R3)*
- [x] **A4** — Preserve all requested visibility, reason, scope, persistence,
  undo and unknown-ID semantics without mirroring external feature authority.
  *Evidence:* `crates/layers/tests/policy.rs`, `document.rs`, and
  `crates/app/src/app/tests/layers_tests.rs`; assertions inspected in scratch
  `delivery-515-4c54d822/criteria.md`.
  → prior full delivery report. *(R1, R4)*
- [x] **A5** — Ordered typed render contributions include candles, indicators
  and all switches; preserve clipping, hit testing, axis claims, projection
  demand and same-frame effects through narrow views and existing owners.
  *Evidence:* `crates/app/src/pane/render_registry/`,
  `crates/app/src/pane/tests/mod.rs`; actual gateway/probe and rendering tests
  in scratch `layer-owner/current-base-final-test-02.log`.
  → scratch `delivery-515-4c54d822/criteria.md` and prior full delivery report. *(R1, R5)*
- [x] **A6** — Pure tests move; a headless independent consumer and ordinary/
  harness app integration plus visual/trader checks cover the affected flows
  and current #493/#489/#498 compatibility, with D17 explicitly retained.
  *Evidence:* `crates/layers/tests/extensions.rs`; scratch
  `layer-owner/current-base-final-test-02.log` (3,984 pass, 0 fail, 21 ignored),
  `layer-owner/current-base-feature-test.log` (2,082 pass, 0 fail, 11 ignored),
  and `layer-registry-review/final-visual-trader-review.md`.
  → [visual/trader report](https://github.com/milocaetano/quantick/pull/515#issuecomment-5692554585)
  and prior full delivery report. *(R6)*
- [x] **A7** — Report measured coupling/ownership/addition deltas and paired
  per-frame overhead with workload and limitations; no score is awarded.
  *Evidence:* scratch `layer-owner/coupling-current.md`,
  `registry-performance-layer-4c54d822.json` and
  `layer-registry-review/committed-evidence-review.md`.
  Eight scoped identity dispatch matches are removed; the report retains four
  root effect-delivery branches, 22 typed stages and test-installation edits.
  Twenty ordinary dense-frame runs give median run means 1.2866625 to
  1.2939635 ms (+0.567%) and median per-run p99 2.3193 to 2.4768 ms (+6.791%).
  Noise/drift precludes a zero-overhead or speedup claim; this measures synthetic
  CPU frames, not native GPU presentation or all live workloads.
  → prior full delivery report and PR performance evidence. *(R2, R7)*

- [ ] **G** — Preserve English artifacts, frozen seven measurement files,
  one-way/headless graph, financial/ticket/replay contracts, bounded
  allocation-free per-frame reads and existing caches/batch draws; retain
  ordered fmt/clippy/build/test, applicable cargo-deny, architecture/visual/
  trader evidence and exact-head CI. Rare registration/operations/persistence
  may allocate. Source: #505 Campaign assignment, CLAUDE.md, mission and
  delivery/integration contracts.
  *Evidence:* prior head's raw logs/manifests and independent reports below.
  Fresh combined-source validation is recorded below. Affected record checks,
  current full architecture/AI/delivery reviews, current visual/performance
  applicability and final-head CI remain required.
  → PR #515 current-head verification section and published reports.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

AI gate evidence destinations: current PR #515 durable AI report, canonical
thread-list/count output and branch-bound projection verified at the final
review key. The prior report is historical input to a follow-up, not approval
of this new archive.

## Closing steps

The former composite C obligation is retained as C with named pending stages.

- [ ] **C** — Coordinator completes the current review/delivery chain, readiness,
  final verifier and authorized serialized campaign integration/readback.
  *Evidence:* current full delivery PASS published after architecture and AI;
  open non-draft PR with exact branch/head/base, current report URLs and all
  registered checks green; successful `mission_ship_gate.sh` literal
  reconciliation; authorized head-pinned campaign merge/readback and prerequisite
  ancestry/CI proof. MS2 #496 and MVU1 #501 remain integration prerequisites.
  → PR #515 and parent #472 checkpoint. No main/settings writes; only the user
  merges main. Final campaign assessment remains separate.

## Non-applicable gates

No engine/bar-construction algorithm changes: engine test-first golden-fixture
work is not newly applicable. No new feed/trade execution or financial contract
is introduced. UI, hot-path, extension, second-operator and full high-tier
reviews do apply and are covered above; no broad exemption is inferred.

## Deferred

- **CP-01 / D16** — Historical independent pre-edit source/map review could not
  be substantiated. [D16](https://github.com/milocaetano/quantick/issues/472#issuecomment-5692520814)
  accepts only that historical process deficiency under the user's D1
  delegation. No missing event is claimed to have happened. Current source-first
  completeness, full criteria/ledger, product requirements, tests, reviews,
  final-head CI and closing gates remain mandatory. Recovery evidence:
  scratch `registry-preflight-recovery/` and
  `delivery-515-4c54d822/completeness.md`. Clean-base guards/app checks were
  reported separately and do not prove independent preflight.
- **LREG-VIS-001 / D17** — The 650-logical-point bar-picker/first-layer-toggle
  overlap remains geometry FAIL/inherited, accepted for this bounded extraction,
  not fixed, zero repair attempts. [D17](https://github.com/milocaetano/quantick/issues/472#issuecomment-5692539224)
  applies to the unchanged geometry at the prior product head. Actual evidence:
  scratch `mvu-visual/layer-4c54-unshared/capture/screenshot.png` versus
  `mvu-visual/bar-baseline-time-narrow/capture/screenshot.png`; reported collision
  around native x609-660/y54-96 at scale 1.5. Pane-local/shared drawing
  preservation passes separately. No claim the user inspected these pixels;
  no final campaign visual acceptance, unseen defect, introduced regression,
  product requirement or current test/review/CI is waived. The changed campaign
  base requires fresh visual confirmation of this geometry before carrying the
  disposition to the combined candidate; that confirmation remains pending.

## Evidence provenance and remaining verification

Prior durable evidence at the stated product head/base/key:

- [Architecture PASS](https://github.com/milocaetano/quantick/pull/515#issuecomment-5692553190),
  scratch `layer-registry-review/final-arch-review.md`.
- [AI COMPLETE](https://github.com/milocaetano/quantick/pull/515#issuecomment-5692553906),
  scratch `layer-registry-review/final-ai-review.md`.
- [Full delivery PASS](https://github.com/milocaetano/quantick/pull/515#issuecomment-5692655278),
  scratch `delivery-515-4c54d822/final-delivery-review.md`.
- [CI run 35055989869](https://github.com/milocaetano/quantick/actions/runs/35055989869).
  This is old-head CI, not proof for the future archive commit.
- [Progress adoption](https://github.com/milocaetano/quantick/pull/515#issuecomment-5692587244):
  LREG-AR-001 fixed at batch 1/attempt 1, LREG-VIS-001 accepted at zero
  attempts. No counter reset; future work reconciles the live cumulative record.

Input-bound local evidence is retained in scratch:
`layer-owner/integrated-final-input-manifest.json` (1,629 file hashes);
`layer-owner/current-base-final-clippy-03.log`,
`current-base-final-build-02.log`, `current-base-final-test-02.log`,
`current-base-feature-test.log`, and `current-base-guards-pass-03.log`.
The historical formatter record is
`registry-preflight-recovery/layer-format-tool-record.json`; the independent
delivery report verifies its original gated exit-zero event and unchanged
formatter inputs. The original failed documentation-delimiter test and its
corrected run remain historical evidence, not silently relabeled success.

The archive itself corrects evidence/traceability only, but the campaign base
has now changed. The old runtime evidence is historical; fresh ordered
validation is required for the combined source, rather than claiming same-base
reuse. Current independent full reviews, source/map reconciliation, final-head
CI and the final verifier remain required.

## Combined candidate after campaign integration

Authorized rebase journal:
[5701565122](https://github.com/milocaetano/quantick/issues/472#issuecomment-5701565122).
The original layer commit was replayed as
`b80e01313a1ff7d8268418626fac32f6f357cb29` onto campaign base
`a5c871e40bcab5253925b830bc09a746f40d3841`, which includes the reviewed bar
registry child #514. Original head/base/test/review identities above remain
historical. Rebase changed the source inputs and invalidated old review keys.

Only the two expected guard-baseline conflicts needed manual resolution.
Six overlapping Rust files merged disjoint bar-selection and layer-policy
edits; the root shape union retains `ChartState.spec: BarConfiguration` and
removes `QuantickApp.layer_actions`. Canonical `--report`/`--tighten` measured
ChartState 571, QuantickApp 10126, root budget 10697 and app UI-free 46270.
All ceilings moved down; no guard was relaxed. Raw measurement output is at
scratch `archive-515-rebase-measure-01/`.

The original ignored mission and initial archive are recoverable in scratch
`archive-515-pre-rebase-01/`. The original ignored mission is also retained as
`GOAL-live-archived.md` there when retiring the live copy; no historical record
is overwritten by this archival step.

Fresh ordered fmt/clippy/build/test passed on 2026-09-16, from 17:19:00 to
17:25:42 UTC, at staged tree `cfcc306a3872e5fd7cc9dcba14d8a3bd9edcc929`
and the rebased head/base above. All four commands exited zero; workspace
results sum to 4,005 passed, zero failed and 22 ignored. The separate app
`quick-range-harness` run passed from 17:26:01 to 17:28:59 UTC with 2,087 passed,
zero failed and 11 ignored. `cargo deny check bans licenses` exited zero at
17:29:01 UTC; warnings remain in the raw output. Toolchain: rustc 1.98.0
(`88d9e12ae`, x86_64-pc-windows-msvc), cargo 1.98.0 (`797e8a9bc`).
`QUANTICK_BUBBLES` was cleared only in the validation processes.

Input identity and complete outputs are at scratch
`archive-515-validation-01/identity.json`, `fmt.log`, `clippy.log`, `build.log`,
`test.log`, `harness.log` and `deny.log`. No unstaged/untracked inputs were
present during the ordered run; its staged tree was unchanged afterward.
This final results-only archive amendment is checked separately with guards,
diff hygiene, source-quote/AI-block verification and a full inspected delta
against that validated tree. It changes no runtime/test/build input; local
runtime results are reused from that exact staged tree, not called new runs.
The coordinator owns publication, new full reviews, current paired performance/
visual reconciliation, final-head CI and final verification.

The prior PR body and scratch evidence index describe the old archive-authority
block. That historical description is superseded only by the authenticated
2026-09-16 archive decision; the coordinator must update the PR's current
state and evidence when publishing the archive. Preparation alone is not ship,
readiness, merge or campaign completion.

## Verbatim user request

Attributed authenticated user request retained in the existing live mission
(D14; spelling preserved):

> Eu preciso que melhore nossa arqutietutra e nao ficar provando ABC + D que tao coisa minuscula que vc fez eh bom só para vc
> Crie um design pattern quebre esse APP deixa essa parada bonita e escalável para recrutador ver e gostar

## Verbatim delegated request

Complete body of [issue #505](https://github.com/milocaetano/quantick/issues/505),
read from GitHub during archive preparation on 2026-09-16. Quotation preserves
the original source, including historical assignment state and original
counters; those initial values do not reset the later recorded progress.

> <!-- campaign-task:milocaetano/quantick#472/REGL -->
> ## Context
>
> D11 complete-family registry child under A2 #479; contributes to headless ownership A1 #478. This is actual policy/state ownership and additive extension, not enum renaming.
>
> ## Scope
>
> Current ownership: chart_layers.rs closed identity/persistence metadata; pane/layers.rs visibility/availability/setter routing; QuantickApp.layer_actions and app/chart_layers_wiring.rs settle masks/defaults/restoration; pane/layer_painters.rs independently routes rendering. Transfer requested visibility, scope, availability, inheritance and persistence-change decisions to a cohesive headless LayerState owner; registered descriptors provide identity, applicability, persistence and typed operations. App-only renderer registrations consume narrow frame/pane views and explicit ordered render contributions. QuantickApp only composes, paints and executes effects.
>
> Do not duplicate authoritative drawing undo/visibility, indicator state or orderflow booleans: commands/effects target their existing owners. No omnibus LayerContext, arbitrary callbacks into app or central match dispatcher for every feature. Coordinate indicator projection #318 rather than take it over.
>
> Complete switchable family: TapeChart, TapeHeatmap, TapeBubbles, Heatmap, Bubbles, Footprint, LiveStrip, LaneMarks, FlowLegend, BookStatus, DepthGaps, Grid, LastPrice, BackfillDivider, SeamDivider, Crosshair, PointerPrice, PointerTime, PaperTrading, TradePaint, Drawings. Also register rendering contributions outside the switch enum, including candles and indicator plots/draw objects; a menu-only registry is incomplete.
>
> ## Acceptance criteria
>
> - [ ] All members have registry-driven discovery/menu/readback, scoped operations, availability, persistence and rendering order; policy state/tests leave root/app for headless owners where appropriate.
> - [ ] Fake layer proves state/discovery/operation/renderer registration with no app-root or giant-dispatcher surgery.
> - [ ] UI/hotkeys/admitted MCP share operations and readback with unchanged permission gates/targeting.
> - [ ] Preserve requested versus effective visibility, tape-off state, capability reasons, per-pane scope, shared grid, preset-owned LaneMarks, drawing undo and indicator persistence; unknown operations fail and persisted unknown IDs keep documented compatibility.
> - [ ] Rendering order, clipping, hit testing, axis claims, projection demand and same-frame effects remain tested; include current main #493/#489/#498 menu/pane/all-chart drawing behavior.
> - [ ] Headless owner tests move, second consumer runs without UI, and real app integration/visual/UX tests cover both ordinary and harness-enabled builds.
> - [ ] Measure identity-dispatch sites, root settlement decisions, mutable owners, fake-addition files/lines and paired per-frame overhead; no cosmetic enum table accepted as full completion.
>
> Duplicates: #319 F6 was only a layer identity audit that allowed no change; this is its explicit D11 extension, not another audit. #471 paper/control ports and #442 control-host are distinct.
>
> Dependencies and priority: P1 after MVU1 #501 is integrated into campaign with exact-head green CI. Coordinate subsequent MT5 #397 first for feed registration; REGB and REGL can run in parallel after MVU1 when ownership is disjoint. MS2 #496 main compatibility is required before integration. Frame #480 stages follow owners incrementally; no wait on aggregate A1/A2 completion.
>
> ## Campaign assignment
>
> Campaign: https://github.com/milocaetano/quantick/issues/472. Authority/priority: D11 https://github.com/milocaetano/quantick/issues/472#issuecomment-5674457925. Owner class: autonomous. Tier: high (state, boundary and hot-path correctness). One isolated mission/PR from reviewed origin/campaign/outside-eight; no main/settings writes. Full source-first independent mission preflight, moved/additive tests, ordered fmt/clippy/build/test, applicable UI/UX/performance checks, exact-diff architecture/AI/delivery reviews and exact-head green CI. Preserve all CLAUDE.md invariants, original safety fixes, ticket/replay, public/financial contracts, guards and frozen seven measurement files. No score awarded by this task.
>
> Evidence: this issue, head-pinned PR reports/CI and parent checkpoint. Issue/Project IDs, owner, branch/worktree, PR/head/base: null until claim/readback. Initial operation/repair counters: 0/0, thereafter cumulative. Report raw before/after coupling, app/root-owned decisions, moved headless tests, affected consumers, and additive probe files/lines/registration sites; distinguish measured deltas from estimates. No new god object or universal dispatcher, renamed roots or file-only moves as acceptance.
