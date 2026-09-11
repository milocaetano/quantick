# Resolve consolidated coordinator and lane-memory review findings

**Tier:** high. **Status:** Implementation, focused checks and scoped CPU comparison passed; full ordered validation, final reviews, current-head CI and integration remain pending.

## Request ledger

| ID | Outcome | Source | Criteria |
| --- | --- | --- | --- |
| R1 | Name existing coordinator policy limits, preserve behavior and provenance | ARC-FINAL-HC-001 full report | A1 |
| R2 | Release retired lane epoch peak without losing active data; prove large-to-small variant | FAI-LANE-PEAK-001 full report | A2; A3 |
| R3 | Validate exact source, independently review and integrate only into campaign; retain history/authority | D1/D4, canonical delivery and both reports | A4; G1-G4 |

## Decisions and assumptions

D1: existing authenticated campaign authority covers these bounded corrections; source.md retains the user request and both actual independent reports. D2: no value/policy/public/financial change, no live-run cap and no point award. S1: use existing retained_lane_for_test and unchanged dense CPU fixture; root owns all builds and measurements. S2: initial helper hash is historical provenance after a reviewed maintenance delta; original53 test bytes and all existing assertions remain immutable.

## Acceptance and gates

- [x] A1: all three coordinator limits have named module owners with unchanged numeric behavior; original53 tests and all6 composed regressions/assertions remain, including exact three-page query and lease/refusal paths. Initial source hashes remain historical provenance; current maintained hash/maintenance delta are explicitly recorded. Evidence: docs/quality/final-review-corrections-evidence.md; named actual worker/coordinator regressions and exact source-bound outputs. (R1)
- [x] A2: actual worker releases retired allocation at each of the three epoch resets; independently specified large-to-small worker fixtures prove zero retained old capacity after reset plus correct subsequent lane/bar/indicator output, with all valid live-run trades retained. Existing reset/order/golden tests pass unchanged. Evidence: docs/quality/final-review-corrections-evidence.md; named actual worker/coordinator regressions and exact source-bound outputs. (R2)
- [x] A3: prospectively freeze current a788 baseline and candidate sources/binaries for the existing unchanged incremental_lane_dense_frame_benchmark. Serialize B1,A1,A2,B2,B3,A3. Time and dollar median-of-three mean ratios <=1.10 and p95/p99 ratios <=1.15; preserve every run/failure and source-bound memory/probe/output evidence. This is scoped CPU evidence, not a new global/GPU/live scalability claim. No rerun until failure classified and finite repair budget reserved. Evidence: docs/quality/final-review-corrections-evidence.md; named actual worker/coordinator regressions and exact source-bound outputs; prospective immutable benchmark manifest, all six run logs, binary/source hashes and comparison JSON. (R2)
- [ ] A4: archive the original source/mission, current helper provenance, full exact-source validation and inspectable review-input package. Focused tests/guards/Python lint/shared hooks and full ordered fmt/clippy/build/workspace-test pass before code commit; current-head CI evidence is included before closing review. Independent reviews, delivery PASS and campaign integration are closing C1-C2, not acceptance prerequisites for their own verdict. Q12 live work and numeric assessment remain pending. Evidence: docs/quality/final-review-corrections-evidence.md, exact source/log/command manifests and current-head CI at the linked child PR. (R3)

- [x] G1: source-first preflight reconciles the full original reports; actual isolated task ownership/current campaign base, branch-bound mission and private guards precede edits. Source: mission/new-task/delivery contract. Evidence: retained v0 and corrected preflight, arming/ownership/base receipts and mission-source hashes in docs/quality/final-review-corrections-evidence.md. (R3)
- [ ] G2: English, determinism/public/financial/authority limits and all existing assertions/thresholds remain intact; no reverse edge, active-trade truncation or global scalability claim. Source: D1, CLAUDE, original reports. Evidence: full source diff, source/provenance hashes and independently specified regression assertions in the same evidence dossier. (R1, R2, R3)
- [ ] G3: root owns serialized source-bound baseline/candidate measurement and ordered full local checks before commit, applicable Python/hooks and actual current-head CI. Source: CLAUDE verification loop and architecture performance rule. Evidence: command/log/input manifests, all six frozen benchmark runs, comparison JSON and current-head CI linked from the evidence dossier. (R2, R3)
- [ ] G4: exact review inputs preserve authority, finite/unknown counters and stable final finding IDs; readiness/merge remain blocked until closing C1-C2. Source: D1/D4, campaign state/integration and review skills. Evidence: prospective finding attempt reservation, branch/private context, source archive and durable review-progress record; actual closing reviews/merge are C steps. (R3)

