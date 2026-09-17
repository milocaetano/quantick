# Unfinished mission snapshot

Preserved for evaluation only. Unchecked criteria and historical failures remain; this is not a completion archive.

# F2 mission: disclose startup handoff and reconnect uncertainty

## Objective and why

Repair the existing Binance and Hyperliquid live-feed handoff paths so every
source fact that can establish a gap, an unknown outage, or an excluded row is
delivered in order through the provider-neutral feed host and reaches the
normal application integrity projection. Prove the repair first with literal
loopback HTTP/WebSocket fixtures and independently specified expected event
sequences. Never fabricate trades, source IDs, missing counts, time endpoints,
or replay provenance.

The current campaign tree starts Binance live tracking after discarding the
last REST-backfill watermark, and Hyperliquid separates coalescing connection
status from trade batches while dropping empty and all-excluded mapped
batches. Those inspected seams can hide uncertainty, but they are not by
themselves a claim that a critical defect has already been reproduced.

**Tier:** `high`. This changes market-data integrity at source/host and
per-trade boundaries, adds an ordered concrete source event path while
preserving public compatibility, reaches an existing user-visible gap/health
surface, and therefore requires source-first completeness, test-first real
transport fixtures, hot-path evidence, current architecture/AI/delivery
reviews, exact-head CI, and the final verifier.

## Exact source and ownership identity

- Campaign base: `4b13b64a0f4556470cbf6b7a5e1c7f1b000d0f50`.
- Base tree: `43b6ed4dc79b37a5808202fca082d1ded5baec38`.
- Branch: `fix/feed-handoff-continuity`.
- Worktree: `C:/src/quantick-worktrees/fix-feed-handoff-continuity`.
- Owner: campaign executor `read_cost`; issue assignment names strongest
  `gpt-6-astra` for the high mission.
- Task source: https://github.com/milocaetano/quantick/issues/495.
- Campaign authority: https://github.com/milocaetano/quantick/issues/472.
- Reviewed dependency proof: issue #491 comment `5672935909`; campaign
  checkpoint `5672951233`; bootstrap journal `5672957371`; executor issue
  comment `5672958747`.
- Source-plan input, not implementation evidence or a verdict: issue #495
  comment `5672509748`.
- Retained replay limitation: https://github.com/milocaetano/quantick/issues/226.
- Operation attempts observed: `0`. Repair attempts observed: `0`.

Relevant source blobs at the exact base:

| Blob | Owner/path |
| --- | --- |
| `d53286e8bec4dc6c5006e3c4fd375a63d76c83be` | `crates/feed/src/lib.rs`: public `FeedEvent`/`FeedHandle` port |
| `020be7ea6eeadfc9bb8953cda25b91eccbeb073a` | `crates/feed/src/continuity.rs`: continuity and cumulative integrity |
| `1caa7bd1d5146b852e73d0447c5362204d3c36a0` | `crates/feed/src/continuity/tests.rs`: delivered F1 tests and current real host fixture |
| `04339795a55db3b03a1f68e99347772bfa49a212` | `crates/feed/src/binance.rs`: REST-to-live host seam |
| `4a6e9b75c152bf551d126b62c2e378d52b3145bf` | `crates/feed/src/hyperliquid.rs`: lower-source-to-host seam |
| `70989c4e189ebfc3580eb19d20e8eaaeb55c86cc` | `crates/feed-binance/src/lib.rs`: existing public Binance API |
| `6b01783abe332a67f2a64a222466a3feee9276a5` | `crates/feed-binance/src/backfill.rs`: real REST backfill owner |
| `e9634142d37e34c51606717c5bf851979a57ecf5` | `crates/feed-binance/src/stream.rs`: existing live source |
| `9aeb203fb4947d96237ca963895c61cfee752122` | `crates/feed-hyperliquid/src/lib.rs`: public source registration/reexports |
| `69f9e1731299b91fd0e9d7547797bfb9b9b25792` | `crates/feed-hyperliquid/src/stream.rs`: WebSocket session/reconnect owner |
| `2fdfd2a867a32b4972dd603cda1c55725696bcc0` | `crates/feed-hyperliquid/src/wire.rs`: existing `MappedBatch` ledger and mapper |
| `a68660a0f79063b3d0fabbad00c3bfbfb6c5a38a` | `crates/app/src/tab/feed.rs`: normal app projection |
| `981d1a8f4e9d998afa10a6d026f36f07f2ff17ab` | `crates/app/src/control/health.rs`: agent-readable integrity snapshot |
| `6b88a1353ab8bd353cd2543a1926d7e26e3a31fc` | `crates/app/src/app/tests/feeds_sources_tests.rs`: gap eviction and health proof |

