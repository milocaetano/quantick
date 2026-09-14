# Disclose feed continuity loss before subsequent trades

Fix automatic Binance reconnect loss and MT5 in-connection sequence gaps so the chart and its public diagnostics admit missing data without fabricating trades.

**Tier:** high. Feed continuity is a data-honesty boundary and runs per message.

## Request ledger and source map (revision 1)

- R1: Fix automatic Binance reconnect loss disclosure. Source: original defect bullet 1; delegated F1 A1. A1.
- R2: Fix MT5 in-connection sequence gaps, including truthful initial attach, duplicates, resets, backwards IDs and reconnect behavior. Source: original defect bullet 1; delegated F1 A2. A2.
- R3: Publish ordered FeedGap and health evidence, preserving incomplete-data honesty without inferred trade fabrication. Source: original invariants; delegated F1 A3/A4. A3.
- R4: Preserve determinism, one shared engine, one-way/headless dependencies, ratchets, English, financial rules, order ticket and replay; do not weaken tests, guards, reviews or contracts. Source: original final invariant paragraphs; delegated F1 A3/A4. A4.
- R5: Preserve the frozen rubric, measure.py and both score skills, score independence and non-regression. Source: original score/measurement paragraphs. A5. Campaign scoring and other-family reproduction remain coordinator-owned; this child supplies code evidence only.

## Decisions

- D1: User explicitly requires fixes and delegates defaults. No qualifying unresolved trader question remains.
- D2: Preserve continuity across automatic reconnect without fabricating trades. Known source-message loss is distinct from unknown transport loss.
- D3: Add to existing FeedGap/health interfaces; no weakened public contract or financial rule.
- D4: Do not invent loss at initial attach or claim synthetic MT5 IDs span connections. Preserve truthful duplicates/reset/reorder behavior.
- D5: Do not edit scoring/measurement artifacts or assess this campaign's score.

## Assumptions and plan

- S1: Own minimal app event consumption and feed/health snapshot wiring in this child. Coordinator explicitly approved necessary end-to-end scope before implementation; gateway belongs to R1 sibling.
- S2: Preserve Binance's existing public Trade sender API; a host-owned tracker sees its ordered channel across automatic reconnect and can publish anomalies before the trade. Safe because the internal reconnect task and receiver live for the same feed session.
- S3: MT5 sequence IDs count wire ticks, including quote-only/history ticks; counters must say messages, not lost executed trades. A typed anomaly preserves known count and optional market-time bounds.
- S4: Per-message detection uses bounded scalar state; steady-state O(1), no extra clock reads. Gap rendering remains bounded. Hot-path fixture benchmark compares baseline and changed feed projection; rare anomaly formatting and snapshot paths are outside steady-state delivery.
- S5: Existing observer/feed diagnostics and gap rendering are reused. No new trader action, feed provider, engine algorithm or financial surface is introduced. UI verification applies to newly reachable gap states, not a new UI control.

## Acceptance criteria

- [x] **A1** — Automatic Binance reconnect with skipped agg IDs emits explicit loss before its next Live trade; contiguous reconnect does not invent missing IDs. Evidence: deterministic local WebSocket reconnect fixture and ordered host projection assertions. → PR validation report. (R1)
- [x] **A2** — MT5 skipped sequence IDs emit ordered diagnostics before later mapped trades, including quote-only tick loss, while initial attach, new-session reset, duplicate and backwards sequences have truthful classifications. Evidence: deterministic session/socket fixtures. → PR validation report. (R2)
- [x] **A3** — Feed gaps and cumulative health diagnostics are visible through supported public snapshots; short confirmed gaps are retained, unknown loss does not claim a count, and no missing trade is synthesized. Evidence: app drain/snapshot tests and source inspection. → PR validation report. (R3)
- [ ] **A4** — All listed invariants and existing replay/order-ticket behavior remain intact; per-event work stays bounded. Evidence: full ordered loop, guards, focused replay/ticket coverage, benchmark, independent reviews. → PR verification/reports. (R4)
- [x] **A5** — Measurement files and score skills remain byte-identical; child makes no success-score claim and returns final blind scoring to coordinator. Evidence: base diff path inspection and campaign handoff. → PR report. (R5)
- [ ] **G1** — Full ordered fmt/clippy/build/test and exact-head CI pass. Authority: CLAUDE.md/ship. → PR checks and validation report.
- [x] **G2** — Source-first independent completeness preflight passes before production edits. Authority: delivery.md. → this ledger review record.
- [ ] **G3** — Performance evidence and applicable UI evidence accompany the review; architecture review resolves blockers/should-fix. Authority: mission step 4. → PR reports.
<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Closing steps