No UI capability/pixel/domain/MT5/Cargo.lock change is planned, so those delta gates are non-applicable; reassess if scope changes. Required code/behavior/source/measurement gates remain above.

C1: archive source/mission/evidence, run independent exact-diff architecture/bug and AI reviews, then full source-first delivery against A1-A4/G1-G4. This verdict does not grade itself. Evidence: actual reports/current-head keys on the child PR.
C2: after delivery/current CI PASS, separately gate campaign-only merge and verify parents/tree/reachability; reverify only the existing consolidated findings plus narrow regression inspection at resulting head. Evidence: actual gate/integration and consolidated thread/report readbacks.
C3: preserve Q13-PREFLIGHT-MAP-001 v0 and its corrected source map, all original final findings/attempt histories, and Q12 final-source live adoption/assessment/main obligations. Evidence: issue/checkpoint indexes and full archived original reports; no point is pre-awarded.

## Original authenticated request

> pode lancar nova meta para 9 ?

## Full delegated repair request

<!-- campaign-task:milocaetano/quantick#330/Q13 -->
## Context

Parent: https://github.com/milocaetano/quantick/issues/330. Exact integrated baseline a78842ec852eacf7aecf9af7613e887c6bff144b. Fresh consolidated architecture/AI reviews identified ARC-FINAL-HC-001 and FAI-LANE-PEAK-001. These are required final review corrections, not additional product scope or a score award. Q11 remains reviewed/integrated; Q12 live adoption waits for the final reviewed maintained source.

## Scope

Name the existing coordinator active-task partition threshold50, comment-page size5 and lease duration15minutes without changing their values, policy, API or assertions. Retain original adopted hashes/tests/history and explicitly document the maintenance delta.

Reclaim retired lane-run allocation on BarClosed, Backfilled and Rebuild while preserving all valid active-run trades, event ordering, lane output, transport and financial behavior. Existing disable/vanish behavior remains covered. Use the existing production worker test-only retained-lane probe to prove a large epoch followed by small epochs releases capacity and preserves output. No cap or silent truncation of a valid forming run, new capability, queue policy or retention semantics change.

## Acceptance criteria

- [ ] A1: all three coordinator limits have named module owners with unchanged numeric behavior; original53 tests and all6 composed regressions/assertions remain, including exact three-page query and lease/refusal paths. Initial source hashes remain historical provenance; current maintained hash/maintenance delta are explicitly recorded. Evidence: docs/quality/final-review-corrections-evidence.md; named actual worker/coordinator regressions and exact source-bound outputs. (R1)
- [ ] A2: actual worker releases retired allocation at each of the three epoch resets; independently specified large-to-small worker fixtures prove zero retained old capacity after reset plus correct subsequent lane/bar/indicator output, with all valid live-run trades retained. Existing reset/order/golden tests pass unchanged. Evidence: docs/quality/final-review-corrections-evidence.md; named actual worker/coordinator regressions and exact source-bound outputs. (R2)
- [ ] A3: prospectively freeze current a788 baseline and candidate sources/binaries for the existing unchanged incremental_lane_dense_frame_benchmark. Serialize B1,A1,A2,B2,B3,A3. Time and dollar median-of-three mean ratios <=1.10 and p95/p99 ratios <=1.15; preserve every run/failure and source-bound memory/probe/output evidence. This is scoped CPU evidence, not a new global/GPU/live scalability claim. No rerun until failure classified and finite repair budget reserved. Evidence: docs/quality/final-review-corrections-evidence.md; named actual worker/coordinator regressions and exact source-bound outputs; prospective immutable benchmark manifest, all six run logs, binary/source hashes and comparison JSON. (R2)
- [ ] A4: archive the original source/mission, current helper provenance, full exact-source validation and inspectable review-input package. Focused tests/guards/Python lint/shared hooks and full ordered fmt/clippy/build/workspace-test pass before code commit; current-head CI evidence is included before closing review. Independent reviews, delivery PASS and campaign integration are closing C1-C2, not acceptance prerequisites for their own verdict. Q12 live work and numeric assessment remain pending. Evidence: docs/quality/final-review-corrections-evidence.md, exact source/log/command manifests and current-head CI at the linked child PR. (R3)

