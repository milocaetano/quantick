# Incremental forming-bar lane transport

Replace repeated full forming-run copies with incremental internal transport while preserving lane, preview and committed indicator behavior.

**Tier:** high, because this changes ordered hot-path worker transport.

## Request ledger

- R1: PartialUpdated carries only newly arrived trades and the worker owns the run.
- R2: Reset exactly with BarClosed, Rebuild and vanished partial; preserve ordered batched delivery.
- R3: Preserve existing ladder golden tests unchanged and independently prove multi-drain parity.
- R4: Preserve lane off/on, cold seed, history prepend/rebuild/rewind and repeated empty publication without duplicate or lost trades.
- R5: Measure actual production producer-boundary N versus 2N traffic across old-history lengths and separate cold seeds.
- R6: Record same-host alternating dense time/dollar lane-enabled CPU and original Perf HUD measurements, rates, tails, traffic, retention, hashes and predeclared thresholds.
- R7: Preserve v1/public contracts, financial/bar rules, trade retention and rendering semantics; do not claim a global constant/linear bound for lane_prefixes.
- R8: Follow campaign #330 Q3 issue #134 ownership, serialized host, full validation and independent review delivery; retain cumulative failures.

- R9: Arm guards and the app all-target check, then persist the mission before the first source edit.
- R10: Respect exact owned paths, including the approved pane test child; do not edit app/tests/mod.rs, Q2 schema paths, or another mission's worktree.
- R11: Deliver an exact locally validated commit/handoff; leave remote writes, review markers and campaign merges to the coordinator.
- R12: Acquire an explicit serialized build/desktop host grant before builds or measurements; retain literal releases and receipt times, and yield at the agreed boundaries.
- R13: Preserve cumulative per-signature failures and report each failure before a bounded repair/retry; retain all valid timing samples and all adverse results.
- R14: Resolve the latest explicit campaign base and revalidate any affected tree before local commit/integration; do not copy stale review markers.
- R15: Preserve the required draft/review/exact-head CI/readiness/campaign-merge progression in the handoff, with coordinator-owned completion explicitly pending.
- R16: Preserve the full received source and stable R/A ledger; no guessed quotations, silent narrowing, or renumbering.
- R17: Do not create a child host goal, request routine user permission, or promise work will continue offline after handoff.
- R18: Disclose that the worker lane_prefixes full fold remains O(forming-bar trades); this task proves neither a global linear/constant bound nor the A+ scalability gate. (S22)
- R19: Keep issue #155's recut investigation separate from Q3. (S23)
- R20: Record actual CARGO_TARGET_DIR and source/binary identities in logs, alongside fixture/environment/source hashes. (S40)
- R21: Depend on no unintegrated work. (S42)
- R22: Deliver evidence to issue #134 and its linked PR, with committed mission/dossier and raw CI logs; the coordinator owns those remote publications. (S52)
- R23: Read Q3-start-issue.json and the current repository instructions and applicable skills before implementation. (S55)
- R24: Retain raw objective command/output/exit evidence under the campaign's Q3-validation directory. (S63)
- R25: Send the coordinator the concise proposed internal boundary, measurement recipe and concrete uncertainties before using the host, then wait for its release. (S64)
- R26: Keep LaneTransport inputs narrow: partial/bar/trade slices, with no ChartState or application ownership. (S65)
- R27: Record the actual desktop UI and CPU harness separately before freezing benchmark conditions. (S66)
- R28: Use only isolated offline stores and an owned desktop fixture; preserve the user's running window and stores. (S67)
- R29: Before every commit, send the coordinator the frozen actual tree plus all four and applicable check exits and raw logs. (S72)
- R30: Reuse a build cache only after a read-only process/path ownership check confirms no active user app or other mission build uses it; use a separate Q3-owned target if ownership is unclear. (S74)
- R31: Assert valid expected CVD lane data during measured production frames before Flush, with the actual bar specification asserted. (S77)
- R32: Preserve the original baseline binary, harness, hash and every original sample as an explicitly reported pilot, disclosing that warmup visibility could satisfy its lane assertion; do not reject it as a timing outlier. (S78)
- R33: Preserve the original candidate test binary and run its original-harness candidate pilot after host return to pair with the original baseline pilot. (S79)
- R34: Rebuild both baseline and candidate tests with the identical strengthened harness and collect the complete unchanged predeclared alternating set. (S80)
- R35: Report both pilot results and the complete final set separately by harness identity; do not blend them. (S81)
- R36: Guarantee restoration of candidate source around the strengthened baseline reconstruction. (S84)
- R37: Explicitly retain and report the original dollar pilot p99 increase of about27.5%, above the15% threshold, alongside the final set, and compare whether the final predeclared aggregate reproduces it. (S87)
- R38: Make no causal or noise claim about an adverse measurement without evidence. (S88)
- R39: Preserve the original partial screenshots and disclose their capture limitations. (S89)
- R40: Check PID-specific capture completeness; make no full visual PASS claim from cropped/partial GPU captures, and preserve the original GUI time-or-dollar acceptance and exact partial dollar-image limitation. (S90)
- R41: If current runs safely yield a complete owned-window image through the existing capture mechanism, retain it as additional evidence. (S91)
- R42: Make no speculative rendering-regression or capture-cause claim; keep the original timing/measurement sequence unchanged. (S92)
- R43: Include executable reproduction commands and the fixture recipe in the evidence document, beyond local temporary paths. (S103)

