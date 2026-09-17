# A1C mission

**Tier:** high — authorization/admission boundary and generic owner design.

# A1C source map and pre-implementation defaults (draft)
tier: high | objective: Move capability registration and admission ownership into control-host while preserving authorization, schemas and observable behavior.

Source: issue521 retained verbatim at C:/src/outside-eight-coordination/a1c-issue.md; campaign472 original source/authority and A1#478 remain binding. Exact base a6e554386b037bb2ec49fe29ff6494e4fef0460a. Bootstrap guard build passed, app check92579 still in progress; no source edits. This external draft is for independent source-first reconciliation, not approval or runtime evidence.

## Stable ledger
R1: Transfer actual generic state and complete registration/admission/output transaction, private ownership/narrow interfaces, into existing control-host; app retains vocabulary/policy/handlers/effects. Source Context + Scope1-5 -> A1,A2,A3.
R2: Preserve validation order, explicit read/external binding, complete dynamic grant before returned execution route, permission union and fail-closed error details. Source Scope2-4/A2 -> A2,A3.
R3: Preserve public descriptors/schema/metadata/wire output, default consent/sensitivity, deterministic order, external schema precedence and app/gateway compatibility. Source Scope3-5/A3 -> A3.
R4: Move tests with generic decisions; preserve primitive/concrete app regressions; prove production API via plain second host with two reads and external action. Source Scope6/A4 -> A4.
R5: Compile schemas once at startup; avoid added unbounded request work/clones/locks; freeze six-case matched corpus and report three paired timing/allocation observations, startup separate, capture budgets preserved. Source A5 -> A5.
R6: Actual ownership/coupling/consumers/headless-test inventory and scratch additive registration probe, no invented root or score improvement. Source Context/A6 -> A6.
R7: Preserve safety/ticket/replay/domain/guards and frozen seven; no main/settings change. Source A7/assignment -> A7 plus operational G gates.

A1-A7 remain the original issue IDs and full wording; copy them without shortening into the live mission, with evidence destinations docs/quality/capability-admission-owner-evidence.md + child/PR reports.

## Decisions under campaign D1
D1: Main/settings remain user-only. Existing D1 delegates engineering defaults; no extra question or narrowing. High tier because this is authorization/admission ownership. No new authority or capability.
D2: Reuse existing admission::register_capability/admit_capability/admit_payload/admit_scopes, catalogue functions and ControlRegistry; no forked validation. Concrete descriptors/effects/modules and safe grants remain app composition.
D3: An explicit private binding distinguishes Read(P) from External. Registration must not let descriptor/token/schema association become partial on failure. Do not add an unrestricted registry_mut escape. Retain a narrow read-only registry/catalogue interface required by current callers.
D4: The admission method owns static -> payload/tier -> selected token/preparation -> dynamic permissions -> finalized result. App maps the finalized generic route into PreparedRequest/PreparedDispatch only after success. No callback executes effects. A typed callback result containing prepared value and dynamic permissions is internal to finalization, not an admitted result.
D5: Per control request, not a per-trade/depth/frame market hot path; startup schema compilation separate. Before runtime changes, add/freeze exact-base observation harness with six corpora: successful small read; dynamic snapshot; denied static grant; malformed payload; registered external action; valid/invalid output validation. Use same case definitions and expectation bytes in candidate. Three AB/BA/AB paired runs, all samples retained; fixed batch count chosen once using a baseline-only duration check before source edits so timing is not a ~16ms control. Report raw durations and ratios per case, baseline/candidate allocation calls/bytes separately from timing instrumentation, and startup compilation cost separately. No pooled acceptance across different request classes. A >5% median increase is a review trigger requiring a concrete explanation or repair, not a score/automatic acceptance. Do not silently exclude or rerun a failed case. Freeze precise batch/warmup/allocator/format details after baseline source harness inspection and before any implementation.
D6: Generated inventory/catalogue/schema files must be byte-identical. Capture complete serialized real app describe/descriptors and request success/error outcome signatures at exact base, retain raw byte files. Only actual build commit identity may be normalized explicitly. Do not merely compare IDs or entry counts.
D7: No intended visual/action behavior change, no new mouse-only surface; public contract/parity and app control integration are the applicable UI evidence, not a visual PASS. If actual delta touches visible behavior, apply ui-harness/visual-qa/trader-ux-review before delivery.
D8: Existing authority vectors retain original order in describe, even where registry iterators sort. Read-only accessors must not introduce catalogue reconstruction/compile per request. App can supply registry-authority composition to a validated constructor/builder; do not expose mutable backing maps after ownership transfer. Exact API choice follows source-first review; no speculative framework.