Owner class autonomous; priority0 final-review repair dependency; risk high because hot-path memory lifecycle and coordinator provenance are touched. One isolated child branch/worktree from current campaign, root-only builds/measurements/GitHub writes. Existing campaign/Q3/Q8/Q10/Q11 counters remain intact; new Q13 finite repair counters start0, while the two consolidated findings reserve attempt1 before edits. No main/auto-merge/queue/protection/paid/deployment action.

## Closing steps

C1: archive source/mission/evidence before independent exact-diff architecture/bug and AI reviews, then full source-first delivery. C2: after delivery and current CI pass, separately gate campaign-only merge, verify integration and resulting-head consolidated findings; no self-acceptance is claimed. C3: retain all final finding histories and pending Q12 live adoption/assessment/main obligations.


## Original independent findings

# Q13 source: resolve consolidated review findings

Authenticated campaign authority and unchanged target are retained in parent330 D1/D4 and D-score9-20260909. User request, verbatim:

> pode lancar nova meta para 9 ?

Existing authority permits behavior-preserving fixes, isolated child PRs and reviewed green campaign-only integration; main remains user-only. The following actual independent final review reports are the bounded repair request. They concern exact candidate a78842ec852eacf7aecf9af7613e887c6bff144b, not an invented feature.

﻿step 0: direct strong-model bug review, requested **medium** (private `mission-tier`: `campaign/architecture-a high`), **0 confirmed correctness findings**. The assignment explicitly requested medium and this direct pass followed that scope before shape verdicts. No bundled `code-review` invocation, cached effort reuse or child delegation occurred. Backend effort is not separately introspectable; bundled effort-first/parser proof is not claimed.

Full consolidated review of **campaign/architecture-a**, `origin/main...HEAD`, all campaign children:
- HEAD: `a78842ec852eacf7aecf9af7613e887c6bff144b`
- Tree: `ccb58ae8b3533d736bc831774e988750ae626836`
- Base: `origin/main` at `9b8495538a9a1af16fce61dcd4805d112fc0c71e`
- Review key: `fad5c130e336c84d2a89833c2e832599784707a5`
- Worktree: `C:/src/quantick-worktrees/campaign-architecture-a-final`

Identity matched before and after, with clean status. The final key was independently recomputed from Git's raw diff bytes. This is a fresh whole-campaign source review, not a reuse of child verdicts.

The direct pass read actual worker/lane production paths and surrounding pane/reset/mirror callers; indicator operation ownership and tests; released-schema inventory/checker tests; Windows filesystem fixtures and production discovery context; import installation seam and disk recovery oracles; lexical root guard and registry integration; screenshot capture/recovery/verifier; campaign helper, offline recovery tests, and hook/CI changes. Child reports supported this reading and did not substitute for it. Canonical architecture skill, step-0 reference, relevant dimension references, CLAUDE.md and campaign integration contract were read.

**Findings**

No confirmed correctness Blocker.

**ARC-FINAL-HC-001 — Should-fix — hardcoded-values**
`tools/campaign/architecture-a-coordinator-v2.py:574` (also `:111`, `:465`): give the maintained coordinator's operational limits named owners.

The helper configures a 15-minute renewal inline, partitions at inline 50 active tasks, and embeds `last:5` in its GraphQL query. These configure production behavior; unlike its byte budgets, they have no named module owner. Changing policy requires hunting executable expressions/query text.

Concrete fix: documented module-top `DEFAULT_LEASE_MINUTES`, `ACTIVE_TASK_PARTITION_THRESHOLD` and `COMMENT_PAGE_SIZE`, preserving values and behavior; use a parameter or named construction for the query. Keep original adoption hashes and existing boundary tests. Whether the lease should follow `retry_policy.lease_minutes` is a separate explicit policy decision, not a correctness assertion made here.

Counterargument checked: README intentionally preserves the adopted external source byte-for-byte. That explains provenance and argues against an unrecorded edit, but does not exempt newly maintained production policy from the canonical rule. The original hashes can remain immutable provenance while a reviewed maintenance delta names unchanged values. This is a narrow maintainability finding, not an alleged authority bypass.

**All nine dimensions**

