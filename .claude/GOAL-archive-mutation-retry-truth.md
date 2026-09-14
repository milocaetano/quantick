# Make mutation retry advice truthful and expose MCP idempotency keys

**Tier:** high. Cross-thread cancellation and uncertain mutating transport results cross a public safety contract. Child R1, issue #475, campaign #472; branch fix/mutation-retry-truth, base origin/campaign/outside-eight (initial eb7bb039).

## Request ledger and source map, revision 1

- R1: Fix unkeyed mutation deadlines and transport loss after possible execution: source user "unkeyed timeouts and lost transports on mutating capabilities answer retryable:true after the action may have run"; delegated A1/A2/A4.
- R2: Carry and validate generic MCP idempotency keys: source user "quantick_invoke cannot carry an idempotency key"; delegated A3.
- R3: Preserve safe retries, distinguish cancellation and unknown execution, and keep every mutable family reconcilable: delegated A2/A4 and "oversized idempotency terminal results not retained: inspect and handle safely if reachable in this same retry contract".
- R4: Preserve authority and all invariants, guards, public/financial rules, order ticket and replay: user's entire invariant/no-regression paragraph; delegated A5 and no insecure reconnect identity.
- R5: Preserve frozen measurement and independent scoring; child supplies executable evidence for campaign gate 5 without self-scoring or claiming campaign/main completion. Source user's independence/freeze/score paragraphs; gate 4 feed repairs and overall score target are sibling/coordinator outcomes.

## Decisions and assumptions

- D1: Parent retained authenticated user grant delegates every default except main merge and GitHub settings; no user question qualifies. Root serializes campaign merges and parent checkpoints. This child never merges.
- D2: Mutations that may have executed and lose their transport are non-retryable even with a key: deduplication ends with the authenticated connection; #362 durable identity is separate.
- S1: Use atomic cancellation versus dispatch arbitration, so cancellation proves no subsequent execution. This is a reversible implementation of the requested distinction, not a new autonomy grant.
- S2: Preserve read-only retry advice using runtime descriptor metadata; unknown metadata is conservatively treated as potentially mutating. Do not infer safety from capability name prefixes.
- S3: A terminal result too large to retain needs a bounded non-retryable uncertainty record, preserving the reservation instead of permitting another execution.
- S4: All changed production paths are rare control requests, not per-trade, per-depth or per-frame market processing. Bounded maps/counters and atomic arbitration have negligible market-path cost; evidence will identify this classification.
- S5: The UI-free guard found 17 excess lines after retry-matrix documentation grew. Move pure response-envelope construction and atomic response-slot reservation, including its regression test, from app to the control-host dispatch owner. This preserves R4's ratchet with reusable headless machinery and no ceiling change.
- S6: Root identified bounded-store eviction before TTL as a reason a keyed timeout or in-flight duplicate cannot promise a future safe retry. Return non-retryable uncertainty for every started mutation and retain keyed replay while reserved/stored; prove cross-principal eviction in the real store. This strengthens R1/R3 without changing identity, limits or removing key support.

## Plan

1. Independent source-first review of this source/map by coordinator before production edits; fixtures inspected meanwhile.
2. Add atomic dispatch cancellation, truthful timeout/disconnection errors with reconciliation details, safe oversize terminal tombstones, and regression fixtures.
3. Carry optional validated idempotency_key through MCP schema, link and wire. Teach transport retry advice the running registry's read-only metadata, conservatively handling unknown calls.
4. Exercise genuine post-execution reply loss and held post-start replies through real transport; verify same-connection keyed replay/conflict/refusal and readback.
5. Update retry documentation/generated contract artifacts, run targeted checks and serialized ordered workspace validation, archive evidence, publish draft and obtain independent reviews, green CI/readiness and final verifier.

## Acceptance criteria

- [x] **A1**: Unkeyed mutable timeouts and lost replies after possible dispatch return retryable:false with explicit unknown outcome and usable readback. Evidence: real gateway/client regression tests, .claude/evidence/mutation-retry-truth/retry-readback.log and docs/workflow/evidence/mutation-retry-truth.md, linked from the PR. (R1)
- [x] **A2**: Generic quantick_invoke accepts an optional valid key, rejects malformed keys, forwards it unchanged to the authenticated runtime and enforces allowed/forbidden/conflicting keys. Evidence: MCP schema/tool and real gateway tests in .claude/evidence/mutation-retry-truth/targeted.log, reconciled in docs/workflow/evidence/mutation-retry-truth.md and the PR. (R2)
- [x] **A3**: Cancellation winning before dispatch prevents execution; reads remain safely retryable; same-connection keyed calls deduplicate; oversized terminal results cannot reopen execution. Every mutable family remains covered by retry/readback matrix. Evidence: focused tests, all 17 family/readback tests in .claude/evidence/mutation-retry-truth/retry-readback.log and docs/workflow/evidence/mutation-retry-truth.md, linked from the PR. (R3)
- [ ] **A4**: No permissions or identity widening, no dependency inversion/UI dependency below app, no weakened test/guard/contract/financial rule, no order-ticket/replay regression. Evidence: independent diff review, guards and workspace tests recorded in PR. (R4)
- [ ] **A5**: Frozen measurement files unchanged; report no score of this campaign. Supply code/test evidence to root for blind assessment. Evidence: final name/status diff and parent handoff; independently assessed scores remain parent obligations. (R5)

