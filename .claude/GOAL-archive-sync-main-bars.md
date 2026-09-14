# GOAL — MS1 #491 history-preserving main BarSpec synchronization

Synchronize exact main commit `1429941feca0efecadb305959f4b3293c84ab323` into `campaign/outside-eight` through a history-preserving reviewed merge that retains the canonical engine BarSpec behavior and every campaign safety invariant.

**Tier:** `high` — the merge joins a public engine vocabulary and chart/backtest behavior with already-integrated feed/control safety repairs, frozen campaign evidence, and campaign-wide regression obligations.

Status: actual high-tier mission awaiting independent root source-first PASS. Product code and merge state remain untouched.

## Request ledger

- **R1 — Preserve both histories and identify the integration.** Starting from verified campaign `432632239f924883c07e76904551be0faf0f37eb`, history-preserving merge exact incoming main `1429941feca0efecadb305959f4b3293c84ab323`. The reviewed head must contain both as ancestors. Identify exact incoming changes and every genuine conflict resolution. Do not rewrite history, push directly to the campaign branch, take over PR #484, or add more than one sync-owned archive; retain the incoming main archive unchanged.
- **R2 — Preserve the incoming BarSpec contract.** Retain one canonical engine `BarSpec`/`BarKind` used by chart and backtest; the existing independent hand-computed parity/golden fixtures; honest `NeedsDealCounter` refusal; the incoming shared time interval bounds `100ms..=24h`; the `60s` default; and Decimal-scale canonical formatting such as `volume(5.50)`. Do not silently restore the old looser backtest behavior or normalize away the recorded scale.
- **R3 — Preserve campaign safety and frozen truth.** Retain all three positive defect outcomes and executable regressions: Binance/MT5 loss disclosure; potentially-executed mutation uncertainty refusing unsafe retry; and `quantick_invoke` idempotency-key carriage. Retain all seven frozen blob IDs, order-ticket/replay behavior, determinism, one engine for chart/backtest/bot, data honesty, one-way/headless dependencies, English, public contracts, financial rules, and every guard/ratchet/test/review. Do not reimplement siblings, score this work, weaken a boundary, guess a baseline, or raise a ceiling.
- **R4 — Produce current integration evidence.** At the actual integrated input tree, run fresh ordered fmt/clippy/build/test plus affected app/engine/backtest and campaign regression checks, retaining raw commands, inputs, failures, repairs, outputs, and provenance. Obtain current architecture, AI, high-tier delivery, exact-head CI, canonical readiness/ship, and root-controlled integration evidence; do not inherit PR #484 approvals.

## Decisions

- **D1:** Exact incoming source is main merge commit `1429941feca0efecadb305959f4b3293c84ab323`; exact starting base is campaign `432632239f924883c07e76904551be0faf0f37eb`.
- **D2:** The integration operation is a history-preserving merge after root source-first PASS, never a squash, cherry-pick, replay, reimplementation, or direct campaign push.
- **D3:** Resolve only conflicts Git reports in the actual merge, using both sides and current tests. Read-only `git merge-tree` inspection is planning evidence, not an assertion that the real merge will be conflict-free.
- **D4:** Root owns GitHub journals, Project state, PR publication, reviews, readiness and serialized campaign integration. Main and settings remain trader-only.
- **D5:** Conventional defaults are delegated; do not ask the trader routine questions. Other writers and PRs #485/#489 remain outside scope.
- **D6:** Pre-edit app/engine/backtest check may reuse the explicitly leased released R1 target with `-j 1`, recorded as cache provenance rather than timing or delivery evidence; own guards use the isolated worktree target; no target clean/deletion.

## Assumptions

- **S1:** Incoming main intentionally makes backtest inherit the chart's `100ms..=24h` time limits and preserves Decimal scale in canonical strings. This is safe because those behaviors and tests are explicit in the exact incoming commit and issue #491; they are acceptance semantics, not regressions to “fix.”
- **S2:** The incoming main archive is historical provenance and stays byte-identical. This is safe because the integration contract explicitly permits retaining it and at most one sync-owned archive.
- **S3:** PR #484 review claims are useful provenance but cannot satisfy current-campaign integration review or CI gates. This is safe because A4 explicitly requires new evidence at the integrated input tree.
- **S4:** Visual/performance/new-extension gates stay not applicable only while actual conflict resolution avoids those surfaces. This is safe because the actual post-merge diff will be reclassified before delivery and any crossed trigger raises the gate.
- **S5:** Frozen-manifest verification compares exact Git blob IDs at the eventual integrated head. This is safe because all seven IDs are fixed in parent #472 and match both current ancestors.
- **S6:** A released build cache can accelerate compatibility checking but proves neither timing nor final-head correctness. This is safe because its source identity is recorded and all final checks must run freshly on the integrated tree.

