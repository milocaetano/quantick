# Own existing indicator operations behind a narrow context

Reduce the dependencies of existing attach/detach operations while preserving
their UI, remote, ownership, persistence and performance behavior. Child F1,
issue #316, campaign #330; base `origin/campaign/architecture-a` at
`a808b2d87b36d73041027e4d20c053544b454a96`.

**Tier:** high, because this extraction crosses ownership and persistence boundaries.

## Request ledger

- R1: Enumerate exact attach/detach inputs and origin paths before extraction.
- R2: Give human and remote attachment one production operation with narrow dependencies.
- R3: Preserve invalid Pine behavior and failure atomicity.
- R4: Preserve operator ownership, saved-overlay exclusions and whole-target identity.
- R5: Preserve human mirroring across two panes and exactly-once save intent.
- R6: Exercise any introduced port with a fake second implementation.
- R7: Preserve v1 catalogue/schema fixtures and legacy detach addressing.
- R8: Declare command/frame/trade rates and run the existing dense-control baseline.
- R9: Report actual changed dependencies and meet the repository checks/reviews.
- R10: Keep this bounded to F1 without a new root field, bus or session redesign.
- R11: Narrow production detach dependencies without app-wide mutable context.
- R12: Keep UI selection, filesystem orchestration, consent, schema validation
  and registry dispatch in their existing adapters.
- R13: Inventory ambiguous local slot identities and a compatible follow-up,
  without changing legacy detach addressing.
- R14: Add no new hot-path work or cost.
- R15: Collect representative attach latency if compilation or queueing changes.

## Decisions

- D1: The campaign authorizes implementation, tests, commits, push and a draft PR
  based exactly on `campaign/architecture-a`. The coordinator owns checkpoints,
  independent reviews and merges; main integration remains the user's action.
- D2: Preserve the existing UI invalid-script error slot and control prevalidation
  refusal. The newer behavior-preservation instruction governs older ambiguous
  issue wording, as confirmed by the coordinator before editing.

## Assumptions

- S1: Invalid-source no-mutation means the existing validated control attach path;
  library attach deliberately keeps worker-reported errors visible. Both receive
  regression coverage. Changing compilation policy would exceed this task.
- S2: Exactly-once dirty marking means one pending save, retaining the existing
  end-of-operation debounce stamp. No externally visible revision is incremented
  by `LayoutStore::mark_changed`; redundant mirror stamps can be removed.
- S3: A local slot-only v1 detach continues choosing the first operator-owned
  `TabSlot` in insertion order. A versioned target-address follow-up is documented,
  not implemented here.
- S4: UI rendering, schemas, compilation and queueing are unchanged; no new
  surface or capability is added. The existing hooks and registry remain valid.

## Acceptance criteria

- [x] **A1**: Pre-edit dependency/origin inventory recorded. Evidence:
  `docs/architecture/f1-indicator-operations.md`. (R1)
- [x] **A2**: UI and remote operations share narrow production attachment,
  without app-wide context or a new root field. Evidence: operation module,
  adapter call sites and fake-host regression tests. (R2, R10)
- [x] **A3**: Invalid control Pine leaves slots/layouts/save intent unchanged;
  UI worker compilation errors remain visible. Evidence: focused regression
  tests and raw focused-test log. (R3)
- [x] **A4**: Operator detach protects human slots and saved layouts exclude
  operator overlays, including numeric collisions. Evidence: existing control
  tests plus new fake-host and two-pane tests. (R4)
- [x] **A5**: Human add/remove mirrors on two panes with one consumed save intent;
  origin removal follows mirror removal. Evidence: two-pane regression test,
  adapter ordering and focused-test log. (R5)
- [x] **A6**: A fake host exercises the production operation, including owner
  cleanup and colliding local slot numbers. Evidence: operation module tests. (R6)
- [x] **A7**: v1 fixtures and legacy detach addressing remain unchanged. Evidence:
  schema/catalogue suite, diff file list and identity audit. (R7)