## Proposed owners and coupling
control-host/src/contract.rs: private registry, token/binding map, compiled input/output indexes, ordered profile/permission vectors, scope descriptor vector and permission index. Cohesive registration/query/admission/output methods; small separate helper if module exceeds ratchet, not split for count.
App ObserverContract: contract owner + Arc<ActionRegistry> + EvidenceStore, concrete registration/authority declarations, app-version/commit/instance describe assembly, safe default/selectable policy, final dispatch adaptation. Generic prepare state machine leaves app.
App reads.rs: prepare_snapshot/evidence_capture use narrow borrowed scope permission lookup, retaining duplicate/unknown-scope errors and actual typed invocation construction. Other handlers unchanged.
control-host/admission.rs and catalogue.rs remain reused primitive owners. dispatch/evidence/idempotency/projection machinery unchanged. No gateway execution, financial/domain/feed/series/indicator/layout expansion.

## Load-bearing tests and evidence
- Plain second host registers two reads + external action through public owner; exact token selection, denied/static no callback, payload/tier/version/key/revision refusal, callback success followed by dynamic denial returns no executable value, final union and sorted missing details.
- Duplicate/invalid schema registration leaves no reachable partial state; missing token/external schema/output validator fail closed. External-schema precedence and matching version, not fallback to accidental read.
- Keep existing primitive admission_contract.rs tests unchanged. Move generic app cases where they truly exercise owned state; keep concrete trade/cockpit consent, grants and real catalogue tests in app.
- Full old app control-plane suite includes queue/timeout/late-work/cancellation/idempotency/uncertainty, wrong token, schema/coherent capture, paper/replay and observer action denial; no assertion weakening.
- Full exact-base byte signature/descriptor comparison, all frozen seven blobs, actual source/caller test relocation inventory and scratch-only additive registration probe.
- Full ordered fmt/clippy/build/test at final tree, relevant extra checks; current full architecture/AI/source-first delivery and head CI. No reuse claim until exact tested inputs checked.

## Operational gates / closing
G1 English/invariants/ratchets/frozen seven; G2 source-first map and deterministic baseline before implementation; G3 declared rate and complete A5 measurements; G4 full checks/current CI; G5 current architecture and AI evidence; G6 actual UI applicability and retained safety regressions.
Copy four reserved G-AI lines and What done means block literally from canonical mission into live goal. Closing C1 draft/archived mission and evidence, C2 full independent delivery last, C3 ready/current CI/canonical final verifier, C4 reviewed campaign-only head-pinned merge then child/Project reconciliation. Full campaigngoal stays active.

## Acceptance criteria

- [ ] A1: One headless owner holds the generic registered state and complete admission decision transaction with private collections and explicit inputs/results. App retains concrete authority/composition/effects; no premature executable result, generic dumping owner or broad forwarding context.
- [ ] A2: Preserve registered read/external-action distinction; static denial precedes payload/preparation; payload/tier/version/key/revision checks and dynamic permission union retain exact behavior. Missing/duplicate/mismatched bindings or validators fail closed. Preparation callback is never invoked on static denial and its successful preparation cannot escape a later dynamic denial.
- [ ] A3: Preserve every public capability/scope/profile/permission descriptor, deterministic ordering, request/result shape, effect/risk/default/sensitivity, denial code/details/retryability and external-action output schema precedence. Generated capability inventory/catalogue/schemas are byte-identical; describe comparison allows only actual build commit identity to differ.
- [ ] A4: Existing primitive and app control-plane regressions remain; relevant owner tests move without weakening fixtures/assertions. Independent non-UI host uses the real public registration/admission/output APIs with two read bindings and an external action, proves valid/invalid routes, dynamic denial, callback/effect counts and deterministic metadata. No hand-reimplemented test-only transaction.
- [ ] A5: Schema compilation remains startup-only; request processing introduces no new unbounded/request-amplified work, unnecessary payload/history clone, lock or GUI construction. Freeze a matched exact-base/candidate corpus before implementation: small successful read, dynamic-scope snapshot, static denial, malformed payload, external action and output validation; three paired same-host timing runs plus explicit allocation observations, startup separately. Preserve app capture-budget tests; explain meaningful regressions before delivery. This is per control request, not per trade/depth/frame; do not claim domain hot-path or incremental-build gains without corresponding proof.
- [ ] A6: Report actual before/after state/decisions, root/app ownership, cross-owner calls, moved headless tests, affected real consumers and a scratch-only localized additive registration probe with files/lines/sites. Distinguish unchanged root branches and equal costs from improvements; no automatic score or denominator padding.
- [ ] A7: Preserve original campaign safety fixes, ticket/replay, determinism/data honesty, one engine, headless/dependency/cycle/size/context/UI-free guards and all seven frozen rubric/scanner/skill blobs. Full fmt/clippy/build/test, relevant extra checks, source-first high-tier completeness, current architecture/AI/delivery reviews and exact-head green CI pass before reviewed campaign-only integration. No main/settings changes.