- C1: Archive mission, publish draft PR against campaign/outside-eight linked to #474, obtain current architecture/AI/delivery PASS with durable reports. Authority: mission/ship; evidence on PR.
- C2: Exact-head green CI, ready state and mission_ship_gate PASS. Authority: ship; evidence on PR.
- C3: Return reviewed child to coordinator for serialized authorized campaign integration. Main merge is trader-only; GitHub settings and campaign checkpoints remain parent-owned. Authority: original user grant/integration.md; evidence parent #472.
- C4: Parent owns fresh-context blind A+ gates 4/5, outside score >=8.0 in every dimension on main, quantick-score non-regression and other-model-family reproduction if claiming 9.0. Neither this child nor PR claims count as success evidence. Authority: original request; evidence parent #472.

## Non-applicable gates

No new capability/provider: no new-extension registration ceremony. No engine implementation: engine-specific test-first golden update is unnecessary. No Python bridge mutation planned: bridge checks become mandatory if touched. No financial action or new mouse control. Scoring is intentionally not performed by the code author.

## Original user request (verbatim, attributed)

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

User follow-up, verbatim attributed quotation:

> esta permitido criar muitos pr em paralelo para nao esperar outros ficarem pronto

## Delegated request (verbatim, attributed to coordinator)

> Execute child F1 issue #474, high-tier mission, under campaign #472. Own only feed-binance/feed-mt5/feed and necessary tests/docs for truthful automatic reconnect and MT5 in-connection gaps. Independent concurrent R1 owns control/mcp/app gateway. Root coordinator owns GitHub parent checkpoints, Project, serialized merges. Follow new-task/mission/ship and required reviews; read full canonical skill instructions in your isolated worktree. Create branch fix/feed-gap-honesty at origin/campaign/outside-eight, worktree C:/src/quantick-worktrees/fix-feed-gap-honesty, arm guards and cargo check relevant crate before editing. mission-base authority URL https://github.com/milocaetano/quantick/issues/472 (D1-D2 retained user grant), campaign context. No merge or parent checkpoint writes. First create mission ledger retaining FULL original user request and bounded child task source, send source/map and plan to root for independent source-first completeness review BEFORE production edits; root reviews while you inspect fixtures. Decisions already settled: D1 user requires defect FIX, no trader questions; D2 retain continuity across auto reconnect without fabricated trades, label unknown loss honestly; D3 no financial rule or public-contract weakening, use existing FeedGap/health path; D4 known initial attach/sequence reset/duplicate behavior must remain truthful; D5 no score/measure/rubric/score-skill changes. Any remaining defaults resolve with evidence; tell root rather than asking trader. Complete code/tests/full loop/commit/draft PR and required fresh reviews through green ready if possible; do not merge. Report worktree, head, PR and evidence. Shared host: avoid concurrent full workspace build with other child; coordinate cargo lock/build scheduling via root. Do not score campaign yourself. Strongest model inherited as campaign boundary/hot path routing requires.

Coordinator scope extension, verbatim:

> Approved as necessary scope under F1: own additive FeedEvent continuity/gap plumbing through app tab/feed.rs, minimal tab state, feed health/control projection and corresponding tests. Existing manual-reconnect-only gap path must become honest end-to-end. Preserve public contracts additively and distinguish known missing counts from unknown reconnect interval. Send exact additional paths to R1 directly; both avoid overlap. Root preflight will reconcile your forthcoming persisted map before production code. Please inspect replay consumption and any recordings honestly: do not silently drop new diagnostic event in supported observer/replay path.

## Pre-edit evidence

Base eb7bb039. Guards built successfully and cargo check -p quantick-feed-binance -p quantick-feed-mt5 -p quantick-feed --all-targets passed before first edit. Coordinator independently reconciled retained sources before map revision 1 and issued preflight PASS for SHA256 AAB084A01543D92B2BC1C2448BD49A75D2D869180A6B00BFA588AAA0616EAEBF before production edits. Binance already retains the stream tracker across reconnect; the defect is that its result is discarded.

