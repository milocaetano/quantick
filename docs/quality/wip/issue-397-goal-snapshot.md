# Unfinished mission snapshot

Preserved for evaluation only. Unchecked criteria and historical failures remain; this is not a completion archive.

Warning: truncated output (original token count: 29954)
Total output lines: 644

# MT5 connection lifecycle decomposition

Refactor one MT5 connection into a private synchronous Sans-IO SessionMachine with concrete hello, session, history and depth decision owners, preserving the exact wire/event contract through an I/O-only async driver.

**Tier:** high — transport cancellation, ordered state transitions and per-tick work make this a state and hot-path extraction. This contributes to E1; it does not discharge aggregate E1 or campaign/main acceptance.

## Identity and why

Issue https://github.com/milocaetano/quantick/issues/397; native parent E1 https://github.com/milocaetano/quantick/issues/482; campaign https://github.com/milocaetano/quantick/issues/472.
Branch feat/mt5-connection-lifecycle; worktree C:/src/quantick-worktrees/feat-mt5-connection-lifecycle.
Current local base/head = c9138e6100921bf70f8126a4cf308c47bbe8b183. This is not a claim that the live campaign tip remains there; current-base synchronization and invalidated validation remain due before delivery. Original bootstrap base was 9ff57501249f51d8f75c21f52094ec3dc3c39af2, tree4a33c57d8f14da72200ba746a69a04a3c5ca9b02; this historical identity is retained, not current validation.
Source feed-mt5 tree04c95b6dd3319017d4ef720bee0c81e5cf9d7805 and connection blob0ef1d08b6dee6e515f0703299df5614c7bc9ecc0 match the original source proposal exactly.
A smaller responsibility owner makes this state machine reviewable without changing what a trader receives.

Private evidence root: C:/src/quantick/.git/worktrees/feat-mt5-connection-lifecycle.
Current evidence root (all `dossier/` destinations below resolve here): C:/src/quantick-worktrees/outside-eight-coordination/mt5-resume-preserved/. Durable publication is root-owned issue/PR reports and evidence links. User archive grant https://github.com/milocaetano/quantick/issues/472#issuecomment-5700791807 supersedes the earlier prohibition on tracked execution evidence and mission archives. The current required archive workflow may be used before review; this is format permission, not adoption of unmerged #507 or a waiver of validation/reviews/final verification. Historical source quotations remain untouched.
Status: synchronous implementation exists as staged/unstaged/untracked work on c913. Historical full crate run repair-test-004.txt passed 118 unit plus 49 integration/bridge/replay tests, with two ignored controls. TCP001 and repair002 are valid performance failures;003 has invalid variability. The two ordinary inline hints produced identical release machine code. Third-attempt before-layout actually executed before representation edits: layout-before-004.txt reports Effect160/result160/After160/Ack24/Machine2104/Event144/Diagnostic160/Trade56 bytes. Compact representation is now implemented; its first library run compact-lib-test-001.txt passed122 tests with3 ignored. One subsequent additive cleanup-diagnostic-order test remains unexecuted. Full current-source crate/control/layout/release-codegen and current-base workspace validation, reviews and CI remain pending. Build host returned to root for native captures before further execution. No retrospective pre-edit, performance or delivery PASS is claimed.
Recovery: retained stash 4c549b77657301c5f8762d45b11d2d7a65a0343c; original complete patch-id b7680f45e9c8d10801b3c34008aa9c3997b23336; external patch SHA256 696B0DE8073523E6049453CE37D4B1ED8F3CDE64C45E7C2C76C74CB4FD9936CD. Scoped stash, fast-forward and --index reapply preserved that patch identity. Only nine newly staged execution files moved into external execution-artifacts/, each verified by SHA256; they were then unstaged. The retained stash is not dropped. Original ignored map is copied to GOAL-before-resume.md. Operation/repair counters remain 0/0 under checkpoint56; no history is reset.

## Request ledger and source mapping

### Current bounded representation amendment (2026-09-16)

The previous complete live map is preserved externally as dossier/GOAL-before-compact-effect.md at SHA256 AE801CD0D00017BFCE444FB02B4AD8BC00416F8CA5CC3A5E110CD69692542B47. Independent source-first design examination C:/Windows/Temp/quantick515-review/mt5-compact-effect-design-review.md, SHA256 61063C55BEE6F4E1B7D85EEF66BF1CF3A0D6695DBA9F16AE929CE58E48FBFA4E, read sources before the proposal and found the bounded third experiment warranted, not implemented or accepted. Root decision https://github.com/milocaetano/quantick/issues/397#issuecomment-5701675591 and pending journal https://github.com/milocaetano/quantick/issues/472#issuecomment-5701680679 reserve cumulative repair attempt3, preserving two stalled outcomes and every failure. D1 extends the stop threshold only through that third outcome; no fourth repair or automatic noisy rerun is authorized.

Map this representation amendment to existing IDs, with no new outcome or criterion:

- R11/A11: measure existing and candidate Effect/result/After/Ack/SessionMachine/Mt5Event/Diagnostic/Trade sizes; account for bounded publication storage and actual emitted common transfers. No added allocation, clone, lock, clock or task on any affected tick publication path. Existing resource/count/TCP limits remain unchanged.
- R14/A14: keep Live(Trade) and Write(FeedMsg) inline; replace only large Publish/Diagnostic return payloads with concrete private issued effects, one bounded machine-owned publication slot and existing four diagnostic slots. Named take_publication/take_diagnostic move payloads once; no universal getter, framework, queue, public API, input-wrapper experiment or driver protocol rule. Remove the two ineffective inline hints in the same batch after the before-layout observation.
- R15/A15: both Published(true) and Published(false) before publication consumption must be rejected without changing payload, continuation, phase or termination reason. The existing successful After::Wait fast path must enforce that rule too. Reject premature/duplicate/wrong getters and acks; preserve diagnostics before Publish, Wait and Finished, including the final taken fact awaiting acknowledgement while count is zero. Taking a payload never advances protocol success. Preserve failed regular and closing sends, original terminal reason, generation copyback, existing four-fact bound and one authoritative pager debt.
- R16/A16: adapt only additive private transcript/benchmark hookups while retaining every existing assertion, event field/order and public fixture. Add consumption/ack and diagnostics-only Wait/Finished assertions; run unchanged semantic/allocation controls. Inspect release code generation at exact recorded inputs before any TCP scheduling. No demonstrated common-transfer improvement means stop without timing. If improved, root may release one quiet unchanged 36-row protocol; median<=1.05, nearest-rank p95<=1.10, both CV<=5%. Layout/codegen/control results alone are not acceptance.

Current host boundary: root released the host for actual before-layout observation and implementation; after the first compact library test, the author returned it for root native captures. No further compilation or measurement starts before another explicit root release. The before-layout observation preceded all representation edits. Historical successful and failed evidence keeps its original source identity. The current archive grant affects G7/C1 only; all A/G/C proof obligations persist.