Evidence destination for all A1-A7 and G gates: docs/quality/capability-admission-owner-evidence.md plus source-bound child/PR reports. Ledger links: A1(R1);A2(R1,R2);A3(R1,R2,R3);A4(R4);A5(R5);A6(R6);A7(R7). Full independent source-first map review is pending; no runtime implementation is authorized before its findings and concrete measurement defaults are resolved.

## Injected acceptance gates

- [ ] **G1** — English, determinism, dependency/headless/size/cycle/context/UI-free invariants and frozen seven verified.
- [ ] **G2** — Independent source-first preflight and exact-base deterministic parity/measurement fixture freeze precede runtime edits.
- [ ] **G3** — Per-control-request/startup rates declared, complete paired corpus/allocation/startup measurements and affected capture-budget tests reported.
- [ ] **G4** — Full ordered fmt/clippy/build/test and affected extra checks pass; exact-head final CI is green.
- [ ] **G5** — Current full architecture review and AI review resolved through canonical durable producers.
- [ ] **G6** — Actual-diff UI applicability checked; public behavior, safety/idempotency/uncertainty, ticket/replay and app gateway coverage preserved.
<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

G-AI evidence: current PR durable report, thread-list output and validated current-key private projection.

## Closing steps

- [ ] **C1** — Draft PR includes archived mission, full source and evidence.
- [ ] **C2** — Independent full delivery review passes last at current review key.
- [ ] **C3** — Non-draft current-head PR, green CI and canonical mission/ship final verifier PASS.
- [ ] **C4** — Reviewed head-pinned campaign-only merge with ancestry, child and Project readback; return to campaign. No main merge.

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

## Verbatim delegated issue request

<!-- campaign-task:milocaetano/quantick#472/A1C -->
## Context

A1 #478 child in campaign https://github.com/milocaetano/quantick/issues/472. Stable key A1C. Own the generic capability registration and admission transaction in the existing headless quantick-control-host crate. Reuse the retained proposal https://github.com/milocaetano/quantick/issues/478#issuecomment-5671255775 and current-source frontier investigation https://github.com/milocaetano/quantick/issues/478#issuecomment-5705338682. Source base: campaign a6e554386b037bb2ec49fe29ff6494e4fef0460a, tree70e1c6eb39c17915c9b5215382bcedafa2401a37.

Current app/control/contract.rs coordinates descriptor/token/schema associations, eight generic collections, static/payload admission, read preparation and final dynamic-scope authorization even though the individual validators already live in control-host. Transfer that real transaction and its tests, not just structs or forwarding methods. Concrete permission policy, handlers and effects remain app responsibilities. This contributes to A1's coherent UI-free ownership; it promises no QuantickApp field/branch/impl-spread reduction and does not close A2. A1 <=25% UI-free share, A2 <=8 impl spread and all campaign score/safety criteria remain required.

## Scope