## Bootstrap evidence

- Isolated worktree: `C:/src/quantick-worktrees/feat-sync-main-bars`; branch `feat/sync-main-bars`; clean exact head `432632239f924883c07e76904551be0faf0f37eb`; tree `fdb46a97219a096e8686c17f4628c15c374b61f3`.
- Canonical mission context: base `origin/campaign/outside-eight`; authority and task source both exact issue #472 URL; context key `de8990720eeda98a686ec4779589ccec419d9846`.
- Own-target `cargo build -p quantick-guards` passed before mission creation.
- The first relevant pre-edit `cargo check -j 1 -p quantick-app -p quantick-engine -p quantick-backtest --all-targets` started at `2026-09-14T21:58:51.0560873Z` and passed after Cargo-reported duration `48.07s`; that invocation did not print an exact finish timestamp. It ran on the exact head/tree above with Cargo.lock SHA-256 `8826f73f0589b91f318c40316c18c1925b926bd7a03c0ddb393ca0a84ec795fa`, Cargo `1.98.0`, Rust `1.98.0`, `QUANTICK_BUBBLES` SHA-256 `5db6b44a4564f7e0cd26482a3f1badd17eb41788e6959453330f2738bb37cd4d`, and released R1 cache `C:/src/quantick-worktrees/fix-mutation-retry-truth/target`.
- A retained bootstrap rerun records exact start `2026-09-14T22:04:54.2349144Z`, exact finish `2026-09-14T22:04:55.3180858Z`, guard exit `0`, relevant-check exit `0`, the same source/tool/cache inputs, and `9.71` GiB free at check start. Raw transcript: private git-dir file `bootstrap-preflight.log`. These checks are baseline compatibility evidence, not timing/calibration or final delivery evidence.
- A read-only PowerShell identity command initially misparsed `HEAD^{tree}` and exited nonzero after already printing the frozen blob matches; the corrected quoted command returned tree `fdb46a97219a096e8686c17f4628c15c374b61f3`. No repository state changed, and the failure is retained rather than erased.
- Initial campaign API operation counter: `0`. Initial campaign repair counter: `0`; the local read-only shell quoting correction is diagnostic history, not an API/PR repair attempt.

## Acceptance criteria

- [ ] **A1** — The reviewed sync head contains exact campaign `432632239f924883c07e76904551be0faf0f37eb` and exact main `1429941feca0efecadb305959f4b3293c84ab323` as ancestors, with incoming changes and every actual conflict resolution identified, no history rewrite/direct campaign push, the incoming archive retained, and at most one sync-owned archive.
      *Evidence:* Git ancestry and tree readbacks, exact diff/conflict manifest, archive inventory, and PR metadata.
      → `.claude/evidence/sync-main-bars/integration.md` and the current PR body. *(R1)*
- [x] **A2** — The integrated tree retains one canonical engine `BarSpec`/`BarKind` for chart/backtest, independent parity/goldens, honest deal-counter refusal, `100ms..=24h` time bounds, the `60s` default, and Decimal-scale formatting such as `volume(5.50)`.
      *Evidence:* Named engine/app/backtest parser, boundary, formatting, parity, golden, and refusal tests at the integrated head.
      → `.claude/evidence/sync-main-bars/affected-tests.log` and the current PR body. *(R2)*
- [x] **A3** — All three campaign defect outcomes, seven frozen blobs, ticket/replay behavior, determinism, public/financial rules, dependency direction, headless boundaries, guards and ratchets survive unchanged or stronger, without sibling reimplementation or score claims.
      *Evidence:* Named feed/control/ticket/replay regression outputs, guard outputs, exact blob-ID manifest, and post-merge diff review.
      → `.claude/evidence/sync-main-bars/safety-and-frozen.md` and the current PR body. *(R3)*
- [x] **A4** — Fresh ordered full validation and affected checks pass at the integrated input tree with raw identity, failures, repairs, outputs and provenance retained.
      *Evidence:* Raw command logs, input manifests, timestamps and exit statuses from the exact integrated tree.
      → `.claude/evidence/sync-main-bars/` and the current PR body. R4's current review, CI and verifier obligations remain mandatory closing steps C1-C3. *(R4)*

