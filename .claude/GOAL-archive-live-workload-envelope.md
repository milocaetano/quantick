# Bound the live queues and retained history to a published workload envelope

Publish the supported live workload (trade rate, depth rate, session length,
retained history per pane) as data the code reads, bound the two worker command
queues to it with a policy that never loses a trade and whose overflow is
observable, measure retained state and backlog inside and above it, and record
the measurements as a maintained artifact — so that SE7 moves from 2/5 to an
earned 3/4 and A+ gate 6 can pass together with Q4.

**Tier:** high. Hot path (per-trade worker sends) and retained market state;
product-visible if retention changed; campaign child of #367 (Q3) under
`campaign/lean-a-plus`, issue #379. The coordinator set the tier; the work is
a transport policy change on the per-trade path plus measured evidence, which
earns the full shape pass, `code-review` at `medium` and `delivery-review`.

## Request ledger

| ID | Outcome | Source | Criteria |
| --- | --- | --- | --- |
| R1 | The envelope is data the code reads: named constants for sustained and burst trade rate, depth rate, session duration, retained history per pane, and the derived queue/retained caps, each with a one-line reason; `docs/quality/live-envelope.md` renders the same numbers, the measurement behind each and the reproducing command. Numbers from measurement of the repo's dense fixtures and the venues' bounds, "not from taste" | Issue scope §1, D1 | A1 |
| R2 | Worker command channels (`indicator_worker`, `orderflow_worker`) are bounded; the UI never blocks on a worker and no trade is lost: superseding commands coalesce, trade batches merge on backpressure and retry next frame; every coalesce/merge counted; an `APP_HEALTH_SUMMARY` field makes overflow observable; no new dependency | Issue scope §2, D2 | A2, A3 |
| R3 | Retained tape and history in `state.rs`: measure first under rate × duration; if bounded and acceptable inside the envelope, publish the envelope and prove no loss; any cap that evicts what the trader can see is a `human_decision` with exact numbers and proposed policy, never implemented here | Issue scope §2 second half and §4, D3 | A4, A7 |
| R4 | Measurements of retained state and backlog inside the envelope on the dense fixture and at a burst above it, as a maintained artifact (test or `cargo run` target with a documented command), raw output plus a table with host, SHA, command and numbers committed under `docs/quality/`; reuse the health summary machinery; nothing wall-clock reaches a headless crate | Issue scope §3, D5 | A5 |
| R5 | Every queue, journal, cache and retained history on the live path has a stated cap in code with a test that exercises the cap and observes the overflow behaviour | Issue A1 | A2, A6 |
| R6 | A burst test through the real workers at the sustained rate and at the burst rate for a stated duration asserts every trade reached the bars/projection (counts equal), overflow counters zero inside the envelope and non-zero with no loss above it; a `cargo test`, `#[ignore]` only if over ~30 s and then still executed with output committed | Issue A3, D4 | A3 |
| R7 | The assessor's observable condition is met and an independent reassessment of SE7 at the PR head records the earned score; the handoff carries an honest self-assessment against the anchors | Issue A4, handoff §8 | A8 |
| R8 | Merged into `campaign/lean-a-plus` through a PR whose base is exactly that branch, merge read back | Issue A5 | C2 (coordinator) |
| R9 | Performance: any change to the worker send path or `state.rs` measured before/after on the dense replay (`APP_HEALTH_SUMMARY` fps/frame_avg/frame_cpu, five interleaved runs each side, release); a regression outside noise is a Blocker; nothing per trade allocates more than today | D6, issue G4 | A6, G4 |
| R10 | "so that" — SE7 earns 3/4 with the evidence each anchor needs and gate 6 is unblocked together with Q4 | Mission preamble | A8 |

## Decisions (coordinator, D1–D9 of the request)