The S identifiers above are the unchanged103-ask source derivation in `Q3-review-5b586571289d/dossier/source-derived-asks.md`. The first independent completeness report found77 mapped and26 UNLEDGERED asks; this is author ledger repair1, not a completeness, criteria, architecture or delivery verdict. Existing R/A/G identifiers and the original source block remain unchanged.

## Decisions

- D1: The coordinator owns remote writes, review markers, PR readiness and campaign merges; implementation delivers a locally validated commit.
- D2: Parent campaign target is campaign/architecture-a, integrated base 0bd50f9b815a05e2ba8d0c9804324dbb415f6658.
- D3: Earlier two-implementation limit is superseded by D4, not silently edited from the original issue source below.
- D4: User authorizes three independent missions at https://github.com/milocaetano/quantick/issues/330#issuecomment-5568580109.
- D5: Explicit build/desktop host release recorded at https://github.com/milocaetano/quantick/issues/330#issuecomment-5568685791 and Q3-validation/host-release.txt before arming.

## Assumptions

- S1: An internal LaneTransport with only partial/bar/trade slice inputs is a reversible implementation choice; it holds no ChartState/application ownership.
- S2: Cold seeding after a reset or lane enable is separately counted; the N bound applies to an uninterrupted continuously enabled forming-bar epoch.
- S3: CPU frame timing and desktop presentation health are separate evidence and neither substitutes for the other.

## Acceptance criteria