Bootstrap evidence is a precondition, not acceptance evidence. The complete
guard receipt is in the worktree git dir as `precheck-guard-build.log`. The
first headless-check attempt remains explicitly `UNKNOWN` because its wrapper
discarded the live session ID and no terminal receipt survived; the partial
receipt and diagnostic remain in `precheck-headless-feed-crates.partial.log`
and `precheck-headless-feed-crates.diagnostic.log`. The one authorized labeled
attempt 2 passed at the unchanged base and is serialized in
`precheck-headless-feed-crates.attempt2.log`. No product source was edited
before these prechecks.

## Request ledger

| ID | Distinct source requirement | Classification | Maps to |
| --- | --- | --- | --- |
| **R1** | Establish deterministic real local HTTP/WebSocket/source/host fixtures with literal independent expected events and retain the pre-repair failures before repair. | Product evidence + operational constraint | A1, G1, G2 |
| **R2** | Preserve Binance's final REST-backfill watermark into live comparison; contiguous/overlap/equal-time cases stay truthful, factual jumps precede their trade, and empty/failed startup remains unknown. | Product outcome | A2 |
| **R3** | Carry Hyperliquid connection transitions and every mapped batch on an ordered bounded path that cannot be lost by watch coalescing; preserve public APIs, deduplication and error behavior; cover rapid/quiet reconnects, empty/all-overlap recovery, malformed/stale exclusions, and unknown loss. | Product outcome + compatibility constraint | A3, G3 |
| **R4** | Prove actual source/host continuity reaches normal app gap/integrity state and agent-readable health, preserving known versus unknown information and cumulative diagnosis after bounded mark eviction. | Product outcome | A4, G-UI1, G-UI2, G-UI3 |
| **R5** | Preserve delivered F1/R1 behavior, existing public contracts, deterministic one-engine and one-way/headless boundaries, frozen seven blobs, ratchets, English, tickets, replay behavior, tests, guards, reviews, authority and financial rules; do no silent repair or score gaming. | Constraint | A5, G4, G5, G6, G-AI1–G-AI4 |
| **R6** | Keep recorded-replay anomaly persistence under issue #226 explicitly unresolved; do not change replay-v1 here, and return a concrete executable blocker as a separately scoped finding if it prevents the parent gate. | Constraint + observable disclosure | A6 |
| **R7** | Complete current architecture/AI/high delivery review, exact-head CI and canonical ship proof before root-only campaign integration; provide positive executable evidence for a future fresh blind gate without author scoring. | Operational obligation | G6, G-AI1–G-AI4, C1–C4 |
| **R8** | Classify all touched paths by rate, add no per-trade allocation or lock, keep work bounded/saturating, and publish paired representative hot-path measurements when the host is quiet. | Performance constraint | A7 |

All source requirements are mapped. Product outcomes are A criteria, process
and quality obligations are G gates or C closing steps, and no closing step is
duplicated as an acceptance criterion.

## Decisions

- **D1 — Preserved campaign boundary.** Keep F1/R1 and all frozen-seven,
  financial, public-contract, replay, ticket, determinism, data-honesty,
  one-engine, dependency and ratchet invariants. F2 is a repair on the exact
  campaign base, not a reimplementation, score, baseline change, main action,
  or settings action.
- **D2 — Binance startup truth.** Seed the existing private host continuity
  tracker from the final backfilled `Trade` before moving the vector. A
  contiguous ID emits no anomaly; overlap/backward/equal ID never invents
  loss; a source-ID jump emits exact known missing-message continuity before
  the unchanged trade, including a zero-duration gap for equal timestamps.
  Empty or failed REST startup arms one unknown handoff which is emitted before
  the first live trade without a count or endpoints; silence alone emits no
  fabricated event. The existing live-only default remains compatible.