- [x] **G1** — Source-first preflight: root independently reads the actual `.claude/GOAL.md` and publishes PASS before any merge or conflict edit. *Evidence:* root's durable issue #491 preflight URL recorded in `.claude/evidence/sync-main-bars/integration.md`.
- [x] **G2** — Bootstrap identity: canonical new-task context proves `feat/sync-main-bars` is isolated from exact campaign base `432632239f924883c07e76904551be0faf0f37eb`, with parent D1-D2 authority and pre-edit checks recorded. *Evidence:* private context receipt summarized in `.claude/evidence/sync-main-bars/inputs.md`.
- [x] **G3** — English: all authored repository prose and identifiers satisfy the repository language contract; attributed source quotations remain explicitly attributed. *Evidence:* guard executable and hook outputs in `.claude/evidence/sync-main-bars/guard-executable.log` and `hooks.log`.
- [x] **G4** — Verification: fresh ordered `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` pass at one recorded integrated tree, plus the affected checks in A2/A3. *Evidence:* `.claude/evidence/sync-main-bars/validation.log`.
- [x] **G5** — Engine determinism: BarSpec parsing/building/formatting and app/backtest parity/goldens demonstrate deterministic shared semantics, including exact time boundaries, Decimal scale, and deal-counter refusal. *Evidence:* `.claude/evidence/sync-main-bars/affected-tests.log` and `validation.log`.
- [x] **G6** — Safety and frozen manifest: the three campaign defect regressions pass; all seven frozen blobs match; ticket/replay/public/financial/dependency/ratchet protections are unchanged or stronger. *Evidence:* `.claude/evidence/sync-main-bars/safety-and-frozen.md`.
- [ ] **G7** — Architecture review: a current integrated-head architecture review has no unresolved required finding and is durably published. *Evidence:* report URL in the current PR body and issue #491.
- [x] **G8** — Retired duplicate: this former high-delivery-review gate is retired without renumbering because delivery PASS is a closing condition. Its mandatory outcome and evidence are owned by C1.
- [ ] **G9** — Integration authority: root alone publishes GitHub state and serializes the reviewed child into `campaign/outside-eight`; this child never writes main/settings/parent state or merges itself into campaign/main. *Evidence:* PR metadata and parent #472 integration checkpoint.
- [x] **G10** — Evidence provenance: raw evidence identifies source/base/head/tree, toolchain, command, environment/cache inputs, timestamps and exit status; operation and repair counters begin at zero and every actual failure/repair is retained. *Evidence:* `.claude/evidence/sync-main-bars/inputs.md` and raw logs.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Non-applicable gates and conditional triggers

- No new capability, bar type, crate, product feature, or trader workflow is authorized. New-extension and capability-registry gates are not applicable.
- No intended visual behavior changes. UI harness/visual QA is not required unless a genuine conflict resolution changes a rendered or interactive surface.
- No new hot-path implementation or calibration is authorized. Existing engine hot paths and financial behavior must remain unchanged; actual timings are explicitly out of scope. If a conflict resolution touches a per-trade/per-frame hot path, root must raise the applicable performance evidence before delivery.
- This is not docs-only. The complete code verification and review contract applies.
- No trader action is required before final main merge; root makes ordinary defaults and preserves the trader-only main/settings boundary.

## Closing conditions

- [ ] **C1** — Current architecture, AI, and high-tier delivery reviews pass with durable reports and zero unresolved required findings. *Evidence:* report URLs and thread-list output in the PR body and issue #491.
- [ ] **C2** — Root-published PR has exact title/branch/head/base, becomes non-draft only after evidence is current, and all exact-head CI is green. *Evidence:* PR metadata and check-run readback in issue #491.
- [ ] **C3** — Canonical readiness and final verifier/ship pass in main-synchronization mode without modifying verifier rules or reusing main's archive as this mission. *Evidence:* verifier URL and readback in the PR body and issue #491.
- [ ] **C4** — Only after C1-C3 and canonical ship PASS, root serializes the reviewed merge into `campaign/outside-eight` and reads back the resulting commit/tree/ancestry containing exact campaign `432632239f924883c07e76904551be0faf0f37eb` and main `1429941feca0efecadb305959f4b3293c84ab323`; this root post-ship integration obligation remains open during pre-merge delivery and cannot be fabricated as child evidence. Only the trader may later merge the consolidated campaign to main. *Evidence:* parent #472 integration checkpoint.

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

## Evidence destinations and counters

- Private bootstrap records: worktree git-dir `mission-base`, `mission-tier`, and mission context receipt.
- Branch evidence: `.claude/evidence/sync-main-bars/` raw input manifests and validation logs.
- Durable public evidence: issue #491, exact-head PR body/checks/review reports, and root parent #472 checkpoints.
- Initial operation counter: `0`. Initial repair counter: `0`. Increment and retain only on actual attempted GitHub/campaign operations or review repairs.