- [x] **A1** Incremental producer traffic and persistent worker concatenation. Evidence: producer tests and source diff, docs/quality/incremental-lane-evidence.md. (R1,R5)
- [x] **A2** All boundary/reset/cold-seed/no-new-data cases preserve final state. Evidence: pane and worker tests, same dossier. (R2,R4)
- [x] **A3** Existing lane goldens unchanged; irregular and batched commands agree with independent expected CVD rungs and committed/preview values. Evidence: source diff and worker tests, same dossier. (R3)
- [x] **A4** Alternating original desktop HUD and deterministic CPU results meet predeclared conditions, with hashes/traffic/retention. Evidence: Q3-validation raw logs and same dossier. (R6)
- [x] **A5** Public/financial/data contracts stay unchanged; residual full worker fold explicitly documented. Evidence: source diff and same dossier. (R7)
- [x] **A6** Owned paths, serialized host and cumulative operation/repair counters retained. Evidence: same dossier and raw commands. (R8)
- [x] **A7** Arming commands precede mission persistence, which precedes the first code edit. Evidence: `arm-guards.log`, `arm-app-check.log`, `commands.txt`, host receipt in Q3-validation, and the objective command/file-change audit. (R9)
- [x] **A8** Final diff is limited to pane.rs, indicator_worker.rs, the approved pane/tests/lane_transport_tests.rs, app/tests/control_plane_tests.rs, this unique mission archive, and docs/quality/incremental-lane-evidence.md. The assigned tab/feed.rs path needed no edit. Evidence: final diff path inventory in the handoff. (R10)
- [ ] **A9** Handoff names the exact local commit/tree and actual validation outcomes; remote writes/markers/merges remain coordinator-owned. Evidence: local commit and handoff record. (R11)
- [x] **A10** Every build/measurement window has an explicit coordinator release; each owned app uses isolated stores and an owned PID, and all measured apps are closed. Evidence: quoted host directives below, Q3-validation/commands.txt, per-run process/environment records. (R12)
- [x] **A11** Each failure is reported before its bounded repair or retry, with cumulative signatures and attempts retained. Both the adverse original pilot and every final sample are reported under their own identities without relaxed thresholds. Evidence: failure ledger, objective command/file-change audit, comparison.json, original/final binary hashes and coordinator's quoted stronger-assertion instruction. (R13)
- [x] **A12** Latest fetched campaign base and reviewed source/tree identity are recorded; any base movement receives affected revalidation. Evidence: final base/readback and command logs. (R14)
- [x] **A13** Local handoff explicitly carries the coordinator-owned draft/review/exact-head CI/readiness/campaign-merge obligations as pending closing work, without claiming those phases complete. Evidence: closing steps and final handoff. (R15)
- [x] **A14** Original issue source and received delegation/host directives are preserved as attributed verbatim quotations; original IDs and issue D3 text remain intact with D4 supersession explicit. Evidence: this archive. (R16)
- [x] **A15** No child host goal, routine user permission prompt, or promise of offline continuation is part of this local delivery; pending external/coordinator work is explicit. Evidence: local handoff scope and closing steps. (R17)
- [ ] **A16** The dossier explicitly discloses the retained O(forming-bar trades) worker fold and excludes both a global linear/constant bound and an A+ scalability-gate proof. Evidence: dossier Scope and boundary section and worker lane_prefixes call. (R18/S22)
- [ ] **A17** The diff and dossier leave #155's recut investigation separate. Evidence: dossier Scope and boundary section and final path/source diff. (R19/S23)
- [ ] **A18** Raw logs record actual CARGO_TARGET_DIR and source/binary identities, with fixture/environment/source hashes sufficient to distinguish the measured artifacts. Evidence: commands.txt, target-ownership.json, measurement-identities.json, final-test-hashes.txt, per-GUI-run environment/executable hashes and dossier artifact table. (R20/S40)
- [ ] **A19** The branch and measured source use the integrated campaign base without depending on unintegrated changes. Evidence: precommit-handoff.json, archive-correction-precommit.json, merge-base readbacks and retained baseline instrumentation patch. (R21/S42)
- [ ] **A20** Objective read events establish that the original issue, repository instructions and applicable skills were read before implementation. Evidence: action-audit execution events13 and18 for issue/CLAUDE/mission/ship/Codex mapping, event35 for UI harness, followed by first source change221. (R23/S55)
- [ ] **A21** Actual objective commands, outputs and exits are retained under Q3-validation, including failures and bounded retries, rather than only success summaries. Evidence: commands.txt, failures.txt, named raw check/benchmark/CI logs and the objective action audit. (R24/S63)
- [ ] **A22** The pre-host submission contains the proposed narrow boundary, desktop/CPU measurement recipe and concrete uncertainties, and precedes host use. Evidence: action-audit pre-host send-call records28,54,68,82,117 and attributed coordinator boundary response. Evidence limit: the audit indexes these calls and their times but deliberately omits message bodies; it alone does not prove submission content. (R25/S64)
- [ ] **A23** LaneTransport holds only its rung budget and sent cursor, and its command inputs are partial/bar/trade slices with no ChartState/application ownership. Evidence: indicator_worker.rs LaneTransport definition and pane.rs call sites. (R26/S65)
- [ ] **A24** The actual desktop and CPU harness records are separate before frozen conditions and remain separate in results. Evidence: objective preparation reads including event35, predeclared-conditions.txt, predeclared-correction.txt and their retained hashes, CPU/GUI fixture descriptions and run records. (R27/S66)
- [ ] **A25** Owned desktop runs use isolated offline configuration and fresh scratch stores, preserving the user's window/stores and closing only their own PID. Evidence: target-ownership.json, launch-gui.ps1, per-run environment.json/process.json, owned-PID capture script and launch/close commands in the action audit. (R28/S67)
- [ ] **A26** Each local commit is preceded by a coordinator submission of its actual frozen tree and all four/applicable check exits/raw logs. Evidence: precommit-handoff.json and archive-correction-precommit.json, their commit-release journal records and subsequent exact-tree commit output; this repair requires the same pre-commit submission. (R29/S72)
- [ ] **A27** Cache reuse satisfies the prior process/path ownership check, with a separate Q3-owned target when unclear. Evidence: action-audit events200,168,175 and target-ownership.json. Temporal applicability: event200 began cache-using arming at09:42:19.908Z, before cache directive receipt163 at09:42:33.170Z; observation168 completed09:42:43.932Z and found no user app and only owned build processes. No new cache-using command event starts between that receipt and observation; event168 is the ownership query. The already-running wrapper contains sequential guard build/app check, whose individual process launch times are not separate audit events. No prelaunch observation is claimed, and the later directive is not silently applied retroactively to already-launched work; independent review determines applicability from these records. (R30/S74)
- [ ] **A28** The identical benchmark harness asserts the actual time/dollar spec and valid expected CVD lane data within measured production frames before Flush. Evidence: incremental_lane_dense_frame_benchmark, final-baseline-instrumentation.diff and retained final binaries/logs. Observation scope: the benchmark asserts the actual spec and a nonempty CVD lane during measured frames; independent numeric ladder/committed/preview expectations are in separate worker correctness tests, not numeric assertions in the timed fixture. (R31/S77)
- [ ] **A29** Original baseline binary/harness/hash and every pilot sample remain intact and are reported with the warmup-visibility limitation, without outlier rejection. Evidence: pilot-baseline-tests.exe, pilot-baseline-instrumentation.diff, pilot-measurement-identities.json, cpu-B1-valid.log and dossier pilot section. (R32/S78)
- [ ] **A30** The original candidate binary is retained and its original-harness pilot supplies the matching baseline/candidate comparison after host return. Evidence: pilot-candidate-tests.exe, cpu-pilot-A1.log, original pilot identities and the ordered host/command record. (R33/S79)
- [ ] **A31** Both retained final test binaries were rebuilt with the identical strengthened harness and the complete B1,A1,A2,B2,B3,A3 set used the unchanged declared conditions. Evidence: final-baseline-test-build.log, final-candidate-test-build.log, final-baseline-instrumentation.diff, final-test-hashes.txt and all six cpu-final logs, preserving both specs per run. (R34/S80)
- [ ] **A32** The dossier reports both original pilot results and every final CPU/GUI sample under separate harness identities. Evidence: separate pilot/final tables and hashes, original/final raw logs and comparison.json; coordinator publication is discharged through C5. (R35/S81)
- [ ] **A33** Baseline reconstruction guarantees candidate source restoration, including failure exits, before candidate validation/measurement. Evidence: action-audit event840's try/finally restore command, retained candidate-source files, commands.txt restore receipt and final source hashes. (R36/S84)
- [ ] **A34** The dossier retains the adverse dollar pilot p99 comparison1.0568ms to1.3478ms (+27.54%, over15%) alongside the final set and explicitly compares the final predeclared aggregate1.2364ms to1.1026ms, which does not reproduce that breach. Evidence: both pilot logs, all final logs, comparison.json and dossier pilot/gate tables; no pilot is discarded. (R37/S87)
- [ ] **A35** Adverse measurements receive no unsupported causal/noise explanation. Evidence: dossier limitations and retained adverse results, with any future public explanation subject to the same evidence requirement in C5. (R38/S88)
- [ ] **A36** Every original screenshot is retained and the dossier discloses white client-bottom bands and the missing candidate-time1 replay controls. Evidence: all eight gui-*/screen.png originals, capture commands and dossier per-image limitation/row counts. (R39/S89)
- [ ] **A37** PID capture completeness is assessed without a broad visual PASS from partial images; the original time-or-dollar HUD acceptance and partial candidate-dollar footer limitation remain explicit. Evidence: complete baseline/candidate time2 images and dossier capture-limit section; complete time evidence does not become a full dollar-image claim. (R40/S90)
- [ ] **A38** Complete owned-window images safely obtained in current runs through the existing capture mechanism remain as additional evidence alongside partial originals. Evidence: gui-baseline-time-2/screen.png and gui-candidate-time-2/screen.png retain chart, CVD/lane, replay controls and original bottom HUD; original timed sequence is unchanged. (R41/S91)
- [ ] **A39** Capture descriptions make no speculative rendering-regression/capture-cause claim and preserve the original timing sequence and all samples. Evidence: dossier capture limitations, ordered commands and all original GUI run records. (R42/S92)
- [ ] **A40** The dossier includes executable CPU/desktop reproduction commands and the deterministic fixture recipe beyond temporary paths. Evidence: Executable reproduction section, retained baseline instrumentation recipe and documented CSV generator readback reproducing SHA256 b957b56c7ea6c89c934b65012aab1dcac1c0328dbbf943760ba0d1f85064d618. (R43/S103)
- [ ] **A41** Evidence has been published to both issue #134 and linked PR #341, identifying the assessed exact SHA and linking the committed mission/dossier and current/prior raw CI logs. Evidence: https://github.com/milocaetano/quantick/issues/134#issuecomment-5570846773 and https://github.com/milocaetano/quantick/pull/341#issuecomment-5570848640 identify assessed SHA cabf3ffe0e0608813d0843ece2eabdbe826ef947, its committed archive and dossier, and CI runs34119574392 and34115975453 with raw job logs. This criterion covers that already-observable candidate publication; subsequent current-evidence updates remain in C5, and independent review supplies its verdict. (R22/S52)