| Dimension | Verdict and evidence |
| --- | --- |
| 1. Docking | No required finding. IndicatorHost has add/remove effects and an independent fake; IndicatorSlots borrows bookkeeping without acquiring focus/layout/authority. Native sources retain existing catalog registration. Worker ProgressClock and observed endpoint are shared by both workers; no reverse Cargo edge or forked aggregator. |
| 2. Performance | Finite measured support; explicit limits. Lane producer copies only unsent suffix per drain, worker folds current full forming run per batch. Q8 adds local accounting/atomic read per command, sampled slot lock, ledger work per batch/output, and owner-count JSON every two seconds. Read experiment-4 manifest/comparison/A1 receipt; domain app/Cargo source unchanged from measured 65e24caa to reviewed HEAD. No fresh timing performed. |
| 3. Hardcoded values | Should-fix. Module-top evidence byte/timeout limits and Python-to-Rust bound pinning are otherwise explicit. Coordinator inline policy limits remain. |
| 4. Tests | Behavioral and port proof present; execution not independently rerun. Read lane suffix/reset tests with independent CVD rungs; worker known/unknown/terminal/interleaving/ownership tests; indicator fake/mirroring/permission tests; released schema negative coverage; real Windows ACL/junction fixtures; import fail-and-repeat disk oracles; rehashed screenshot corruptions and offline two-process recovery. Test-only probes add observation, not alternate domain behavior. |
| 5. Standardisation | No required finding. Existing registry/Guard/Policy and production_flags reused; published schema checker reuses require_compatible_version; test support stays test-only; shared review defaults centralized rather than second implementation. |
| 6. Human-friendly | No required finding. Age distinguishes Known/Unknown/NotApplicable/Invalid, clock units and authorship retained, queued is explicitly not instantaneous channel length; synthetic DPI and partial-import limitations are candid. |
| 7. Second operator | No new required surface gap. Existing native/Pine attach/remove adapters preserve human/operator ownership and full pane target; existing control read/discovery contracts unchanged. Worker tracing is internal diagnostics with machine-readable JSON, not falsely advertised as a new MCP capability. New evidence/coordinator operations are named Python APIs/CLI with readable outputs. |
| 8. English | Manual inspected prose/branch/commit messages acceptable; fresh guard result pending. Read English implementation, explanatory evidence/README, campaign branch, complete commit subjects and bodies. Attributed source quotations are data. Did not run language guard or independently inspect a final PR title/body. |
| 9. Trunk | No unexplained baseline raise or new application-root field. app.rs +1 registration; pane.rs net -5; indicator_manager.rs -30; layout_wiring.rs -12. indicator_worker.rs net +342 includes substantial new tests plus transport/telemetry code; orderflow_worker.rs +28. Existing size-baseline unchanged. New separate root caps QuantickApp 10240 / ChartState 429 document origin and prohibit cross-root budget transfer; historical Q6 metrics are not claimed as fresh final-head measurements. |

**Rates and performance evidence**

The hot-path benefit is specific: continuously enabled lane transport copies N new prints, replacing repeated full-prefix copies; enable/rebuild cold-seeds once. The worker still folds O(current forming-run trades) per batch and retains allocation high-water capacity across closes/rebuilds. Existing command channels and market history are unbounded. The implementation does not establish a total memory or supported-live-rate bound.

Worker telemetry adds checked local counts and an atomic pending read per command; sampled commands acquire the sample mutex and read the clock. Consumer batches/output publications update a fixed ledger. The two-second health summary reads every owner, including hidden panes, and serializes after releasing diagnostic locks. A held worker can never hold diagnostic locks during domain work or phase callbacks, but mutex acquisition has no hard wall-clock guarantee.

I read the retained experiment-4 manifest, numeric comparison and A1 exit/binary receipt, plus the checked-in harness and supporting Q8 evidence. The measured source was `65e24caa23b35381471b84eb69ae69db09ac794f`; `git diff --name-only` from that commit to this HEAD returned no changes under app/engine/orderbook/orderflow or Cargo manifests/lock. Thus the evidence remains attributable to these runtime inputs; it is not relabeled as a new final-head benchmark.

| Experiment 4 completion candidate/base | Mean | p95 | p99 |
| --- | ---: | ---: | ---: |
| Time | 1.022367 | 1.012538 | 1.032396 |
| Dollar | 1.051347 | 1.048045 | 1.014022 |
| Depth | 0.993455 | 0.983488 | 0.984447 |