The app pre-edit check started before app edits but observed the already-added FeedEvent variant and failed E0004 at its unfinished consumer. The coordinator accepted this bounded chronology exception under the user's delegated defaults; this is not a pristine app baseline PASS. After wiring, cargo check -p quantick-app --all-targets passed in 45.73 seconds. Product, final verification, review and scoring requirements remain intact.

## Implementation evidence before final freeze

See docs/workflow/evidence/feed-gap-honesty.md. The live replay-v1 exporter is a separate broker-history process, not a FeedEvent recorder; existing issue #226 owns live anomaly persistence. No completeness claim is made for that format. Coordinator approved this bounded scope after inspecting that evidence.

Coordinator pre-freeze feedback found a timestamp-watermark issue and a test-seam coverage issue. Both were repaired before formal reviews: late lower IDs cannot move the next gap's start, and the Binance WebSocket fixture now invokes the real host with private loopback REST/WS transport inputs. Production endpoint defaults remain unchanged. This is not a final architecture or score verdict.

## Draft handoff evidence

Local ordered fmt/clippy/build/test passed at tree 606946fc4f33a4e5afc0399454e92fa27e2a66c7; app 2068 passed, zero failed, 11 ignored, all other workspace suites passed. Process-local QUANTICK_BUBBLES used the tracked fixture rather than inherited trader settings. Cargo deny and guards passed. Detailed command, source/toolchain/config identities, original failures and paired performance results are recorded in docs/workflow/evidence/feed-gap-honesty.md.

A1-A3 local proofs are the production-host Binance reconnect socket fixture, MT5 quote-only/reset/reorder and host socket fixtures, and confirmed_feed_loss_reaches_gap_and_health_snapshots_without_changing_trades. A5 is proven by the declared-base file diff, which touches no frozen score/rubric/measurement files. The enlarged Binance benchmark measured approximately +0.49%, retaining all earlier noisy observations. No trader input or score assessment was requested from this child.

A4/G1/G3 and review/closing gates remain open pending coordinator-owned independent visual/architecture/AI/delivery reviews, exact-head CI and readiness reconciliation. This archive is a phase-1 draft handoff, not mission completion. Final evidence/archive changes are prose only and reuse the named runtime tree after guards, diff hygiene and full delta inspection.

Before publication, a late coordinator visual finding exposed subsecond captions truncating to zero and overlap with the book-sync overlay. It arrived immediately after the initial local archive commit; no push or PR had occurred. The bounded repair preserves exact gap duration and moves its caption above the lower footer, with source/paint-shape regressions passing. It is covered by existing R3/A3 and R4/A4/G3, not a new requirement. The repaired runtime's fresh ordered loop, updated executable and repeat visual capture remain pending; the earlier loop is historical evidence until that repair is revalidated.

The caption repair's ordered loop passed at tree 73f4e4f6e58b9e41dfab8093a7c909ad32597c48, with a fresh executable retained for independent capture. Before publication the coordinator integrated R1 at campaign tip 05bc95ecfa36339be75411b251edf67cffa79992 and directed a clean rebase. The resulting changed-base full validation and repeat visual review remain due. All earlier outputs and initial visual findings remain retained.

The clean rebase produced head 4c394f63dc8fba5e68e221f3d680feb2e20bb8b3. Its changed-base full ordered loop passed at tree a8cebfcffb26dc8a384bcc840895c8b2854c17e2 (3910 passed, zero failed, 21 ignored). Independent source review found only F1-AR1, a Should-fix requiring the renderer's footer-clearance tuning value at documented module scope. Attempt 1 names that constant without changing geometry; its fresh full loop and same-reviewer delta verdict remain pending alongside coordinator-owned pixel checks F1-V1/F1-V2. No source-map change or requirement deferral was made.

F1-AR1 attempt 1's fresh ordered loop passed at tree 5dd1f20b76e3d5076375b49ab98776b4496994ac, 17:27:16 to 17:29:30 on 2026-09-14 (America/Sao_Paulo): all four commands exited zero, 3910 tests passed, zero failed, 21 ignored. The final archive/evidence-only delta reuses that input-bound runtime proof after guards and full record-delta inspection; base 05bc95ecfa36339be75411b251edf67cffa79992, toolchain, dependencies and tracked fixture are unchanged. Independent delta review, pixel observations, final-head CI and closing gates remain open. This is a phase-1 handoff, not mission completion.