The newly appended criteria are deliberately ungraded pending independent criteria review. Evidence references are candidate discharge artifacts, not manufactured verdicts. Relative raw paths resolve under `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q3-validation/`; the objective audit is `Q3-action-audit/ec0a756b127c2262`, an immutable command/file-change snapshot with SHA256 manifest. Its message indexes carry identity/time only, not message content or an inferred narrative.
- [x] **G1** English artifacts. Evidence: quantick-guards exit0; attributed original source is preserved verbatim.
- [x] **G2** cargo fmt/clippy/build/test workspace all exit zero before every commit; guards after edit batches. Evidence: Q3-validation final-fmt.log, final-clippy.log, final-build.log and final-test-repair1.log, all exit0. The initial workspace test failure is retained below and in the dossier.
- [ ] **G3** Architecture review resolves every Blocker/Should-fix; coordinator retains exact-head review markers. Evidence: independent review reports.
- [x] **G4** Performance impact: producer per-live-drain copies only unsent forming trades; per-trade reset on close; per-frame lane budget check; rare rebuild/enable cold seed; worker per-command concatenation and per-batch full lane fold. Evidence: predeclared conditions and measured dossier.

No new capability, action, registry, financial rule or UI surface is added. Existing UI harness hooks cover the unchanged lane and HUD; screenshot parity and desktop health are retained as original acceptance evidence. Engine source is untouched; independent app/worker fixtures guard ordered behavior.