All are within the recorded 1.10 mean / 1.15 tail limits. Mean added admission cost was 3.669 / 4.682 / 7.855 ns per time/dollar/depth command. These are finite synthetic paired results, not supported live throughput. The separate summary fixture increases median mean synchronous cost from 7.8325 to 31.2817 microseconds for five additional worker records per synthetic two-second opportunity; it excludes disk I/O, GUI cost and asynchronous reset completion. Earlier failed experiments and the adverse Q3 pilot are preserved in the committed evidence. I did not rerun benchmarks or independently recalculate all raw distributions.

**Execution limits**

No build, test, application launch, source/private-marker/GitHub write or delegation was performed. Only these report artifacts were written. Fresh ordered workspace checks, applicable Python/hook checks, language/ratchet guard output and final-head CI must be bound by the coordinator before using this source review for delivery. Historical child checks are not a fresh final-head pass. Original screenshots and OS DPI were not freshly exercised; synthetic geometry is not an OS-DPI proof. No numeric score or readiness is claimed.

**Correctness** — Direct bug pass confirmed zero defects; no open correctness Blocker.
**Docking** — Borrowed indicator operation context and a two-effect host port improve ownership; native registry and crate direction remain intact.
**Performance** — Incremental per-drain transport reduces copying; bounded telemetry has attributable paired support and explicit per-command/per-batch/two-second costs, with retained hot-path limits.
**Operability** — Existing named/registered indicator authority surfaces survive; internal worker records are machine-readable and do not claim new MCP reachability.
**Proof** — Independent unit/disk/socket oracles cover lane boundaries, held worker accounting, fake host effects, human/operator refusal, released-schema removal/narrowing, Windows ACL/junctions, screenshot corruption and offline process restart. Fresh execution remains coordinator-owned.
**Accumulation** — app.rs +1, pane.rs -5, indicator_manager.rs -30, layout_wiring.rs -12 raw net lines; worker files grow for transport/telemetry/tests. No size ceiling raised; separate root caps are documented.
**Language** — Read prose, branch, commit subjects and bodies are English; fresh guard and final PR prose checks are not claimed.



## AI review - consolidated campaign/architecture-a @ a78842ec852eacf7aecf9af7613e887c6bff144b

Identity: HEAD a78842ec852eacf7aecf9af7613e887c6bff144b; tree ccb58ae8b3533d736bc831774e988750ae626836; branch campaign/architecture-a; worktree C:/src/quantick-worktrees/campaign-architecture-a-final; base origin/main; base tip 9b8495538a9a1af16fce61dcd4805d112fc0c71e; review key fad5c130e336c84d2a89833c2e832599784707a5.

Scope: fresh full first-round review of origin/main...HEAD, including the complete campaign, not just Q11. Source, actual fixtures, neighboring contracts and archived scope were read. 151 changed files; 29 pre-existing files edited. Findings: one verified source finding, FAI-LANE-PEAK-001, unresolved and unpublished. This is a stable dossier identifier, not an invented GitHub thread ID; root owns publication and the durable thread mapping. No accepted or fixed findings asserted. No builds or tests executed by this reviewer.

1. **Modular: PASS.** Rare indicator mutation bookkeeping moved behind IndicatorSlots and IndicatorHost (crates/app/src/app/indicator_operations.rs:19,38,55); application adapters retain focus/layout policy (crates/app/src/app/indicator_manager.rs:85,108). Worker telemetry has its own fixed-state module (crates/app/src/worker_progress.rs:124); diagnostics attribution lives in a separate cadence adapter (crates/app/src/app/health/worker_diagnostics.rs:8). The extension guard docks in the existing GUARDS registration at crates/guards/src/lib.rs:183. Blast radius: 29 pre-existing files, largest crates/app/src/indicator_worker.rs, 360 additions and 18 deletions, 378 edited lines (including its new test cases). New scanner, lexical handling and tests are split by responsibility.

2. **Decoupled: PASS.** IndicatorSlots borrows only slot collections, with host effects limited to add/remove (indicator_operations.rs:19-44); it cannot navigate the application or change authority. LaneTransport accepts partial/trade slices and owns a cursor rather than ChartState (indicator_worker.rs:248-285). Worker progress owns accounting and exposes an injectable ProgressClock; Rc confines the single producer and observer while only consumer/sample state crosses the thread boundary (worker_progress.rs:30,124-151). The import seam injects only final rename, preserving the ordinary validator/stager (workspace_bundle.rs:192-208). Schema tests share the existing quantick-control comparator instead of implementing another compatibility policy (crates/control/tests/support/published_schema.rs:142). No reverse crate edge was added.

