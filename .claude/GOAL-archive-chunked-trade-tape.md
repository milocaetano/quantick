# Store the retained trade tape in fixed-size chunks so growth never stalls the UI thread

Store each pane's retained trade tape in fixed-size chunks so that appending a
print never copies the tape already held — removing the ~25 ms UI-thread stall
at each capacity doubling (2,097,152 prints and beyond) — while every consumer
of the tape produces byte-identical output, so that SE6 moves from 4 to an
earned 5 (campaign #367, D21).

**Tier:** high. The tape is written on every live print on the UI thread and
read per frame by the lane transport: the hot path, and retained state every
consumer shares. Campaign child of #367 (Q12) under `campaign/lean-a-plus`,
issue #423. The coordinator set the tier; it earns the full shape pass,
`code-review` at `medium` and `delivery-review` in full.

## Request ledger

| ID | Outcome | Source | Criteria |
| --- | --- | --- | --- |
| R1 | The retained trade tape is stored in fixed-size chunks (a justified number) behind a read API with length, index, range iteration, suffix since an index and the last N; an append allocates at most one new chunk and never copies existing trades | Request objective; D1; issue scope §1 | A1, A2 |
| R2 | The UI-thread stall at each capacity doubling disappears: the long measurement shows no ingest above a stated bound at the former doubling points 2^21 and 2^22, and per-trade cost is unchanged | Request objective ("removing the ~25 ms UI-thread stall"); D3(b); issue scope §2, issue A1 | A5, A6 |
| R3 | Every consumer's output is byte-identical to the `Vec` implementation on the golden fixtures and on Q4's session-length harness, with the old path kept as a test oracle where practical | Request objective ("byte-identical output for every consumer"); D2; issue A2 | A3, A4 |
| R4 | No consumer clones the whole tape; slice readers get an iterator or chunk-slice view; any contiguous copy is of the requested range only, and where it happens is said | D1 | A2, A4 |
| R5 | A counts-based fast regression test, no wall-clock assertion, fails if an append copies more than one chunk or if any read path's per-unit work grows with tape length (Q4's harness and budgets extended) | D3(a); issue scope §3, issue A1 | A2, A5 |
| R6 | Frame timing on the dense replay is within noise or better: release builds, five interleaved runs each side, PR #388's method | D3(c); issue A3 | A7 |
| R7 | The tape's resident size at the envelope's edge is reported before and after; the chunked form exceeds the `Vec` form by no more than one chunk plus bookkeeping per pane | D4; issue scope §2 ("memory within the envelope's figures") | A8 |
| R8 | Raw outputs are committed under `docs/quality/` with host, SHA and command | D3 | A5, A6, A7, A8 |
| R9 | No change visible to the trader | D21 ("no change visible to the trader"); issue context | A3, A4 |
| R10 | "so that" — SE6 4 → 5: the evidence and an honest self-assessment against the rubric anchors are handed to the coordinator, which runs the independent rescore at the head | Request objective; D5; issue A4 | A9 |
| R11 | Merged into `campaign/lean-a-plus` through a PR whose base is exactly that branch, merge read back | Issue A5 | C2, C3 (coordinator) |

## Decisions (coordinator, D1–D6 of the request)