## Closing steps

- C1: Independent delivery review PASS against this archive, coordinated after local validation.
- C2: Coordinator opens the draft PR to campaign/architecture-a, completes AI review, then architecture/delivery reviews of the final archived diff, resolves required findings with stale checks/reviews rerun, verifies exact-head green CI and marks ready under the repository gates before the authorized campaign merge in C3.
- C3: Before the authorized campaign merge, the coordinator re-reads authority, fetches the latest campaign base, rebases/revalidates affected behavior and reruns stale reviews as needed, then performs the head-pinned campaign merge. Main merge remains the user's action.
- C4: These coordinator-owned remote closing steps remain pending at local handoff. The local commit does not prove remote delivery, authorize another writer, or promise continued offline execution.
- C5: The coordinator publishes the evidence to existing issue #134 and linked PR #341, including the committed mission/dossier, actual before/after original Perf HUD evidence, separate original pilots and complete final measurement identities/results, accurately scoped capture limitations, and raw CI logs including failed runs. The already-completed candidate publication is mapped to A41. C5 continues to require coordinator-owned updates of current evidence and exact-head CI records as the candidate and integration state change; discharge is the issue/PR evidence links and exact-head CI records. Unsupported causal/capture claims remain prohibited. (R22/S52; publication portions of R35,R38-R42)

## Failure ledger

Initial operation failures=0, repair failures=0. Current cumulative operation failures=7, repair failures=0, successful repair/retry attempts=7; the per-signature history and adverse retained pilot are in docs/quality/incremental-lane-evidence.md and Q3-validation/failures.txt. No counter resets or discarded valid samples. The seventh operation was the unchanged observer capture budget test: initial full-suite median268us exceeded250us; isolated test and the single unchanged full-suite retry passed. No source, threshold or runner change and no causal attribution.

The preceding seven-count statement is the historical first-commit record, retained unchanged. The later shared-key shell-path failure and successful absolute-Git-Bash repair brought the recorded implementation/delivery-tooling counters to8 operations,8 successful repair/retry attempts and0 repair failures (`local-commit-handoff.json`); both archive-correction checks/commit added no failure. Subsequent independent completeness review of5b586571289d found26 UNLEDGERED asks in103: occurrence1, this authorized ledger repair1 is in progress, with no successful independent re-review yet. CI run34115975453 separately failed the existing `the_trade_paint_layer_switch_stops_the_marks` assertion309vs224 on Linux, exit101, occurrence1/attempt0; `ci-34115975453-failed.log` is retained. Prior-head CI passed; the failed current run is not replaced by that earlier result. No unrelated test-code repair or CI rerun is part of this ledger edit; the next authorized archive commit triggers fresh exact-head CI. These later signatures and prior counts remain cumulative history, without a reset or claimed repair success.

The subsequent A41 publication repair passed guards and all four local checks (3,464 tests passed, 13 existing ignored) before commit805fed71772e177008bb266bba929cff27b10ca8. Its Linux CI run34180564397 then failed the existing two-pane market-grid fixture (0.1 versus0.01); Windows passed. This distinct signature has occurrence1 and bounded fixture-repair attempt1, retaining the raw failed job output. Independent source inspection identified an unawaited initial book-worker publication as a timing hypothesis, not a reproduced CI schedule. One unchanged isolated local execution passed; it does not replace failed CI. The bounded repair waits through the existing test-only worker barrier, settles a frame while the footprint is still enabled, and checks the independently known0.1 fixture grid before running every original hidden-layer and disagreement assertion. Only `app/tests/layers_tests.rs` joins the assigned test ownership; production and shared harness behavior remain unchanged. Exact repair checks and final-head CI remain required before delivery. The three historical communication criteria A11/A14/A22 remain UNPROVEN without a granted deferral.

The fixture repair passed guards, its focused test, fmt, clippy and build; the full workspace execution then failed the existing live-client Success assertion in `gateway_a_client_that_never_reads_does_not_stall_another` (1,913 app tests passed, one failed, five ignored). Original `grid-repair1-06.log` is retained. This separately recurring signature has occurrence1 and diagnostic attempt1: retain the actual returned outcome in the unchanged Success assertion, then execute one bounded diagnostic verification. Source inspection suggests that eight stalled requests can occupy eight global response slots before DESCRIBE reserves a slot; the hidden outcome means this remains a hypothesis. No response budget, request count, accepted outcome or production behavior changes. A passing isolated execution alone does not establish the failed schedule's cause or clear current CI. The prior frozen tree was not committed after the failed suite.