- Introduce a narrow concrete owner, provisionally CapabilityContract<P>, in control-host, using existing ControlRegistry, admission and catalogue primitives. Own the registry, read preparation-token associations, compiled input/output schemas, profile/permission descriptors and indexed scope catalogue. Keep collections private; no blanket context, Deref forwarding, new dependency, replacement god object or per-request full-app snapshot.
- Registration associates descriptors, tokens and compile-once schemas through production operations. Make external action bindings explicit without treating an accidentally missing read handler as a valid action. Preserve today's registration errors, discovery and denial precedence.
- Own static envelope/scope checks -> payload/tier checks -> route resolution -> opaque read-preparation callback -> dynamic grant finalization -> final required-permission union. Never return an executable read after preparation but before its dynamic scopes are authorized. Callback receives only token/payload/narrow catalogue view and does not execute effects.
- Own generic output validation selection, retaining external action schema precedence over the registered read validator and fail-closed missing validators. Keep concrete ActionRegistry, evidence handles, gateway/socket/frame execution, default grants, permission sensitivity and handler vocabulary in app.
- Keep app ObserverContract/PreparedRequest compatibility adapters and existing gateway consumers. Replace reads.rs direct scope-map access with narrow indexed queries. No unrelated projection registry, control dispatch/idempotency/cancellation, feed, series, layout, financial or UI behavior change.
- Move generic owner tests and add a plain second host using two opaque read tokens plus an external action schema provider through the PUBLIC production API, depending only on control-host/control. Keep concrete app-policy and end-to-end control-plane tests intact.

Expected reservation: new control-host/src/contract.rs plus integration tests, control-host lib export/README, app/control/contract.rs and narrow contract/reads.rs queries. Compiler-required import adaptation only elsewhere unless explicitly justified before expansion. Dependency direction app -> control-host -> control. No new capability, wire version, authority scope, clock or GUI dependency.

## Acceptance criteria

- [ ] A1: One headless owner holds the generic registered state and complete admission decision transaction with private collections and explicit inputs/results. App retains concrete authority/composition/effects; no premature executable result, generic dumping owner or broad forwarding context.
- [ ] A2: Preserve registered read/external-action distinction; static denial precedes payload/preparation; payload/tier/version/key/revision checks and dynamic permission union retain exact behavior. Missing/duplicate/mismatched bindings or validators fail closed. Preparation callback is never invoked on static denial and its successful preparation cannot escape a later dynamic denial.
- [ ] A3: Preserve every public capability/scope/profile/permission descriptor, deterministic ordering, request/result shape, effect/risk/default/sensitivity, denial code/details/retryability and external-action output schema precedence. Generated capability inventory/catalogue/schemas are byte-identical; describe comparison allows only actual build commit identity to differ.
- [ ] A4: Existing primitive and app control-plane regressions remain; relevant owner tests move without weakening fixtures/assertions. Independent non-UI host uses the real public registration/admission/output APIs with two read bindings and an external action, proves valid/invalid routes, dynamic denial, callback/effect counts and deterministic metadata. No hand-reimplemented test-only transaction.
- [ ] A5: Schema compilation remains startup-only; request processing introduces no new unbounded/request-amplified work, unnecessary payload/history clone, lock or GUI construction. Freeze a matched exact-base/candidate corpus before implementation: small successful read, dynamic-scope snapshot, static denial, malformed payload, external action and output validation; three paired same-host timing runs plus explicit allocation observations, startup separately. Preserve app capture-budget tests; explain meaningful regressions before delivery. This is per control request, not per trade/depth/frame; do not claim domain hot-path or incremental-build gains without corresponding proof.
- [ ] A6: Report actual before/after state/decisions, root/app ownership, cross-owner calls, moved headless tests, affected real consumers and a scratch-only localized additive registration probe with files/lines/sites. Distinguish unchanged root branches and equal costs from improvements; no automatic score or denominator padding.
- [ ] A7: Preserve original campaign safety fixes, ticket/replay, determinism/data honesty, one engine, headless/dependency/cycle/size/context/UI-free guards and all seven frozen rubric/scanner/skill blobs. Full fmt/clippy/build/test, relevant extra checks, source-first high-tier completeness, current architecture/AI/delivery reviews and exact-head green CI pass before reviewed campaign-only integration. No main/settings changes.

## Campaign assignment

Parent aggregate: #478; campaign #472. Owner class autonomous; priority P0 architecture frontier under D11/D14, after active bounded delivery work releases the build host. Tier high: admission/security boundary and generic owner design. Existing F1/R1 reviewed campaign integrations are prerequisites satisfied in a6e55438; REGB/REGL/MVU1 already integrated. No dependency on blocked MT5 #397, series #517 or projection #502; preserve their work/counters. Source-disjoint implementation requires its own new-task worktree from current campaign base and source-first mission preflight before edits. Root serializes build-host use and merges under D1/D6.