## Verbatim bounded child source — issue #491

> <!-- campaign-task:milocaetano/quantick#472/MS1 -->
> ## Context
>
> Parent campaign: https://github.com/milocaetano/quantick/issues/472. Stable key MS1. Synchronize externally integrated main commit `1429941feca0efecadb305959f4b3293c84ab323` (PR #484) into `campaign/outside-eight` through a separate reviewed PR, preserving campaign history and all fixes. The campaign currently ends at `432632239f924883c07e76904551be0faf0f37eb`; main changed after its F1 integration. This is not an agent merge to main or a takeover of PR #484.
>
> PR #484 consolidates the chart/backtest BarSpec vocabulary in engine. Its incoming main diff contains 23 files, tests and its mission archive. Main's seven frozen measurement blobs still match the initial campaign manifest. Reuse the actual merged source and its provenance; PR prose and earlier reviews are not independent current-campaign integration evidence.
>
> ## Scope
>
> Create an isolated `feat/sync-main-bars` worktree from the latest verified campaign base. After canonical prechecks and independent high-tier actual-mission source preflight, integrate exact main commit1429941f with a history-preserving merge. Resolve only genuine integration conflicts using both sides and current guards; never select a ratchet number blindly, raise a ceiling, discard tests, or overwrite another writer. Preserve incoming main's typed BarSpec and parser behavior, including documented backtest time bounds and canonical decimal formatting, while proving campaign feed/control/ticket/replay compatibility. No unrelated cleanup, new feature, settings change, direct campaign push, main merge or score.
>
> All original parent invariants and frozen manifest apply: determinism; one engine for chart/backtest/bot; data honesty; one-way/headless dependencies; English; unchanged financial/public rules; no weakened tests/guards/reviews/ratchets; no ticket/replay regression. Preserve the three positive defect fixes and executable regressions: Binance/MT5 loss disclosure, potentially-executed mutation uncertainty refusing unsafe retry, and quantick_invoke idempotency-key carriage. Frozen rubrics/measure.py/both score skills remain untouched. Independent outside/quantick assessment and other-model-family same-SHA reproduction for a 9.0 remain parent duties, never claimed from this sync.
>
> ## Acceptance criteria
>
> - [ ] A1: Reviewed sync head contains both campaign43263223 and main1429941f as ancestors; exact incoming-main changes and any conflict resolutions are identified. No history rewrite/direct integration-branch push; zero or one sync-owned archive, existing main archive preserved.
> - [ ] A2: One canonical engine BarSpec/BarKind is used by chart/backtest, with existing independent hand-computed parity/golden and honest unsupported deal-counter refusal tests preserved. Intentional incoming main time bounds/formatting are documented, not mistaken for unchanged old backtest behavior or silently reverted.
> - [ ] A3: All campaign F1/R1 fixes and named regression assertions survive integration, seven frozen blobs match, and ticket/replay/determinism/public-contract/financial/dependency/ratchet constraints remain intact. Any necessary baseline recomputation uses actual integrated source and cannot weaken a guard.
> - [ ] A4: Fresh ordered fmt/clippy/build/test and affected checks pass at the integrated input tree; raw failures/results, identities and provenance retained. Current full architecture/AI/high delivery and exact-head CI verify the integration rather than inheriting PR484 approvals.
>
> ## Campaign assignment
>
> Owner class autonomous; priority1 integration prerequisite; tier high because engine vocabulary and campaign safety proofs meet in this merge. Required condition: exact main source exists and F1/R1/C1 campaign integrations are reachable with their green receipts. Actual issue/Project/branch/worktree/head/PR IDs remain null until readback. Branch planned `feat/sync-main-bars`; worktree `C:/src/quantick-worktrees/feat-sync-main-bars`. Counters operation=0, repair=0; preserve every actual failure and bounded repair history.
>
> Root owns journals, Project state, publication authorization, reviews and serialized campaign merges. Main/settings remain trader-only; defaults delegated, no routine user questions. Other live writers and PRs #485/#489 are outside scope. C2 and A1R can continue disjoint work but must adopt the resulting base with fresh affected verification before final review/calibration.
>
> Evidence destinations: this issue, current-head PR raw validation and canonical review/ship reports, parent checkpoints. Canonical new-task guards and relevant pre-edit checks precede any mission/source edit; full original parent request, this bounded source and delegated instructions remain verbatim in the high mission. Independent source-first actual-map preflight precedes merge/conflict edits. Archive once, then draft against campaign/outside-eight, current reviews and exact-head CI, canonical readiness/ship PASS, and return to root. Main-synchronization archive counting follows docs/campaign/integration.md; do not modify the verifier or reuse main's archive as this task's mission.
>
> executor: gpt-6-astra - strongest implementation for current-main synchronization across engine vocabulary and campaign safety boundaries; one isolated MS1 mission.

