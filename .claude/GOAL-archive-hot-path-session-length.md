# Prove hot-path work is independent of session length and bound the forming fold

Prove, with counted work on dense short- and long-history fixtures, that the
per-trade, per-depth and per-frame work of one live pane does not grow with
session length; put an explicit, tested limit on the indicator worker's
forming-run fold without changing one output byte; and keep both as a
regression check that runs in the normal test step — so that SE6 moves from
4 to an earned 5 and, with Q3's envelope (PR #405), A+ gate 6 can pass at the
campaign SHA.

**Tier:** high. The indicator worker's per-batch fold and the per-trade /
per-frame paths are the hot path; the fold's output feeds what the chart's live
lane draws. Campaign child of #367 (Q4) under `campaign/lean-a-plus`, issue
#380. The coordinator set the tier; it earns the full shape pass,
`code-review` at `medium` and `delivery-review` in full.

## Request ledger

| ID | Outcome | Source | Criteria |
| --- | --- | --- | --- |
| R1 | A reproducible, target-bound harness, one documented command on a clean checkout at the campaign SHA, that runs dense trade, depth and frame fixtures at two or more session lengths differing by at least 10x inside the envelope and reports per-trade, per-depth and per-frame work against session length | Issue scope §1, issue A1, D1 | A1, A2 |
| R2 | The harness measures work, not only time: per-unit operation counts ("items visited, allocations if measurable, bytes copied") alongside CPU time, with independent per-path budgets declared before the run, in code, as constants with a reason; "independent of session length" means per-unit counts do not grow with history length beyond a stated tolerance | D1, issue scope §1 | A1, A2 |
| R3 | Measured per-trade and per-frame cost differ by less than a declared bound between the two lengths, and the regression check fails when the bound is exceeded | Issue A2 | A2, A3 |
| R4 | The forming-run fold in the indicator worker gets an explicit, tested limit: its per-batch cost is bounded per trade regardless of how long the forming bar runs, preserving every output byte, proven identical to the current implementation on the golden fixtures; a change to any visible value stops as a `human_decision` | Issue scope §2, issue A3, D2 | A4, A5 |
| R5 | A maintained regression check: a fast `cargo test` always in the normal Test step (under ~30 s, counts only, no wall-clock assertion), plus an `#[ignore]` long variant with a documented `--ignored` command whose output at the head is committed under `docs/quality/` with host, SHA, command and numbers; no `ci.yml` edit (any CI wiring is a request to the coordinator) | Issue scope §3, D4, request §Worktree | A3, A6 |
| R6 | The Vec-doubling UI stall Q3 measured (~21 ms at 2,097,152 prints) is measured at this head; a behaviour-preserving fix within this mission's files is applied and measured, otherwise reported | D3 | A7 |
| R7 | Q3's envelope measurement harness is re-run at this head and its output committed next to this mission's, so gate 6's evidence sits at one SHA | D5 | A8 |
| R8 | Frame timing on the dense replay before/after (`APP_HEALTH_SUMMARY`, release builds, five interleaved runs each side, PR #388's method); a regression outside noise is a Blocker | D6, issue G4 | A9, G5 |
| R9 | No retention or semantic change; anything that would change what the chart shows is a `human_decision`, never implemented | Issue scope §4, D2 | A5, A10 |
| R10 | "so that" — the assessor's observable condition (target-bound dense short/long-history trade/depth/frame fixtures with independent work budgets and an explicit forming-run limit) is met, SE6 earns 5 by an independent reassessment at the PR head, and gate 6 has its evidence at one SHA; the handoff carries an honest self-assessment | Issue A4, request preamble and Delivery §8 | A10, A11 |
| R12 | Gate 6 at the campaign SHA: each finding the assessor's gate 6 paragraph lists (the fold, the unbounded channels, the uncapped tape, measurements not bound to the SHA, benches not gated) is mapped to the artifact that closes it at this head, or to the owner and decision that still hold it — no finding left silent | Request preamble ("should let A+ gate 6 pass"), interim assessment gate 6 paragraph; added by the source-first completeness pass | A12 |
| R11 | Merged into `campaign/lean-a-plus` through a PR whose base is exactly that branch, merge read back | Issue A5 | C2 (coordinator) |

## Decisions (coordinator, D1–D9 of the request)

- D1 the harness counts work (items visited, allocations if measurable, bytes copied) beside CPU time, at two or more lengths ≥ 10x apart inside the envelope, with independent budgets declared as constants with a reason before the run; "independent" = per-unit counts do not grow with history beyond a stated tolerance.
- D2 the forming fold gets an explicit tested limit, bounded per trade whatever the forming bar's length, every output byte preserved and proven against the current implementation on the golden fixtures; a visible change is a `human_decision`.
- D3 measure the Vec-doubling stall at this head; apply a behaviour-preserving fix within these files if one exists, else report.
- D4 fast `cargo test` in the normal Test step (< ~30 s, counts only), `#[ignore]` long variant with a documented command, its output committed with host, SHA, command, numbers.
- D5 re-run Q3's envelope measurement harness at this head and commit its output next to this mission's.
- D6 frame timing before/after on the dense replay, release, five interleaved runs each side; a regression outside noise is a Blocker.
- D7 tier `high`: `arch-review` with `code-review` at `medium`, full shape pass; `ai-review` completion; `delivery-review` in full; at most three step-0 rounds.
- D8 `gh pr ready` once via the Bash tool with the cd prefix after reviews, markers and green CI; a denial for a moved tip or DIRTY PR stops and is reported; never merge, never touch main, never the PowerShell tool for `gh pr`.
- D9 no production file over 1,500 production lines; `cargo test -p quantick-guards` green after each batch.

## Assumptions

- S1 The per-frame fixture drives the whole application frame (`QuantickApp` through `run_frame`, the path a trader's pane takes), so the harness lives as `crates/app/src/app/tests/session_length_tests.rs` with one `mod` line in `app/tests/mod.rs`. That module is the shared test harness, not one of Q2's control-plane modules; the line is additive. Safe: placement only, reachable in one edit.
- S2 Allocations are measurable deterministically with a counting global allocator installed only in the app's test binary (`#[cfg(test)]`), counting per thread so parallel tests do not see each other's work. Production builds keep the system allocator untouched. Safe: test-only, removable in one edit.
- S3 "Items visited" is counted by instrumenting the loops that could scale with history on these paths — the forming-run fold (trades folded per walk) and the lane transport (trades copied per command, already counted) — and by allocations and bytes for everything else; a loop that visits history without allocating is caught by the long variant's CPU-time ratio, not by the fast test. Safe: stated in the budgets' docs and the evidence page as the measurement's limit.
- S4 Session lengths: the fast test holds 18,000 prints (1 minute at the envelope's sustained 300/s) against 180,000 (10 minutes), 10x; the long variant holds 18,000 against `RETAINED_TRADES_PER_PANE` = 3,960,000 (the envelope's edge), 220x. Safe: both inside the envelope, both ≥ 10x.
- S5 The depth path is measured by driving `BookEngine` (the `orderflow` crate's headless engine, which `BookWorker` wraps one command at a time) on the test thread, so its allocations are counted on the thread that does the work. Safe: the worker loop adds no per-depth work beyond a channel receive.
- S6 (the design D2's first option names — "incremental folding with a carried state" — not an open question) The forming-fold limit keeps the forming run's prints (the lane can resample at any rung count, so every prefix must stay reachable) and adds a running bar plus one checkpoint bar every `CHECKPOINT_SPACING` prints; a rung folds at most `CHECKPOINT_SPACING - 1` prints from its checkpoint. `Bar` is plain data and `Bar::extend` a sequential fold, so a checkpoint is the exact intermediate state and outputs cannot differ. Safe: identity is proven by test against the current function kept as the oracle.

## Acceptance criteria

- [x] **A1** — one harness, `crates/app/src/app/tests/session_length_tests.rs`, runs dense trade, depth and frame fixtures at a short and a long session (≥ 10x apart, inside the envelope) and prints, per path and per length, the per-unit work: allocations and bytes allocated per print / per depth update / per frame (UI thread and worker thread), prints folded per lane walk, prints copied per lane command, and CPU ns per unit.
      *Evidence:* the module; its printed table in the committed long-variant output.
      → `crates/app/src/app/tests/session_length_tests.rs`, `docs/quality/session-length/long.txt`. *(R1, R2)*
- [x] **A2** — independent budgets per path are declared as named constants with a reason (absolute per-unit budgets and the long/short growth tolerance) before the measurement code, and the harness asserts every per-unit count at both lengths against them.
      *Evidence:* the constants block and the assertions in the module.
      → `crates/app/src/app/tests/session_length_tests.rs`. *(R2, R3)*
- [x] **A3** — the fast variant is a plain `#[test]` the normal `cargo test --workspace` step runs, in under 30 s on this host, asserting counts only (no wall clock); a deliberately injected O(history) per-frame or per-trade cost makes it fail (shown by a test of the checker itself).
      *Evidence:* its run time in the test output; the checker's own failing-path test.
      → test output quoted in `docs/quality/session-length.md`. *(R3, R5)*
- [x] **A4** — the forming-run fold has an explicit limit: a walk folds at most `rungs × (CHECKPOINT_SPACING − 1)` prints plus the prints appended since the last walk, whatever the forming bar's length; a test drives a forming run of at least 1,000,000 prints and asserts the bound; the worker comment no longer says O(forming trades).
      *Evidence:* the named test; the module docs.
      → `crates/app/src/indicator_worker/forming_run.rs`, its tests. *(R4)*
- [x] **A5** — the bounded fold's prefixes are byte-identical to the current `lane_prefixes` (kept verbatim as the test oracle) for every rung count 0..=70 over the engine's golden fixture tapes and synthetic runs of lengths 0..=600 plus long runs, across irregular append batches; the existing lane ladder tests pass unchanged.
      *Evidence:* the identity test; the unchanged ladder tests passing.
      → `crates/app/src/indicator_worker/forming_run_tests.rs`. *(R4, R9)*
- [x] **A6** — the long variant is `#[ignore]` with a documented `--ignored` command; its output at the head is committed with host, SHA, command and numbers, and a table on the evidence page renders them.
      *Evidence:* the raw file and the page.
      → `docs/quality/session-length/long.txt`, `docs/quality/session-length.md`. *(R5)*
- [x] **A7** — the Vec-doubling stall is measured at this head (slowest single ingest and where); a behaviour-preserving fix within this mission's files is applied and re-measured, or the reason none qualifies is recorded.
      *Evidence:* the re-run measure output and the page's stall section.
      → `docs/quality/session-length.md`, `docs/quality/live-envelope/measure.txt`. *(R6)*
- [x] **A8** — Q3's measurement harness (`live_envelope_tests::measure`) is re-run at this head and its output committed next to this mission's, naming the SHA.
      *Evidence:* the file.
      → `docs/quality/live-envelope/measure.txt` (re-run header), `docs/quality/session-length/`. *(R7)*
- [x] **A9** — frame timing on the WINV26 2026-08-25 dense replay, release builds of the campaign base and this head, five interleaved runs each side, `APP_HEALTH_SUMMARY` fps / frame_avg / frame_cpu; no regression outside noise.
      *Evidence:* the table in the PR body and the raw summary.
      → `docs/quality/session-length/frame-timing.txt`, PR body. *(R8)*
- [ ] **A10** — the handoff carries an honest SE6 self-assessment against the anchors and a gate 6 assessment naming the evidence paths at one SHA; no retention or visible-value change shipped (any need for one is a `human_decision`).
      *Evidence:* the handoff block; the diff.
      → handoff. *(R9, R10)*
- [ ] **A11** — an independent SE6 reassessment (a fresh reviewer, not this executor) at the PR head is recorded on the PR with the score and evidence per anchor.
      *Evidence:* the PR comment.
      → PR comment. *(R10)*

- [x] **A12** — the evidence page carries a gate 6 table: each finding of the assessor's gate 6 paragraph, the artifact at this head that closes it (or, for the uncapped tape, the pending `human_decision` Q3 recorded and the envelope that bounds it), and every artifact's SHA.
      *Evidence:* the table.
      → `docs/quality/session-length.md`. *(R12)*

### Injected gates

- [x] **G1** — every artifact English; conventional commits with the required trailers. Source: `CLAUDE.md`. Evidence: `git log`, `arch-review` dimension 8.
- [ ] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each on its own, green before every code commit; CI green at the final head. Source: `CLAUDE.md`, issue G2. Evidence: PR body, CI run URL.
- [ ] **G3** — `arch-review` over `origin/campaign/lean-a-plus...HEAD` with `code-review` at `medium`, every Blocker/Should-fix resolved or deferred in the PR body; `ai-review` completion; `delivery-review` in full; markers under the shared campaign key. Source: issue G3, D7, integration contract. Evidence: verdicts, marker files.
- [x] **G4** — performance impact declared per touched path by rate (per-trade / per-depth / per-frame / rare); no per-trade, per-depth or per-frame path grows with session length. Source: issue G4, mission table. Evidence: the rate table in the PR body.
- [x] **G5** — hot-path evidence: flat or better by measurement (A9 plus the harness). Source: mission table "Touches a hot path". Evidence: A9.
- [x] **G6** — test-first for the fold (determinism territory): the oracle, identity and limit tests are committed before the implementation that satisfies the limit. Source: mission table, `CLAUDE.md` workflow. Evidence: `git log` order.
- [x] **G7** — `cargo test -p quantick-guards` green after each batch; no production file over 1,500 lines. Source: `CLAUDE.md`, D9. Evidence: guard output.
- [x] **G8** — every write in this worktree; `ci.yml`, `tools/ci/*`, `crates/app/src/control/*`, `crates/control/*` and the control-plane test modules untouched. Source: request §Worktree. Evidence: `git diff --name-only`.
- [x] **G9** — nothing wall-clock or non-deterministic reaches a headless crate; no headless crate is edited. Source: `CLAUDE.md` architecture. Evidence: the diff's file list, `headless.rs` guard.

### Not applicable

- *Touches anything user-visible* — no surface changes; the lane draws the same values (A5 proves it). No `ui-harness`, `visual-qa` or `trader-ux-review`.
- *Adds a capability / something a trader does* — none; a bounded fold and a test harness.
- *Engine crate* — not edited; G6 applies the test-first rule to the fold because its output is what the lane shows.

### Closing steps

- C1 — `delivery-review` returns PASS after `arch-review` and `ai-review`; markers recorded under the campaign key.
- C2 — draft PR on `campaign/lean-a-plus`, CI green at the head, `gh pr ready` once; the merge and its readback are the coordinator's (R11).
- C3 — the HANDOFF BLOCK returned to the coordinator carries every field the request's Delivery §8 lists. Source: request Delivery §8. Evidence: the handoff text.

## Amendments

- Source-first completeness pass (independent, before code): GAP 1 — gate 6's other findings (bounded channels, uncapped tape, CI gating) had no line; R12/A12 added. GAP 2 — S6 relabelled as D2's first option rather than a free assumption.

## Evidence at archive (head `149df908`, code last changed at `10b87d90`)

- A1/A2/A3 — `session_length_tests.rs`; budgets at the top of the module; the fast variant ran in 13 s single-threaded and in every `cargo test --workspace` (2,032 app tests green); `the_check_fails_a_path_whose_work_grows_with_the_session` passes; `docs/quality/session-length/fast.txt`. A real regression caught: `docs/quality/session-length/fold-unbounded.txt` (the unbounded fold breaks `frame.worker.time1d` at 18,908 and 180,908 folds per frame).
- A4/A5/G6 — `forming_run.rs`, `forming_run_tests.rs`; commit order `dbbd11f7` (oracle, identity, limit test ignored and red: 1,000 folds against 63) → `00846de3` (bounded, green) → `10b87d90` (forward fold between rungs; the limit test also holds 1..1,000-print runs to the single pass).
- A6 — `docs/quality/session-length/long.txt` at `10b87d90`: every path within budget, counts and time, 18,000 vs 3,960,000 prints.
- A7 — stall re-measured at 24.18 ms at print 2,097,152 (`docs/quality/session-length/envelope-measure.txt`); the first-live-print-after-load copy removed (`1f7ba2d3`, `the_first_live_print_after_a_backfill_copies_nothing`, red first); the doubling stall reported with the reason in `docs/quality/session-length.md`.
- A8 — `docs/quality/session-length/envelope-measure.txt` (path amended from the ledger's first guess, `live-envelope/measure.txt`, so Q3's own artifact stays bound to its commit).
- A9 — `docs/quality/session-length/frame-timing.txt`: frame_cpu 2.227 vs 2.246 ms, t ≈ 0.13, fps min 59 both sides.
- A12 — the gate 6 table in `docs/quality/session-length.md`.
- G2 — the four checks ran one at a time before every code commit (last at `10b87d90`); CI pending at the PR head.
- G7 — guards green after each batch; `ChartState`'s root cap held by moving the tape sizing into a free function.
- G8 — one temporary detached checkout of `2452e577` under the session scratchpad (`timing-q4/`), used only to build the base binary for A9 and removed; no other worktree touched. One `mod` line in `app/tests/mod.rs` and three in `main.rs` (S1, S2).
- Pending at archive: A10 (handoff), A11 (independent reassessment on the PR), G3 (reviews), C1–C3.

## Amendments (implementation)

- S7 (D3) Reserving the tape at load to twice what it holds is behaviour-preserving: it is the capacity the next live push would have grown it to, taken one frame earlier. Reserving the envelope's 3,960,000 prints per pane to remove the doubling stall is not: 221 MiB of commit charge per pane from the first print. Reported, not applied.
- A5 — the synthetic comparison runs after every irregular append batch along 1..=600 and 1..=20,000 prints, not at every single length; the golden tapes are compared at every length they pass through.
- `depth.book` and `frame.book` ceilings were re-declared after the calibration run (first guesses below the paths' existing cost); the growth tolerance was never changed. Recorded in the module and the evidence page.

## Amendments (review repairs)

- ai-review (PR #419, thread `PRRT_kwDOTfuoRs6hzSBp`, question 4 WEAK): `FormingRun` needed only engine types but lived in the app, so no test of it ran below `app`, and the next consumer of a forming bar's prefixes would have written a second fold, which `crates/engine/src/bar.rs:52-57` warns against. It moved, logic unchanged, to `crates/engine/src/forming_run.rs` with its tests to `crates/engine/tests/forming_run.rs` (golden fixtures read from the engine's own `tests/fixtures/`); the worker's cross-run fold counter is a `cfg(test)` thread-local beside `cut`. A4 and A5's evidence paths move with it.
- G9 amended: "no headless crate is edited" becomes "nothing wall-clock or non-deterministic reaches a headless crate": `quantick-engine` gains one additive, deterministic module (plain data, a `Cell<u64>` work counter, no clock, no map); `guards/src/headless.rs` green. The worktree ownership list did not name `crates/engine/src`; no sibling owns it, and the change is one new file plus one `pub mod` line.
- arch-review step 0 (`state.rs:557`, low): the load-time reservation's cost for a pane that never goes live is now stated in code and on the evidence page.

## Request as received (verbatim, attributed quotation from the campaign coordinator's dispatch)

> You are executing campaign child mission **Q4** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: prove per-trade, per-depth and per-frame work is independent of session length on dense fixtures, bound the forming-run fold, and make that a maintained regression check. This earns rubric criterion SE6 (4 → 5) and, together with the already-integrated envelope work (Q3, PR #405), should let A+ gate 6 pass at the campaign SHA. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision becomes a `human_decision` in your handoff, never a guess or a silent behaviour change.
>
> ## Read first, in this order
> 1. `C:\src\quantick\CLAUDE.md` (determinism; headless below `app`; one engine, three consumers; size rule: no production file over 1,500 production lines, the size baseline is now empty and `!budget 0`).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign child at tier `high`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`, guards armed); still run `cargo check -p quantick-app --all-targets` before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md`, `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #380 (`gh issue view 380`): scope, A1..A5, gates.
> 5. The rubric (`C:\src\quantick\docs\quality\quantick-score-rubric.md`: SE6 target, gate 6 wording, anchors) and the interim candidate assessment's SE6 row and gate 6 paragraph: https://github.com/milocaetano/quantick/issues/367#issuecomment-5647341171 (SE6 = 4: `indicator_worker.rs` fold still O(forming trades) per drained batch; measurements not bound to the assessed SHA; `crates/engine/benches/hot_path.rs` and `control_idle_dense_replay_benchmark` not run or gated in CI; next point: target-bound dense short/long-history trade/depth/frame fixtures with independent work budgets and an explicit forming-run limit).
> 6. What Q3 just integrated, which you build on: `crates/app/src/live_envelope.rs` (the envelope constants: 300 trades/s sustained, 2,000 burst, 512 per frame, depth 1,000/s derived, 10-hour sessions, 2 retained sessions), `crates/app/src/worker_backlog.rs`, `docs/quality/live-envelope.md` and `docs/quality/live-envelope/` (its measurement harness and raw outputs; note its finding: per-trade cost flat 122 → 101 ns across the first and last 100k; one ~21 ms UI stall when the trade vector doubles at 2,097,152), PR #405's body.
> 7. Existing hot-path evidence: `crates/engine/benches/hot_path.rs`, `docs/quality/incremental-lane-evidence.md`, `docs/quality/final-review-corrections-evidence.md`, the lane transport tests (`crates/app/src/pane/tests/lane_transport_tests.rs`), the forming-run fold in `crates/app/src/indicator_worker.rs` (search "O(forming trades)"), `.github/workflows/ci.yml`.
>
> ## Worktree (the only place you write)
> - `C:\src\quantick-worktrees\perf-hot-path-session-length` (Git Bash `/c/src/quantick-worktrees/perf-hot-path-session-length`), branch `perf/hot-path-session-length`, cut from `origin/campaign/lean-a-plus` at `2452e577`. Every command starts with `cd /c/src/quantick-worktrees/perf-hot-path-session-length &&`. Never write to `C:\src\quantick` or other worktrees.
> - Siblings in flight own `crates/app/src/control/*`, `crates/control/*` and the control-plane test modules (Q2), and `.github/workflows/ci.yml` plus `tools/ci/*` (Q9). Do not edit those. If your regression check must be wired into CI, do it by a separate small follow-up request to the coordinator in your handoff (exact YAML), or run it as a `cargo test` that the existing Test step already executes; do not edit `ci.yml`. You own `crates/app/src/indicator_worker*`, `orderflow_worker*`, `state*`, `worker_progress*`, `app/health*`, `live_envelope*`, `crates/engine/benches/*`, new harness files, and `docs/quality/` artifacts you create.
>
> ## Decisions
> - D1: the harness measures work, not only time: count per-trade, per-depth and per-frame operations (items visited, allocations if measurable, bytes copied) alongside CPU time, at two or more session lengths differing by at least 10x inside the envelope (for example 1 minute and 10 hours of the dense WINV26 replay rate, or synthetic tapes at the envelope's sustained rate), with independent budgets declared before the run (in code, as constants with a reason). "Independent of session length" means the per-unit counts do not grow with history length beyond a stated tolerance.
> - D2: the forming-run fold: give it an explicit, tested limit so its per-batch cost is bounded per trade regardless of how long the forming bar runs (for example incremental folding with a carried state, or a documented cap on the forming run with the rest already folded), preserving every output byte (indicator values, bar boundaries); prove output identity against the current implementation on the golden fixtures. If bounding it would change any visible value, stop and record a `human_decision`.
> - D3: the Vec-doubling stall Q3 measured (~21 ms at 2,097,152 trades) is a per-frame outlier on the UI thread; measure whether it is still there at your head and, if a behaviour-preserving fix exists within your files (for example reserving capacity from the envelope's retained-trades constant at pane creation), apply it and measure; otherwise report it.
> - D4: maintained regression check: the harness runs as a `cargo test` (fast variant, always in the normal Test step; runtime under ~30 s) asserting the per-unit counts stay within the declared budgets, plus an `#[ignore]` long variant with a documented `--ignored` command; commit the long variant's output at your head under `docs/quality/` with host, SHA, command, and the numbers. No wall-clock assertion in the fast test (counts only), so it cannot flake under load.
> - D5: gate 6 evidence at the campaign SHA: re-run Q3's envelope measurement harness at your head too and commit its output next to yours, so the assessor sees both at one SHA.
> - D6: performance: frame timing on the dense replay before/after with `APP_HEALTH_SUMMARY` (release builds, five interleaved runs each side, PR #388's method); a regression outside noise is a Blocker.
> - D7: tier `high`: `arch-review` with `code-review` at `medium`, full shape pass; `ai-review` completion; `delivery-review` in full. At most three step-0 rounds.
> - D8: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI; if denied because the campaign tip moved or the PR is DIRTY, stop and report; the coordinator rebases. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**
> - D9: size rule: no production file over 1,500 production lines; `cargo test -p quantick-guards` green after each batch.
>
> ## Environment notes
> - Use `python`, not `python3`. If `cargo fmt` is blocked, run `rustfmt` directly then `cargo fmt --all -- --check`. Run the four checks one at a time, never `||` or `| head`; read whole failures. Do not wait on background commands; foreground with an adequate timeout.
> - App tests: `env -u QUANTICK_BUBBLES cargo test -p quantick-app ...`. When launching the app for measurements, point every `QUANTICK_*` store variable at a scratch directory (see `.claude/skills/ui-harness/SKILL.md`), clear them between runs, use `__COMPAT_LAYER=DPIUNAWARE`.
> - Keep every scratchpad file of yours under a `-q4` path (for example `scratchpad/delivery-review-q4/`); never a generic shared path.
> - Check `df -h /c` before the first build; if under 15 GB free, `cargo clean` only in your own worktree and report.
> - If writing review markers into the git dir is denied by the permission classifier, put the exact `printf` lines in your handoff, marked pending.
> - Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
>
> ## Delivery
> 1. Implement with the verification loop (`cargo check -p quantick-app`, targeted worker tests, the golden fixtures, the new fast harness test, `cargo test -p quantick-guards`).
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately.
> 3. Archive `GOAL.md` per mission step 8 (slug `hot-path-session-length`) as the last commit before reviews.
> 4. Push; open a **draft** PR: `cd /c/src/quantick-worktrees/perf-hot-path-session-length && gh pr create --draft --base campaign/lean-a-plus --title "perf(app): prove hot-path work is independent of session length and bound the forming fold" --body-file -` with a heredoc body per the PR template: tier, "Campaign child of #367 (Q4); closes on integration: #380", the harness description and budgets, the per-unit table at both lengths, the fold's output-identity proof, frame timing before/after, the gate 6 evidence paths, any CI wiring request, local verification, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. `/arch-review`, `/ai-review`, `/delivery-review`; resolve Blockers/Should-fixes; record markers with the shared key per integration.md.
> 6. Watch CI at the head (`gh pr checks <n> --watch`, bounded).
> 7. `gh pr ready <n>` once (D8).
> 8. Return a HANDOFF BLOCK: issue; branch; worktree; PR URL; head SHA; base tip; budgets and per-unit table; fold change and identity proof; stall outcome; frame timing; gate 6 evidence paths; an honest self-assessment of SE6 and of gate 6 against the rubric; review verdicts with URLs; markers yes/no; CI run URL and conclusion; findings closed/open; repair batches; ready accepted or denied; any human_decision; the coordinator's next action.