Overlap: #442 includes a broader control-host outcome on a different campaign; this does not close/retarget it or prove its incremental rebuild criterion. #471 still owns paper Deref/strategy-index and ProjectionRegistry forwarding; excluded here. #318's indicator projection/shared revision/coherent captures are excluded. #490 already delivered evidence ownership. Searches on 2026-09-16 found no dedicated admission issue/PR; repeat exact marker discovery before creation/recovery.

Evidence: child comments, docs/quality/capability-admission-owner-evidence.md, source-bound mission archive, current-head PR reports/CI and campaign checkpoints. IDs, worktree, branch, PR/head and owner remain null until claim/readback. Initial operation/repair counts0; preserve all later attempts. Final A1/A2 measurements and independent campaign/main assessment remain outstanding.

executor: gpt-6-astra — strongest model for admission boundary design, source-first reconciliation and independent judgment under campaign D4.


## Verbatim campaign continuation

> terminar a campanha... fazer oq ue precisa ser feito para entregar tem toda autoização

The full original campaign source remains in https://github.com/milocaetano/quantick/issues/472; K1-K6/M1-M3 and exclusive user main/settings boundaries remain unchanged. This child is not a substitute for the campaign's aggregate outcomes.

## Preflight status and concrete follow-up defaults

Bootstrap92579 completed with both guard-build/app-check exit0 before this live mission. Independent source-first map reconciliation is complete and outcome-complete: https://github.com/milocaetano/quantick/issues/521#issuecomment-5706253618 . Runtime implementation still waits for the concrete D9-D11 review and baseline freeze. Additive baseline fixture preparation may proceed only under its separately recorded operation. Existing work_meter::Counting is always active in test builds; timing is matched instrumented app-test timing, not uninstrumented production latency.

# A1C concrete pre-implementation decisions D9-D11
Under campaign D1, preserving issue521 A1-A7. These complete draft D3/D5/D8; independent source-first confirmation remains required before runtime edits.

D9 (constructor): use a small concrete ContractBuilder created from owned ordered profile/permission vectors. It registers those exact values and finalizes authority using existing ControlRegistry. Expose only explicit register_module and register_effect operations during builder composition, retaining app declaration order/errors. build(projections: &ProjectionRegistry<H>) derives the validated scope catalogue with the existing function and moves the same state into CapabilityContract<P>. No accepting unrelated prepopulated registry/vector pairs, no capability registration on builder, no registry_mut, no blanket Deref. After build, register_read and register_external own associations. This avoids inconsistent metadata and unbound prepopulated capabilities by construction; public API tests prove available construction/registration paths. Preserve profiles observer/annotator/trader/cockpit and permission/scope/registry orders, not lexicalizing profiles.

D10 (external binding): private Read(P)/External binding discriminant. External registration uses the actual externally owned descriptor and existing registry validation; no read-handler absence implies an external route. At request time a narrow borrowed ExternalSchemas view names the provider's actual descriptor ID/version plus compiled input/output validators. A provider must report its real descriptor identity; owner checks requested registered ID/version exactly, after envelope/static admission. Missing/mismatched external input provider uses existing CAPABILITY_UNAVAILABLE with 'registered observer capability has no input validator', retryablefalse, and cannot call read preparation/fallback. Do not compare/recompile JSON schemas per request. The provider is trusted host composition, not an untrusted client; its association of compiled validators to descriptor is guaranteed by ActionRegistry::register_resolved, not cryptographically attested by these types. Do not claim defense against a malicious host supplying falsely labelled schemas. App retains Arc<ActionRegistry>; no external schema or handler ownership moves.
For output validation, first require a registered exact ID/version; a matching external output schema still takes precedence over a read output validator when supplied. Mismatched provider identity returnsfalse; an External binding with no provider returnsfalse, while a Read with no provider uses its registered output schema. Existing primitive external-input override test remains untouched; explicit owner READ admission uses registered read schema, with no accidental action promotion. All currently reachable app behavior remains parity-checked. Preserve RequestEnvelope::validate canonical checks before static denial and TierPolicy::STRICT omitted-revision behavior.