- **D3 — One bounded ordered Hyperliquid path.** Add a concrete lower-source
  event value carrying connection transitions and the existing `MappedBatch`,
  over bounded `tokio::mpsc`; await sends and stop on consumer close. Do not
  add a speculative trait, unbounded queue, lock, second mapper, or duplicate
  parser. Additive ordered functions are reexported from the existing lower
  crate; the existing public `run_trade_session` and
  `run_trades_with_reconnect` signatures remain as compatibility adapters with
  their present trade/watch behavior. The production host consumes the
  full-fidelity path. Only an acknowledged connected-to-disconnected edge
  yields one unknown outage; repeated failed attempts do not inflate it.
  Every batch, including empty/all-overlap/all-rejected, reaches the host.
  Exact malformed and stale source-row exclusions are observable without
  counting overlap duplicates as missing; no exclusion invents a gap endpoint.
- **D4 — Fixtures before code.** Before implementation, add loopback Binance
  HTTP+WebSocket and Hyperliquid WebSocket fixtures with literal frames and
  independently written ordered expectations. Add a normal app drain/control
  snapshot assertion over the actual host-produced events. Never fake an
  unknown value as zero or invent missing source data. The app must pass its
  own pre-edit baseline before any app source/test edit.
- **D5 — Replay limitation retained.** Issue #226 remains the sole owner of
  recorded anomaly persistence. F2 does not edit replay format or claim live
  health survives recording. Any executable parent-gate blocker is reported
  with its exact input and output for a separately scoped follow-up.
- **D6 — Authority and concurrency.** Root owns GitHub writes, markers,
  reviews, PR publication, readiness, campaign integration and readback.
  Trader alone owns main and settings. Destination is only
  `campaign/outside-eight`. Up to three disjoint implementation authors are
  authorized; F2 owns only the feed/source/app-test paths listed here and does
  not touch A1R evidence-resource or C2 CI/timing owners. No subagents are used.
- **D7 — Rate budgets.** Binance continuity observation is per-trade: scalar,
  bounded, saturating where arithmetic can overflow, zero allocation and no
  lock. Hyperliquid parsing/mapping is per-batch with the existing allocation
  ledger; the new transition/forwarding work adds no allocation per trade, no
  lock, no clone of `MappedBatch`, and bounded channel backpressure. App event
  drain is per-event and uses the existing bounded gap list/cumulative
  counters. Run paired real fixture benches for changed hot paths only when the
  host is quiet, using the exact campaign-base implementation as control and
  the candidate as treatment over identical fixed fixture events: three warmup
  runs, then 15 measured runs per side with alternating first-run order. Retain
  every raw sample and exact binary/input/host identity. The candidate passes
  only when median cost per event is at most 1.05x control and p95 is at most
  1.10x control. A median ratio from 0.95 through 1.05 is labelled flat/noise,
  below 0.95 better, and above 1.05 a regression; p95 above 1.10 also fails.
  If either side's coefficient of variation exceeds 5%, the run is invalid
  host noise rather than a pass and is rescheduled under the same unchanged
  protocol and thresholds. Never tune a threshold after seeing samples or
  invent timings.
- **D8 — Scheduling and proof ownership.** Headless F2 prechecks use this
  worktree's own target at one job while A1R owns the shared R1 target. No app
  build competes with A1R; app baseline is deferred until the coordinator
  grants a safe slot and remains mandatory before an app edit. Root performs
  the independent source-first preflight and all independent reviews. Existing
  source-plan prose is input only, never evidence that code works.

## Assumptions

- **S1** — The ordinary implementation defaults not fixed above may be chosen
  autonomously because the authenticated campaign request delegates every
  default except main and settings, and separately authorizes parallel PRs.
  This is safe only inside R1–R8 and the owned paths.
- **S2** — The existing provider-neutral `FeedEvent::Continuity`,
  `FeedContinuity`, `FeedIntegrity`, and health snapshot can carry exact known
  excluded-row counts and unknown outage counts without a breaking field or
  schema change. This is safe because the inspected public shape already
  distinguishes `Some(count)` from `None` and no time gap is required; source
  comments and tests must make the generalized source-row semantics explicit.