- [x] **A8**: Rate declaration and three same-host paired dense measurements
  recorded with raw logs and limitations. Compilation/queueing unchanged, so
  representative attach latency is not a triggered additional gate. Evidence:
  `docs/architecture/f1-indicator-operations.md`, `%TEMP%/quantick-f1-indicator-operation-owner/`. (R8)
- [x] **A9**: Actual dependency reduction and changed files recorded for reviewers.
  Evidence: architecture dossier and draft PR body. (R9)
- [x] **A10**: Production detach resolution and cleanup receive only the
  documented slot collections and resolved pane host, without app-wide context.
  Evidence: `IndicatorSlots::operator_target` and `IndicatorSlots::remove` in
  `crates/app/src/app/indicator_operations.rs`; production calls in
  `crates/app/src/app/indicator_manager.rs` and
  `crates/app/src/app/layout_wiring.rs`; the operation module's
  `fake_hosts_exercise_production_ownership_and_complete_target_cleanup` test
  and `fix-focused-tests.log` in the raw evidence directory below. (R11)
- [x] **A11**: UI selection and filesystem orchestration remain in the existing
  app adapters; consent, schema validation and registry dispatch remain in the
  existing control adapters. Evidence: the dependency inventory in
  `docs/architecture/f1-indicator-operations.md`, the manager adapter call sites,
  the narrow `IndicatorHost`/`IndicatorSlots` interface, and the unchanged
  `crates/app/src/control/` subtree against the declared campaign base. (R12)
- [x] **A12**: The identity inventory records per-pane numeric collisions,
  complete internal `TabSlot` targets, insertion-order v1 operator matching and
  a versioned follow-up, without implementing an addressing migration.
  Evidence: the Compatibility inventory in
  `docs/architecture/f1-indicator-operations.md`, `operator_target` and the
  fake-host collision assertions in the operation module. (R13, R7)
- [x] **A13**: The extraction adds no per-frame, per-trade or per-depth work;
  the existing dense-control evidence supports no new hot-path cost within
  its recorded measurement limits. Evidence: the branch diff confines
  production changes to rare operation wiring, and the Rate and measurement
  plan and Same-host measurements in `docs/architecture/f1-indicator-operations.md`
  record the paired results and limitations. `ledger-source-equivalence.txt`
  in the raw evidence directory confirms this ledger repair leaves production
  source and performance fixtures equivalent to the measured candidate. (R14, R8)
- [x] **A14**: Representative attach latency is collected if compilation or
  queueing changes; neither changes in this extraction, so the condition is
  not triggered. Evidence: unchanged control precompilation, pane allocation
  and worker/compiler sources against the declared campaign base; the
  `IndicatorHost` implementation delegates to the existing Add/Remove path.
  The Rate and measurement plan in `docs/architecture/f1-indicator-operations.md`
  records this applicability decision; dense replay is not attach-latency
  evidence. (R15)

## Injected gates

- [x] **G1**: English artifacts and guards pass. Evidence: guard log; independent review follows the draft.
- [x] **G2**: Before every commit, fmt check, workspace clippy, build and test
  pass in order. Evidence: `%TEMP%/quantick-f1-indicator-operation-owner/` logs.
- [ ] **G3**: Independent architecture review resolves Blockers/Should-fix.
  Evidence: coordinator review dossier and exact-diff marker after the draft.
- [x] **G4**: Performance declaration, no new hot-path work/cost and required
  measurements have evidence.
  Evidence: architecture dossier and dense logs.

R11-R15 and A10-A14 restore five explicit obligations from the original request
after the delivery completeness finding; no existing request is withdrawn or
deferred. This mission remains archived. The raw evidence directory is
`C:/Users/camil/AppData/Local/Temp/quantick-f1-indicator-operation-owner/`.
The `ledger-*` logs record this documentation repair and its verification;
the earlier `fix-*` logs belong to the separate save-assertion test repair.
Dense measurements retain their original candidate attribution in the dossier.

## Not applicable