#### Full coordinator third-experiment decision, attributed source5701675591

> # E1M bounded third representation experiment
>
> Coordinator decision under the authenticated user's D1 standing delegation in [campaign472](https://github.com/milocaetano/quantick/issues/472), not a new product waiver. The user authorizes autonomous defaults and completion while retaining main/settings boundaries.
>
> Two optimization attempts and two stalled outcomes are retained. Original TCP001 and repair002 are valid performance failures;003 is invalid variability and the inline hints produced byte-identical release code. [Attempt2 evidence](https://github.com/milocaetano/quantick/issues/397#issuecomment-5701600026) preserves the timing-before-codegen-interpretation chronology. No earlier result is erased or promoted.
>
> The source-first independent examination at `C:/Windows/Temp/quantick515-review/mt5-compact-effect-design-review.md` supports one concrete representation hypothesis: the common returned effect currently transfers160 bytes, including Wait; a bounded machine-owned publication slot and existing diagnostic slots can remove large inline payloads while retaining Live/Write inline. The added consume-before-ack invariant is a real complexity cost and must earn its place through code generation and unchanged end-to-end performance proof.
>
> Decision: permit exactly one further bounded repair batch, cumulative attempt3, retaining stalled count2 and extending the stop threshold only through this third outcome under D1. No fourth attempt is authorized here. This is not prospective implementation, correctness or performance acceptance. Source/map amendment uses existing R11/A11, R14/A14, R15/A15 and R16/A16; no new product outcome or changed criterion.
>
> Required sequence:
>
> 1. Preserve current source/index and all original public fixtures, parser/mapper and prior measurements. Record measured before-layout sizes before changing representation.
> 2. Apply only the proposed rare payload transfer and its mandatory adapter/assertion changes; remove the ineffective two inline hints in the same batch. No input-wrapper experiment, universal framework, per-event allocation/lock/clock/task, queue, unsafe or new public API.
> 3. Explicitly reject both Published(true) and Published(false) before payload consumption; preserve state after every invalid getter/ack. Prove diagnostic ordering before Publish, Wait and Finished, duplicate/wrong operations, four-fact bound, unchanged continuation timing, failed cleanup and original terminal reason.
> 4. Re-run existing semantic/allocation controls and crate tests; account for total machine and continuation sizes, not only Effect.
> 5. Build release at recorded exact inputs, then inspect emitted common-path code before any TCP timing. No demonstrable code-generation improvement means stop this experiment without a speculative timing run.
> 6. Only after coordinator releases a quiet host, run one fresh paired protocol with all36rows retained. Median<=1.05, nearest-rank p95<=1.10 and both CV<=5% remain required. Invalid variability is not PASS and does not authorize an automatic rerun.
>
> No commit/push/review marker or final delivery is authorized by this experiment. Current-base full verification, independent reviews and exact-head CI remain required for eventual delivery. Layer finishing work has build-host priority; no build or timing begins while its reservation remains active. The operation journal and coordinator dispatch control actual start.

#### Full archive-format grant, attributed source5700791807

> ## User decision: permit the current evidence archive workflow
>
> Source: authenticated Codex conversation on 2026-09-16, thread `01a0aaf3-a596-76a1-98f2-8da37a15567f`.
>
> The user explicitly permits retaining evidence in the existing workflow format and defers changing that workflow until later. This supersedes the earlier prohibition on tracked execution evidence and mission archives for this campaign. The required mission archive may therefore be committed with each child under the current delivery contract.
>
> H-workflow is no longer an authorization prerequisite for campaign delivery: PR #507 does not need to merge first solely to resolve that prohibition. This decision does not close or reject #507, change repository rules, waive reviews or validation, or authorize a main merge.
>
> PRs #514 and #515 still require their actual archives, current review evidence, exact-head green CI and successful final verification before serialized integration into `campaign/outside-eight` under the existing campaign authority. The MT5 performance failure and all other outstanding criteria remain in force. This comment records authorization only; it does not claim either PR has been released or merged.

Root supplied the authenticated user wording underlying that grant: attributed quotation, "okay deixa a sua evidencia do jeit oque vc quer", followed by "depois a ngt resolve isso". This is format permission only; D1/main/settings and current validation/review boundaries are unchanged.

Source IDs: I = original issue397 snapshot below; P = full source proposal/comment482#5672510228 below; U = full historical parent472 charter below including original request and D1/D6/D9; D = original root delegation below; I2 = full resumed live issue397; U11 = full D11 decision and attributed user request; N = full independent Sans-IO design preparation; D2R = full resumed root delegation. I2/U11/N/D2R are appended verbatim. Span names refer to exact headings/numbered clauses in those retained quotations. Historical source text is not silently rewritten as current state.

- R1 — Make serve_connection shorter than 200 lines under the unchanged frozen function-span definition; extract parts named for their actual decisions, no cosmetic compression or forwarding-only layers. Sources I original scope/criterion1; P Recommended bounded child; D decisionD1/D2. → A1.
- R2 — Keep all pre-existing feed-mt5 integration/unit assertions and fixture bytes unchanged; full crate tests pass. Sources I original scope/criterion2, additionalA4; P tests/fixture additions; D decisionD1/D4. → A2.
- R3 — Keep connection.rs below 1,500 production lines, no new module cycle, no new or raised ceiling. Sources I originalcriterion3; U invariants; D decisionD1. → A3.
- R4 — Preserve admission-before-Connected, exact hello refusals, Connected fields/defaults, unsupported paging zero wire bytes and separation of admission failure from connected cleanup. Sources P ordering1/8 and hello fixture bullet; I bounded scope; D decisionD3. → A4.
- R5 — Preserve split socket, one persistent cancel-safe bounded reader and pinned pager future, biased request selection, rearm only on request, timeout reset on either wake, classifications, and cancel-safe partial lines. Sources P transport owner, ordering9 and partial-line fixture; D decisionD3. → A5.
- R6 — Preserve anomaly-before-deal-before-print, high-water timestamps only advance on higher sequence, quote-only evidence, map exactly once, page-before-backfill-before-live, latency before live send and unchanged backpressure/cadence. Sources P ordering2/3/4, combined script fixture; I bounded scope; D decisionD3. → A6.
- R7 — Preserve page price-context parking/restoration including repeated starts, mapper stats/clock intact, opening-before-debt handling, one owed reply, unsolicited collection/discard, startless end semantics, current UTC cursor conversion, 250,000 page bound/saturating overflow/one end log, no new backfill cap. Sources P history owner and ordering5, history fixtures; I bounded scope; D decisionD3. → A7.
- R8 — Preserve rates validation/drop counts, BTreeMap ascending correction, million-bar cap, bridge-partial OR clipping, original-hello rates offset, unfinished rates/backfill discard; preserve existing depth owner/capture/image-loss/resync/base-offset generations and terminal closure. Sources P ordering4/6/7, rates/depth fixtures; I additionalA4; D decisionD3. → A8.
- R9 — Preserve exact malformed/UTF8/EOF/silent/oversized/socket/bye classes and connected terminal sequence: discard backfill; abandon queued/in-flight page with one owed empty reply; discard rates; mapper ledger; pending deal flush; depth close; original end reason. Preserve closed-consumer sends and no asynchronous Drop cleanup. Sources P ordering8 and terminal fixtures; I bounded scope; D decisionD3. → A9.
- R10 — Stay within existing concrete owners and public run_bridge_server/ServerConfig/HistoryPager/protocol/events; no exported session, generic plugin/handler framework, crate/dependency, effect list or production feature. No app, feed-host, hook, bridge implementation/format, manifest, score or ratchet edits. Sources I bounded scope/additionalA6; P exact paths/owner contract; U no expansion/dilution; D decisionD2. → A10.
- R11 — Zero additional per-tick allocations/locks/clock reads/tasks; bounded unchanged state/event counts and real same-input current-base public TCP performance proof with microbench controls. Sources I additionalA5; P hot-path/delivery proof and ordering9; D decisionD5. → A11.
- R12 — Preserve all three integrated feed/retry/idempotency repairs, ticket/replay, one deterministic engine, honest data, one-way/headless dependencies, English, public/financial protections, tests/guards/ratchets and seven frozen measurement blobs. Sources I additionalA6; U original request/K2-K4/freeze; P authority/limitations; D decisionD6. → A12.
- R13 — Add independently expected regression fixtures at public TCP ports and narrow owners before extraction, keep their actual baseline outcomes, reconcile any discovered defect through root rather than preserve a known bug. Sources I additionalA4; P full fixture additions; D decisionD4. → A13.
- R14 — Transfer actual protocol state, rules and tests into private synchronous AwaitHello/Connected/Closing/Closed SessionMachine and narrow pure owners. Async driver owns socket/framing/decode, one reader/pager wait, time/capture/pager operations, tracing and backpressured effect execution only. No Tokio/sender/socket/clock/shared capture/pager handle in machine; no state-rule leakage into driver, giant replacement dispatcher or module cycle. Sources I2 D11 ownership/inputs/effects and A8; U11 clauses2/4; N concrete ownership. → A14.
- R15 — Typed one-effect-at-a-time continuations preserve exact mutation/observation boundaries across success/failure acknowledgements, closure, pager query/settlement and timeout. HistoryPager stays sole concurrent debt owner. Preserve per-wait timeout, pager bias and ready-input-before-timer behavior without new clock reads. Sources I2 A10 and bounded yielded effects; N ordering/deadline/concurrency. → A15.
- R16 — Add pure transcript and machine benchmark proof alongside unchanged public TCP and fixture evidence; measure actual coupling and additive change cost before/after without claiming a score. Sources I2 A9/A11/A12; U11 clauses4/5; N reuse/evidence. → A16.

Operational source clauses I additionalA7, U D1/D6/D9/K5-K6/M1-M3, P scheduling/delivery, D bootstrap/decisionD7 map once to G1-G8/C1-C5 below; aggregate scores remain coordinator/independent V1 obligations, not an invented child outcome.

## Decisions

The root resolved mission step3 under retained user D1. No new qualifying question remains; these are delegation decisions, not new user quotations.

> D1 same original3 issuecriteria mandatory, serve_connection<200 by frozen definition, existing test/fixturebytes unchanged, no newcycles/ceilings. D2 existing concrete hello/session/history owners only, public ports unchanged, no generic framework/dependency/crate, no app/feed-host/hook/bridge-format changes. D3 preserve exact ordered admission, single cancel-safe reader+pinnedpager, anomaly->deal->print and page->backfill->live, historycontext/debt/offset/depth/rates/terminal semantics; source-plan fullconstraints mandatory. D4 literal independently expected public TCP/owner regression fixtures written+executed on baseline BEFORE extraction (refactorpreservesbehavior, fixtures neednot fail); actualnewdefect returns root for bounded reconciliation, nevercarryknownbug or fabricate red. D5 performance zero added pertick allocations/locks/clock/tasks unconditionally; later quiet pairedbase/candidate actualTCP fixedinput eventcounts,3warmups+15alternating rounds, median<=1.05/p95<=1.10;CV>5% invalid retainsallraw and schedules sameprotocol quiet, not tuningthreshold. Existing microbenches only controls, not substituteTCPdispatch. D6 preserve seven frozenrubric/measure/score blobs and all3 integratedfeed/retry/idempotency repairs, no financial/public/guard/test weakening, no score/E1/maincompletionclaim. D7 high fullsource/map preflight, fullorderedcurrentbase4checks/targetedtests/fullreviews/CI/canonicalship; root owns defaults, publication/integration onlycampaign, trader main/settings.

Decision mapping: D1→R1-R3; D2→R10; D3→R4-R9; D4→R2/R13 and G3; D5→R11; D6→R12 and C5; D7→G1-G8/C1-C5. Parent decisions retain their own namespace U.D1/U.D6/U.D9.

D8 — After the own-target bootstrap, baseline and focused candidate checks, root granted exclusive use of C:/src/quantick-worktrees/fix-mutation-retry-truth/target for ordered workspace fmt-check then all-target Clippy, jobs1. Authority: https://github.com/milocaetano/quantick/issues/472#issuecomment-5673888510. C2 released that target; F2 uses its own. No concurrent target writer, cleanup, deletion, benchmark or workspace build/test is authorized by this grant. Stop before step3 until the next capacity release. Preserve the original own-target command identity and original delegation verbatim; this is later operational permission, not a claim that bootstrap used this target. D8 clarifies G5/G8 chronology only; R/A, thresholds and review gates do not change.

D8 is historical and no longer applies to resumed execution. D2R authorizes only this worktree's own target, jobs1; no shared-target use or cleanup. U11 strengthens the same mission after reviewed MVU1 integration; the paused async ConnectedSession does not satisfy Sans-IO. D1-D7 remain in force without a new qualifying user question. The later archive-format grant5700791807 permits the current mission archive workflow; #507 is not a delivery prerequisite solely for that former prohibition. No current review, CI or final verification is skipped.

## Assumptions

Scope amendment 2026-09-16, within existing R10/A10/A14: depth.rs mapping itself currently traces. Root approved the necessary private pure BookMapper seam in https://github.com/milocaetano/quantick/issues/397#issuecomment-5696622882. Allow only a crate-private map_with_diagnostics with bounded concrete facts, preserving ONE mapping algorithm, the existing public map API and its logging wrapper, all original tests and ordered warn-once/clamp/crossed-debug/synchronized metadata. No effect Vec or per-tick allocation. This is the sole exception to the earlier mapper-file exclusion, required for actual I/O-free depth ownership; no new outcome, gate waiver or counter reset. Independent delta completeness check remains required before editing depth.rs; other previously reviewed owners may proceed.

Attributed full root scope decision:
> ## E1M bounded implementation reconciliation: pure depth mapping
>
> Root inspected c913 `crates/feed-mt5/src/depth.rs`: public BookMapper::map and timeline_ms directly trace synchronized, malformed, crossed and backward-time cases. Keeping that call inside SessionMachine would violate the already accepted no-I/O owner requirement.
>
> Under retained user D1 delegated implementation defaults, expand the previously excluded mapper-file scope only for a crate-private pure map-with-diagnostics seam. Preserve the public map signature and its existing logging through a compatibility wrapper; share one mapping algorithm, not two. Return bounded concrete diagnostic facts (including ordering and warn-once counts), no per-image effect vector or extra per-tick allocation, and preserve all public mapper tests/assertions and event contents. Pure owner calls the private seam; driver executes diagnostics. No dependency, protocol, financial, guard or measurement change.
>
> Map the necessary depth.rs exception in existing R10/A10 and A14; no new product outcome or weakened criterion. Independent source/map delta check precedes depth.rs edits. Other already-approved owner work continues. This is not a delivery waiver or score claim.

- S1 — Use connection/{hello,session,history}.rs and a short exhaustive BridgeMsg dispatch, with minimum parent-visible methods. File placement is reversible and agrees with P; no new trait/registry needed.
- S2 — Existing HistoryPager remains the sole debt owner and existing DepthSession/RatesBlock remain domain owners. Session supplies mutable mapper to history; moving a Trade into a concrete internal result avoids clones/effect vectors. Code answers ownership directly.
- S3 — New public loopback fixtures live in tests/connection_lifecycle.rs; narrow baseline owner fixtures may live in new cfg(test) sibling files and move with owners after baseline execution. Existing test files/fixture bytes remain unchanged. Their expectations are literal, never calculated by the extracted dispatcher.
- S4 — Timing artifact compares identical compiled public-TCP fixture input and bounded consumption on one machine/toolchain/profile. Three warmups per side precede 15 alternating base/candidate paired rounds. Record raw times/event counts, median and p95 elapsed ratios, and CV for each side. CV>5% invalidates that run; retain it and schedule identical quiet protocol, with no relaxed thresholds. Exact input/counts are frozen before baseline execution.
- S5 — No UI/action/capability/engine-code change is planned. Thus no visual feature or new-extension seam is needed; source scope changes return to root. A new runtime failure is a real retained outcome, never called an expected red merely because test-first was requested.
- S6 — Rework hello/session/history into synchronous decisions and typed continuations; convert DepthSession similarly and restrict publish.rs to effect execution. A private effect vocabulary file is allowed if it reduces coupling; depth-local outcomes must not introduce blocks→connection→blocks cycles. Retain RatesBlock algorithms and listener/busy-refusal machinery. This is the concrete D11 implementation boundary, not a new public extension.
- S7 — Boundedness means bounded new control/continuation storage and unchanged existing page/rates bounds. Existing legacy backfill Vec has no local cap and stays unchanged. Failed pre-hello admission does not abandon prequeued debt in the base; preserve and characterize that distinction rather than silently broadening cleanup.
- S8 — Pure RatesBlock absorb/finish and mapper/depth summary decisions yield bounded diagnostic/stat snapshots for driver tracing, preserving existing event codes/fields. Existing public mapper APIs remain unchanged; use current stats types or private concrete values. Diagnostics add no per-tick allocation. This implements R14/A14's I/O-free decision owner, including existing indirect tracing calls.

## Acceptance criteria

All criteria remain unchecked until their named evidence is durable.

- [ ] **A1** — serve_connection is <200 frozen function-span lines; named hello/session/history operations own real decisions, with no replacement oversized dispatcher, padding or forwarding-only layer. Evidence: unchanged measure.py function-span result plus source review → dossier/shape.md. *(R1)*
- [ ] **A2** — cargo test -p quantick-feed-mt5 passes; every pre-existing test/fixture blob remains unchanged, including bridge_server, bridge_paging, both recorded replay suites and both NDJSON fixtures. Evidence: baseline/candidate manifests and full crate test logs → dossier/tests.md. *(R2)*
- [ ] **A3** — guards report connection.rs <1,500 production lines and no new cycle/ceiling or ratchet weakening. Evidence: current report and inspected diff → dossier/guards.md. *(R3)*
- [ ] **A4** — Exact hello refusal table (non-Hello, malformed JSON, UTF8, oversized, EOF, timeout, schema/symbol, injected read error), Connected metadata/defaults and zero-byte unsupported paging remain unchanged. Evidence: literal public/owner fixture assertions and runs → dossier/admission.md. *(R4)*
- [ ] **A5** — A fragmented tick survives a pager wake with exact outbound bytes and one delivered tick; pager pin/rearm, reader continuity, selection and timeout policy are unchanged. Evidence: baseline/candidate TCP fixture and source audit → dossier/transport.md. *(R5)*
- [ ] **A6** — Literal mixed sequence/deal/tick and heartbeat scripts prove anomaly→deal→print, high-water gap bounds, quote-only evidence, routing priority, original-clock flush, and latency-before-send/backpressure behavior. Evidence: ordered event fixtures and source audit → dossier/dispatch.md. *(R6)*
- [ ] **A7** — Repeated/opening/unsolicited/startless history, pending/queued/in-flight requests, current-offset cursor, context restoration and page overflow retain exact semantics. Evidence: TCP and bounded owner fixtures; unchanged debt owner audit → dossier/history.md. *(R7)*
- [ ] **A8** — Rates bridge-partial/clipping/repeated/startless/unfinished handling and original-hello offset, plus depth capture on/off/on, missing images/resync/reconnect generation and final closure are preserved. Evidence: public TCP and owner fixtures including bounded cap test → dossier/rates-depth.md. *(R8)*
- [ ] **A9** — Every terminal reason and connected cleanup order remains exact; closed consumer completes the server/session according to current public contract, not a timeout-only success. Pending samples precede depth/lost events; unfinished batches are absent and page debt clears once without next-session replay. Evidence: literal terminal/error fixtures and source audit → dossier/terminal.md. *(R9)*
- [ ] **A10** — Only scoped concrete owners, added regression fixtures/benchmark harness and necessary ownership prose/evidence change; all public signatures/ports and excluded files remain unchanged. Evidence: complete name/status/content diff and owner map → dossier/scope.md. *(R10)*
- [ ] **A11** — Zero added per-tick allocations/locks/clock/tasks by full changed-hot-path audit; identical event counts; valid quiet paired TCP median elapse…14954 tokens truncated…d, not routed live. End without start settles an owed request empty/nonexhausted. Completion scan cursor uses current mapper UTC conversion. Keep 250,000-trade page bound and saturating overflow counter with one end log, not per-tick logging. Do not silently invent a new backfill cap during this refactor.
> 6. Rates keep BTreeMap ascending/last-write correction, validation/drop counts, one-million bar cap and `partial = bridge_partial || clipped`. Incomplete rates/backfill are discarded, not half-published.
> 7. Existing DepthSession remains the owner of capture toggles, missing images, base+offset generations, ordered Connecting/Snapshot/Synchronized, and disconnected generation closure. `generation_offset` remains server-owned across connections, never reset as part of constructing the new session owner.
> 8. Preserve malformed/UTF-8 skipping and exact EOF/silent/oversized/socket/bye reason classes. On connected termination: discard incomplete backfill; abandon queued/in-flight page and answer once if owed; discard incomplete rates; log mapper ledger; flush pending deal sample; close depth; return original terminal reason. Closed-consumer sends retain their existing error handling. Do not move cleanup into Drop (it awaits).
> 9. Keep one pager future pinned across ticks, rearming only after a request. Keep the same reader so a pager wake cannot discard a partial line. No new task per message, additional lock per tick, full-session clones, per-tick Strings/effect Vecs, or semantic sorting/deduplication of the trade stream.
>
> ## Actual test assertions inspected (not executed)
>
> Read the entire `tests/bridge_server.rs` and both committed replay integration suites, plus reader and SeqTracker unit tests. These are current source assertions, not a claimed green run.
>
> - `sequence_loss_precedes_live_data_and_survives_quote_only_ticks`: after live ID501, Gap expected502/got505/missing3 with exact UTC stamps precedes live506, despite the gap tick mapping to no trade.
> - `sequence_duplicates_and_backwards_ids_are_not_claimed_as_missing`: duplicate10/backward7 are NotMonotonic against10; live11 follows; reconnect IDs1/2 fabricate no anomaly. `late_mt5_reorder_cannot_move_the_next_gap_backwards`: late90/time500 does not move the next Gap102's from stamp below high-water100/time1000.
> - Paging round trip asserts exact load_older JSON/count/server-time cursor, mapped page timestamps and Buy/Sell sides, and cleared in-flight flag. Context test establishes live100→101, page90→91, then resumed100 must be Sell; page first print is dropped, not compared against live101.
> - Empty page can honestly report exhausted=true. Unsupported bridge produces empty/nonexhausted answer, still emits live data, and an actual socket read times out rather than receiving bytes or a close. In-flight request followed by socket drop produces empty/nonexhausted answer and clears latch.
> - Opening tests assert trades/remaining, preservation of first-session opening slices, and that an opening slice cannot clear the pending click; a real requested answer permits the next click. One-request test rejects a second click, drops unsolicited history, but retains backward-sequence diagnostics before later live data.
> - Full-session test asserts Connected fields/default tape, Backfilled Buy/Sell and exact UTC time, live ID4, exact `bye: test_done`, then Waiting. Schema/symbol mismatch tests assert reason substrings. Garbage/unknown/multibyte/invalid-UTF8 lines leave later ID2 intact; oversized input gives exact `oversized line` then Waiting; silence gives exact `silent` then Waiting.
> - Depth test requires Connecting generation above base1,000,000, matching Snapshot generation/UTC/price-step/limited5 coverage, then Synchronized and only the changed bid delta. Capture-off asserts no depth while one trade flows; unsupported depth heartbeat asserts Disconnected/bridge_without_depth at consumer base2,000,000.
> - Rates tests assert advertised capability; sorted two bars after duplicate correction and empty-bucket removal, exact UTC/open-close interval, corrected close177799, volume10 and zero unmeasured delta. Corrupt OHLC/invalid batch does not prevent valid two bars; unfinished rates must never emit Rates before `bye: test_done`. Current positive test asserts partial=false only.
> - Two-port test proves independent symbols/quantities/sides and no stray queued events. Busy-refusal tests require socket closure with zero reply bytes and continued live data, accounting for independent refusal/live task order.
> - WIN recorded fixture freezes 1,500 input ticks, 2 unclassifiable drops, 1,498 trades, IDs3..1500 and UTC endpoint; complete TCP trades equal the pure mapper. Quote fixture freezes 1,200 inputs, 1 drop, 1,199 synthetic one-unit trades, exact first price7446.18/UTC time and IDs2..1200; complete TCP trades equal the pure mapper. Both reject unexpected depth/rates/paging/deals/sequence events. Differential comparison alone shares the mapper, so retain the independent frozen numeric assertions too.
> - Reader unit assertions cover CRLF/EOF, too-long classification and surviving following line, and invalid-UTF8 continuation. SeqTracker unit assertions cover arbitrary first ID, exact missing count, duplicates/backwards without shrinking expectations, and u64::MAX without wrap.
>
> ## Fixture additions required before extracting the touched behavior
>
> The existing public-port suite is already a real second consumer (`feed/src/metatrader.rs:368,825–838` is production); no new public abstraction is necessary just to write a fake.
>
> Add independently expected event-sequence fixtures through `run_bridge_server`, explicit config, loopback fake bridge and public mpsc receiver. Never compute expected ordering by calling the proposed private dispatcher itself.
>
> - Hello table: non-Hello/invalid JSON/invalid UTF8/oversized/early EOF/timeout, exact reason and no Connected. Inject read errors into the generic helper if reliably forcing TCP read failure is platform-dependent. Distinguish existing substring-only mismatch coverage from exact reasons.
> - Fragment a tick line, wake a pending pager request before its remainder, then complete it; assert no lost/duplicated tick and exact request bytes. Exercise request/write failure via a deterministic internal failing writer or narrowly scoped writer helper using existing AsyncWrite, not a new transport framework. Preserve one empty reply/no replay into next connection for queued and in-flight teardown states.
> - Repeated history_start, history_end without start, scanned_to conversion after heartbeat offset change, page cap/overflow, and opening-with-pending-click at disconnect. For overlapping malformed markers, freeze actual policy rather than silently redefining it. These are not currently direct assertions in bridge_server.rs.
> - A combined tick script with both a sequence anomaly and deal-counter transition must assert anomaly → deal sample → mapped print; quote-only stamps must still emit evidence. Heartbeat offset change and EOF must flush a pending sample on the original tick clock, before terminal depth/lost states as applicable. Existing recorded fixtures carry no deal stamps, so cannot prove this.
> - Capture on/off/on, lost book image/resync, reconnect generation monotonicity and terminal depth closure; distinguish current first-generation positive coverage from these lifecycle assertions. Retain per-owner BookMapper tests rather than claiming them as connection-wiring coverage.
> - Rates bridge partial=true, cap clipping, repeated/startless markers and terminal unfinished block. Exercise cap at owner level with bounded fixtures rather than sending a million rows over TCP purely for coverage.
> - Closed consumer during dispatch must terminate correctly; terminal cleanup still executes according to the connected path. Do not accept a test that merely times out and calls that successful shutdown.
>
> If a pre-extraction fixture exposes an actual defect rather than a proposed regression risk, retain the original failure and escalate it for bounded repair/scope approval. This proposal does not declare those untested cases broken or authorize moving known defects unchanged.
>
> ## Hot-path and delivery proof
>
> Rate classes: hello/teardown/page request are rare; tick mapping/routing is per-tick; book mapping is per-depth; block appends are bounded historical work. Preserve existing allocations from JSON decoding/line ownership and buffered history; require **zero additional** per-tick allocations, locks, clock reads or task spawns introduced by decomposition. Existing page debt mutex is on request/boundary operations, not on every live tick.
>
> `benches/tape_burst.rs` measures real decode/map/latency accumulation but explicitly excludes channel cost and does not execute connection dispatch; `session.rs::benchmark_contiguous_mt5_delivery` measures decode/tracker/map, likewise not the full socket path. Re-running those alone cannot establish the extracted dispatch's cost. A child needs same-machine/current-base comparison over an identical dense public TCP script with event counts and bounded channel consumption, plus the existing microbench controls; retain raw rounds and account for clock/channel/scheduling noise. Add no synthetic production work to improve denominators. No benchmark was run for this plan.
>
> After authorized bootstrap/current-source mapping and independent preflight: capture behavior fixtures before movement, implement the named owners, run targeted feed-mt5 plus feed-host continuity and engine/ticket/replay regressions, then canonical full ordered fmt/clippy/build/test, affected bridge Python suites (the existing Rust test discovers them with a stub terminal), guards, independent reviews, and exact-final-head CI. Measurement/final assessment stays with an independent assessor and unchanged tools. No threshold/metric or overall E1 completion claim follows from this proposal.
>
> ## Limitations and handoff
>
> A1R remains priority; its source and target-release state are preserved. There is no E1 implementation/preflight/CI/visual PASS. Root must decide whether to replace the aggregate A4 dependency with a narrower child dependency, then create the bounded issue and mission at the actual reviewed campaign base. Public permission schemas, trading authority and financial mutation paths are outside this slice; no new permissions, synthetic trades, inferred cancels or recovery authority may appear through refactoring.

### U — parent472 full charter, original user request and decisions
> <!-- quantick-campaign:v1 -->
> <!-- campaign-bootstrap:milocaetano/quantick/outside-eight -->
> ## Context
>
> Schema: 1. Campaign ID: milocaetano/quantick#472. Objective key: milocaetano/quantick/outside-eight.
>
> Take Quantick to at least 8.0 overall and in every dimension on outside-score rubric v1.0, measured on main, from committed baseline docs/quality/outside-score/d3d4b23d.md (6.475 unrounded, displayed 6.5; architecture 6.3, engineering 7.6, change cost 4.0). Initial main: eb7bb039434667bb150be9cdf5237e172c4198fe. Current-SHA independent baseline: outside 6.115 (display 6.1), dimensions 6.3/7.0/3.2; quantick-score 79/100 (7.9), gates 4 and 5 BLOCKED. No inherited campaign score is acceptance evidence.
>
> Expected outcome: corrected feed continuity and mutation uncertainty, smaller responsibility owners, explicit pipelines, isolated harnesses, measurable change costs, reviewed green consolidated PR, then independently verified main.
>
> ## Scope
>
> Implementation, deterministic failure fixtures, architecture extraction with tests moving to their owners, production harness isolation, CI/read-cost/edit-loop evidence, independent assessments and durable coordination. No product expansion for points, artificial code dilution, score gaming, weakened tests/guards/reviews/contracts/financial rules, deployment, spending, main merge or GitHub settings changes.
>
> Preserve every CLAUDE.md invariant: determinism; one engine for chart, backtest and bot; data honesty; one-way dependencies; headless crates below app with existing documented exceptions; size, cycle, context and UI-free ratchets; English; order-ticket and replay behavior.
>
> ## Acceptance criteria
>
> Candidate criteria:
> - [ ] K1: a fresh-context assessor who wrote none of this campaign's code follows outside-score Independence and reports unrounded overall >=8.0 and each dimension >=8.0 at the exact campaign head. Assessor receives code, frozen rubric, measured rows and CI, never campaign scorecards, PR claims or prior grading before scoring. Candidate is explicitly not main completion.
> - [ ] K2: blind quantick-score assessment at that head passes A+ gates 4 and 5 with positive executable evidence: Binance automatic reconnect and MT5 in-connection sequence gaps emit truthful FeedGap/health; mutating unkeyed timeout/lost transport never advertises unsafe retry after possible execution; quantick_invoke carries and enforces an idempotency key.
> - [ ] K3: quantick-score v1.0 does not fall from the independent current-main baseline of 79/100 (full report linked below); retain all per-criterion deltas and gate evidence. Prior campaign self-grades are not the baseline.
> - [ ] K4: frozen files match the blob IDs below throughout the campaign; no weakening of tests, guards, reviews, public contracts or financial rules; no ticket/replay regression.
> - [ ] K5: child PRs integrated only into campaign/outside-eight after exact-diff reviews and exact-head green CI; final campaign-to-main PR has fresh architecture, AI and delivery reviews and full integration CI, with no unresolved required findings.
> - [ ] K6: independent report >=9.0 remains provisional unless reproduced within 0.5 at identical SHA/version by another model family; same-family agreement does not qualify.
> Main completion criteria:
> - [ ] M1: trader alone merges consolidated PR; read back actor, head/base, merge SHA and ancestry; main CI green at the assessed SHA.
> - [ ] M2: fresh-context independent main assessment proves K1-K4 and applicable K6 at that main SHA; campaign claims and PR self-assessments are never success evidence.
> - [ ] M3: required child evidence and pending Project writes reconciled, final report linked, then close parent/Project. No closure at candidate readiness.
>
> ## Authority and decisions
>
> D1, authenticated user request in Codex session on 2026-09-14 (source retained verbatim):
> > Main merges and GitHub settings belong to the trader: prepare settings changes as one-line human tasks on the parent issue and never block other work on them. Decide every other default yourself, and never wait on the trader for anything but the merge to main.
>
> The same request explicitly orders this campaign, the three listed defect fixes, frozen measurements, independent assessment and invariant preservation. This authorizes issue/Project management, isolated implementation, tests, commits, pushes, draft/ready PRs, independent assessors and reviewed child integration; main and settings remain excluded. Campaign workflow supplies child/review delegation; implementation concurrency initially defaulted to one; D6 below authorizes parallel work. No request for additional defaults is needed.
>
> D2, coordinator default under D1: exact integration destination campaign/outside-eight. Serialize reviewed head-pinned child merges into this destination; never main. Child mission-base records point to this charter's retained user authority. Existing dirty main checkout and all other worktrees preserved.
> D3: closed predecessor #367 and open predecessor #330 do not discharge this request: different rubric and acceptance evidence. #378's historical gate-5 PASS does not prove these failure paths. #362 is a related durable-identity extension, not proof that unkeyed uncertainty is safe; no unauthenticated client identity shortcuts. #226 records replay-integrity follow-up; inspect before expanding format.
> D4: strongest model for boundary/financial/hot-path changes and independent judgment; ordinary implementation per campaign routing. Assessors never implement campaign code.
> D5: measurement freeze covers both rubrics and both canonical/Codex score skills, broader than minimum request.
>
> D6, authenticated user steering (2026-09-14), verbatim attributed quotation:
> > esta permitido criar muitos pr em paralelo para nao esperar outros ficarem pronto
>
> English: multiple PRs may be created in parallel without waiting for other PRs to finish. Default under this grant: up to three independent implementation missions, isolated worktrees/file ownership, plus read-only assessment/review when capacity permits. Coordinator serializes campaign merges; all original gates and main/settings restrictions hold.
>
>
>
> D9: reuse existing unassigned #397 as E1M under E1#482, preserving all original acceptance criteria and fixtures. Source inspection proves the MT5 connection slice has no runtime harness-hook dependency (hook belongs to excluded feed/src/metatrader.rs); it may run after reviewed MS1/F1 with capacity under D1/D6. Aggregate E1 <=15/100k and A4 ordering for app-dependent remainder are unchanged. No implementation/score/waiver is implied. Source: https://github.com/milocaetano/quantick/issues/482#issuecomment-5672510228.
>
> ## Frozen measurement manifest
>
> Git blob IDs at initial main (verify before every integration and final assessment):
> - docs/quality/outside-score-rubric.md: 0c2583cc2a71a0e15f38ae4d6ea7cf39c2fdec76
> - tools/outside_score/measure.py: 494ff11e6231f513d500fe935fe27fe3c2d2e2e5
> - .claude/skills/outside-score/SKILL.md: 81d101d7e0fad73f1390977be7671545da46ec51
> - .agents/skills/outside-score/SKILL.md: 45cc224fa29f2881601e03fe462e242bd50e8521
> - .claude/skills/quantick-score/SKILL.md: a0d008fbab2565fcfa1fc1b6894b075bf47f3526
> - .agents/skills/quantick-score/SKILL.md: 5489246f5748b8b8abc80696d88cb75e331d6179
> - docs/quality/quantick-score-rubric.md: c7fd2af3aa108abdff7b3083b4321fd941d8a813
>
> ## Baseline, targets and risks
>
> Committed baseline: https://github.com/milocaetano/quantick/blob/eb7bb039434667bb150be9cdf5237e172c4198fe/docs/quality/outside-score/d3d4b23d.md
> Independently reproduced current rows: ui_free_share_percent 39.3; impl_spread.QuantickApp 20; fns.over_200.per_100k 29.9; harness_hooks 134.
> Planning thresholds, not scores: UI-free share <=25%; maximum inherent-impl spread <=8 for crates >20k production lines; long functions <=15/100k; explicit tested stage ordering; feature-isolated automation; CI records per-PR read cost; fresh committed incremental timings for three largest crates. Architecture may require additional verified seams; decompose actual gaps rather than stop at exhausted backlog.
> Risks: moving financial/state owners, changing error truthfulness, hidden harness dependencies and historical CI flakes. Mitigate with lowest-layer fixtures, realistic replay/ticket regressions, no authority widening, honest uncertainty, independent review, final full suites. Historical main CI failures cannot be erased or manufactured away.
>
> Independent baseline reports (fresh assessor, no campaign code authorship):
> - Outside: https://github.com/milocaetano/quantick/issues/473#issuecomment-5668744488
> - Quantick: https://github.com/milocaetano/quantick/issues/473#issuecomment-5668745129
>
> The historical committed 6.5 report remains the requested comparison. The new 6.1 is a separate current-SHA assessment, not a rewritten historical baseline. Quantick's former campaign-authored 9.6 does not replace blind current evidence. E4 performance evidence and concrete change-cost gaps were assessed more strictly; preserve both ledgers. No score is awarded by this coordinator.
>
> ## Integration and durable index
>
> Mode: integration branch. Branch: campaign/outside-eight. Initial main SHA: eb7bb039434667bb150be9cdf5237e172c4198fe. Current campaign SHA: 4b13b64a0f4556470cbf6b7a5e1c7f1b000d0f50 (reviewed MS1 #491 / PR #494 integrated after F1/R1/C1). Merge authority: D1-D2. Final PR URL/head/base: null/null/main. Status: running. Observed current main: aaae6f32943ea559df8b5e53da1c4ecb0183be01 (external PRs #493 and #489 merged by milocaetano at 2026-09-15T00:46:12Z and 00:47:27Z). MS2 #496 queues separately reviewed synchronization without interrupting current child validation. All seven frozen blobs match initial main at both observed tips; baseline and score criteria are not reset.
> Project: https://github.com/users/milocaetano/projects/5; ID PVT_kwHOA0fkv84BjfBr; Campaign state field PVTSSF_lAHOA0fkv84BjfBrzhiTKjI. Mapping: Backlog=d6c74926, Ready=b8769a27, In progress=517b6514, Blocked=d49a5a27, Awaiting human=e6a540fe, Done=4021410b. Parent and all children added; roadmap placement remains separate.
> Children:
> - B0: #473 — docs(app): establish blind current-main baseline and immutable measurement manifest
> - B1: #492 — test(app): independently reassess the interim campaign tree (read-only; final V1 remains pending)
> - F1: #474 — fix(feed): disclose automatic reconnect and in-connection sequence gaps
> - F2: #495 — fix(feed): disclose startup handoff and reconnect uncertainty; bootstrap and independent source-first preflight passed, test-first headless implementation authorized; app pre-edit check remains due.
> - R1: #475 — fix(control): make mutation uncertainty non-retryable and expose MCP idempotency keys
> - C1: #476 — feat(app): record reproducible per-PR read cost in CI
> - C2: #477 — ci(app): measure incremental edit loops and run full checks on Windows
> - A1: #478 — refactor(app): move cohesive UI-free owners below the desktop shell
>   - A1R: #490 — refactor(control-host): own retained evidence resources and paging
> - A2: #479 — refactor(app): replace dispersed app methods with explicit responsibility owners
> - A3: #480 — refactor(app): declare and test frame and event stage dependencies
> - A4: #481 — refactor(app): isolate automation hooks from normal production paths
> - E1: #482 — refactor(app): reduce oversized functions through tested domain operations
>   - E1M: #397 — reuse existing MT5 connection lifecycle decomposition; reviewed MS1/F1 and capacity required
> - V1: #483 — test(app): independently validate campaign candidate and main acceptance
> - MS1: #491 — reviewed synchronization completed via PR #494 into campaign only; all 18 delivery criteria passed with CI green. Full proof: https://github.com/milocaetano/quantick/issues/491#issuecomment-5672935909.
> - MS2: #496 — synchronize main chart-context-menu and stacked-pane layout work; queued Ready, no implementation started.
>
> Latest checkpoint: https://github.com/milocaetano/quantick/issues/472#issuecomment-5673195337. Read subsequent marked journals before resume.
> Retry policy: 3 operation attempts (transient backoff 5s/20s), 3 repair attempts per signature subject to stricter delivery limits, 15-minute lease, CI diagnosis at 30 minutes stalled, review diagnosis at 24 hours.
> Decision log: D1-D6 in this charter; D7 process chronology exception in checkpoint 3; later decisions linked in checkpoints.
>
> ## Human tasks
>
> H-main [external_authorization; not actionable until final readiness]: merge the final reviewed green campaign/outside-eight PR into main; agent will verify the actor, merge SHA and final main evidence.
> H-settings [external_authorization; no implementation dependency]: in Settings → Rules → Protect main (ruleset 21073042), require successful CI jobs ci and windows before merge and require resolution of review conversations; agent verifies the final check names against the consolidated PR; retain trader-only merge, no bypass. Live ruleset readback currently has no required_status_checks rule or required thread resolution.
>
> ## Verbatim original request
>
> > $campaign  create Take Quantick to at least 8.0 on the outside-score rubric v1.0
> > (docs/quality/outside-score-rubric.md), measured on main, with every dimension at
> > least 8.0, from the committed baseline docs/quality/outside-score/d3d4b23d.md (6.5).
> > Main has not moved materially since: at eb7bb039, ui_free_share_percent is 39.3,
> > impl_spread.QuantickApp is 20, fns.over_200.per_100k is 29.9 and harness_hooks is
> > 134. Only a fresh-context assessor may score, one that wrote none of the campaign's
> > code and follows the rubric's Independence section. The campaign's own scorecards
> > and PR claims are never success evidence, and a 9.0 stands only after the rubric's
> > other-model-family reproduction at the same SHA. Freeze the measurement: do not
> > change the rubric, measure.py or either score skill while the campaign runs. The
> > work must also fix, not merely keep, the confirmed defects that quantick-score's
> > A+ gates 4 and 5 block on:
> > - automatic Binance reconnects and MT5 in-connection sequence gaps drop trades
> >   without a FeedGap or health counter (feed-binance/src/stream.rs:77);
> > - unkeyed timeouts and lost transports on mutating capabilities answer
> >   retryable:true after the action may have run;
> > - quantick_invoke cannot carry an idempotency key.
> > Those gates must pass by blind assessment at the campaign head, and quantick-score
> > must not fall. Keep every CLAUDE.md invariant: determinism, data honesty, one engine
> > for chart, backtest and bot, one-way dependencies, headless crates below app, the
> > ratchets, English in the repo, and no regression in the order ticket or replay. Do
> > not weaken tests, guards, reviews, public contracts or financial rules. Main merges
> > and GitHub settings belong to the trader: prepare settings changes as one-line human
> > tasks on the parent issue and never block other work on them. Decide every other
> > default yourself, and never wait on the trader for anything but the merge to main.
>

### D — root delegation, verbatim
> Start ONE bounded HIGH E1M#397 bootstrap/mission for Quantick outside-eight campaign; NO PRODUCT IMPLEMENTATION until root independent source-first actual-map preflight. Root D1/D6/D9 authority/three parallel fronts recorded parent472; CP36 https://github.com/milocaetano/quantick/issues/472#issuecomment-5673568825, pendingoperation5673556318, executor3975673560005, sameownerlease02:25. Existing issue397 is OPEN/reused/nativeparent482, no planned branch/worktree/PR found at02:01. Campaign latest reviewedbase9ff57501249f51d8f75c21f52094ec3dc3c39af2 tree4a33c57d8f14da72200ba746a69a04a3c5ca9b02, A1Rproof4905673535262; MS1/F1 prerequisites integratedgreen. Use skills new-task and mission faithfully: read current AGENTS/CLAUDE, .agents wrappers/canonical new-task/mission/campaign integration/delivery and mapping yourself. Actual source issue397 and source proposal4825672510228 (local C:/src/quantick-worktrees/outside-eight-coordination/e1-mt5-source-plan.md) plus parent472 originalrequest/decisions must be fully retained, not summaries replacing source. Branch feat/mt5-connection-lifecycle, WT C:/src/quantick-worktrees/feat-mt5-connection-lifecycle. Recheck duplicates/liveowner first, fetch/create from exactverifiedcampaign, own guards build and cargo check -j1 -p quantick-feed-mt5 --all-targets BEFORE any GOAL/product edit. Own target only, no R1 shared target, no app build/benchmark now. Preserve actual command/time/source/toolchain/exits in finite private gitdir evidence, never Start-Transcript or ambient transcript. Private mission-tier '<branch> high', mission-base '<branch> origin/campaign/outside-eight https://github.com/milocaetano/quantick/issues/472 https://github.com/milocaetano/quantick/issues/472'. Root handles GitHub/boards/publication/merge; do not write GH, ask trader, spawn agents, change goal facility or score.
> Root mission step3/default decisions under user D1: D1 same original3 issuecriteria mandatory, serve_connection<200 by frozen definition, existing test/fixturebytes unchanged, no newcycles/ceilings. D2 existing concrete hello/session/history owners only, public ports unchanged, no generic framework/dependency/crate, no app/feed-host/hook/bridge-format changes. D3 preserve exact ordered admission, single cancel-safe reader+pinnedpager, anomaly->deal->print and page->backfill->live, historycontext/debt/offset/depth/rates/terminal semantics; source-plan fullconstraints mandatory. D4 literal independently expected public TCP/owner regression fixtures written+executed on baseline BEFORE extraction (refactorpreservesbehavior, fixtures neednot fail); actualnewdefect returns root for bounded reconciliation, nevercarryknownbug or fabricate red. D5 performance zero added pertick allocations/locks/clock/tasks unconditionally; later quiet pairedbase/candidate actualTCP fixedinput eventcounts,3warmups+15alternating rounds, median<=1.05/p95<=1.10;CV>5% invalid retainsallraw and schedules sameprotocol quiet, not tuningthreshold. Existing microbenches only controls, not substituteTCPdispatch. D6 preserve seven frozenrubric/measure/score blobs and all3 integratedfeed/retry/idempotency repairs, no financial/public/guard/test weakening, no score/E1/maincompletionclaim. D7 high fullsource/map preflight, fullorderedcurrentbase4checks/targetedtests/fullreviews/CI/canonicalship; root owns defaults, publication/integration onlycampaign, trader main/settings. No new qualifying question remains; reversible implementationdetails are S assumptions, no scope narrowing. Retain these delegation decisions verbatim in GOAL and map once viaR/A/G/C, reservedliteralG-AI lines. Preserve operational command receipts incrementally for G gates, not huge reconstruction later. Return actualprecheckheads/hashes/results then fullGOAL path/hash/source artifacts for root preflight; stop before product edits, but do not stop observation of any liveprecheck process. Send short actualhandle/status early.