- D1 a chunked tape type (fixed chunk size, justified) with the read API the consumers need (length, index, range iteration, suffix since index, last N); appending allocates at most one new chunk and never copies existing trades. Consumers that took `&[Trade]` get an iterator or chunk-slice view; none clones the whole tape; a contiguous copy is only of the requested range, said where.
- D2 output identity: every consumer byte-identical on the golden fixtures and Q4's harness; keep the old path as a test oracle where practical.
- D3 evidence: (a) a counts-based fast test failing on an append that copies more than one chunk or on read-path per-unit growth (Q4's harness and budgets extended, no wall clock); (b) the long variant at 3,960,000 prints shows no frame above a stated bound at the former doubling points (2^21, 2^22), per-trade cost unchanged; (c) frame timing on the dense replay within noise or better (release, five interleaved runs each side, PR #388's method). Raw outputs under `docs/quality/` with host, SHA, command.
- D4 memory: the tape's resident size at the envelope's edge before and after; chunked ≤ `Vec` + one chunk + bookkeeping per pane.
- D5 tier `high`: `arch-review` with `code-review` at `medium`, full shape pass; `ai-review` completion; `delivery-review` in full; at most three step-0 rounds; the coordinator runs the independent SE6 rescore, this mission states the evidence and a self-assessment.
- D6 the readiness command runs once via the Bash tool with the cd prefix after reviews, markers and green CI; a denial for a moved tip or a DIRTY PR stops and is reported; never merge, never touch main, never the PowerShell tool for PR commands.

## Assumptions