- **S3** — The existing `MappedBatch` remains the single Hyperliquid mapping
  ledger. This is safe because it already owns valid trades, duplicates,
  stale-row count and typed per-row errors and persists through reconnects;
  production changes to `wire.rs` are excluded unless a failing fixture proves
  the current ledger insufficient and root reconciles that scope before edit.
- **S4** — No new user action, permission, feed identity, config entry, crate,
  engine behavior, replay format, or financial behavior is required. The task
  repairs evidence delivery for already selectable feeds and reads through the
  existing observer health surface.

## Acceptance criteria

- [ ] **A1** — Test-first real local transport fixtures independently specify
  and reproduce the pre-repair Binance REST-to-live and Hyperliquid
  reconnect/exclusion event-order failures, with literal frames, expected
  sequences, exact base/input identity, and unmodified failing outputs retained
  before implementation.
  *Evidence:* named fixture tests and pre-repair raw logs.
  → `crates/feed/src/continuity/tests.rs`,
  `crates/feed-hyperliquid/src/stream.rs` or an owner-local integration test,
  and `.claude/evidence/feed-handoff-continuity/`. *(R1)*
- [ ] **A2** — The Binance host preserves the final backfill watermark: exact
  fixtures prove contiguous and overlap cases do not invent loss, an ID jump
  emits ordered exact continuity before the unchanged first live trade at both
  increasing and equal timestamps, and empty/failed backfill emits one
  countless/end-point-free unknown only when the first live trade arrives.
  *Evidence:* real loopback HTTP+WebSocket host tests plus focused tracker tests.
  → `crates/feed/src/continuity/tests.rs` and
  `.claude/evidence/feed-handoff-continuity/`. *(R2)*
- [ ] **A3** — Hyperliquid's bounded ordered source path delivers every
  acknowledged connection transition and every `MappedBatch` in source order;
  two rapid cycles and a quiet reconnect cannot coalesce, empty/all-overlap
  recovery still resolves, overlap remains deduplicated without being counted
  missing, malformed/stale rows are exact and observable, and unknown loss is
  never represented as known zero. Existing public functions compile and keep
  their legacy behavior.
  *Evidence:* literal local WebSocket tests, host ordering tests, public API
  compile/use tests, and unchanged live-smoke source.
  → `crates/feed-hyperliquid/src/stream.rs`,
  `crates/feed-hyperliquid/src/lib.rs`, `crates/feed/src/hyperliquid.rs`, and
  `.claude/evidence/feed-handoff-continuity/`. *(R3)*
- [ ] **A4** — Events produced by the real repaired source/host path drive the
  normal app drain: known Binance gaps retain factual bounds (including zero
  duration), Hyperliquid unknown/exclusion evidence updates the correct
  cumulative counters without fabricating a gap, agent-readable
  `health.summary` and `feed.status` expose known versus unknown values, and
  cumulative diagnosis survives eviction beyond `MAX_REMEMBERED_GAPS` while
  the trade tape stays unchanged.
  *Evidence:* compositional real-host event fixture plus normal app registry
  capture test and existing gap UI hook/readback.
  → `crates/app/src/app/tests/feeds_sources_tests.rs` and
  `.claude/evidence/feed-handoff-continuity/`. *(R4)*
- [ ] **A5** — Public API compatibility, all delivered F1/R1/MT5/control retry
  and idempotency/ticket/replay tests, dependency/headless/size/context/cycle/UI
  ratchets, English, financial and authority rules remain unchanged or
  stronger; the frozen seven blobs match their campaign manifest exactly.
  *Evidence:* affected tests, API call-site scan, guard report/diff, frozen-blob
  receipt, architecture report and full ordered validation.
  → `.claude/evidence/feed-handoff-continuity/` and the PR architecture report.
  *(R5)*
- [ ] **A6** — Delivery evidence explicitly says replay-v1 still does not
  persist these live anomaly events and links issue #226; if an executable
  check proves this prevents the parent gate, it records the concrete input and
  output as a separate follow-up finding without editing replay here.
  *Evidence:* limitation receipt and any actual blocker artifact.
  → `.claude/evidence/feed-handoff-continuity/replay-limitation.md` and the PR
  body. *(R6)*