## Original source (verbatim attributed quotation)

Source: https://github.com/milocaetano/quantick/issues/134, captured before work in Q3-start-issue.json.

> ## What
> 
> `ChartPane::partial_command()` (`crates/app/src/pane.rs`) clones the forming bar's **entire** run of trades on every drain that took in live trades:
> 
> ```rust
> let trades = self.state.trades();
> let count = usize::try_from(bar.trade_count).unwrap_or(usize::MAX);
> trades[trades.len().saturating_sub(count)..].to_vec()
> ```
> 
> ## Why it matters
> 
> - **Rate**: once per drain of live trades — up to ~60 Hz, on the render thread.
> - **Cost**: one allocation proportional to the forming bar's `trade_count`, each `Trade` ~56 B. On a `time:1m` spec over BTCUSDT aggTrades that run is thousands of trades, so it is hundreds of KB allocated, copied and dropped per frame, where before #130 the same call cloned one `Bar`.
> 
> It is correctly gated — nothing is cloned when `lane_rungs == 0`, so a chart with no lane pays nothing — and it is bounded by a single bar, which is why it was not held against #130.
> 
> ## The shape that fixes it
> 
> Send only the trades that arrived **this drain** and let the worker keep the run, resetting it on `BarClosed` / `Rebuild` / `partial = None`. The batch loop in `indicator_worker::run` then extends its accumulator instead of replacing it (today `lane_request` is latest-wins, which would have to become concatenating). Cost becomes proportional to prints arrived, not to bar size.
> 
> ## Acceptance
> 
> - [ ] `PartialUpdated` carries only the newly arrived trades; the worker owns the run.
> - [ ] The run is reset exactly where the forming bar is: `BarClosed`, `Rebuild`, and a vanished partial.
> - [ ] The existing ladder tests still pass unchanged — `the_lane_samples_end_where_the_preview_does` in particular, since it is what proves the curve's live end and the pane's headline are the same fact.
> - [ ] A test that a multi-drain sequence produces the same rungs as one drain carrying the whole run.
> - [ ] Perf HUD frame time on the `dense tape btc` preset with a time or dollar spec and the lane on, before and after, recorded in the PR.
> 
> Found by arch-review of #130 (https://github.com/milocaetano/quantick/pull/130#issuecomment-5210381255), deliberately deferred there.
> 
> <!-- campaign-task:milocaetano/quantick#330/Q3 -->
> ## Architecture A campaign assignment
> 
> Parent: https://github.com/milocaetano/quantick/issues/330. Stable key Q3, priority 5, autonomous. Risk high (hot-path concurrency and ordered data). Current baseline a808b2d87b36d73041027e4d20c053544b454a96 confirms the issue remains: pane.rs:2400 copies the full forming run on each live drain at tab/feed.rs:481-486. Worker coalescing happens only after those copies. This is forming-bar-length cost, not automatically whole-session-length cost.
> 
> Scope: fulfill original #134 with incremental internal run transport, explicit reset/cold-seed handling and unchanged final ladder/preview behavior. Keep BarClosed ordering, lane off/on, history prepend/rebuild/rewind and repeated empty publication correct. Preserve v1 wire/public contracts, financial/bar rules, trade retention and rendering semantics. The worker lane_prefixes full fold remains O(forming-bar trades) unless a separately justified outcome is scheduled; do not claim this task alone proves global linear/constant cost or the A+ scalability gate. #155 is a separate recut investigation.
> 
> Additional acceptance evidence:
> - Actual production producer-boundary counts show N one-print drains transport N new Trade entries, instead of N*(N+1)/2, within an uninterrupted continuously enabled epoch. N and 2N variants and different old-history lengths establish the operation-count bound; cold seed traffic is explicit and separately counted.
> - Irregular multi-drain and batched command delivery produce the same independently specified final ladder and committed/preview state; existing golden lane tests remain unchanged.
> - Boundary tests cover closes/rebuild/vanished partial, disable/re-enable, cold seed and repeated no-new-data publication without duplicates or silent loss.
> - Same-host alternating before/after dense time/dollar lane-enabled fixture records rates, frame average/tails, transported traffic and retained accumulator sizes, with fixture/environment/source hashes and predeclared regression thresholds. The old tick(16) control fixture alone is insufficient.
> 
> Dependencies: none on unintegrated work. D3 authorizes at most two independent implementations. Claimed by `codex/01a07841-8868-75a1-b6f6-9cb93837d72d/implement_q3` in `fix/incremental-lane-transport` at `C:/src/quantick-worktrees/fix-incremental-lane-transport`, starting from integrated campaign `0bd50f9b815a05e2ba8d0c9804324dbb415f6658`. Campaign Project item `PVTI_lAHOA0fkv84Bipkmzg5sR_k`. Q2 paths are disjoint; build/benchmark host and merges remain serialized. PR/head publication follows validation. Base latest origin/campaign/architecture-a; PR base campaign/architecture-a. Full local fmt/clippy/build/test before every commit, guards after edits, independent architecture/AI/delivery reviews and exact-head CI required. Initial counters operation=0, repair=0; preserve per-signature history. Evidence destination: this issue and linked PR, with committed mission/dossier and raw CI logs. Existing #134 is reused; no duplicate perf issue is created.