3. **AI-ready: PASS for the changed surface.** No new mouse-only application capability is introduced. Existing script attach/detach routes still register named actions and typed AttachInput/AttachResult/DetachInput/DetachResult with ControlError (crates/app/src/control/script.rs:69-101,121-124,249-254); the modified adapter uses the new operation owner. The agent-path fixture verifies attach, detach, forbidden human-slot removal and unchanged human saved content (crates/app/src/app/tests/indicator_operations_tests.rs:63). Repeated ordinary import is covered by recovery fixtures. Attach remains deliberately additive under its existing descriptor policy, not newly promised to deduplicate. Internal APP_WORKER_PROGRESS is typed serialized diagnostic telemetry at the existing log cadence, not a new public v1 command or scope. Existing String import/worker build errors are unchanged compatibility surfaces; the new rename seam retains std::io::Result. The new external coordinator explicitly records intent/results, refuses uncertain retries, and separates inactive activation from publication (tools/campaign/architecture-a-coordinator-v2.py:340,374,498); these are tooling calls, not UI capabilities.

4. **Agent-tested: PASS.** Below-app runnable error-path examples are cargo test -p quantick-control published_core_required_field_narrowing_fails_even_after_snapshot_regeneration (crates/control/tests/published_schema_compatibility.rs:29) and cargo test -p quantick-guards field_visibility_authority_and_host_effect_changes_are_rejected (crates/guards/tests/extension_boundary.rs:128). The latter's literal foreign shape/host-effect mutations exercise the new guard and would not be rejected without it. The former protects a released required-field contract independently of regenerated snapshots. All 56 retained schema byte hashes were independently read and matched their manifest; this read-only hash inspection is not a test-suite result. Windows tests call production discovery against independently inspected .NET ACL/junction specimens (crates/control-local/src/discovery/windows_tests.rs:285,311,326), with execution added to Windows CI. App-owned worker seams additionally have deterministic injected clock/phase schedules: sampled_ticket_does_not_claim_unobserved_head_or_idle_stall and overflow_and_clock_regression_are_explicit (worker_progress/tests.rs:89,124), actual consumer output fixtures (indicator_worker/progress_tests.rs:76), and independent CVD literals. Fractional screenshot fixtures use literal geometry and pixels (app/tests/screenshot_evidence_tests.rs:151), and workspace recovery tests inject second/fourth rename failure while inspecting actual bytes (workspace_bundle/recovery_tests.rs:261,346). Instant is confined to app timing/telemetry and benchmarks; SystemTime in operation bookkeeping is existing file metadata. BTreeMap/BTreeSet preserve deterministic ordering. Fresh-process coordinator recovery uses literal expected retained counters and outcomes (tools/campaign/test_campaign_recovery.py:272).

5. **Extensible: PASS.** Native IDs still resolve through the existing native catalog, rather than adding an enum case per native (indicator_worker.rs:90-107); an independent NativeHost exercises both a valid ID and an unknown ID through the exact same port (indicator_operations.rs:241). The new guard protects authority shape and per-root growth while allowing focused native-owner additions (crates/guards/tests/extension_boundary.rs:269); unsupported macros/aliases/layouts are diagnosed rather than silently exempted (crates/guards/src/extension_boundary/scan.rs:53-131). Released v1 schema inventory remains present when a v2 successor is appended (crates/control/tests/support/published_schema.rs:174-198). The guard is intentionally lexical and does not claim semantic whole-program coupling analysis; its documented macro/alias/free-helper limits remain.

6. **Scalable: WEAK — FAI-LANE-PEAK-001.** Producer transport now copies only unsent suffixes (indicator_worker.rs:269-285), and diagnostic storage is fixed per owner with no history-growing telemetry buffer (worker_progress.rs:109-151). However the new worker-owned lane Vec survives between batches, and BarClosed/Backfilled/Rebuild only clear its length, preserving peak allocation (indicator_worker.rs:625,658,664,692). The concrete breaking workload variant is a very large forming bar followed by many small bars while the lane stays enabled: the retired bar's peak allocation remains resident in every affected worker, even though subsequent lanes need very little memory. The baseline's per-batch forming-run Vec was dropped after each publication. This is specifically cross-epoch retained allocation, not a request to truncate current valid trades or eliminate the separately scoped full-run fold.