- [ ] **A7** — Every changed path has its declared rate class; structural
  inspection proves no new per-trade allocation or lock and bounded/saturating
  work, while paired representative fixture measurements on a quiet host show
  the changed Binance and Hyperliquid paths flat or better within the
  predeclared budget, with raw control/candidate samples and exact identities.
  *Evidence:* structural diff receipt and paired benchmark raw output.
  → `.claude/evidence/feed-handoff-continuity/performance/`. *(R8)*

- [ ] **G1** — A strongest independent source-first completeness reviewer who
  did not author this GOAL reads the full retained source and this exact GOAL
  hash, reports every distinct request mapped with no omission, and publishes
  PASS before the first product or fixture edit.
  *Evidence:* root-owned durable preflight report and exact hash readback.
  → issue #495 and parent campaign checkpoint.
- [ ] **G2** — Before implementation, the A1 fixture files and literal
  expected events have exact retained hashes and are run on the current base;
  their raw pre-repair failures are retained unchanged. They may remain
  uncommitted while red so the repository's full-green-before-code-commit rule
  is preserved; the implementation commit includes them without weakening any
  expectation.
  *Evidence:* pre-implementation fixture/expectation hashes, raw pre-repair
  outputs, and the implementation commit diff/ancestry.
  → `.claude/evidence/feed-handoff-continuity/` and PR body.
- [ ] **G3** — New-extension review confirms the existing `FeedEvent` channel
  remains the provider-neutral docking port, the lower Hyperliquid addition is
  a concrete additive event path rather than a speculative trait, only the
  existing `lib.rs` reexport is registration, the production host and legacy
  compatibility adapter exercise both consumer shapes, defaults preserve
  current behavior, and blast radius is recorded.
  *Evidence:* call-site/API tests, diff inventory and architecture report.
  → `.claude/evidence/feed-handoff-continuity/extension.md` and PR report.
- [ ] **G4** — Worktree, mission, English, frozen seven, dependency, size,
  context, cycle, UI-free, ticket, replay, control, financial and authority
  guards pass without weakening; any Cargo.lock change additionally passes
  `cargo deny check bans licenses`.
  *Evidence:* raw guard/hook/affected-test outputs and frozen-blob manifest.
  → `.claude/evidence/feed-handoff-continuity/validation/`.
- [ ] **G5** — After the final source change and current campaign-base
  reconciliation, the ordered `cargo fmt --all -- --check`, `cargo clippy
  --workspace --all-targets`, `cargo build --workspace`, and `cargo test
  --workspace` loop passes, followed by every affected non-Cargo check, with
  full command/input/time/exit receipts from execution start.
  *Evidence:* current-tree raw logs and input manifest.
  → `.claude/evidence/feed-handoff-continuity/validation/` and PR body.
- [ ] **G6** — Current-head `arch-review` covers source ordering, data honesty,
  compatibility, dependency direction, rate budgets, app/control observability
  and all campaign invariants; every Blocker/Should-fix is resolved or durably
  deferred under the delivery contract, and the durable report is published.
  *Evidence:* current review projection, report and zero unresolved required
  findings.
  → PR report and review projection.
- [ ] **G-UI1** — The changed existing on-chart gap/health effect remains
  reachable through the established `QUANTICK_FEED_GAP` UI harness and normal
  agent-readable health path; no new mouse-only surface or action is added.
  *Evidence:* exact hook/source identity plus current functional UI harness run.
  → `.claude/evidence/feed-handoff-continuity/ui/` and PR body.
- [ ] **G-UI2** — `visual-qa` passes the affected existing gap/health surface,
  or every actual defect is explicitly accepted by authorized scope.
  *Evidence:* current visual QA report and screenshots when required.
  → PR report/evidence links.
- [ ] **G-UI3** — `trader-ux-review` has no unresolved Blocker for truthful
  known/unknown feed-integrity presentation.
  *Evidence:* current trader UX report and thread list.
  → PR report/evidence links.