- D1 envelope as code + doc, numbers from measurement and venue bounds.
- D2 bounded channels, coalesce/merge policy, health counter and summary field, `std::sync::mpsc::sync_channel`, no new dependency.
- D3 retained history: measure; no product-visible cap; `human_decision` with numbers.
- D4 burst test through real workers, both rates, counts equal, counters zero inside / non-zero above.
- D5 measurement harness maintained, raw output and table under `docs/quality/`.
- D6 before/after frame timing on the dense replay, five interleaved runs each side, release.
- D7 tier `high` reviews; at most three step-0 rounds (the delivery contract's batch limit); remaining minor findings ship as PR follow-ups.
- D8 `gh pr ready` once via Bash after reviews, markers and green CI; never merge, never touch main, no rebase.
- D9 size rule: no new or touched file over 1,500 production lines.

## Resumed work (previous executor, terminated by quota)

The first executor left uncommitted work; each file was read and judged
before any new edit (re-dispatch request, first paragraph):

- `crates/app/src/worker_backlog.rs` (new) — **kept**. The park/fold/never-drop
  buffer with its three tests is sound: order-preserving, folds only into the
  last parked command, drains oldest first, counts a disconnect's losses.
- `crates/app/src/worker_progress.rs` (+112/-11) — **kept and finished**. The
  bounded `ObservedSender` (parked buffer, `pump`, `parked`/`deferred`/
  `coalesced_parked` counts, acceptance recorded on channel entry) is sound;
  `bind` became test-only because both workers now fold.
- `crates/app/src/live_envelope.rs` (new) — **kept, numbers re-derived**. It
  cited a `tools/tape_rates.py` that did not exist; the script was written
  (`tools/live_envelope/tape_rates.py`), run, and every figure re-stated from
  its output (p99 224–286/s, max 1,882/s, max 267 per 16 ms frame).
- `crates/app/src/main.rs` (two `mod` lines) — **kept**.
- `crates/app/src/app.rs` (one field initializer) — **replaced**: the added
  line pushed the `QuantickApp` root one line past its extension-roots
  ceiling, so the counters' initializer moved to `HealthCounters::new()`.
- `.claude/GOAL.md` — **kept and amended** (this section, D7, S6–S9, the
  verbatim request now quoting the re-dispatch).

## Assumptions

- S1 The depth-update rate has no measurement on this host: every recording is a trade tape with no book. The depth envelope is therefore derived from the venues' own bounds (Binance `depth@100ms`, the 8,192-event feed channel, the 2,048-per-frame drain budget) and labelled as derived, not measured. Safe: it is stated as such in code and doc, which is what data honesty asks; measuring a live book is a separate session on a live terminal.
- S2 The sustained rate is the p99 one-second rate over the thirteen WINV26 sessions on this host (226–292 prints/s) and the burst rate their maximum one-second rate (1,849), rounded up; the per-frame burst is their maximum 17 ms window (264). Safe: these are the dense fixtures the request names, measured by a committed script.
- S3 The pending (parked) buffer that holds commands while a queue is full is bounded by the envelope, not by a hard cap: a hard cap would drop trades, which D2 forbids. Its length is observable. Safe: D2 chose merge-on-backpressure over loss.
- S4 The retry point for parked commands is the worker handle's own per-frame read (`drain_events`, `published`, `published_base_grouping`) plus every later send, so no file outside this mission's ownership needs a new call. Safe: those reads already run every frame; a pane that stops reading has also stopped drawing.
- S5 A measurement of RSS needs a platform API this workspace does not depend on; retained bytes are computed from element size and vector capacity and labelled as computed. Safe: no new dependency (D2's rule applied generally), and the figure is reproducible.
- S6 The envelope figures on the health line are window-wide: queue figures sum over every pane of every tab, `retained_trades` is the largest pane's tape, and `live_rate` classes the window's measured ingest against one pane's envelope (it can only overstate a pane's rate). Safe: an overflow in a background tab is an overflow all the same; the fields sit beside the existing `live_trades`.
- S7 The burst test and the harness live in a sibling module `live_envelope_tests`, not inside `live_envelope`: the workers read the envelope's caps, so tests inside it that drive the workers would form a module cycle the guard rejects. Safe: placement only.
- S8 The book fold rule folds only `Project` (layout); `ApplyVisualConfig` keeps its place because applying one can prune retained history that the next would not have. The indicator rule folds adjacent forming-bar updates (run appended), same-slot `SetInputs`, and replay-from-scratch pairs. Safe: each fold is what the batch loop would have produced from the pair; tests pin every pair.
- S9 The harness reads the process working set through the platform's own command (`powershell Get-Process` / `ps`) and labels it as OS-reported; no dependency is added. Safe: measurement-only, `#[ignore]`d.

## Acceptance criteria

- [x] **A1** — `crates/app/src/live_envelope.rs` declares the envelope constants (sustained and burst trades/s, burst trades per frame, depth updates/s and burst, session hours, retained sessions, retained trades per pane, the two queue capacities) each with a one-line reason and derivation; `docs/quality/live-envelope.md` renders the same numbers, the measurement behind each and the command that reproduces it; a test pins doc and code to the same numbers.
      *Evidence:* the module, the doc, `cargo test -p quantick-app live_envelope`.
      → `crates/app/src/live_envelope.rs`, `docs/quality/live-envelope.md`. *(R1)*
- [x] **A2** — both worker command channels are `sync_channel`s sized from the envelope; a full queue parks the command on the UI side, superseding commands coalesce (`PartialUpdated` merges its run, `SetInputs` per slot and `Project` latest-wins), nothing is dropped; `ProgressSnapshot` carries `parked`, `deferred` and `coalesced_parked`; `APP_HEALTH_SUMMARY` carries `worker_backlog`, `worker_parked`, `worker_deferred`, `worker_coalesced`, `retained_trades` and `live_rate`, and a `LIVE_QUEUE_DEFERRED` warning names every new overflow; a test exercises a full queue and observes the counters.
      *Evidence:* named tests in `worker_backlog`, `worker_progress::tests::admission`, the two workers' `fold_tests` and `app::health::envelope`; the summary field list in `health.rs`.
      → `crates/app/src/worker_backlog.rs`, `crates/app/src/worker_progress.rs`, `crates/app/src/app/health.rs`. *(R2, R5)*
- [x] **A3** — the burst test drives the real `IndicatorWorker` and `BookWorker` through `ChartState` at the sustained rate and at the burst rate for stated durations: every trade reaches the bars and the projection (counts equal, independently computed CVD), `deferred == 0` and `parked == 0` inside the envelope; above it (a parked worker, more than a queue's worth of commands) the counters become non-zero and, once released, every trade is still accounted for.
      *Evidence:* `cargo test -p quantick-app live_envelope_tests::burst` output committed.
      → `crates/app/src/live_envelope_tests/burst.rs`, `docs/quality/live-envelope/burst-test.txt`. *(R2, R6)*
- [x] **A4** — retained state measured under the envelope (rate × duration, two sessions): trades, bars and computed bytes per pane, per-trade ingest cost at the start and the end of the session; the health line reports `retained_trades` and a `LIVE_ENVELOPE_EXCEEDED` warning is emitted at summary cadence when a pane holds more than the envelope; no trade or bar is evicted.
      *Evidence:* measurement output and table; the warning's test.
      → `docs/quality/live-envelope.md`, `docs/quality/live-envelope/measure.txt`. *(R3)*
- [x] **A5** — the measurement harness is an `#[ignore]` test with a documented `--ignored` command, run on this host at the branch head; raw output and a table with host, SHA, command, queue depth over time, retained state and per-frame cost committed under `docs/quality/live-envelope/`.
      *Evidence:* the files and the command in the doc.
      → `docs/quality/live-envelope/`. *(R4)*
- [x] **A6** — before/after frame timing on the WINV26 2026-08-25 dense replay at speed 60, release builds of the campaign base and this head, five interleaved runs each side, `APP_HEALTH_SUMMARY` fps / frame_avg / frame_cpu, with `worker_deferred == 0` throughout on the head; no regression outside noise; every touched path classified by rate.
      *Evidence:* the table in the PR body and in the doc; the per-path rate table.
      → PR body, `docs/quality/live-envelope.md`. *(R9, R5)*
- [ ] **A7** — no cap that evicts visible tape or bars is implemented; the exact measured numbers and a proposed policy are written as a `human_decision` in the handoff and the PR body.
      *Evidence:* the PR body section and the handoff block.
      → PR body. *(R3)*
- [ ] **A8** — the handoff block carries an honest SE7 self-assessment against the anchors (3, 4 or 5) naming the evidence each anchor needs and what this PR supplies; the assessor's observable condition (published budgets with retained-state/backlog/overflow observations inside them) is met by A1–A5.
      *Evidence:* the handoff block.
      → handoff. *(R7, R10)*

### Injected gates

- [x] **G1** — every artifact English; conventional commits with the required trailers. Source: `CLAUDE.md`. Evidence: `git log`, `arch-review` dimension 8.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each on its own, green before every commit; CI green at the final head. Source: `CLAUDE.md`, issue G2. Evidence: exit codes in the PR body, CI run URL.
- [ ] **G3** — `arch-review` over `origin/campaign/lean-a-plus...HEAD` with `code-review` at `medium` (step 0), every Blocker/Should-fix resolved or deferred in the PR body; `ai-review` completion recorded; `delivery-review` in full; markers under the shared campaign key. Source: issue G3, D7, integration contract. Evidence: review verdicts and marker files.
- [x] **G4** — performance impact declared per touched path by rate (per-trade / per-depth / per-frame / rare); no per-trade, per-depth or per-frame path grows with session length; hot-path evidence per A6. Source: issue G4, `mission` table. Evidence: the rate table in the PR body.
- [x] **G5** — `cargo test -p quantick-guards` green after every batch; no file over 1,500 production lines; `size-baseline.txt` untouched. Source: `CLAUDE.md`, D9. Evidence: guard output.
- [x] **G6** — nothing wall-clock or non-deterministic reaches a headless crate; the new modules live in `app`. Source: `CLAUDE.md` architecture. Evidence: `headless.rs` guard, the diff's file list.
- [x] **G7** — every write lands in the mission worktree; siblings' files (`crates/orderflow/src/projection*`, `config*`, `crates/app/src/control/*`, `crates/app/src/app/tests/*`) untouched. Source: request §Worktree. Evidence: `git diff --name-only`.

### Not applicable

- *Touches anything user-visible* — no UI surface changes; the health line and a warning event are log output. No `ui-harness` hook, `visual-qa` or `trader-ux-review`.
- *Adds a capability / something a trader does* — no new feed, layer, panel or action; the envelope module is data plus a transport policy.
- *Engine / determinism territory* — the engine is not edited; the burst test uses fixture trades and independently computed expectations as the golden.

### Closing steps

- C1 — `delivery-review` returns PASS after `arch-review` and `ai-review`; markers recorded under the campaign key.
- C2 — draft PR on `campaign/lean-a-plus`, CI green at the head, `gh pr ready` once; the merge and its readback are the coordinator's (R8).

## Request as received (verbatim, attributed quotation from the campaign coordinator's re-dispatch)

> You are executing campaign child mission **Q3** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: bound the live queues and retained market history to a published workload envelope, with observable overflow and measured evidence, earning rubric criterion SE7 (currently 2/5) and unblocking A+ gate 6 together with a later mission Q4. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision becomes a `human_decision` in your handoff, never a guess, and never a silent product change.
>
> **A previous executor started this mission and was terminated by a quota limit.** It left uncommitted work in the worktree: modified `crates/app/src/app.rs`, `crates/app/src/main.rs`, `crates/app/src/worker_progress.rs` (+112/-11), new untracked `crates/app/src/live_envelope.rs` and `crates/app/src/worker_backlog.rs`, and a `.claude/GOAL.md`. Nothing is committed. Before anything else, read that work (`git status`, `git diff`, the two new files, the GOAL.md) and decide per file whether it is a sound start you keep and finish, or something you discard (`git checkout -- <file>` / delete the untracked file). Record the decision in GOAL.md. Do not trust it blindly; do not throw away good work either.
>
> ## Read first, in this order
> 1. `C:\src\quantick\CLAUDE.md` (determinism; data honesty: inferred, dropped or incomplete data is labelled, never silently patched; `feed` owns runtimes and clocks; everything below `app` is headless; size ratchet: no file over 1,500 production lines; the campaign is retiring every signed entry, do not add one).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign child at tier `high`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`, guards armed); still run `cargo check -p quantick-app --all-targets` before the first new edit.
> 3. `C:\src\quantick\docs\campaign\integration.md` (base `origin/campaign/lean-a-plus`, PR base exactly `campaign/lean-a-plus`, review key from `sh .claude/hooks/campaign_context.sh key "$WT"`), `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #379 (`gh issue view 379`): scope, A1..A5, gates, and the ledger text it quotes (SE7 blocker: `indicator_worker.rs` and `orderflow_worker.rs` unbounded `std::sync::mpsc` command channels; `state.rs` retained `trades` grows with no cap; no stated supported rate/duration/history envelope).
> 5. `C:\src\quantick\docs\quality\quantick-score-rubric.md` (SE6, SE7, gate 6; anchors: 3 = implemented on the normal path with direct evidence; 4 = enforced or tested including a failure/boundary path; 5 = reproducible at the target revision, maintained against drift, demonstrated on the next realistic variant).
> 6. Existing evidence and owners: `docs/quality/incremental-lane-evidence.md`, `docs/quality/final-review-corrections-evidence.md`, `crates/app/src/indicator_worker.rs`, `crates/app/src/orderflow_worker.rs`, `crates/app/src/state.rs`, `crates/app/src/worker_progress/`, `crates/app/src/app/health.rs` and `app/health/worker_diagnostics.rs` (`APP_HEALTH_SUMMARY`, `APP_WORKER_PROGRESS`), the venue-side bounded channels (`crates/feed/src/binance.rs` 4,096, `metatrader.rs`), `crates/control/src/limits.rs` (how limits are named as data today), issue #155 (context only).
>
> ## Worktree (the only place you write)
> - `C:\src\quantick-worktrees\feat-live-workload-envelope` (Git Bash `/c/src/quantick-worktrees/feat-live-workload-envelope`), branch `feat/live-workload-envelope`, cut from `origin/campaign/lean-a-plus` at `c1b4002e` (the tip has moved since; do not rebase, the coordinator does that at integration). Every command starts with `cd /c/src/quantick-worktrees/feat-live-workload-envelope &&`. Never write to `C:\src\quantick` or other worktrees.
> - Siblings in flight own `crates/orderflow/src/projection*` and `config*` (S8), `crates/app/src/control/*` (S5), and the control-plane test modules under `crates/app/src/app/tests/` (Q6). Do not edit those. You own `crates/app/src/indicator_worker*`, `orderflow_worker*`, `state*`, `worker_progress*`, `app/health*`, new files you create (the envelope module, a burst fixture, a measurement harness), `docs/quality/live-envelope.md` and its measurement artifacts, the app tests for those owners, and minimal registration lines (`mod`, a `main.rs` flag if the harness needs one). You may edit `crates/orderflow/src/history.rs` if strictly needed (S8 does not own it); keep that minimal and say so.
>
> ## Decisions
> - D1: the envelope is data the code reads, not prose: one module (for example `crates/app/src/live_envelope.rs`, or in `orderflow` if it must be headless-shared) declaring the supported live envelope as named constants: trade rate (sustained and burst trades/s), depth update rate, session duration, retained history per pane (trades and bars), with the derived caps for queue depth and retained state, each with a one-line reason. `docs/quality/live-envelope.md` renders the same numbers with the measurement that justified them and the command that reproduces it. Pick the numbers from measurement of dense fixtures already in the repo (WINV26 dense replay, `hot_path` bench inputs) and the venues' own bounds (Binance channel 4,096; B3 mini index bursts), not from taste; state each derivation.
> - D2: worker command channels become bounded. The UI thread must never block on a worker and no trade may be lost: coalesce superseding commands (a newer config/layout command replaces the queued older one) and merge-on-backpressure for trade batches (if the channel is full, the sender keeps the batch, appends the next batch to it, and retries next frame), with a health counter for every coalesce/merge and an `APP_HEALTH_SUMMARY` field so overflow is observable. Use `std::sync::mpsc::sync_channel` or the crate's existing channel type; no new dependency.
> - D3: retained history: measure first. Under the envelope (rate x duration) compute and measure the retained `trades` in `state.rs` and the orderflow history; if measured memory and per-frame cost inside the envelope are bounded and acceptable (state the bound), a hard cap is not required: publish the envelope and prove no loss within it. Any cap that evicts trades or bars the trader can see is a product decision: do NOT implement it; write the exact numbers and the proposed policy as a `human_decision` in your handoff and the PR body.
> - D4: the burst test runs at the envelope's sustained rate and at the burst rate for a stated duration through the real workers (not a mock), asserts every trade reached the bars/projection (counts equal), and asserts the overflow counters stay at zero within the envelope and become non-zero, with no loss, above it. It must be a `cargo test` (may be `#[ignore]` with a documented `--ignored` run if it takes more than ~30 s; if ignored, it is still executed and its output committed under `docs/quality/`).
> - D5: measurements: the harness is a maintained artifact (a test or `cargo run` target with a documented command) run on this host at your head; commit raw output and a short table under `docs/quality/` with host, SHA, command, and the numbers (queue depth over time, retained state, per-frame cost, RSS if available). Reuse the health summary machinery. Nothing wall-clock reaches a headless crate.
> - D6: performance: this is the hot path. Any change to the worker send path or `state.rs` is measured before/after with `APP_HEALTH_SUMMARY` fps/frame_avg/frame_cpu on the dense replay (the method of PR #388: five interleaved runs each side, release build); a regression outside noise is a Blocker. Nothing per trade may allocate more than today.
> - D7: tier `high`: `arch-review` with `code-review` at `medium`, full shape pass (hot path, headless, data honesty, English); `ai-review` completion; `delivery-review` in full. At most three step-0 rounds (the delivery contract's batch limit); remaining minor findings ship as PR follow-ups named in the PR body.
> - D8: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI. If it is denied because the campaign tip moved ("remote base advanced") or the PR is DIRTY, stop there and report; the coordinator rebases. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**
> - D9: the size rule applies to you: no new file over 1,500 production lines and no touched file crossing it; `cargo test -p quantick-guards` green after each batch.
>
> ## Environment notes
> - Use `python`, not `python3`. If `cargo fmt` is blocked, run `rustfmt` directly then `cargo fmt --all -- --check`. Run the four checks one at a time, never `||` or `| head`; read whole failures. Do not wait on background commands; foreground with an adequate timeout (a full workspace test is ~10 min).
> - App tests: `env -u QUANTICK_BUBBLES cargo test -p quantick-app ...`. Point every `QUANTICK_*` store variable at a scratch directory when launching the app for measurements (see `.claude/skills/ui-harness/SKILL.md`), so the trader's live workspace is never overwritten; clear them between runs; use `__COMPAT_LAYER=DPIUNAWARE` for captures.
> - Check `df -h /c` before the first build; if under 15 GB free, `cargo clean` only in your own worktree and report.
> - If writing review markers into the git dir is denied by the permission classifier, put the exact `printf` lines in your handoff, marked pending.
> - Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> - Known CI load flake: `layers_tests::the_trade_paint_layer_switch_stops_the_marks`; report, rerun the job once at most.
>
> ## Delivery
> 1. Implement with the verification loop (`cargo check -p quantick-app`, the worker/state targeted tests, the burst test, `cargo test -p quantick-guards`).
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately.
> 3. Archive `GOAL.md` per mission step 8 (slug `live-workload-envelope`) as the last commit before reviews.
> 4. Push; open a **draft** PR: `cd /c/src/quantick-worktrees/feat-live-workload-envelope && gh pr create --draft --base campaign/lean-a-plus --title "feat(app): bound live queues and retained history to a published workload envelope" --body-file -` with a heredoc body per the PR template: tier, "Campaign child of #367 (Q3); closes on integration: #379", the envelope table, the cap and overflow policy per queue, measurements before/after, burst-test results, any human_decision, local verification, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. `/arch-review`, `/ai-review`, `/delivery-review`; resolve Blockers/Should-fixes; record markers with the shared key per integration.md.
> 6. Watch CI at the head (`gh pr checks <n> --watch`, bounded).
> 7. `gh pr ready <n>` once (D8).
> 8. Return a HANDOFF BLOCK: issue; branch; worktree; PR URL; head SHA; base tip; what you kept or discarded from the previous executor; the envelope numbers; per-queue cap and policy; measurement table; burst results; frame timing before/after; an honest self-assessment of SE7 against the rubric anchors; review verdicts with URLs; markers yes/no; CI run URL and conclusion; findings closed/open; repair batches; ready accepted or denied; any human_decision; the coordinator's next action.