D11 (baseline-only observation freeze): use existing real ObserverContract and standard_registry/actions/EvidenceStore without a window or action execution. Six fixed cases:
1 control.describe input{} with defaultgrant -> prepared Worker;
2 snapshot.read with scopes [system.info,health.summary] and exact grant [observe,observe.system,observe.health,observe.indicators,observe.orderflow] -> prepared Ui with that full permissionunion;
3 snapshot.read payload{scopes:42} and emptygrant -> static PERMISSION_DENIED, not payload error;
4 snapshot.read payload{scopes:42} and defaultgrant -> schema INVALID_REQUEST;
5 attention.mark.create payload{} with exact annotate+annotate.attention staticrequiredgrant -> prepared Action, no execution;
6 outputvalidation for control.describe using the actual complete describe value plus invalid{} -> true/false; include an external mark output pair if required by actual action schema, classify it separately within outputcase.
Confirm exact IDs/field schemas from source before fixture edits; if a named fixture is not valid, correct and document before baselinefreeze, not after candidate results. Add exact signature/parity cases for dynamicdenied sensitive scopes, strict revisions/key/dryrun, malformed envelope, version mismatch, external output precedence and full ControlError serialization, beyond six timed cases.

Preparation executor may add ONLY additive test harness/fixture files and one cfg(test)module registration to app contract before runtime edits; existing tests/runtime bodies unchanged. Capture serialized complete describe and outcome/permission/error signatures to external raw files, normalizing only actual build commit. Do not execute prepared UI/action effects. Timing API calls include required envelope.clone and result/drop work consistently; fixture/contract/grant/outputsetup and serialization/logging excluded. Returned results black_box, discarded inside loop. Separate startup case repeatedly constructs/drops fresh wholecontract; label it construction+registration/schema-compilation, not isolated compiler time.

One baseline-only calibration per case: 3 untimed warmup operations before the first pilot; start1000 iterations (startup1), double until one whole timed block >=250ms, ceiling1048576 iterations (startup128). The next count is min(current*2,ceiling), so the final pilot hits the declared ceiling exactly. Keep every calibration sample, use the first qualifying count, no CPU/priority/power changes and no fastestsample choice. Failure to reach target at ceiling requires a new decision; no silent count beyondceiling. Freeze resulting counts before runtime edits. Subsequent measurement: 3 untimed same-count blocks then one measured block per case/process, three paired independentprocess runs AB/BA/AB; retainallwarmup/main samples, exactbaseline/candidateexe hashes and samefixture/config/toolchain. No automaticretry/exclusions/thresholdchange. Per-case pairedmedian >1.05 triggers source-based explanation or repair; report all3ratios and range, do not infer a statistically strong p95/CV gate from3pairs. No speedup/productionlatency claim from matchedinstrumented app-test builds.

Allocation observations are separate calls/windows using existing work_meter::Counting, same thread, reset_largest immediatelybefore, tallybefore/after. Report allfive fields exactly (allocs,alloc_bytes,reallocs,realloc_copy_bytes,largest_realloc_copy). Only the four cumulative counters may be normalized per operation, without integer truncation; largest_realloc_copy stays the raw undivided window maximum. Do not call them live/peakheap/deallocationbytes/totalrequestedreallocbytes. Three observations percase, fixedsamecount, fixture/format/logsetup outsidewindow. Any increased allocationwork must have source explanation/review; newunbounded/cloning/request-amplifiedwork isnotacceptable. Existing globallyactive counter remains during timing and is labelled; do not add another allocator or alter production allocation behavior. No fixed zero-allocation requirement invented for request/startup code. Frozen runtime/guard tests and fullreviews remain independentlyrequired.

Case6 external valid/invalid output pair is required in deterministic parity before freeze; the timed case remains the specified real describe valid/invalid pair. The fixture report must identify the external descriptor and both expected results; this does not add another timed corpus after calibration.

## Baseline completed and runtime phase authorized

The earlier draft/pending statements above describe historical stages, not current authorization. Source-first review521#5706253618, decision follow-up521#5706285424 and actual harness review521#5706387850 completed before runtime. Root artifact audit521#5706387994 verified complete baseline signatures, seven first-qualifying counts and all21 allocation windows.