## Verbatim original parent request

> $campaign  create Take Quantick to at least 8.0 on the outside-score rubric v1.0
> (docs/quality/outside-score-rubric.md), measured on main, with every dimension at
> least 8.0, from the committed baseline docs/quality/outside-score/d3d4b23d.md (6.5).
> Main has not moved materially since: at eb7bb039, ui_free_share_percent is 39.3,
> impl_spread.QuantickApp is 20, fns.over_200.per_100k is 29.9 and harness_hooks is
> 134. Only a fresh-context assessor may score, one that wrote none of the campaign's
> code and follows the rubric's Independence section. The campaign's own scorecards
> and PR claims are never success evidence, and a 9.0 stands only after the rubric's
> other-model-family reproduction at the same SHA. Freeze the measurement: do not
> change the rubric, measure.py or either score skill while the campaign runs. The
> work must also fix, not merely keep, the confirmed defects that quantick-score's
> A+ gates 4 and 5 block on:
> - automatic Binance reconnects and MT5 in-connection sequence gaps drop trades
>   without a FeedGap or health counter (feed-binance/src/stream.rs:77);
> - unkeyed timeouts and lost transports on mutating capabilities answer
>   retryable:true after the action may have run;
> - quantick_invoke cannot carry an idempotency key.
> Those gates must pass by blind assessment at the campaign head, and quantick-score
> must not fall. Keep every CLAUDE.md invariant: determinism, data honesty, one engine
> for chart, backtest and bot, one-way dependencies, headless crates below app, the
> ratchets, English in the repo, and no regression in the order ticket or replay. Do
> not weaken tests, guards, reviews, public contracts or financial rules. Main merges
> and GitHub settings belong to the trader: prepare settings changes as one-line human
> tasks on the parent issue and never block other work on them. Decide every other
> default yourself, and never wait on the trader for anything but the merge to main.

## Verbatim root delegation

> Your admission proposal fullyread and retained on A1#478 comment5671255775 for nextcapacity. NEW high MS1#491 bootstrap task nowpriority: externallymergedmain1429941f must sync separately intocampaign43263223. Parentjournal5671248184; checkpoint20 comment5671279227 andparentindex readback. Executor#491comment5671265836; Project5Inprogress PVTI_lAHOA0fkv84BjfBrzg68Qqs, roadmap1InProgress PVTI_lAHOA0fkv84BeK9czg68QsY; nativeparent472/deps474/475. Own feat/sync-main-bars WT C:/src/quantick-worktrees/feat-sync-main-bars. ONLY canonicalnewtaskprechecks/guardshighmission BEFORE any merge/conflictedits. A1R just RELEASED R1target: you nowlease C:/src/quantick-worktrees/fix-mutation-retry-truth/target forordinary app/engine/backtest baselinecheck only; own guards targetfirst, recordinputscache, no clean/noactualtimings. Readissue491/fullparentoriginalsource+canonicalnewtask/mission/ship/integration/CLAUDE. Return actualGOALhashandfullsourceR1-R4/A1-A4/G/C withliteralAIgates for rootindependent sourcefirstpreflight. Decisions: exactsource main1429941f historypreservingmerge intoownedbranch afterpreflight; preserve bothsides andall3defects/frozen7/ticket/replay/financialrules, keepmainBarSpec timeboundsanddecimalformatting explicitlyidentified; no reimplementation/score, nobaselineguess/raise; onlygenuineconflictresolutions. Existing mainarchive preserved plusatmostoneownsyncarchive; finalverifier syncmode supports this. Main/settings/parentwrites/merge notauthorizedtochild; rootownsGHpublicationintegration. No userquestions/no subagents. Currentcampaign43263223; verifyrefs. No productedits until rootPASS.

> Root checked host21:52:54Z: ~5.0GiB free of31.8GiB while unrelated builds run. Safe to build tiny local guards now; no need wait allhostidle for that. For app/engine/backtest baseline use leased R1target with jobs1 and recordit when memoryheadroom sufficient (prefer>=8GiB free), not calibration; do not stopothers. Meanwhile prepare source/map in coordination scratch (outsideWT) and inspectincomingmain/tests so rootcanpreflight withoutwaitingbuild; persistactualGOALonlyafterprechecks asrequired. One targetlease owneryou; A1Rreleased. Sharedhostexclusive isfor actualtimings, not allordinarychecks, but currentlowmemory warrants bounded deferralofheavycheck. No userwait/request.