- [ ] **G7** — Evidence and archive identify exact base/head/tree, dirty/index
  state, source blobs, caches, environment, command, start/finish, exit and raw
  output limitations; operation/repair counters and every actual failure remain
  preserved without transcript narrative being used as proof. The observed
  business/external operation and repair counters remain `0/0` unless such an
  operation actually occurs; they are distinct from precheck attempt 1
  (`UNKNOWN`) and authorized attempt 2 (`PASS`), both of which remain in the
  command receipt inventory.
  *Evidence:* committed evidence index, raw artifacts and mission archive.
  → `.claude/evidence/feed-handoff-continuity/` and
  `.claude/GOAL-archive-feed-handoff-continuity.md`.
- [ ] **G8** — Root-owned authority checks confirm the branch only targets
  `campaign/outside-eight`; no child writes main, settings, parent journals,
  review markers, scores or merge state, and no disjoint A1R/C2 product file is
  touched.
  *Evidence:* an incrementally retained task-scoped inventory of actual command
  and operation receipts from bootstrap through final handoff, exact
  diff/branch/base, and root-owned GitHub/campaign checkpoint readbacks; prose
  recollection is not proof of a negative operation claim.
  → issue #495 and parent campaign checkpoint.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Closing steps

- [ ] **C1** — A high-tier `delivery-review` reads the full retained source,
  current exact diff, criteria/gates and raw evidence, returns PASS with zero
  unledgered requests, and publishes the durable current-head report. *(R7)*
- [ ] **C2** — The one PR is open, non-draft, titled for issue #495, and matches
  branch `fix/feed-handoff-continuity`, the frozen reviewed head, and base
  `campaign/outside-eight`; every registered exact-head CI check is green and
  the PR body carries current evidence/report URLs. *(R7)*
- [ ] **C3** — `mission_ship_gate.sh mission <pr>` publishes and verifies the
  literal canonical reconciliation after current architecture, AI, delivery,
  thread and CI proof. *(R7)*
- [ ] **C4** — Only after C1–C3, root—not this child—performs the separately
  authorized history-preserving integration into `campaign/outside-eight` and
  reads back actor, source head, destination, merge SHA/tree, ancestry and
  campaign CI. This is a post-ship root obligation and is never fabricated as
  pre-merge delivery evidence. Main remains trader-only. *(R7)*

## Canonical definition of done

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

## N/A and bounded exclusions

- A new feed, config entry, capability registry item, trader action, permission,
  crate or polymorphic trait is N/A. F2 repairs two existing feeds through the
  existing provider-neutral `FeedEvent` port. A fake second trait
  implementation would create the speculative abstraction the task forbids;
  real local transports plus the full-fidelity host and compatibility adapter
  prove the concrete seam instead.
- Engine/bar golden changes are N/A because no aggregator, bar, simulation,
  strategy, paper or engine source is in scope. Deterministic fixture outputs
  are still mandatory for the feed behavior.
- A new UI surface/hook or second-operator action is N/A because F2 adds no
  control or action; it feeds the existing gap and observer-health surfaces.
  Because the effect is user-visible, the existing hook, visual QA and trader
  UX gates remain applicable as G-UI1–G-UI3.
- Replay-format changes are N/A and excluded by R6/#226. Ticket, control retry,
  idempotency, financial/trading access and credentials are regression-only.
- MT5 Python/ruff checks are N/A unless the corresponding paths move. Cargo
  deny is N/A unless Cargo.lock moves. Full workspace and relevant hook tests
  remain mandatory regardless.
- Main, GitHub settings, campaign scoring, rubric/measure/score-skill edits,
  deployment, spending, venue actions and the assessor's report are excluded.

## Verbatim source

### Authenticated parent request retained in issue #472

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

### Authenticated parallel-PR steering retained in issue #472

> esta permitido criar muitos pr em paralelo para nao esperar outros ficarem pronto

### Issue #495 body read live before mission creation