D12: test-only harness files moved byte-identically under contract/tests; initial UI-free guard failures+447/+1 and E0583 path failure retained. No guard/rubric/cap/exclusion change. Final guard258PASS/fmt0. Designated relocated baseline executable SHA2560320A9824D897188902FFB79A2AFE9EE1C1FD260432105462BE340C20DFFFF81; all four signature files exactly equal original baseline and emission-disabled byte comparison passed. Final source differs from that binary's compiled source only by removal of one additive blank line, separately retained. Original calibration binary3625C846797496A413A6C92171514F5A6E163F2D3CEF81B7CC264A6192E5F50A and all original receipts preserved. Counts in case order:512000,128000,1024000,512000,512000,4000,8; no recalibration. Decisions521#5706370091 and521#5706415634.

D13: add narrow ActionRegistry::schemas(&CapabilityId,version) borrowing actual registered descriptor identity and compiled input/output for the D10 provider. Existing owned lookup and execution callers stay unchanged. This necessary supporting file scope was independently source-checked before grant; no new graph edge, handler exposure or schema compilation. Dossier521#5706422744 specifies the full implementation scope.

Preparation result472#5706422598 and runtime intent472#5706426398 authorize actual owner extraction plus targeted/full checks and deterministic candidate parity, but no paired timing, commit or PR yet. Candidate comparisons must unset A1C_SIGNATURE_DIR while reading immutable original baseline; otherwise independently prove distinct resolved directories and preserve original hashes. Worker/Ui signatures cover admission route/envelope/union, not opaque invocation execution; concrete retained/new tests remain required. A1-A7 and every G/C closing gate are unchanged and are not marked complete by preparation.

## Archive checkpoint: implementation and observations retained; delivery pending

Everything above is the preserved mission history, including the verbatim delegated request, stable R/A/G/C IDs, reserved AI gates and literal What done means clauses. Earlier draft, pending and phase-restricted authorization statements describe their historical stage; this archive does not reset criteria or repair counters. The original live mission is retained externally as `a1c-archive-input-GOAL.md`, SHA256 `544145584300D44F6CC879E32D9ED121397F3020A015BCF95DB359B73C6DBA46`.

Runtime grant [472#5706426398](https://github.com/milocaetano/quantick/issues/472#issuecomment-5706426398) was followed by the [source-bound implementation receipt](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706659437): real private headless ownership, second-host tests, full ordered local validation and exact-byte parity. Subsequent [comparison authorization](https://github.com/milocaetano/quantick/issues/472#issuecomment-5706664663) enabled the one frozen [comparison001 observation set](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706696942). No automatic retry, sample exclusion or fixture recalibration occurred.

D14: the coordinator explicitly [accepted the observed tradeoff for proceeding to formal review](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706750579), under the original D11 review trigger rather than a flatness ceiling. Static denial's median B/A is 1.0542905900 (+5.429%); all three ratios 1.0542905900/0.8909156832/1.0563268557 remain. Its machine-level cause is unresolved: no noise, no-regression or worst-case latency claim. Additional external-admission allocation is one ID allocation/21 bytes for the fixture (validated ID bound 128 bytes), and startup adds 52 allocation calls/4182 requested bytes. External output also adds an ID-key clone by source inspection but has deterministic-only coverage, not timed/allocation or whole-roundtrip evidence. Faster cases do not compensate for these costs.

The [final independent observation review](https://github.com/milocaetano/quantick/issues/521#issuecomment-5706752907), SHA256 `9D1F5A2BA31F99C95D029BA9B02E7C95BAD1EC8650B3262DBA3836CD20C5E8F3`, supersedes 5706746496, whose publication preceded the reviewer's final external-output scope paragraph. The exact-readback mismatch was reconciled; use the final report. D14 resolves the observation disposition only, not formal architecture/AI/delivery review or campaign completion.

The [evidence-only archive grant](https://github.com/milocaetano/quantick/issues/472#issuecomment-5706757748) permits this archive and [complete evidence record](../docs/quality/capability-admission-owner-evidence.md), with proportional prose checks and exact-input reuse of the prior full suite. Eleven runtime/fixture file identities remain frozen over `a6e554386b037bb2ec49fe29ff6494e4fef0460a`. No additional runtime change, measurement, commit, PR, remote write or review marker is part of this checkpoint.

Evidence for A1-A6, G1-G3 and applicable G6 is recorded for independent judgment, not self-certified formal acceptance. A7/G4/G5, all G-AI gates and C1-C4 remain unchecked: current full formal reviews, draft/publication, exact-head CI, final verifier and authorized campaign-only integration remain due. No aggregate A1/A2 score, root-field/branch improvement, incremental-build gain, main merge or campaign completion is claimed.