## Received delegation and directives (verbatim attributed quotations)

The following are exact received coordinator payloads. No message timestamp was supplied with the initial delegation; the host receipts are separately timestamped in raw evidence.

Source: Initial /root delegation.

> Implement campaign #330 task Q3 / existing issue #134. Repository C:/src/quantick-worktrees/fix-incremental-lane-transport; branch fix/incremental-lane-transport, current integrated base0bd50f9b815a05e2ba8d0c9804324dbb415f6658; PR target campaign/architecture-a. Read issue source C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q3-start-issue.json and current repo instructions/skills. Own pane.rs, indicator_worker.rs, tab/feed.rs, app/tests/control_plane_tests.rs under crates/app/src, unique mission archive .claude/GOAL-archive-incremental-lane-transport.md and docs/quality/incremental-lane-evidence.md. Do not edit app/tests/mod.rs or Q2 schema paths; report any necessary ownership change to coordinator. D4 actual userauthority https://github.com/milocaetano/quantick/issues/330#issuecomment-5568580109 raises independentmissions max3; earlier issueD3max2 is superseded. Begin READ-ONLY preparation now; wait for explicit coordinator host release before arming, benchmarks or repo edits. Follow existing mission/ship/review workflow, preserve all issue acceptance and originalsource in stableledger. Deliver exact local validated commit/handoff; coordinator owns remote writes, reviewmarkers and campaignmerges. No childhostgoal or routine userpermission prompts. Preserve original behavior/publiccontracts/financialrules and cumulative failurecounters. Raw objective command/exit evidence under temp campaign Q3-validation; predeclare perf conditions before measurements. Send concise proposed internalboundary/measurement recipe and any concrete uncertainty, then wait for host.

Source: Boundary and desktop authority from /root.

> Boundary direction looks appropriate: keep LaneTransport input narrow (partial/bar/trade slices), do not give it ChartState/app ownership. Before freezingbenchconditions record actualUI and CPU harness separately; desktop fixture slot will be serialized with host and use only isolated offline stores/app fixture, preserve user's runningwindow/stores. You have authorization for required owned desktop measurement; no userpermissionprompt needed. Do not drop originalPerfHUD acceptance in favor of headless-only. Keep priorgoldenassertions unchanged; reset changes must prove issueprescribed parity, no unrelated semanticfix. Q2 stillvalidating; retainread-only hostwait. If pane.rs testorganization requires new pane testchild, giveexactpath; coordinator can verifydisjointness routinely. D4three confirmed.

Source: Initial exclusive host release and exact additional path from /root.

> EXPLICIT HOST RELEASE: Q2 finished all required checks and released build/benchmarkhost. Q3 may acquire host NOW, arm guards and app all-target check, then create mission and edit/implement/measure. Q4 remainsread-only. Durablepre-actionrecord https://github.com/milocaetano/quantick/issues/330#issuecomment-5568685791. Exact additional owned path crates/app/src/pane/tests/lane_transport_tests.rs approveddisjoint and recorded. Record this literalrelease plusreceipt/time beforearming in raw evidence. Q2onlycommit/publication/reviews now; before anycommit send frozen tree+actualallfour and applicablechecks; cumulativecounters preserved. No routineuserapproval needed.

Source: Target ownership directive from /root.

> Q2 used its OWN worktree target C:/src/quantick-worktrees/feat-published-schema-compatibility/target; do notshare/overwrite its retainedreview evidence. Existing quantick-agent-target-fix cache may be reused only after read-onlyprocess/path ownership check confirms no activeuserapp or othermissionbuild uses it; recordactualCARGO_TARGET_DIR and source/binidentity inlogs. SeparateCARGO_TARGET_DIR Q3-owned safest ifownershipunclear. No needpermission; preserveexistinguserwork. Release isactive, Q4notbuilding.

Source: Q4-to-Q3 host release from /root.

> Exclusive build/benchmark host returned to Q3 NOW: Q4 confirms guards/app-alltargets/control-local checks all0 and cargo exited. Checkpoint https://github.com/milocaetano/quantick/issues/330#issuecomment-5569008810. Retain release evidence, validate E0502 repair and proceed candidate binaries plus isolated alternating measurements. Q4 writes source; Q5 read-only awaiting arming. Please release host at next useful source/evidence pause; Q5 needs its initial arming but avoid interrupting measurement pairs.

