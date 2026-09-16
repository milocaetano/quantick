# Complete bar registration and headless selection ownership

**Tier:** high; deterministic engine factories and application state boundaries.
Objective: transfer complete bar definitions and selection policy to headless
owners so a new supported definition reaches real consumers through registration.
This serves the architecture and additive-extension purpose of campaign #472.

Branch: `feat/bar-selection-registry`; worktree:
`C:/src/quantick-worktrees/feat-bar-selection-registry`.
Issue: https://github.com/milocaetano/quantick/issues/504.
PR: https://github.com/milocaetano/quantick/pull/514.
Declared base: `origin/campaign/outside-eight`.

This archive records the existing mission; it does not assert completion of
post-archive reviews, final-head CI, ship, campaign integration or main delivery.
The live ignored goal is preserved during preparation. The authenticated
[user decision](https://github.com/milocaetano/quantick/issues/472#issuecomment-5700791807)
permits the existing tracked archive workflow again. It supersedes the earlier
no-archive restriction in the retained claim and D16, and the obsolete
cleanup-PR prerequisite in the old evidence index. It waives no current gate.

## Request ledger and source reconciliation

Stable IDs from the live goal are retained. Source spans refer to the full
attributed issue and executor claim below; equivalent wording is not a new ask.

- **R1** — Complete six-family definition ownership across all consumers.
  Source: Scope paragraphs 1-2 and acceptance bullet 1. → A1.
- **R2** — Headless retained selection, validation and availability ownership.
  Source: Scope paragraph 1 and acceptance bullet 2. → A2.
- **R3** — Real additive seventh-definition seam without consumer kind arms.
  Source: acceptance bullet 3 and claim's prohibition on fake-only paths. → A3.
- **R4** — Honest refusal and existing serialization, parser and golden behavior.
  Source: acceptance bullet 4 and claim's public/legacy contract constraint. → A4.
- **R5** — Equivalent UI/hotkey/admitted changes, effects, permissions and bars.
  Source: acceptance bullet 5 and the one-engine constraint in Scope. → A5.
- **R6** — Measured dispatch, moved state, consumers, probe cost and hot-path impact.
  Source: acceptance bullet 6 and Campaign assignment's evidence paragraph. → A6.
- **R7** — Actual responsibility transfer; preserve invariants and frozen contracts.
  Source: Context, Duplicates and Campaign assignment's preservation/no-dispatcher
  clauses; original user architecture steering below. → A7.

The full current source-first reconciliation is retained at
`delivery-514-4be2b63b/completeness-final.md` under the external root below,
and in the published delivery report. It independently recovered the source
before reconciling the same seven R/A pairs; workflow, authority and dependency
clauses map once to G1/G-AI/C1/C2. It did not establish historical pre-edit
compliance. The archive's additional source/traceability text still needs its
own independent current-diff follow-up.

## Decisions and assumptions

D14 permits independent development before MVU1/MS2 integration, retaining
compatibility before integration:
https://github.com/milocaetano/quantick/issues/472#issuecomment-5691228359.
D15 and D16 have the bounded dispositions below; neither is a new user statement.

S1: existing closed BarSpec remains a compatibility codec; generic
BarConfiguration is the internal consumer representation. Only the existing six
definitions register in production. This preserves public behavior while the
test-only seventh fixture checks the extension seam.

S2: configuration/parse/validation and edits are rare and may allocate; frame
descriptor reads borrow static data and scale with catalog size; per-trade and
per-depth builder processing is unchanged. Historical performance evidence is
bounded to its measured workloads, not a general speedup guarantee.

Approved companion decomposition: BarTimeline owns borrowed closed/partial time
lookup; IndicatorState owns whole-tab slot cleanup. The authorized protected
shape amendment is ChartState.spec's type, not numeric cap growth or scanner
changes. The drawing demo preserves u64 saturation. Initial failed guard and
integrated-fixture runs remain failed evidence.

## Acceptance criteria

The following outcomes were independently graded delivered at implementation
head `4be2b63b1c7569b2507729349180c1b0fd53ab55`. Boxes remain open for the
archive-bearing current review; the old report is evidence, not new approval.
All scratch references below are relative to the external evidence root.

- [ ] **A1** — All six definitions own stable IDs, schema/defaults, parse/format,
  requirements and factories; toolbar/config/restore/control/backtest consume them.
  *Evidence:* `crates/engine/tests/bar_registry.rs` six-definition assertions;
  actual config/restore/control and runner paths, as cited by
  `delivery-514-4be2b63b/criteria.md`. → Published delivery report. *(R1)*
- [ ] **A2** — BarSelection owns retained parameters, selected/pending states,
  validation and neutral availability, with moved headless tests.
  *Evidence:* `crates/engine/src/bar_selection.rs` and headless retention,
  refusal and debounce assertions in `crates/engine/tests/bar_registry.rs`.
  → Published delivery report and external criteria report. *(R2)*
- [ ] **A3** — Fake seventh implementation plus registration uses production
  selection, descriptor editor/projection and factory, no consumer kind arms.
  *Evidence:* `crates/engine/tests/support/seventh_bar.rs`, engine registry
  tests, actual editor/ChartState test in `crates/app/src/toolbar/bar_parameters.rs`,
  DTO tests in `crates/app/src/control/types.rs`, and actual runner test in
  `crates/backtest/tests/harness.rs`. → Delivery and coupling reports. *(R3)*
- [ ] **A4** — Preserve legacy BarSpec/DTO formats, parser acceptance, clamp rules
  and goldens; reject unknown IDs/parameters and unavailable deal counts honestly.
  *Evidence:* engine registry/golden/refusal tests, legacy codec and DTO fixtures,
  renderer clamp tests, backtest deal-counter refusal, and retained BR-ARCH-01
  fail-before/pass-after logs. → Delivery report and repair record. *(R4)*
- [ ] **A5** — UI and admitted control converge on typed selection commands/effects;
  preserve deferred loading/rebuild, chart/backtest parity and permission gates.
  *Evidence:* `crates/app/src/app/tests/bar_registry_tests.rs` actual admitted
  gateway, normalization, parity and missing-scope assertions; engine settle
  tests and backtest parity assertions. → Delivery and architecture reports. *(R5)*
- [ ] **A6** — Measure dispatch/state/blast radius; registry work is construction-time
  with no additional per-trade lookup/allocation.
  *Evidence:* `bar-owner/coupling-current.md`: kind sites/files 21/7 → 6/1,
  app 9/6 → 0/0; three retained fields move; 28 app/backtest consumer paths;
  probe 178 narrowly attributable lines across six files, four actual test
  registration sites and zero new production consumer kind arms.
  `registry-performance-bar-4be2b63b.json` contains 20 paired frame samples:
  median average CPU 0.6335335 → 0.636015 ms (+0.39%); no demonstrated material
  regression or speedup. Historical ingest timing excludes construction.
  → Published coupling report and independent performance report. *(R6)*
- [ ] **A7** — Real responsibility ownership, no global dispatcher; preserve frozen
  measurement files, guards, financial contracts, ticket and replay.
  *Evidence:* full 56-file implementation diff, separate definition/selection
  owners, protected spec type-only amendment, tightened root/UI-free caps and
  ordered workspace/guard proof. → Architecture and delivery reports. *(R7)*
- [ ] **G1** — Ordered fmt/clippy/build/test, guards, moved/fake fixtures, proportional
  UI/UX/performance validation. Raw output outside Git; concise PR reports.
  *Authority:* CLAUDE.md, mission/ship/delivery skills, issue Campaign assignment
  and delivery contract. Includes current source-first reconciliation, English,
  architecture review and exact-head CI. *Evidence:* indexed logs, independent
  source/pixel/performance reports and CI below. → Current PR reports/CI;
  historical preflight and inherited visual debt use only D16/D15 below.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

AI gate destinations: current PR AI report published through the shared producer,
its matching private projection, and current zero-thread listing. Historical
report/key below must not be copied or re-stamped as archive-bearing approval.

## Closing steps

- [ ] **C1** — Root owns commits, PR publication, independent reviews and CI/readiness.
  *Authority:* mission step 8, ship and delivery contract. Implementation draft
  and prior reviews exist; archive-bearing current reviews, exact-head green CI,
  current PR-body evidence and final `mission_ship_gate.sh` reconciliation remain
  pending. → PR #514 and its final-verifier report.
- [ ] **C2** — MVU1/MS2 compatibility before integration; no main/settings changes.
  *Authority:* issue Dependencies/Campaign assignment, D14 and campaign integration
  contract. Coordinator verifies latest campaign ancestry, compatibility and
  serialized head-pinned integration. Only the user merges to main.
  → Campaign #472 checkpoint and integration readback.

No new bar type, language/protocol expansion, financial/order authority or
campaign score is claimed. This child cannot discharge final campaign UI debt,
score gates or main completion.

## Evidence identity and reuse boundary

External root: `C:/src/quantick-worktrees/outside-eight-coordination`.

Prior verified implementation commit:
`4be2b63b1c7569b2507729349180c1b0fd53ab55`;
tree `f4c6887fac6da5b493553fe4bf906b9486b2a0aa`;
base `c9138e6100921bf70f8126a4cf308c47bbe8b183`;
review key `e73128d02e50082552c0a96bbaf210f633bdf59f`;
binary-diff fingerprint `c96b5999ce6d0c034f06cae14d38243b42d0f842`.
The 56-file implementation diff was +2757/-880 before this archive.

- Ordered outputs: `bar-owner/BR-ARCH-01-current-{fmt,clippy,build,test,guards}.log`.
  `bar-owner/BR-ARCH-01-repair.md` records successful statuses, source fingerprint
  and prior failed runs; empty fmt/guard output alone is not proof.
  Independent analysis in `bar-registry-review/committed-repair-and-performance.md`
  counts 3989 passed, zero failed, 22 ignored across 113 result groups.
- Original regression failures: `bar-owner/BR-ARCH-01-{engine,app}-before.log`;
  integrated failed fixture: `bar-owner/BR-ARCH-01-integrated-test.log`.
  BR-ARCH-01 remains fixed at batch 1 / attempt 1; no counters reset.
- Source reviews: `bar-registry-review/initial-source-review.md`,
  `integrated-source-followup.md`, `arch-review-final.md`, `ai-review-final.md`.
- Complete delivery dossier: `delivery-514-4be2b63b/{identity.json,branch.diff,
  criteria.md,completeness-final.md,final-delivery-review.md,evidence-index.md}`.
- Visuals: `bar-registry-review/pixel-assessment.md` and
  `mvu-visual/bar-4be2-*`; initial tick-wide missing-metadata capture is refused,
  not PASS; `bar-4be2-tick-wide-v2` is the replacement validated capture.

Durable reports for the prior implementation identity:
[architecture](https://github.com/milocaetano/quantick/pull/514#issuecomment-5692079973),
[AI](https://github.com/milocaetano/quantick/pull/514#issuecomment-5692080852),
[visual/trader](https://github.com/milocaetano/quantick/pull/514#issuecomment-5692081439),
[coupling](https://github.com/milocaetano/quantick/pull/514#issuecomment-5692081622),
[delivery](https://github.com/milocaetano/quantick/pull/514#issuecomment-5692577908),
and [implementation-head CI](https://github.com/milocaetano/quantick/actions/runs/35055009288).

This preparation adds only a mission/evidence record. Runtime reuse is a candidate,
not yet a new validation result. Before reuse, inspect the entire final name/status
and content delta, unchanged base, tracked and relevant untracked/generated inputs,
toolchain/dependencies/config/environment identity, successful source-bound outputs,
guards, hygiene and affected links under docs/workflow/delivery.md. The historical
toolchain file includes an unusable `System.Object[]` line and cargo version only;
it cannot alone prove full environment identity. Unknown relevant inputs or base
changes require affected verification. Record reused-from identity separately
from newly-run checks; current independent verdicts and final-head CI remain due.

## Deferred

**CP-01 / D16:** historical independent pre-edit source/map review is
unsubstantiated; #504's first test edit preceded its ignored goal. Recovered
chronology at `registry-preflight-recovery/504-bar.md` records first test
02:47:08.055Z, goal 02:47:41.737Z and first identified runtime edit
02:49:55.643Z. No later review proves the missing historical action occurred.
The [D16 coordinator decision](https://github.com/milocaetano/quantick/issues/472#issuecomment-5692520814)
accepts only this retrospective process deficiency under the user's standing D1
decision delegation and the adopted delivery contract. Full current source-first
reconciliation, product criteria, tests, reviews, visual/score obligations, CI and
closing gates remain required. The independent completeness report checked the
grant and disposition and closed CP-01 by this bounded exception, not historical
compliance. The later archive authorization supersedes only D16's no-archive clause.

**D15 inherited visual debt:** the
[coordinator decision](https://github.com/milocaetano/quantick/issues/472#issuecomment-5692030706)
accepts unchanged 650-logical-point collapsed-picker/layer overlap and loading
overlay dropdown occlusion for this child. Baseline `97edcb664af0fbbafc444e36f787c9bae03f4328`
has the same source tree as base c913 and reproduces both. These pixels are
not PASS; final campaign UI debt remains. No introduced defect, permission or
data-honesty regression is covered; no original #504 outcome is deferred.
Source-identified baseline/candidate captures are
`mvu-visual/bar-baseline-time-narrow` and `mvu-visual/bar-4be2-time-narrow`.
The independent pixel report preserves one-drain skew, missing fine-grained
bounds, absent original OS-pixel/DPI proof and transient wall-stall limitations.

## Attributed original user steering — verbatim

> Eu preciso que melhore nossa arqutietutra e nao ficar provando ABC + D que tao coisa minuscula que vc fez eh bom só para vc
> Crie um design pattern quebre esse APP deixa essa parada bonita e escalável para recrutador ver e gostar

## Full delegated issue source — verbatim

Source: https://github.com/milocaetano/quantick/issues/504,
read from the GitHub issue body on 2026-09-16. Quotation retained in full:

> <!-- campaign-task:milocaetano/quantick#472/REGB -->
> ## Context
>
> D11 complete-family registry child under A2 #479; contributes to headless ownership A1 #478. This is actual policy/state ownership and additive extension, not enum renaming.
>
> ## Scope
>
> Equivalent owner/source: engine/spec.rs owns shared BarSpec but still separate kind/default/parse/build matches. App state.rs SpecSelector owns retained kind parameters; toolbar.rs, control/types.rs and bar_kind_reason.rs repeat policy/projection decisions. Transfer definitions to engine's bar registry and selection state to a headless BarSelection owner. A definition provides stable ID, neutral parameter schema/defaults, validation/parsing/formatting, input requirements and builder factory. Typed select/update commands plus neutral BarInputAvailability produce validated changes; UI renders descriptors and executes changes. Engine cannot import feed, control or UI.
>
> Complete family: Tick, Volume, Dollar, Time, Imbalance (trades/volume/dollar units) and Trades, including interval/deal-counter availability. Keep one engine path for chart/backtest/bot; registration is configuration-time, never per-trade dispatch overhead.
>
> ## Acceptance criteria
>
> - [ ] All six definitions register parameters/defaults/parse/requirements/builders; UI, config/workspace restore, control and backtest consume the same definitions.
> - [ ] Retained selection parameters, active spec, validation and availability decisions leave app; owner tests run without app and existing goldens remain.
> - [ ] Fake seventh kind docks as implementation plus registration without toolbar/control/backtest match additions; neutral fake fixture covers the same factory path.
> - [ ] Unknown IDs/parameters, invalid thresholds and unavailable deal counters refuse honestly, with existing public serialization/legacy behavior preserved by contract tests; no silent substitutions.
> - [ ] UI/hotkey/admitted MCP changes produce equivalent validated state/effects without weakening permission checks or changing chart/backtest bars.
> - [ ] Measure kind-dispatch sites/files, state moved, consumers edited and fake addition lines/registrations; no hot-path allocation/lookup regression.
>
> Duplicates: PR #484 delivered shared BarSpec vocabulary, not complete registration. #479/#478 remain aggregate parents; no new bar type or language/protocol expansion is requested.
>
> Dependencies and priority: P1 after MVU1 #501 is integrated into campaign with exact-head green CI. Coordinate subsequent MT5 #397 first for feed registration; REGB and REGL can run in parallel after MVU1 when ownership is disjoint. MS2 #496 main compatibility is required before integration. Frame #480 stages follow owners incrementally; no wait on aggregate A1/A2 completion.
>
> ## Campaign assignment
>
> Campaign: https://github.com/milocaetano/quantick/issues/472. Authority/priority: D11 https://github.com/milocaetano/quantick/issues/472#issuecomment-5674457925. Owner class: autonomous. Tier: high (state, boundary and hot-path correctness). One isolated mission/PR from reviewed origin/campaign/outside-eight; no main/settings writes. Full source-first independent mission preflight, moved/additive tests, ordered fmt/clippy/build/test, applicable UI/UX/performance checks, exact-diff architecture/AI/delivery reviews and exact-head green CI. Preserve all CLAUDE.md invariants, original safety fixes, ticket/replay, public/financial contracts, guards and frozen seven measurement files. No score awarded by this task.
>
> Evidence: this issue, head-pinned PR reports/CI and parent checkpoint. Issue/Project IDs, owner, branch/worktree, PR/head/base: null until claim/readback. Initial operation/repair counters: 0/0, thereafter cumulative. Report raw before/after coupling, app/root-owned decisions, moved headless tests, affected consumers, and additive probe files/lines/registration sites; distinguish measured deltas from estimates. No new god object or universal dispatcher, renamed roots or file-only moves as acceptance.

## Full delegated executor claim — verbatim

Source: https://github.com/milocaetano/quantick/issues/504#issuecomment-5691233184.
Historical archive restriction is superseded as recorded above; other constraints
remain mapped to the existing R/A/G/C IDs.

> executor: gpt-6-astra — strongest model for deterministic engine registration, headless selection ownership and additive boundary design.
>
> D14 architecture-first parallel claim under D1/D6: https://github.com/milocaetano/quantick/issues/472#issuecomment-5691228359. Complete existing #504 outcome; independent development from reviewed campaign9ff57501249f51d8f75c21f52094ec3dc3c39af2 in `feat/bar-selection-registry`. MVU1/MS2 compatibility remains required before integration, not before independent development. No stacked unmerged source or main/settings changes.
>
> Only ignored local mission; raw verification outside Git and concise PR reports. Root independently reconciles source-first criteria before runtime edits. Keep one deterministic builder path, existing public/legacy behavior and unknown-input refusals. No blanket forwarding, fake-only extension path or cosmetic enums/table replacement.