- S1 The tape type lives in `quantick-engine` as one additive file (`crates/engine/src/trade_tape.rs`, one `pub mod` line): it is headless, owns no clock, and the one reader outside the app — `quantick_feed::history_reach`'s campaign, which walks the tape backwards from an anchor — depends on the engine, not on the app. Safe: placement, and the engine already owns `Trade`.
- S2 The feed's reach helpers read the tape through a small positional read trait (`TradeSeq`: length and index, with a default binary search), implemented for `[Trade]`, `Vec<Trade>` and the tape, so their existing callers and tests keep passing slices and vectors unchanged. Safe: the functions' logic is unchanged; a test holds the tape and slice answers equal.
- S3 Chunk size 65,536 trades (3.5 MiB of 56-byte trades): a power of two, so an index is a shift and a mask; one chunk is under 2 % of the envelope's 222 MB tape, so the unused tail of the last chunk is a small bound on memory; at the sustained 300 prints/s a chunk fills in 3.6 minutes, so a pane opens one new chunk rarely; and the chunk directory at the envelope's edge is 61 entries. Safe: one constant, reversible in one edit, and D1 names this very number as the example.
- S4 Paging older history in (`prepend_history`, rare: once per history reply, followed by a full rebuild that is already O(tape)) copies the tape once into a fresh chunked tape rather than keeping a partial first chunk, so every chunk but the last stays full and indexing stays a shift and a mask. Its peak memory falls from two whole tapes to one tape plus one chunk, because each old chunk is freed as soon as it is copied. Safe: rare path, the copy existed before (`Vec::append` into a fresh vector).
- S5 The former doubling points 2^21 and 2^22: the envelope's edge (3,960,000) lies below 2^22 (4,194,304), so a dedicated ignored probe builds one pane's tape live to 2^22 + 65,536 prints to observe both points, at base and at head. Safe: measurement only.
- S7 The consumer survey (the brief's "lane transport, indicator worker, footprint, replay export and the control plane's reads"): every reader of `ChartState::trades()` is in A2's list. The indicator worker and the footprint ladders never read the tape — the worker receives the lane transport's copied suffix and folds it in its own bounded `FormingRun`, and the footprints are fed each print as it is ingested (and by the rebuild/refold walks, listed). The control plane reads the tape only through the health envelope's retained-print count (`len()`, listed). No replay export reads the tape: a replay is a feed source, exported by `tools/mt5/`, upstream of the chart. Safe: established by `grep` for `trades()` across the workspace at `eb60ed41`; the source-first completeness pass (F2) asked for it to be recorded.
- S6 The bars and footprint vectors still grow by doubling; they are not the tape and are not in this mission's scope. Their largest single copy is measured and reported beside the tape's so the remaining stall, if any, is named rather than hidden. Safe: reporting only; changing them would widen the mission.

## Performance impact (declared before code)

| Path | Rate | Thread | Change |
| --- | --- | --- | --- |
| `ChartState::ingest_live` → tape push | per-trade | UI | a push into the last chunk; every 65,536th opens a chunk (one allocation, no copy) |
| `LaneTransport::command` suffix copy | per-frame | UI | the same prints copied, chunk slice by chunk slice instead of one slice |
| `ingest_backfill`, `seed_from`, `rebuild`, `refold_footprints` | rare (load, split, spec change, footprint toggle) | UI | the same O(tape) walks through the tape's iterator |
| `prepend_history` | rare (history page) | UI | one O(tape) copy into a fresh tape, as before |
| `Campaign::advance`, `oldest_retained_trade_ms`, `empty_page_verdict` | rare (history reply) | UI | the same binary search and backward walk through the read trait |
| health envelope retained count | per publish | UI | `len()`, O(1) as before |

## Acceptance criteria

- [x] **A1** — `quantick_engine::trade_tape::TradeTape` stores trades in chunks of `CHUNK_TRADES` = 65,536, every chunk allocated at full capacity; it offers `len`, `is_empty`, indexing and `get`, `first`, `last`, `iter` and `range` (double-ended, exact-size), `slices` (chunk slices of a range), `since` (the suffix from an index), `last_n`, `partition_point`, `push`, `extend_from_slice` and `prepend`; a test pins that after appends across several chunk boundaries no previously held trade moved in memory, and that the tape answers every read like a `Vec` oracle.
      *Evidence:* the module; `cargo test -p quantick-engine --test trade_tape`.
      → `crates/engine/src/trade_tape.rs`, `crates/engine/tests/trade_tape.rs`. *(R1)*
      *Done:* `crates/engine/src/trade_tape.rs` at `3cc6a7db` (the newest chunk kept beside a directory of full ones, `d11d1b98`/`3cc6a7db`); `cargo test -p quantick-engine --test trade_tape` 9 passed, including `appending_never_moves_a_trade_already_held`, `the_tape_reserves_at_most_one_chunk_it_does_not_use` and `a_stretch_comes_back_as_one_slice_per_chunk`.
- [x] **A2** — `ChartState` holds a `TradeTape`; `trades()` returns `&TradeTape`; every reader (`LaneTransport::command`, `ChartPane::seed_from`, `Tab` history reads, the reach campaign, the health envelope, the rebuild and refold walks, the test harnesses) reads it without cloning the whole tape; the lane transport's per-frame copy is of the unsent suffix only, as before; a counts test with the counting allocator shows that building a tape live across several chunks never reallocates more than a chunk directory (far below one chunk).
      *Evidence:* the diff; `grep` for `to_vec`/`clone` of the tape; the named counts test.
      → `crates/app/src/state.rs`, `crates/app/src/indicator_worker.rs`, `crates/app/src/pane/series.rs`, `crates/app/src/tab/*.rs`, `crates/feed/src/history_reach.rs`, their tests. *(R1, R4, R5)*
      *Done:* every reader listed in `docs/quality/chunked-tape.md` § The consumers; no `to_vec`/clone of the tape (the lane transport copies its unsent suffix only). The tape itself moves no print (A1's address test); the counts test is `building_the_tape_live_never_copies_more_than_one_chunk` (whole ingest path bounded at one chunk; its largest copy, 245,760 B, is the bar vector, not the tape). The lane oracle lives in `crates/app/src/pane/tests/lane_transport_tests.rs`.
- [x] **A3** — every golden trade fixture in `crates/engine/tests/fixtures/`, alone and repeated to cross at least three chunk boundaries, gives byte-identical closed bars, forming bar, backfill boundary and footprint ladders through `ChartState` (backfill, live, mixed, prepended, seeded) against an oracle that folds a plain `Vec<Trade>` through the same builder; the lane transport's runs equal the old slice-based `command`, kept verbatim as the oracle; the reach helpers answer the same over a slice and over a tape.
      *Evidence:* the named identity tests.
      → `crates/app/src/state/tape_identity_tests.rs` (or beside `state.rs`), `crates/app/src/indicator_worker.rs` tests, `crates/feed/src/history_reach.rs` tests. *(R3, R9)*
      *Done:* `state::tape_identity_tests::every_way_in_shows_what_a_contiguous_tape_showed` (+ spec switch and refold on the long tape), `pane::lane_transport_tests::the_chunked_tape_sends_the_worker_what_the_slice_sent` (the oracle is in the pane tests, not `indicator_worker.rs`), `history_reach::tests::the_reach_reads_a_chunked_tape_as_it_read_a_slice`; green on the contiguous stage `33a1d75f` and unchanged on the chunked storage.
- [x] **A4** — Q4's session-length harness reports identical lane entries per unit and folds per unit before and after, and every other count within its budgets; the planted whole-tape clone still fails the checker.
      *Evidence:* the fast variant's output before (base) and after (head).
      → `docs/quality/session-length/fast.txt` (head) and the before/after table in `docs/quality/chunked-tape.md`. *(R3, R4, R9)*
      *Done:* folds and lane entries identical in `docs/quality/chunked-tape/fast-base.txt` / `fast-head.txt` and `long-base.txt` / `long-head.txt` (the raw files live under `docs/quality/chunked-tape/`, not `session-length/`); `the_check_fails_a_path_whose_work_grows_with_the_session` still passes.
- [x] **A5** — the fast variant also asserts that building the long session's tape live never makes a single reallocation copy larger than one chunk (`CHUNK_TRADES × size_of::<Trade>()`), counts only; the long variant's growth table at 3,960,000 prints reports the largest single copy while the tape was built, before and after.
      *Evidence:* the assertion in `session_length_tests.rs`; `fast.txt`, `long.txt`.
      → `crates/app/src/app/tests/session_length_tests.rs`, `docs/quality/session-length/fast.txt`, `docs/quality/session-length/long.txt`. *(R2, R5, R8)*
      *Done:* assertion `building_the_tape_live_never_copies_more_than_one_chunk` (+ `the_growth_check_fails_a_contiguous_tape`); largest copy building live: fast 7,340,032 -> 245,760 B, long 117,440,512 -> 7,864,320 B (the bar vector); raw files under `docs/quality/chunked-tape/`.
- [x] **A6** — an ignored probe builds one pane's tape live (tick:50, footprint off and on) to 2^22 + 65,536 prints and reports the slowest single ingest within ±1,024 prints of 2^21 and of 2^22, the slowest anywhere and the median ingest cost per print, run at base and head (release); at the head no ingest near either point exceeds the stated bound of 4 ms (a quarter of a 60 fps frame), and the median per-print cost is within noise of base.
      *Evidence:* the probe's raw output at base and head with host, SHA and command.
      → `docs/quality/chunked-tape/stall-base.txt`, `docs/quality/chunked-tape/stall-head.txt`, summarised in `docs/quality/chunked-tape.md`. *(R2, R8)*
      *Done:* head: slowest near 2^21 0.007/0.011 ms, near 2^22 0.012/0.064 ms (bound 4 ms), fourteen interleaved runs at most 0.119 ms; base 20.5-29.4 and 41.8-58.3 ms; median per print within noise (107.6 vs 106.6, 196.8 vs 191.8 ns), whole-run mean -20 % / -11 % — `stall-base.txt`, `stall-head.txt`, `probe-interleaved*.txt`.
- [x] **A7** — frame timing on the WINV26 2026-08-25 dense replay (release builds of the base `eb60ed41` and the head, five interleaved 45 s runs a side, `tools/live_envelope/run_replay.ps1` and `frame_timing.py`) is within noise or better: frame_cpu mean difference within two standard errors or lower, fps min unchanged, no `APP_SLOW_FRAMES` added.
      *Evidence:* the raw table.
      → `docs/quality/chunked-tape/frame-timing.txt`. *(R6, R8)*
      *Done:* set 2: frame_cpu 2.175 vs 2.202 ms, t 0.39 (within two standard errors), fps min 59 both, no slow-frame lines — `docs/quality/chunked-tape/frame-timing.txt`.
- [x] **A8** — the tape's resident bytes at the envelope's edge (3,960,000 prints) are reported before (`Vec` capacity, live-built and backfilled) and after (chunks × chunk bytes + directory), with the difference; the chunked form is ≤ the `Vec` form + one chunk + bookkeeping.
      *Evidence:* the probe output and the memory table.
      → `docs/quality/chunked-tape/stall-*.txt`, `docs/quality/chunked-tape.md`. *(R7, R8)*
      *Done:* Vec 234,881,024 B built live / 443,520,000 B loaded vs chunked 223,872,440 B both — `stall-contiguous.txt`, `stall-head.txt`, `chunked-tape.md` § Memory.
- [ ] **A9** — the handoff states the SE6 evidence and an honest self-assessment against the rubric anchors; the coordinator runs the independent rescore at the head.
      *Evidence:* the handoff block; the PR body.
      → PR body, handoff. *(R10)*

## Injected gates

- [ ] **G1** — every artifact in English (`CLAUDE.md`; `arch-review` dimension 8; `crates/guards/src/language.rs`). → review report.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace` each exit 0 on the final head, run separately; `cargo test -p quantick-guards` green after each batch. → PR body, local verification.
      *Done:* the four checks ran separately and green before every code commit, and again at the rebased head `8f5a2969` (onto `a3c962fa`); there the first workspace test run lost `control_plane_tests::attaching_a_script_and_detaching_it_leaves_the_pane_as_it_was` to host contention (listed as a known contention flake, #409, on `origin/ci/app-tests-under-contention`; it passed three times alone) and the full rerun was green; guards green. Final-head CI pending.
- [x] **G3** — performance impact declared (the table above) and the hot-path evidence measured before the PR: A5–A7. → PR body.
      *Done:* the declared table above; A5-A7 measured before the PR.
- [x] **G4** — engine/determinism territory is test-first: the tape's tests (oracle, address stability) are committed red (ignored) against a `Vec`-backed tape before the chunked implementation, in a separate commit. → `git log`.
      *Done:* `dafcaa90` (tests red/ignored over a contiguous tape) precedes `bcffcf2a` (chunked storage, ignores removed).
- [ ] **G5** — `arch-review` over `origin/campaign/lean-a-plus...HEAD` with `code-review` at `medium` as step 0 and the full shape pass (hot path, determinism, headless); every Blocker and Should-fix resolved or deferred in the PR body; at most three step-0 rounds. → review report on the PR.
- [ ] **G6** — `ai-review` completion recorded, zero unresolved `ai-review` threads. → PR.
- [ ] **G7** — CI green at the final head (`gh pr checks <n> --watch`). → CI run URL.
- [x] **G8** — size ratchet: no production file over 1,500 production lines, baseline empty, `!budget 0`. → `cargo test -p quantick-guards`.
      *Done:* `cargo test -p quantick-guards` green; `ChartState` root lines 429 -> 426 (tightened), shape amendment recorded.

## Not applicable

- *Touches anything user-visible* — the mission's contract is no visible change (D21); the chart's output is proven byte-identical by tests instead of screenshots, and no surface, hook or string is added or changed.
- *Adds a capability* — a storage type behind the existing `trades()` accessor is not a feed, bar type, indicator, layer, panel or crate.
- *Adds something a trader does* — nothing a trader does changes.
- *Docs/skills only* — this is a code change.
- *Scope fence (the brief's worktree section)* — the control-plane test modules and the evidence path belong to Q10 and `crates/mcp/*` to Q11; this mission edits neither. A2's file list is the whole of the code this mission touches beyond the engine's new file and its tests; a needed edit outside it would be a stated detour or a `human_decision`.

## Rebase

Before the archive the branch was rebased from `eb60ed41` onto `a3c962fa` (the campaign tip after #424: `crates/mcp/*`, a control-plane contract page and a mission archive only). Measurements name the commits as measured; `docs/quality/chunked-tape.md` maps each to its rebased ID, with `git diff --name-only` showing only those base files between them.

## Reconciliation

An independent source-first completeness pass (sonnet, read-only) reviewed the map at `eb60ed41` before code: no omitted outcome, no invented scope; two traceability notes — F1, the Q10/Q11 scope fence, recorded above; F2, the control plane's reads, recorded as S7 — both amended here.

## Closing steps

- **C1** — `delivery-review` returns PASS on the final branch.
- **C2** — the draft PR against `campaign/lean-a-plus` is open, reviewed, CI green, and readiness accepted once (or its denial reported).
- **C3** — the handoff block returns control to the coordinator, which merges into `campaign/lean-a-plus` and reads the merge back (issue A5).

## The request as received

Attributed quotation (the coordinator's brief for this child, verbatim), under
`CLAUDE.md`'s language exemption for a marked quotation:

> You are executing campaign child mission **Q12** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: store the retained trade tape in fixed-size chunks so appending never copies the existing tape, removing the ~25 ms UI-thread stall at each capacity doubling, with byte-identical output for every consumer. The goal is rubric criterion SE6 4 → 5. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision becomes a `human_decision` in your handoff, never a guess or a silent behaviour change.
>
> ## Read first, in this order
> 1. `C:\src\quantick\CLAUDE.md` (determinism; one engine, three consumers; size rule: no production file over 1,500 production lines, baseline empty, `!budget 0`).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign child at tier `high`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`, guards armed); still run `cargo check -p quantick-app --all-targets` before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md`, `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #423 (`gh issue view 423`): scope, A1..A5, and the trader's decision D21 (https://github.com/milocaetano/quantick/issues/367#issuecomment-5650222968).
> 5. What Q4 (PR #419) and Q3 (PR #405) built and measured, which you extend: `docs/quality/session-length.md`, `docs/quality/session-length/*.txt` (the stall at 2,097,152 prints, 24.79 ms), `docs/quality/live-envelope.md`, the session-length harness `crates/app/src/app/tests/session_length_tests.rs`, the work meter and test allocator registered in `crates/app/src/main.rs`, `crates/engine/src/forming_run.rs`, `crates/orderflow/src/history.rs`'s newest-timestamp index. Then find the tape's owner: the retained trade `Vec` in `crates/app/src/state.rs` and/or `crates/app/src/tab/*` (grep for where live trades are pushed and where they are sliced for the lane transport, the indicator worker, footprint, replay export and the control plane's reads).
>
> ## Worktree (the only place you write)
> - `C:\src\quantick-worktrees\perf-chunked-trade-tape` (Git Bash `/c/src/quantick-worktrees/perf-chunked-trade-tape`), branch `perf/chunked-trade-tape`, cut from `origin/campaign/lean-a-plus` at `eb60ed41`. Every command starts with `cd /c/src/quantick-worktrees/perf-chunked-trade-tape &&`. Never write to `C:\src\quantick` or other worktrees.
> - Siblings in flight own the control-plane test modules and the evidence path (Q10) and `crates/mcp/*` (Q11). You own the tape's owner module(s) under `crates/app/src/state*`, `crates/app/src/tab/*`, the readers you must adapt, a new chunked-tape type (in `crates/app` or, if it is headless and reusable, `crates/engine` or `crates/orderflow` as one additive file), its tests, and `docs/quality/` artifacts you create.
>
> ## Decisions
> - D1: a chunked tape type (fixed chunk size, justify the number: for example 65,536 trades per chunk) with the read API the consumers need (length, index, range iteration, suffix since index, last N); appending allocates at most one new chunk and never copies existing trades. Consumers that took `&[Trade]` slices get an iterator or chunk-slice view; no consumer may clone the whole tape. If a consumer truly needs a contiguous slice (for example a third-party API), copy only the requested range and say where.
> - D2: output identity: every consumer's output on the golden fixtures and on Q4's session-length harness is byte-identical to the current `Vec` implementation; keep the old path as a test oracle where practical (the way Q4 kept `lane_prefixes`).
> - D3: evidence for SE6 5: (a) a counts-based fast test that fails if an append copies more than one chunk or if any read path's per-unit work grows with tape length (extend Q4's harness and budgets; no wall-clock assertion in the fast test); (b) the long variant at the envelope's 3,960,000 prints shows no frame above a stated bound at the former doubling points (2^21, 2^22) and per-trade cost unchanged; (c) frame timing on the dense replay within noise or better (release, five interleaved runs each side, PR #388's method). Commit raw outputs under `docs/quality/` with host, SHA and command.
> - D4: memory: report the tape's resident size at the envelope's edge before and after; the chunked form must not exceed the `Vec` form by more than one chunk plus bookkeeping per pane.
> - D5: tier `high`: `arch-review` with `code-review` at `medium`, full shape pass (hot path, determinism, headless if you add to a headless crate); `ai-review` completion; `delivery-review` in full. At most three step-0 rounds. At the end, get an independent read-only SE6 reassessment at your head (dispatch nothing yourself: state the evidence and your honest self-assessment against the rubric anchors; the coordinator runs the independent rescore).
> - D6: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI; if denied because the campaign tip moved or the PR is DIRTY, stop and report; the coordinator rebases. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**
>
> ## Environment notes
> - Use `python`, not `python3`. Run the four checks one at a time, never `||` or `| head`; read whole failures. Foreground commands only, with adequate timeouts.
> - App tests: `env -u QUANTICK_BUBBLES cargo test -p quantick-app ...`. When launching the app for measurements, point every `QUANTICK_*` store variable at a scratch directory (`.claude/skills/ui-harness/SKILL.md`), clear them between runs, use `__COMPAT_LAYER=DPIUNAWARE`.
> - Keep every scratchpad file of yours under a `-q12` path.
> - Check `df -h /c` before the first build; if under 15 GB free, `cargo clean` only in your own worktree and report.
> - If writing review markers into the git dir is denied by the permission classifier, put the exact `printf` lines in your handoff, marked pending.
> - Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> - Known CI load flakes are listed in `tools/ci/contention-known-issues.txt` on `origin/ci/app-tests-under-contention` (not yet merged); if one fails your CI, report and rerun the job once at most.
>
> ## Delivery
> 1. Test-first where the engine or a headless crate is touched (commit the failing test before the code, as Q4 did); verification loop between edits.
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately.
> 3. Archive `GOAL.md` per mission step 8 (slug `chunked-trade-tape`) as the last commit before reviews.
> 4. Push; open a **draft** PR: `cd /c/src/quantick-worktrees/perf-chunked-trade-tape && gh pr create --draft --base campaign/lean-a-plus --title "perf(app): store the retained trade tape in chunks so growth never stalls the UI thread" --body-file -` with a heredoc body per the PR template: tier, "Campaign child of #367 (Q12); closes on integration: #423", the chunk design, consumer changes, identity proof, the counts and long-variant tables, frame timing, memory, local verification, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. `/arch-review`, `/ai-review`, `/delivery-review`; resolve Blockers/Should-fixes; record markers with the shared key per integration.md.
> 6. Watch CI at the head (`gh pr checks <n> --watch`, bounded).
> 7. `gh pr ready <n>` once (D6).
> 8. Return a HANDOFF BLOCK: issue; branch; worktree; PR URL; head SHA; base tip; chunk size and API; consumers changed; identity proof; counts and long-variant tables; stall outcome at 2^21 and 2^22; frame timing; memory; honest SE6 self-assessment against the anchors; review verdicts with URLs; markers yes/no; CI run URL and conclusion; findings closed/open; ready accepted or denied; any human_decision; the coordinator's next action.