## Gates

Preflight evidence: coordinator independently read retained sources then reconciled map revision 1, PASS before production edits. Reviewed GOAL blob c02ac24d9a68b6b19854cbefa32bc376665163d0. It specifically checked partial-write uncertainty and conservative metadata failure. Guards build and MCP/control-host all-target check passed before edits; app all-target arming check passed before app production edits after correcting an intermediate client Debug derive. Historical failures and the inherited preset correction are retained in docs/workflow/evidence/mutation-retry-truth.md.

- [ ] G1: English artifacts; language and architecture dimension 8 pass. Source CLAUDE.md; evidence PR review and guard output.
- [ ] G2: Ordered fmt/clippy/build/test loop passes at frozen input, final-head CI green. Source CLAUDE.md and ship; evidence docs/workflow/evidence/mutation-retry-truth.md and PR checks.
- [ ] G3: Current independent architecture review resolves all required findings; performance classification verified. Source mission/arch-review; evidence PR report.
- [x] G4: Source-first completeness review before production edits, exact source/map revision recorded. Source delivery contract high tier; evidence this goal and docs/workflow/evidence/mutation-retry-truth.md, linked from the PR.
- [ ] G5: Authenticated campaign base and branch-private markers; no main/settings writes or child merges. Source parent D1-D2/integration; evidence branch/base/PR readback.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

AI gate evidence destinations: current PR durable AI report, thread listing and producer projection, linked in its body.

## Closing steps

- [ ] C1: Full independent delivery-review PASS published on current PR/key.
- [ ] C2: Open reviewed ready PR against campaign/outside-eight with exact-head green CI and evidence links.
- [ ] C3: Shared mission/ship verifier PASS, then return branch/head/PR/evidence to root for serialized integration.

## Non-applicable gates

No new capability/registry, money action or GUI surface: this fixes existing callable error/envelope behavior. Thus new-extension, visual-qa and trader-ux-review do not apply. No engine or market hot-path changes: engine test-first/performance benchmark gate does not apply; existing ticket/replay tests remain mandatory. Feed gap repairs and final independent scoring are explicitly delegated to sibling/coordinator, not waived campaign outcomes.

## Full verbatim user source

Attributed quotation, authenticated campaign request:
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

Attributed quotation, subsequent authenticated user instruction:
> esta permitido criar muitos pr em paralelo para nao esperar outros ficarem pronto

## Full verbatim delegated source

Attributed quotation, coordinator task:
> Execute child R1 issue #475, high-tier mission, under campaign #472. Own control/control-local/control-host/mcp and app control gateway tests/docs for truthful mutation uncertainty and quantick_invoke idempotency key. F1 separately owns feed crates. Root owns parent checkpoints/Project/serialized merges. Follow new-task/mission/ship and review skills fully. Create branch fix/mutation-retry-truth from origin/campaign/outside-eight, worktree C:/src/quantick-worktrees/fix-mutation-retry-truth; arm guards and cargo check relevant crate before edits. mission-base authority URL https://github.com/milocaetano/quantick/issues/472 (D1-D2 retained grant). No merges or parent checkpoint writes. First persist high mission ledger including full user source and child task, send source/map and proposed plan to root for independent source-first completeness review BEFORE production edits; inspect fixtures meanwhile. Decisions already settled: user demands fix, no trader questions; unkeyed mutation timeout/transport loss after possible dispatch is non-retryable unknown outcome with usable reconciliation; provably cancelled-before-dispatch distinct; safe keyed same-connection dedup retained; generic quantick_invoke accepts/passes optional idempotency_key; no unauthenticated reconnect identity workaround (#362 separate); public contracts strengthened compatibly, permissions/financial semantics not weakened; frozen rubrics measure.py both score skills untouched. Cover real wire post-execution response loss and post-start timeout. Baseline assessor also flagged oversized idempotency terminal results not retained: inspect and handle safely if reachable in this same retry contract; no optimistic retries. Finish implementation targeted/full loop, commits, draft PR, required independent reviews and green readiness; no merge. Coordinate full-workspace build host with root/F1 to avoid competing expensive builds. Never score own campaign. Report branch/head/PR/evidence.