Top fix: crates/app/src/indicator_worker.rs — release or bound spare lane allocation at epoch-reset boundaries while preserving every live trade and all existing ordering/preview semantics; add a large-epoch-to-small-epoch retained-capacity fixture. Flips: Scalable WEAK for FAI-LANE-PEAK-001.

### Finding FAI-LANE-PEAK-001

Should-fix; anchor crates/app/src/indicator_worker.rs:664. The newly persistent lane_run retains its largest capacity across BarClosed, Backfilled and Rebuild, because those branches use clear(). An unusually large bar followed by quiet small bars therefore leaves an additional peak-sized Trade allocation resident indefinitely until lane disable/vanished partial or worker destruction. docs/quality/incremental-lane-evidence.md:13,186 explicitly corroborates the new retention, but disclosure is not a trader acceptance. Release/reclaim spare storage at epoch boundaries using a behavior-preserving policy; preserve full active-run data and do not silently cap or drop trades. Verify independently that a large run followed by close/rebuild and a small run preserves identical lane outputs and reduces retained capacity. GitHub thread ID: not yet published.

### Touched-loop rates and limits

- Per trade/bar close: existing engine/indicator command emission; producer cursor reset is O(1). New acceptance accounting is O(1), sampled timestamp storage is one slot. Per depth update: existing BookCommand application plus O(1) admission accounting; no depth history scan was added.
- Per feed drain / changed lane budget (up to frame cadence): producer copy O(new forming trades), with an explicit O(current forming run) cold seed on enable/rebuild; unchanged no-data updates carry zero trades (pane.rs:2387,3655; pane/tests/lane_transport_tests.rs:28).
- Per worker batch: drain O(queued commands); slot/input coalescing and publication retain existing slot/output costs. Progress begin/finish and coalescing accounting are fixed-state. Ordered suffix append is amortized O(new trades). Lane evaluation still folds O(current forming-run trades) and evaluates at most 64 rungs per indicator (indicator_worker.rs:36,59,868,892). Neither trade/session retention nor the existing unbounded mpsc queues have acquired a finite global cap. Burst commands are not silently discarded; only documented preview/input/project coalescing is counted. Current-run storage is intentionally retained; cross-close spare allocation is the new finding above.
- Per frame: lane budget comparison is O(1); sending occurs only on changed budget. Every two seconds, diagnostics traverse all tabs/panes/workers, serialize one fixed snapshot each, and perform no projection/history clones (app/health/worker_diagnostics.rs:8-44).
- Rare user indicator mutations: bookkeeping scans/retains O(attached slots), with existing layout mirror fanout and script compilation/history replay unchanged. Rare workspace import stages and installs O(registered stores + their bytes), preserving partial-failure/re-import behavior.
- Guard/CI invocations: source-tree traversal and lexical/shape analysis scale with source size, including rescans for protected items/exclusions; not on trading paths. Released-schema comparisons scale with the finite 56-document baseline and generated documents.
- External evidence capture/recovery: per artifact/chunk and per pixel/control; verifier limits bundle input to 8 MiB and decoded image to 16,777,216 pixels, with bounded inflate output. Recovery uses 36,000-byte blocks, 128-KiB comment reads and 30-second HTTP timeout; capture uses 45-second response and 5-second shutdown limits. These remain rare tools against an explicitly owned local adapter/pinned recovery artifacts, not streaming trading work.
- External campaign operations: checkpoint/task/journal traversal is proportional to boundary state; comments are paged five at a time back to the expected boundary. Individual checkpoint/journal records have 24-KiB/8-KiB limits and partition payloads use 12-KiB chunks. Total campaign history is not globally capped; explicit checkpoints bound the operational reconciliation interval. No automatic blind append retries were added. Test/benchmark loops are finite fixtures and are not runtime throughput guarantees.

Final identity readbacks matched HEAD, tree, base, branch and review key; status remained clean. No source/private-marker/GitHub writes occurred. Only this review's dossier files were written. Root still owes finding/report publication, post-publication identity checks and all validation/delivery decisions.



## Execution evidence at archive

Focused source-bound checks and all18 checks of original CPU experiment1 passed; see docs/quality/final-review-corrections-evidence.md and https://github.com/milocaetano/quantick/issues/357#issuecomment-5611225380. No source/helper policy/assertion/threshold changed beyond the declared correction. Full ordered validation and closing review/CI/integration receipts remain external closing evidence; unchecked A4/G/C obligations are not silently satisfied.