> <!-- campaign-task:milocaetano/quantick#472/F2 -->
> ## Context
>
> Parent: https://github.com/milocaetano/quantick/issues/472. Stable key: F2. Close source-confirmed feed-integrity coverage gaps discovered by interim B1 at campaign432632239f924883c07e76904551be0faf0f37eb. Report: https://github.com/milocaetano/quantick/issues/492#issuecomment-5672049477; coordinator source reconciliation: https://github.com/milocaetano/quantick/issues/492#issuecomment-5672070496.
>
> Binance feed_task publishes REST backfill and only afterward constructs default BinanceContinuity (crates/feed/src/binance.rs:100-128); the first live trade is not compared with the final backfill source ID. Hyperliquid's host forwards connected notices and trade batches without typed continuity events (crates/feed/src/hyperliquid.rs:127-163); the source reconnect loop publishes a coalescing bool watch signal, while malformed/stale mapped rows are logged rather than represented in the host integrity stream (feed-hyperliquid/src/stream.rs). These are inspected structural gaps, not a claim that an adversarial fixture has already established a reachable critical defect.
>
> The original F1 Binance reconnect and MT5 in-connection repairs remain delivered on the campaign. Do not reopen or erase them. B1 gate4 remained BLOCKED for incomplete verification and gate5 passed its bounded contract; neither constitutes final candidate/main acceptance. Score and rubric changes are out of scope.
>
> ## Scope
>
> - Add adversarial, deterministic local transport fixtures reproducing the initial Binance REST-to-live handoff and automatic Hyperliquid reconnect uncertainty through the real source/host path. Establish the failing behavior before repair.
> - Preserve a trustworthy Binance last-backfill source watermark into the first live comparison, including overlap, source-ID jumps and equal timestamps. Empty/failed backfill must not invent a known count or claim proven continuity.
> - Carry Hyperliquid reconnect uncertainty on an ordered, bounded event path that cannot disappear when status notifications coalesce. Distinguish unknown loss from a measured missing count; retain deduplication and recovery-overlap semantics. Surface excluded malformed/stale rows honestly without treating legitimate overlap duplicates as missing trades.
> - Prove continuity reaches the normal application gap/integrity projection and agent-readable health, including quiet reconnects and recovery batches with no new usable trade. Preserve valid time intervals and explicit unknown information; do not fabricate trades, source IDs, missing counts or time endpoints.
> - Preserve existing public API contracts via compatible additive ports/adapters where necessary. Follow new-extension for a new port and move tests with ownership. Preserve F1/R1 regression tests, ticket/replay behavior, one deterministic engine, one-way/headless boundaries, all ratchets, English and financial/authority rules.
> - Keep recorded-replay anomaly persistence explicitly tracked by existing issue #226; this child does not silently rewrite the replay format or claim that live disclosure alone fixes recorded provenance. If executable evidence shows that exclusion prevents the parent's gate, return the concrete finding for a separately scoped follow-up.
>
> Out of scope: trading access, credentials, external venue actions, changing financial rules, frozen rubrics/measure/score skills, full history-retention policy (#412), main merges or GitHub settings. Do not alter the assessor's report or award a score.
>
> ## Acceptance criteria
>
> - [ ] A1: Independent fixture trades/expected anomalies expose the existing Binance startup seam and Hyperliquid reconnect uncertainty through real local HTTP/WebSocket/host paths; retain pre-repair failures and exact inputs.
> - [ ] A2: Binance source watermark survives backfill-to-live transition. Contiguous and overlap cases do not invent loss; a factual source-ID jump emits ordered truthful continuity before its qualifying trade, including equal-timestamp input. Empty/failed backfill remains explicitly unknown where completeness cannot be established.
> - [ ] A3: Hyperliquid rapid disconnect/reconnect cannot lose the integrity transition through watch coalescing. Recovery overlap remains deduplicated, unknown loss never becomes zero/known, and rejected malformed/stale rows are observable with correct semantics. Tests include empty/all-overlap recovery and at least two rapid connection cycles.
> - [ ] A4: Normal app gap/health and agent-readable snapshot assert the actual source/host output and known-versus-unknown counts; bounded mark eviction preserves cumulative diagnosis. Existing MT5 in-connection and Binance automatic-reconnect, control retry/idempotency, ticket and replay regressions still pass.
> - [ ] A5: No silent data repair or public/financial/authority weakening; current frozen seven-blob manifest and guards remain exact/stronger. Disclose remaining recorded-replay provenance limitations under #226 instead of claiming they were fixed here.
> - [ ] A6: Full current architecture/AI/high-tier delivery review, exact-head green CI and canonical ship proof precede root-only integration into campaign/outside-eight. Positive executable evidence is available for a future fresh blind gate4 assessment; author claims do not determine that verdict.
>
> ## Campaign assignment
>
> Owner class: autonomous. Priority:1 safety verification. Tier:high (market data integrity, source/host port and per-trade behavior). Executor: strongest gpt-6-astra, to be dispatched after bootstrap/source-first mission preflight. Dependencies: F1 (#474) integrated_campaign_with_green_ci, and MS1 (#491) integrated_campaign_with_green_ci before implementation; no stacking on its unmerged PR494. Existing C2/A1R delivery obligations retain their scheduling priority; read-only planning may proceed independently.
>
> Planned branch: fix/feed-handoff-continuity in an isolated worktree from the then-current verified campaign base. Branch/worktree owner/PR/head/Project IDs remain null until observed and claimed. Native parent472 and blocked-by474/491; linked issue bodies/conditions remain the portable record. Operation/repair counters start0/0 and persist.
>
> Validation: source-first high mission reconciliation before code; lowest-layer and real local transport negative fixtures; app gap/snapshot plus existing safety tests; performance classification and representative budgets for touched hot paths; fresh ordered fmt/clippy/build/test before code commits plus affected non-Cargo checks; current exact-diff reviews and CI. No scoring builds or author numeric score. Evidence destinations: this issue, committed mission/validation artifacts, PR reports and parent checkpoints.

### Current delegated executor request

> Claim HIGH F2#495 bootstrap only, new worktree fix-feed-handoff-continuity / branchfix/feed-handoff-continuity from campaign4b13b64a0f4556470cbf6b7a5e1c7f1b000d0f50. Proof4915672935909; CP31parent5672951233 lease01:01; journal5672957371; executor4955672958747. Rootreadissuefull and duplicatechecknoexistingbranch/localremoteWT. Recheck beforecreation. Follownew-task/mission/new-extension/CLAUDE: ownguardbuild and relevant feed/feed-binance/feed-hyperliquid checks BEFORE firstmission/sourceedit, rawfullcommand/input/timestamp/exitpersisted fromstart. Own targetjobs1, neverR1leasedtoA1R. Appbaselinecheck laterwhenhostslotavailable, noappeditbeforecheck. Only bootstrap/sourceinspection/actualGOAL allowed until root independent source-first PASS. Use full retained originalparentrequest, issue495 source and thisdelegation, stableR/A/G/C andliteralAI/closingblocks; no author transcriptnarrative asproof. Existing sourceplan4955672509748 remains input, notimplementation evidence. D1preserveF1/R1/frozen7/financial/public/replayticket invariants; D2truthfulBinanceREST-livewatermark inclcontiguousoverlapjump/equaltimestamps/emptyfailedunknown; D3boundedorderedHLtransitionpath preservingexistingpublicAPIs/dedup/errorsemantics, actualquiet/empty/alloverlaprapidcycles and rejectedrows; D4realHTTPWS+normalapphealthfixtures literalexpected beforecode, nofakeunknownzero/noinventeddata; D5recordedreplay226limitation retained, reportifblocksparentgate; D6rootownsGH/markers/merges, onlycampaign destination, main/settings excluded; D7rateclasses+pairedrealbenchwhenhostquiet, no additionalpertickallocation/lock; D8up3disjointauthors A1R/C2/F2, ownheadlessprechecks whileA1Rfullsuite, no competingappbuild. Ordinarydefaults delegated,no userquestions/no subagents. ReturnactualGOALhash/fullmap/precheckreceipts and ownershipidentity beforeproductedits. Keep operation/repaircountersobserved0 and preserveeveryactualfailure.

### Durable precheck recovery authorization

> Recovery reconciled; stop broad transcript/temp searches. Root inspected original call Re99 and partial receipt, no original handle retained; original OS process tree absent on root readback too. This is a missing-handle/terminal-process evidence gap, NOT observation timeout or proof of failure/PASS. Journal https://github.com/milocaetano/quantick/issues/472#issuecomment-5673083800 authorizes exactly one labelled precheck attempt2, same clean4b13 base, own target jobs1, full result serialization and file receipts including session ID on first yield. Preserve original attempt1 UNKNOWN and diagnostic unchanged except appended outcome. After terminal PASS proceed mission-tier/GOAL then return for independent root source-first preflight before product changes. No further request needed; no R1 target/app build. Lease renewed same owner to01:22.
