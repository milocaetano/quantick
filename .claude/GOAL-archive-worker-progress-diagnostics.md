# Attributable worker progress diagnostics

**Status:** Active campaign child mission after verified Q10 synchronization/adoption, actual Q8 claim and both required pre-edit arming commands. Product implementation and acceptance evidence remain pending. Root owns coordinator/GitHub writes, commits and merges; the delegated implementation agent owns this worktree runtime/test diff.

**Tier:** high — real concurrent worker instrumentation affects command admission, processing, publication and diagnosis; it needs the full independent source-first delivery, architecture/bug and AI review path.

**Objective:** Make queued work, processing latency, progress, terminal state and existing-lifecycle recovery attributable for both indicator and orderflow workers through bounded internal observations and the existing application diagnostic path, while preserving domain outputs, public contracts, ordering and queue/retention behavior.

**Why:** The chart already reports feed and frame health, but those observations cannot distinguish a paused worker from healthy rendering or feed backlog. Issue #340 asks for worker attribution, not new queue policy, automatic recovery or a public capability.

**Identity:** Campaign [#330](https://github.com/milocaetano/quantick/issues/330), child [#340](https://github.com/milocaetano/quantick/issues/340), stable task Q8. Verified base `98c1955ba1d0e5dc78d16f0bd25ac13cce1c21b4`, branch `fix/worker-progress-diagnostics`, worktree `C:\src\quantick-worktrees\fix-worker-progress-diagnostics`, PR target exactly `campaign/architecture-a`. Q3 is integrated, Q10 is closed with current reviewed helper activation, and main `9b8495538a9a1af16fce61dcd4805d112fc0c71e` is synchronized. Duplicate checks and both arming commands passed before mission persistence. [Claim boundary](https://github.com/milocaetano/quantick/issues/330#issuecomment-5596112642).

Q3 dependency is the already integrated `2d8e235b53617cce0f93746f215545a8c75ec556`, with [integration proof](https://github.com/milocaetano/quantick/issues/134#issuecomment-5585648062) and green CI 34228898322, subsequently exercised in target-equivalent CI 34308739933. Q10's [integration proof](https://github.com/milocaetano/quantick/issues/350#issuecomment-5595699619) establishes the synchronized source, not protocol activation or final main delivery.

Root's later status report supplies the separate [Q10 v2 first activation checkpoint](https://github.com/milocaetano/quantick/issues/330#issuecomment-5595804897) and [current campaign score report](https://github.com/milocaetano/quantick/issues/330#issuecomment-5595791214). Root reports C4 review-progress migration completed for all 11 findings, attempt 1, batches 3; issue/Projects closure remains underway. These attributed operational updates establish the known preparation context. Q8 claim, worktree creation and implementation have not been established by this draft.

## Request ledger

The issue's numbered source spans refer to the unchanged `Q8-original-issue340-99f.md`. Equivalent repeated constraints share IDs; operational requirements are G/C. The full attributed issue and root prospective decision document appear verbatim at the end. `Q8-source-first-asks.md` was frozen before this draft; it retains the complete source-facet table and identity hashes.

- **R1** — Attributable typed progress and honest counter conservation for each worker. *Source:* Issue lines 4, 8, 12, 17. → **A1**.
- **R2** — Precisely labeled latency and unavailable states. *Source:* Issue lines 8, 12. → **A2**.
- **R3** — Actual application diagnostics with stable event and owner attribution. *Source:* Issue lines 8, 13. → **A3**.
- **R4** — Deterministic real-worker observation and drain proof. *Source:* Issue line 14; prospective decisions: Production-path determinism fixtures. → **A4**.
- **R5** — Honest terminal behavior and existing-lifecycle recovery. *Source:* Issue lines 8, 15. → **A5**.
- **R6** — Existing domain, wire and queue semantics preserved, with limitations explicit. *Source:* Issue lines 8, 13, 15, 16, 24. → **A6**.
- **R7** — Bounded observer cost and predeclared measured overhead. *Source:* Issue line 17; prospective decisions: Prospective overhead acceptance. → **A7**.
- **R8** — Reproducible checked-in evidence of normal, degraded and recovered behavior. *Source:* Issue line 18. → **A8**.

## Decisions

- **D1 — Existing authority only.** Q8 is an autonomous high-risk campaign child under existing parent authority and D4 (at most three independent disjoint implementations). Main merging is exclusively human. Read the actual current parent/grant at claim and merge; the issue and this draft cannot enlarge it. No new user decision, permission question or historical exception is created here.
- **D2 — One production execution path.** Select the root's prospective seam refinement: a narrow injected clock/observation port shared by normal worker execution and deterministic fixture fakes. Do not insert a cfg(test) alternate lifecycle/admission/processing/publication path. A fake may synchronize a known phase, but neither the normal nor test path waits on a barrier while holding telemetry, channel or publication locks. A test-only wrapper may instantiate the seam, not replace behavior. Architecture review verifies the actual path, and performance measurements include normal production instrumentation.
- **D3 — Pre-measurement ceilings.** Select the root's prospective replacement for the unadopted percentage-only producer proposal. For actual dense completion/processing runs, candidate/baseline median of run means must be <=1.10 and p95/p99 <=1.15 separately for time and dollar indicator variants and dense depth/orderflow work. Producer admission added median of run means must be <=250 ns/command; added p95/p99 <=500 ns/command. Report raw baseline/candidate values and relative changes, including adverse percentages. Convert added cost using the actual measured admission rate; do not invent a supported-live rate. These are prospective criteria, not a waiver of an executed failure.
- **D4 — Paired experiment.** At least B1,A1,A2,B2,B3,A3 on the same exclusive host, identical instrumentation/warmup and exact source/fixture/build/environment identities. Baseline is the actual latest synchronized production base used for this mission. Dense depth means the real BookWorker snapshot, contiguous updates, Trade/Project and mailbox-publication path; time/dollar means actual indicator/lane work. Command counts and independent final outputs must agree. No queue scans or session-sized monitoring storage. No threshold or existing test changes after results; keep all original adverse, failed or inconclusive samples. A prospectively corrected experiment retains its earlier pilot and rationale.
- **D5 — Honest internal semantics.** Diagnostic backlog is accepted work not yet admitted to batch accounting, not a claim to inspect the instantaneous mpsc queue. Batch membership is not simultaneous CPU work. Completion at a publication boundary is not proof every event reached or was adopted by the UI. Explicitly distinguish batch/publication cycles, output send attempts/success/failures where applicable, and Book mailbox replacement. Overflow or unavailable monitoring never silently wraps, saturates into false exactness or alters domain work.
- **D6 — Narrow local scope.** Proposed production ownership: app-local worker progress owner and one main.rs registration; IndicatorWorker and BookWorker adapters; narrow OrderflowView accessor; existing app health cadence plus focused emitter. Owner tests and committed evidence/mission archive are included. No generic job framework, unrelated redesign or hook/guard/workflow edits are assigned. If a genuine ratchet failure requires another path, state the necessary detour and resolve scope under existing policy before editing; never weaken a baseline.

## Assumptions

- **S1 — No new qualifying user question.** The source expressly permits equivalent attributable latency and command-processing evidence; root chose numeric ceilings prospectively. Naming, source layout and an internal observer seam are reversible choices. No remaining source contradiction or proposed narrowing requires a new user decision. An actual implementation discovery affecting money, safety, authority or required scope must be escalated immediately.
- **S2 — Bounded telemetry design.** A fixed-size per-instance ledger, short bounded critical sections and a single sampled accepted-command ticket are suitable starting choices, subject to implementation review and measured overhead. No lock is held across domain work, channel receive, mailbox access, tracing or fake synchronization. Sender admission may hold the ledger only across existing unbounded send and successful-send bookkeeping to prevent accounting races; the actual implementation must prove this does not introduce deadlock or change order.
- **S3 — Equivalent latency with limits.** A sampled ticket can establish exact head age only when it is provably next to admit; otherwise oldest age is Unknown. Sample residence, sample age, processing elapsed and time since last completed progress are separately named, with known/unknown/not-applicable/overflow semantics and explicit bias. No venue timestamp is used as queue residence and no empty-worker condition is labeled stalled merely because no completion has occurred.
- **S4 — Internal attribution only.** Per-instance identity is process-local telemetry, not domain or wire state. Use actual tab/pane/side/kind/instance ownership at the existing two-second diagnostic cadence, including owned off-screen panes so background stalls are not hidden. Do not clone full indicator/projection state just to observe counters. Preserve every existing public log field and DTO meaning; new internal logs alone prove no MCP reachability.
- **S5 — Recovery is observed, not automated.** Release resumes the same paused live worker. A genuinely terminated worker stays terminal; an existing constructor with deliberate normal fixture replay creates a different instance and fresh counters. ResetForSymbol/PrepareRestart/SetEnabled are not thread resurrection. No automatic restart, retry, replay or drop is introduced into product execution.
- **S6 — Original counters are history.** Issue text's initial zero values describe creation. Actual claim/review progress must retain every subsequent observed operation/finding counter and unknown state; no stale snapshot supplies permission to reset. The root's adopted coordinator protocol governs operational publication after its explicit activation.

## Acceptance criteria

The criteria are unchecked because no implementation or evidence is delivered by this proposed draft. D2–D6 and S1–S6 refine how A1–A8 will be proven; they may not narrow the quoted original source. A7 uses the exact D3/D4 ceilings, not the superseded percentage-only send proposal.

- [x] **A1** — IndicatorWorker and BookWorker production observations distinguish admission backlog, batch/processing phase, progress and terminal state. Independently specified tests reconcile accepted, queued, inflight, retired, unfinished, failed sends and explicitly named coalescing/publication subsets without false exactness after overflow/unavailability. Owner tests plus `Q8-validation/` counter schedules and raw outputs.
  *Evidence:* independently specified owner/production-path assertions and raw source-bound outputs as specified above; source schedule, manifests, transcript, or comparison records under external `Q8-validation/`, linked from `docs/quality/worker-progress-evidence.md`. *(R1, R7)*

- [x] **A2** — Known/unknown/not-applicable and overflow/invalid readings have precise units, sample identity/age and documented sampling bias. A real sampled ticket supplies queue residence; exact oldest age is Unknown whenever earlier unobserved backlog exists. Processing/progress ages use monotonic lifecycle clocks and distinguish no pending work from degradation. Independent fake-clock cases cover empty, pre-first-completion, behind-head sample and invalid/overflow cases.
  *Evidence:* independently specified owner/production-path assertions and raw source-bound outputs as specified above; source schedule, manifests, transcript, or comparison records under external `Q8-validation/`, linked from `docs/quality/worker-progress-evidence.md`. *(R2)*

- [x] **A3** — An ordinary existing health entrypoint emits structured per-worker records for actual owning panes/tabs, including off-screen owners as selected prospectively; stable event/schema version and tab/pane/side/kind/instance identity are asserted through a captured real tracing subscriber. Existing APP_HEALTH_SUMMARY meanings, public DTO bytes/catalog/schema fixtures remain unchanged. Helper-only tests do not satisfy this criterion. Destination: actual-entrypoint fixture plus normal/degraded/recovered JSON excerpts.
  *Evidence:* independently specified owner/production-path assertions and raw source-bound outputs as specified above; source schedule, manifests, transcript, or comparison records under external `Q8-validation/`, linked from `docs/quality/worker-progress-evidence.md`. *(R3, R6)*

- [x] **A4** — Both adapters run through the same compiled production lifecycle/admission/processing/publication path under an injected clock/observation seam. Fixtures hold a known phase without locks held, enqueue an independent schedule, inspect exact expected state, release and explicitly assert acknowledgement/output/drain. Test barriers live in the supplied fake, not alternate production execution. Destination: two worker-owned deterministic fixtures and raw clock/command schedules.
  *Evidence:* independently specified owner/production-path assertions and raw source-bound outputs as specified above; source schedule, manifests, transcript, or comparison records under external `Q8-validation/`, linked from `docs/quality/worker-progress-evidence.md`. *(R4)*

- [x] **A5** — Prove clean sender closure, observed abnormal terminal state and failed-send accounting, with old terminal identity retained. Paused live work resumes on the same instance; replacement through existing constructors gets a new ID/counters and fixture replay produces independently expected uninterrupted final state. Live symbol reset remains the same worker and is never described as resurrection. No automatic restart/drop is added. Destination: lifecycle fixtures and terminal/recovered transcripts.
  *Evidence:* independently specified owner/production-path assertions and raw source-bound outputs as specified above; source schedule, manifests, transcript, or comparison records under external `Q8-validation/`, linked from `docs/quality/worker-progress-evidence.md`. *(R5, R6)*

- [x] **A6** — Existing and additional fixtures retain SetInputs last-survivor position, every PartialUpdated trade suffix, preview/commit/reset and event ordering, Book Project coalescing while applying all ordered data, and independent final indicator/book output. Exact source/contract comparisons prove no public fields/capabilities/meaning changes or new financial behavior; docs retain unbounded queues/history and no SE7 claim. Destination: existing regression names plus independent oracles, full diff and evidence report.
  *Evidence:* independently specified owner/production-path assertions and raw source-bound outputs as specified above; source schedule, manifests, transcript, or comparison records under external `Q8-validation/`, linked from `docs/quality/worker-progress-evidence.md`. *(R6)*

- [x] **A7** — Freeze the root's chosen thresholds and run design before any samples. Measure actual dense time/dollar indicator completion and dense depth/orderflow work with mean/tail ceilings, plus producer admission absolute added-cost ceilings and raw relative deltas. Identical instrumentation, at least B1,A1,A2,B2,B3,A3, actual command rates/counts, output parity, source/fixture/binary/environment hashes and exclusive host proof are retained. Constant observer storage and no queue scan are verified. Destination: predeclared conditions, frozen harnesses, all raw samples and comparison tables in `Q8-validation/` and committed evidence.
  *Evidence:* independently specified owner/production-path assertions and raw source-bound outputs as specified above; source schedule, manifests, transcript, or comparison records under external `Q8-validation/`, linked from `docs/quality/worker-progress-evidence.md`. *(R7, R1)*

- [x] **A8** — Commit clearly labeled transcripts and reproduction instructions for all three stages, tied to actual tested source SHA/tree and independent oracles. Retain raw failures/adverse/inconclusive samples, invocation/output hashes, sampling semantics and exact evidence limits. Destination: `docs/quality/worker-progress-evidence.md`, mission archive and raw `Q8-validation/` records. A self-referential commit hash is not required: an exact external receipt or follow-up public identity can associate the containing source commit.
  *Evidence:* independently specified owner/production-path assertions and raw source-bound outputs as specified above; source schedule, manifests, transcript, or comparison records under external `Q8-validation/`, linked from `docs/quality/worker-progress-evidence.md`. *(R8)*

## Implemented acceptance evidence before final review

These checks record implementation evidence, not independent final delivery or
integrated score approval. The source, full validation, transcript and complete
experiment identities are in `docs/quality/worker-progress-evidence.md` and its
linked `worker-progress-transcripts.md`. Original R/A/G/C text and source spans
are preserved. G2 remains fully applicable to the forthcoming archive commit;
root runs its complete ordered loop before committing, with receipts on issue340.

| Criteria | Actual evidence |
| --- | --- |
| A1-A2 | Typed worker observations, checked ownership/accounting and explicit-age variants; progress29 and existing real-worker assertions; fixed storage and unchanged-domain source preflight. |
| A3 | Actual `maybe_emit_summary` tracing fixture, all owner/identity assertions and complete captured JSON rows in the transcript. |
| A4-A5 | Original real-worker held-phase/drain/terminal/replacement fixtures with independent literal counts/domain outputs; source-bound transcript and 29 focused passes. |
| A6 | Unchanged original lane/indicator/orderflow assertions and public contracts; 3544 workspace passes and independent source preflights. |
| A7 | Prospectively frozen experiment4, unchanged baseline/recipe/limits, all six worker and summary runs retained, all18 domain/counter and numerical gates passed; original failures1/2/3 retained. |
| A8 | Committed source-bound normal/degraded/recovered/terminal transcript excerpts, reproducible commands, exact source/tree/log/binary/manifests and linked full raw comparison evidence. |

G1/G4 retain their original authority/arming records and current source/host
verification. G3 and C2-C4 require the actual final PR/review/CI/integration and
assessment records; they remain closing obligations and are not self-certified
by these implementation checkboxes. No score increment or main merge is claimed.

## Gates

- [ ] **G1 — English and authority.** All authored repository/PR/evidence artifacts satisfy CLAUDE's English rule. Preserve all A2/A3/A5/A6 product/contract/financial/queue constraints. No main mutation, deployment, paid services, purchased credits, new financial rule, weakened guard/rubric or unauthorized scope. *Source:* issue lines 8, 13, 15, 24; CLAUDE; mission standard gates; parent integration authority. *Evidence:* language guards, complete diff, contract/source comparisons, current authority readbacks → `Q8-validation/` and PR.
- [ ] **G2 — Full ordered verification before EACH commit, including archive.** After every edit batch run guards; for every commit, including mission archive or later evidence/prose corrections, complete `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `cargo test --workspace` successfully in that order before committing. Preserve relevant targeted regressions, command/exit/log/source/environment identity and failed evidence. The general delivery contract's prose/reuse path is not an override of this explicit issue requirement. Do not change existing timing thresholds, filters, ignored status or runner behavior to turn a failure green. *Source:* issue line 18; mission verification; root prospective decisions. *Evidence:* actual named-test output, ordered receipts and complete source manifests → `Q8-validation/` and PR.
- [ ] **G3 — Current independent review protocol and bounded history.** High-tier architecture includes direct bug review at the prescribed level and full shape review, with required findings resolved; full delivery uses independent source-first completeness and the tier's criteria/escalation split. AI completion is separate at every tier and zero unresolved required threads is retained. All reviews bind current head/base/key/worktree; changed or uncertain inputs invalidate old approval. Preserve stable IDs, original attempts, batch reservations and finite grants across restarts/rebases; unknown history is not zero. *Source:* issue lines 18, 24; current mission, delivery-review, AI and delivery contract. *Evidence:* source-first reports, actual review records and append-only progress readback → review dossier and PR. Actual final delivery PASS/readiness/CI/integration are C3, not self-certified by this gate.
- [ ] **G4 — Campaign setup, arming and serialized ownership.** Before the first worktree edit, including mission persistence, establish isolated owned branch/worktree, rule out a duplicate/live writer, read actual latest campaign/Q3 prerequisite and root protocol activation, and run `cargo build -p quantick-guards` then `cargo check -p quantick-app --all-targets`. Record branch-bound mission-base and high tier in the owned git directory only. Check/record cache and exclusive build/measurement ownership before use, and expected binary/test identities where caches may be reused. Respect D4 disjoint concurrency, serialize campaign merges, refresh affected checks/reviews after base changes, and retain clean actual head/base/key identities. *Source:* issue lines 22, 24; mission step 6; new-task; campaign integration; preserved prospective preparation. *Evidence:* timestamped identity/arming/host/source receipts → `Q8-validation/` and parent operation records.

## Closing steps

- [ ] **C1 — Accepted source map and real mission before code.** Retain the recorded Q10 C4 activation checkpoint and establish the actual Q8 claim/readback. A fresh source-first reviewer compares full issue, root prospective decisions, frozen independent asks and this draft, plus any actual delegated implementation request supplied at activation. Retain every actual source verbatim and preserve stable IDs/chronology. Resolve uncovered source facets before source edits; after required arming, persist the real high-tier mission in the actual Q8 worktree. This external draft is not that event. *Evidence:* source identity/map verdict, claim and pre-edit receipts → parent and `Q8-validation/`.
- [ ] **C2 — Archive and draft evidence.** Archive the real mission in the reviewed diff, verifying G2 before its commit. Publish a draft PR with base exactly `campaign/architecture-a`, link issue340, state actual branch/worktree/head/base/key, tier, touched rates and performance impact, raw/reused/CI labels, measured numbers and limitations. Publish the committed diagnostic evidence and immutable raw artifact identities; update/read back relevant Project state. *Evidence:* actual archive commit, PR URL/body and board readbacks → issue340/parent/PR.
- [ ] **C3 — Review, readiness and campaign integration.** Obtain current architecture, full source-first delivery PASS, completed AI and zero unresolved required threads, and every required exact-head CI job green. Recheck authority/base/head/cleanliness before readiness and the explicitly authorized head-pinned campaign merge. Record actual merge commit, parents/tree, branch reachability and green source association. Close issue/Project only after integrated-campaign proof; preserve all failed history/counters. No main merge is authorized. *Evidence:* actual review/CI/readiness/merge readbacks → issue340/parent/PR.
- [ ] **C4 — Exact integrated score and coordinator return.** Reassess the actual integrated campaign SHA under unchanged quantick-score v1.0, publish points/limits without presumed SE9 credit, reconcile issue/Projects and return control to root without another user-pasted child goal. Exhausting the original backlog below target requires replanning. Consolidated campaign-to-main review/evaluation and the user's main merge remain separate. *Evidence:* exact-SHA report/publication and state readbacks → parent and issue340.

## Non-applicable gates and scope limits

- No rendered UI surface or trader interaction is requested. UI hooks, screenshot visual QA and trader-UX surface review are not applicable to internal diagnostic logs alone; classify again if code actually changes a surface. Existing GUI/renderer behavior must remain unchanged and normal app diagnostic-call-chain evidence is required.
- No new feed/bar/indicator/layer/panel/crate or public capability is requested. The new helper is an internal owner used by existing workers; a product registry addition would violate this scope. Use a narrow observation port and both concrete worker consumers, not a generic background-job framework. Reassess any new capability-class proposal before code.
- Engine/bar/financial algorithms are not edited. Deterministic expected fixture values and synchronized worker oracles must still be specified before relying on implementation results; existing engine/lane/book golden coverage remains. If actual changes enter engine/determinism logic, the test-first engine gate applies and scope must be revisited.
- Docs-only validation exemption does not apply: concurrent production instrumentation is executable work, and G2 expressly retains the original before-each-commit loop even for archive/evidence commits.
- No bounded-market-history, queue cap, drop/overflow policy, automatic recovery, public DTO extension, real trading, deployment or paid service is included. Bounded monitoring storage and finite test timings cannot be called SE7 completion or supported-live scalability proof.

## Execution and evidence plan

Write independent expected command/count/clock/output schedules before implementation-dependent observations. Use the same production clock/observation seam for normal and fake execution; retain normal monotonic units and fake clock semantics separately. Owner tests cover both real workers at admission/applying/publication/exit boundaries; a bounded timeout is a failure escape and never a PASS condition. Explicitly assert successful acknowledgements because existing flush wrappers may ignore timeout errors.

Preserve SetInputs survivor position, ordered partial suffix accumulation, commit/preview/reset and existing event order. Book fixtures include valid snapshot generation, contiguous updates, ordered Trade/Project work and independently expected quantities/cells. Actual receiver-disconnect publication evidence separates domain batch completion from successful output delivery; a completed cycle must not imply every event arrived. Terminal unwind/closure observations retain unfinished work without pretending it was unapplied; an existing-lifecycle replacement carries a new identity and independent final-state oracle.

The application proof captures a real tracing subscriber while invoking the existing summary path with multiple owners, including indicator and orderflow workers; it checks stable event/owner identity and normal/degraded/recovered content. The production adapter must actually run, not only its formatter. Schema/catalog/DTO comparisons confirm no public contract edits. Classify and quantify the two-second reader/logging path, event-volume growth per owner and clock/lock/publication accounting overhead; worker-completion numbers do not silently certify a different touched rate. No queue scanning, full projection cloning or per-frame formatting is introduced.

Freeze D3/D4 predeclared conditions, baseline/candidate harnesses, build/source/fixture/environment hashes and exclusive host identity before any sample. Retain all original pilots, failures, adverse ratios and inconclusive observations. Baseline telemetry that does not exist is unavailable, not fabricated. Current source/test names and binary SHA256 bind execution, especially across source-root/cache changes. Do not invoke a benchmark or claim threshold compliance in advance.

Evidence destinations: `.claude/GOAL-archive-worker-progress-diagnostics.md`; `docs/quality/worker-progress-evidence.md`; focused app-local owner/worker/app-entrypoint tests in their reviewed modules; external `Q8-validation/` for schedules, frozen conditions, source manifests, command/executable hashes, all raw outputs/transcripts, comparisons and reviews; PR durable progress and issue340/parent links for actual remote state. The final implementation may choose bounded module filenames under D6, but it may not silently add guard/workflow scope or discard evidence.

## Deferred

None. No product requirement, current check, review, authority boundary or score criterion is waived. A later genuine decision belongs in a separately recorded authorized disposition; pending requests are not granted deferrals.

## Completion condition returned to the coordinator

Deliver both workers' bounded attributable observations and actual production diagnostic logs, with independent synchronized normal/degraded/terminal/recovered counter/latency/output proof; preserve domain/queue/public-contract semantics; pass the prospectively frozen paired overhead gates with exact evidence; complete guards and the full ordered loop before each commit including archive; obtain current independent architecture/AI/full-delivery and exact-head green CI; integrate only into the authorized campaign; then publish the exact integrated-SHA reassessment and return to root, or stop at the existing recorded finite repair/authority boundary with concrete evidence. This condition supplies no new token/turn budget or main-merge authority.

## Request as received — source 1: original issue #340

The following attributed issue body is preserved in full. It was read directly from GitHub and equals the saved JSON body; language and formatting inside the quote are source data.

> <!-- campaign-task:milocaetano/quantick#330/Q8 -->
> ## Context
>
> Parent: https://github.com/milocaetano/quantick/issues/330. Stable key Q8. The assessment at 0bd50f9b815a05e2ba8d0c9804324dbb415f6658 leaves SE9 at4/5 (https://github.com/milocaetano/quantick/issues/330#issuecomment-5568748114); the cited worker/health sources are unchanged from c3a92d58bb8a41ec4d78d73e60312b5f765b4da5: existing APP_HEALTH_SUMMARY distinguishes frame, feed and order-book latency, but indicator/orderflow worker pending work, oldest waiting work and termination are not attributed. `indicator_worker.rs:413-482` and `orderflow_worker.rs:65-132` expose send/publication/flush handles over unbounded command channels; send failure logs do not provide a maintained backlog/degradation/recovery picture. This is a verified diagnosis gap, not permission to change queue overflow, retention or public control contracts.
>
> ## Scope
>
> Add small owner-local worker progress/latency observations and expose them through structured internal diagnostics/logging using existing application diagnostic entry points. Demonstrate waiting, progress, terminal/disconnected state and recovery through existing worker lifecycle operations. Preserve command order/coalescing, indicators and ladders, engine/financial determinism, queue/retention semantics and all current public schemas/capabilities/health DTOs. Do not add a new wire capability or DTO field, change existing field meaning, automatically restart/discard work, or silently label unavailable information measured. Work follows Q3's integrated incremental lane transport to avoid competing worker refactors.
>
> ## Acceptance criteria
>
> - [ ] Each affected worker exposes an internal typed observation that distinguishes queued versus processing work, progress and terminal/disconnected state. Report oldest-wait or equivalent attributable latency with precise units/sampling semantics, and unknown/unavailable states explicitly. Counters reconcile successful sends, coalescing, processing, publication and failed sends without underflow, double-count or unbounded auxiliary history.
> - [ ] Production owner-local observations appear in structured diagnostic output with stable event identity and worker/pane attribution through an existing application entry point. Preserve every existing public schema/DTO/capability and existing diagnostic field meaning; no fabricated control-plane reachability claim. A helper only exercised by tests is insufficient.
> - [ ] Deterministic fixtures use explicit clocks and synchronization to hold a real worker at a known processing point, queue known work, inspect the actual pending/latency observation, release it and prove progress/drain. Expected counters/times are independently specified, not read back from the implementation to construct the oracle. No sleep race, ignored assertion or weakened timing threshold supplies evidence.
> - [ ] Terminal/disconnected and existing-lifecycle recovery variants are observable and preserve the same final domain/indicator/orderflow state as uninterrupted processing. No new automatic recovery behavior is introduced. A fresh existing worker instance is identified as such rather than silently continuing another instance's counters.
> - [ ] Regression fixtures and existing lane/orderflow/indicator tests prove original ordering, coalescing, preview/commit/reset and no-loss semantics. Document current unbounded-channel and retained-history limitations; diagnosis is not a new cap or an SE7 completion claim.
> - [ ] Classify actual touched rates and predeclare overhead thresholds before measurements. Same-host alternating dense trade/depth variants record exact source/fixture/build identities, operation counts, worker telemetry and CPU/frame or command-processing measurements sufficient to reject a material hot-path regression. Monitoring storage is bounded independently of total session history and readers do not scan the queue.
> - [ ] Commit exact-SHA reproducible normal/degraded/recovered diagnostic transcripts and source/test oracles; full ordered fmt/clippy/build/workspace-test passes before each commit, guards after edits, and independent architecture/AI/source-first delivery plus exact-head CI close before merge. No score increment until unchanged-rubric reassessment at integrated campaign SHA.
>
> ## Campaign record
>
> Owner class:autonomous. Priority:10. Risk:high (hot-path concurrency/diagnosis). Estimated scope: worker-local telemetry module, indicator/orderflow worker adapters, narrow application health/logging registration and focused tests/evidence. Dependency:Q3/#134 must be integrated into campaign/architecture-a with passing required evidence before implementation begins; record actual merge SHA and green proof. Other independent planning may proceed. Base latest integrated campaign SHA; PR target exactly campaign/architecture-a.
>
> Owner/branch/worktree/PR/head/Project item:null until claim/readback. D4 maximum three independent disjoint implementations (https://github.com/milocaetano/quantick/issues/330#issuecomment-5568580109); merges serialized and affected branches revalidated after base advances. Main merge exclusively human. Evidence:this issue, committed mission/dossier, raw measurements/check logs and PR/CI. Initial operation/repair counters0; preserve task/signature history and retry limits. No deployment, paid services, purchased credits, new financial rules or weakened rubric/guards.

## Request as received — source 2: root prospective preparation decisions

This is the complete coordinator-authored prospective refinement document, not a new user grant or proof of implementation/measurement. Its own statement that choices are proposals is retained. The operative selected choices for this draft are D2–D6 above; an independent source-first verdict must precede actual mission/source edits.

> # Q8 preparation decisions before implementation or measurement
>
> This is prospective coordinator planning, not a claimed mission start, accepted
> preflight, observed benchmark, score increment or changed user requirement.
> The original issue340 and Q8-preparation-99f.md remain unchanged. Instantiate the
> mission only after Q10 synchronization is integrated and its actual base is read.
>
> ## Production-path determinism fixtures
>
> Do not implement the proposal's controlled worker hold by inserting a
> `#[cfg(test)]` alternate execution branch into production processing. Both the
> normal worker and deterministic fixture must use the same compiled lifecycle,
> admission, processing and publication path. A narrow injected clock/observation
> port can supply a fake that synchronizes a known phase, while the normal
> implementation supplies monotonic observations. Never wait on a test barrier
> while holding telemetry state, channel or publication locks. A test-only wrapper
> may instantiate that existing production seam; it cannot replace its behavior.
> Independent architecture review must verify this distinction and the benchmark
> must include the actual production instrumentation cost. Avoid a generic job
> framework or unrelated worker redesign.
>
> ## Prospective overhead acceptance
>
> The preparation's relative producer-send threshold was only a proposal and has
> not been measured or adopted as a mission requirement. A tiny baseline send's
> percentage alone does not establish a material application regression. Before
> any measurement, freeze the following prospective checks in the actual mission
> and source-first map, retaining the earlier proposal:
>
> - Actual dense worker-completion/processing runs: candidate/baseline median run
>   mean <=1.10 and p95/p99 <=1.15 for time and dollar indicator variants and dense
>   depth/orderflow work. All command counts and independent final outputs equal.
> - Producer admission cost: added median run mean <=250 ns/command, added p95/p99
>   <=500 ns/command. Report baseline and candidate raw values and relative changes
>   too; never omit an adverse percentage. Translate added cost using the actual
>   measured admission rate, not an invented supported-live workload claim.
> - Paired ordering B1,A1,A2,B2,B3,A3 minimum, identical instrumentation/warmup,
>   source/fixture/build/environment identity, exclusive measurement host. No queue
>   scan or session-sized auxiliary storage; no threshold or existing test change
>   after results. Inconclusive results remain inconclusive with original samples.
>
> These ceilings concern the newly selected diagnostic work; no existing timing
> test, Q3 comparison or score criterion is relaxed. The actual preflight may
> identify a missing issue requirement, and implementation review still decides
> whether this design and evidence are sufficient for the unchanged SE9 condition.

## Draft provenance

Frozen independent asks raw-file SHA256: `df5537efcd3358f1e2f0b983f70ea56284ee4efd227cd86e37f3df427ac8bfd4`. Original issue body SHA256: `6c1c3b403279fdad27f1bfb7f88d2c443d2097f5838572bcfc0bef2c894bee82`. Root prospective decision decoded-text SHA256: `6b3266100acd832bb43a84b1cdc990df2e4e9465f317e1489478263a4f38affd`. Draft produced UTC: `2026-09-09T04:42:14.154871+00:00`. Source files and prior asks were not changed. Actual delegated implementation-request text, if separate from these two sources, must be retained verbatim and reconciled at C1 before activation; this draft does not invent it.

## Request as received — implementation delegation

The root coordinator delegates this existing approved scope; this is no new user grant.

> Implement Q8 according to the approved worker-progress-diagnostics mission in C:/src/quantick-worktrees/fix-worker-progress-diagnostics/.claude/GOAL.md. Own its runtime changes and focused tests; coordinate build and measurement windows with root. Root owns GitHub, review markers, commits, PRs and merges. Return the tested change and evidence to root. Read the actual worktree AGENTS.md and CLAUDE.md first. ROOT evidence directory is C:/Users/camil/AppData/Local/Temp/quantick-architecture-a; Q8-benchmark-harness-handoff.md and the external harness files are preparation inputs, not measured results. Do not start measurements until root records the exclusive window and frozen source/build identities.

## Activation receipt

The independent source-first preflight passed the original draft SHA256 `fe836e133e72be6c937fa32e7fdb435027097bb24ae1b3c2ef24e3cc1e9d898e`; only the observed activation/identity metadata above and the quoted implementation responsibility assignment were added. All R/A/G/C IDs, outcomes, source quotes, performance ceilings and evidence obligations remain. Arming receipts: `Q8-validation/01-arm-guards.json` and `02-arm-app.json`, both exit0 at the exact base. Source-map delta confirmation precedes runtime edits.