Source: Stronger assertion and preservation directive from /root.

> Proceed with the stronger measured-frame-only assertion. Preserve original baseline binary/harness/hash and every original sample as an explicitly reported earlier pilot with its limitation (warmup visibility could satisfy the lane assertion), not as a rejected timing outlier. Retain current candidate test binary too; run an original-harness candidate pilot when host is returned so that pilot has its own B/A comparison. Then rebuild both baseline/candidate tests with the identical strengthened harness and collect the full predeclared alternating set unchanged. Report BOTH pilot results and complete final set; do not blend different harness identities, silently discard valid samples, change thresholds, or select favorable runs. Test-only observation strength, no production/golden weakening. Q5 is being released now; remain source-only until its arming finishes.

Source: Q5-to-Q3 measurement host release from /root.

> Q3 exclusive host returned NOW. Q5 guards and app-alltargets checks passed, cargo exited; Q4/Q5 source-only. Checkpoint https://github.com/milocaetano/quantick/issues/330#issuecomment-5569127342. Execute original candidate pilot, guaranteed source restore around strengthened baseline reconstruction, final identical-harness alternating CPU sets and remaining GUI sets with all valid samples retained and original thresholds. Release for Q4 targeted tests when measurements finish; full four can follow next window. Record all SHA/fixture identities.

Source: Adverse pilot readback from /root.

> Read original-harness pilot A1: dollar p99 1.3478ms versus B1 1.0568ms (~+27.5%), above per-comparison 15% tail threshold; time pilot improved. Preserve and explicitly report this adverse pilot result alongside final set; investigate if final predeclared aggregate reproduces it. No causal/noise claim without evidence. The stronger harness decision was prior to A pilot and remains unchanged; don't relabel/drop unfavorable samples or relax thresholds.

Source: Capture limitation directive from /root.

> I visually inspected baseline-time-1 and candidate-time-1 PNGs. Both show chart/CVD, but captured client bottoms contain white bands of different heights; candidate-time-1 omits the replay controls that baseline-time-1 shows. Preserve originals and disclose this capture limitation. Check whether PID-specific capture is complete (no full visual PASS claim from cropped/partial GPU capture). If current runs can obtain a complete owned-window image safely with the existing capture mechanism, retain it as additional evidence; no speculative render-regression or capture-cause claim. Timings/measurement sequence unchanged.

Source: Author completeness directive from /root.

> Before final archive/checks, complete mission source capture and traceability: current GOAL has only issue source and R1-R8. Retain original received delegation and host directives verbatim as attributed source (do not reconstruct guessed text); preserve current IDs and append atomic R/A for independently owed obligations not carried by R8 (e.g. arming→mission→source, exact owned paths, no remote writes, stop/report before retries, draft→reviews/CI→campaign merge, latest base/revalidation, no offline execution promise). This is author completeness correction before any independent delivery review; no verdict/waiver. Current issue D3 text remains unchanged with explicit D4 supersession. Ensure GUI original time-or-dollar criterion and partial dollar image limitation remain exact, not broad full visual PASS. Q4 hit fixture-only privilege setup and cleanup double-panic; retaining host for one bounded targeted repair/retry.

Source: Failure/retry and measured-frame directive from /root.

> Received both baseline harness failures; preserve them as Q3_BENCH_FINAL_LANE_ASSERT occurrence 1/repair 1 and Q3_BENCH_SPEC_UNAPPLIED occurrence 1/repair 1, alongside operation total 2 (no counter reset). I am journaling them now. The intended observation must assert valid expected CVD lane during measured production frames before Flush, with actual spec assertion and identical before/after harness; retain existing golden assertions and predeclared thresholds. No valid timing sample may be discarded. Continue within bounded repairs; report next failure before retry. Please give next exclusive host release window so Q4 can arm/build, then implement independently while you code; measurements stay isolated.

Source: Final-check host release from /root. Receipt 2026-09-07T10:42:10.5383533Z, commands.txt.

> Q3 exclusive FINAL CHECK host released NOW: Q5 5 recovery + all20 workspace_bundle tests passed, cargo exited. Checkpoint https://github.com/milocaetano/quantick/issues/330#issuecomment-5569402506. Fetch/read current base, archive complete mission, final guards and full ordered fmt/clippy/build/workspace-test. Ensure evidence doc provides executable reproduction commands/fixture recipe, not only local-temp paths; preserve hashes/all adverse samples. Freeze actual tree and report exits/raw logs before any commit. Q4/Q5 source-only. Report failures before retries, all signature counters retained.

Final base readback: git fetch origin exit0; HEAD, origin/campaign/architecture-a and merge-base all 0bd50f9b815a05e2ba8d0c9804324dbb415f6658 on 2026-09-07. No base movement or rebase.