No engine algorithms, financial logic, capability or visual surfaces change.
Engine test-first, new UI hooks, visual QA and trader UX review are therefore
not triggered. Compilation and worker queueing are retained unchanged. The
new narrow host port follows `new-extension` with a fake consumer; it is not a
new end-user extension framework. No Python or Cargo manifest/lock changes.

## Closing steps

- C1: Archive this mission before independent architecture/delivery review.
- C2: Commit, push and open a draft PR explicitly against the campaign branch.
- C3: Coordinator runs independent AI/architecture/delivery reviews and watches
  full CI; reviews and campaign integration remain pending at implementation handoff.

## Request as received

Attributed English mission request from the campaign coordinator, verbatim:

> Fulfil every existing #316 criterion, narrow production indicator attach/detach dependencies without app-wide context, UI/MCP same implementation, preserve mirroring and exactly-once dirty marking, invalid Pine no mutation, remote ownership, saved-overlay exclusions and v1 catalogue/schema; internal identity audit only, no legacy addressing fix; no root field/new bus/session redesign.

The full issue specification is retained in the pre-edit evidence bundle as
`issue-316.json`; the campaign assignment, its original acceptance criteria and
the coordinator's compatibility clarification govern this child together.

Full delegated assignment from the campaign coordinator, verbatim:

> Implement bounded campaign child F1, existing GitHub issue https://github.com/milocaetano/quantick/issues/316, under parent https://github.com/milocaetano/quantick/issues/330. User explicitly authorizes implementation/tests/commits/push/draft PR and independent reviews, intermediate reviewed-green merges ONLY campaign/architecture-a. Main merge forbidden. Coordinator alone publishes campaign checkpoints and merges. You own ONLY C:/src/quantick-worktrees/feat-indicator-operation-owner branch feat/indicator-operation-owner, created at latest origin/campaign/architecture-a SHA a808b2d87b36d73041027e4d20c053544b454a96. Private git mission-base and tier high already correct. Read current AGENTS/CLAUDE, mission/new-task/ship skills and campaign integration override. Full filesystem/network access; NEVER request routine command approval; do not set sandbox_permissions. Arm guards cargo build -p quantick-guards and cargo check -p quantick-app --all-targets before first edit. One implementation at a time and exclusive benchmark host now yours. No other worker builds. Create high-tier .claude/GOAL.md with atomic R/A/G and English verbatim mission request, archive before reviews. Do NOT create child host goal (parent handles continuity). Mission: Fulfil every existing #316 criterion, narrow production indicator attach/detach dependencies without app-wide context, UI/MCP same implementation, preserve mirroring and exactly-once dirty marking, invalid Pine no mutation, remote ownership, saved-overlay exclusions and v1 catalogue/schema; internal identity audit only, no legacy addressing fix; no root field/new bus/session redesign. Read docs/architecture/foundation.md/preparation-tasks.md F1. Enumerate exact inputs and origins before editing. Introduce minimal internal owner/context; no app-under-another-name. Meaningful production-path second/fake consumer if port introduced; two-pane human mirror/failure tests. Declare rates and run existing dense-control baseline as required, same host control/candidate; no new hot-path cost. Before EVERY commit run fmt check, workspace clippy, workspace build, workspace tests in order and preserve logs. Guards after every edit batch. No weakening tests/guards/rubric or intentional behavior/public contract/financial changes. If genuine scope blocker stop expansion and report precise prerequisite; independent coordinator continues. You may commit implementation + mission archive together after verification. Push and open DRAFT PR explicitly --base campaign/architecture-a, references #316 and campaign parent. CLAUDE's two-phase draft then AI-review is authoritative over stale ship pre-PR prose. Do not review your own work or stamp review markers. Coordinator dispatches independent reviewers after your code/dossier. Return draft PR/head/base, changed-dependency evidence, raw logs paths and concise criterion map. Keep me updated at meaningful stages including before any GitHub business mutation so I can journal intended action. Do not write campaign issue/checkpoints. Existing unrelated dirty worktrees preserved.
