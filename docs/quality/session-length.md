# Hot-path work against session length

Whether one live pane's per-print, per-depth-update and per-frame work grows
as the session gets longer, measured by counting the work rather than timing
it, and kept as a regression check that runs in the ordinary test step. It is
the evidence for rubric criterion SE6 and, beside
[the live envelope](live-envelope.md), for A+ gate 6.

Measured on `DESKTOP-BTVJFFR` (Intel Core i5-12400F, 31.8 GB, Windows 11 Pro)
at `df40008e` of `perf/hot-path-session-length`, the last commit that changed
measured code (a later test-only commit moved the checker's planted cost out of
the lap clock; the measured paths plant nothing); each raw output below names its command and commit.

## The harness

`crates/app/src/app/tests/session_length_tests.rs` runs each path after a
*short* and a *long* session of dense prints at the envelope's sustained rate
(300 prints/s), then drives the same window of live work and divides what
that window cost by its units:

| Path | Unit | Thread | What runs |
| --- | --- | --- | --- |
| `trade.chart.backfilled` / `.live` | print | UI | `ChartState::ingest_live` on tick:50 with the footprint on, and one lane command per 5 prints, after a session loaded at once or built print by print |
| `trade.book` | print | book worker | `BookEngine::record_trade` |
| `depth.book` | depth update | book worker | `BookEngine::handle_depth_event_at` |
| `frame.book` | frame | book worker | `BookEngine::project_at` over the newest 120 bars, heatmap and bubbles on |
| `frame.app.tick50` / `.time1d` | frame of 5 prints | UI | the whole application frame (`run_frame`): feed drain, ingest, worker sends and reads, layout and paint, lane and CVD on |
| `frame.worker.tick50` / `.time1d` | frame of 5 prints | indicator worker | the same frames on the worker: the batch, the lane walk, the deltas |

`time:1d` keeps the whole session inside one forming bar, so its lane walk is
where an O(forming prints) fold shows; tick:50 is the trader's footprint chart.

**What is counted.** Heap work on the thread doing it — allocations, bytes
allocated, and bytes a reallocation may copy — through `crate::work_meter`, a
counting allocator compiled into the app's test binary only; prints folded by
the forming-run walk; prints copied to the worker by the lane transport; and
CPU time, the median lap per unit (a lap is one frame, print or depth update;
the timer's resolution on this host is 100 ns). Both sessions of a path are
built first and take their window in six alternating rounds — short, long,
long, short, … — so load on a shared host lands on both alike. Counting is
deterministic where a stopwatch is not, so the fast variant asserts counts
only and cannot flake under load. A loop that walks history *without
allocating* is invisible to the counts: the long variant's time ratio is what
bounds it, and it found one (below).

**Budgets**, declared as constants with their reasons at the top of the
module: an absolute ceiling per unit for every path, and a growth tolerance —
the long session may cost at most **10 %** more per unit than the short one,
plus a small per-path slack for timer-driven work that lands in one window and
not the other. Work that grows with history grows by the session ratio
(tenfold in the fast variant), so the tolerance separates the two. The long
variant also bounds CPU time per unit at **+50 %**. The ceilings for
`depth.book` and `frame.book` were first declared below the paths' existing
cost (8 allocations and 2 KiB per depth update, 2,000 allocations and 4 MiB
per projection) and re-declared at twice the calibration run at `00846de3`;
the doc comments say so. The growth tolerance was never changed.

| Variant | Sessions (prints) | Window | Runs | Asserts |
| --- | --- | --- | --- | --- |
| fast, `hot_path_work_is_independent_of_session_length` | 18,000 (1 min) and 180,000 (10 min), 10x | 300 frames, 5,000 depth updates | every `cargo test --workspace`, about 13 s | counts |
| long, `long` (`#[ignore]`) | 18,000 and 3,960,000 (`RETAINED_TRADES_PER_PANE`, the envelope's edge), 220x | 1,800 frames, 30,000 depth updates | on demand, about 2.5 min in release | counts and CPU time |

`the_check_fails_a_path_whose_work_grows_with_the_session` plants a per-frame
clone of the tape in the ingest path and requires the growth check to fail;
[fold-unbounded.txt](session-length/fold-unbounded.txt) is the same check
failing on the real regression this mission removed (below).

## Per-unit work at both lengths

The long variant at `df40008e`, [long.txt](session-length/long.txt) (the fast
variant's numbers are in [fast.txt](session-length/fast.txt)). *Copy bytes*
count every reallocation inside the window's laps; the largest single one is
in the raw file — a stall, not a rate.

| Path | Session | allocs/unit | bytes/unit | copy bytes/unit | folds/unit | lane entries/unit | median ns/unit |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `trade.chart.backfilled` | 18,000 | 0.287 | 110.7 | 13.2 | — | 0.90 | 140 |
| `trade.chart.backfilled` | 3,960,000 | 0.287 | 110.7 | 3.6 | — | 0.90 | 140 |
| `trade.chart.live` | 18,000 | 0.287 | 110.7 | 13.2 | — | 0.90 | 120 |
| `trade.chart.live` | 3,960,000 | 0.287 | 110.7 | 3.6 | — | 0.90 | 120 |
| `trade.book` | 18,000 | 0.010 | 3.2 | 29.1 | — | — | 400 |
| `trade.book` | 3,960,000 | 0.010 | 3.2 | 29.1 | — | — | 400 |
| `depth.book` | 18,000 | 19.110 | 6,720.6 | 332.0 | — | — | 7,000 |
| `depth.book` | 3,960,000 | 19.110 | 6,720.6 | 17.5 | — | — | 7,100 |
| `frame.book` | 18,000 | 2,128.4 | 4,761,780 | 2,887,094 | — | — | 243,900 |
| `frame.book` | 3,960,000 | 2,128.4 | 4,761,754 | 2,887,094 | — | — | 311,000 |
| `frame.app.tick50` | 18,000 | 2,896.8 | 854,969 | 1,276,680 | — | 4.50 | 1,537,200 |
| `frame.app.tick50` | 3,960,000 | 2,896.2 | 855,373 | 1,277,069 | — | 4.50 | 1,598,000 |
| `frame.worker.tick50` | 18,000 | 55.5 | 4,918 | 460.9 | 26.10 | — | n/a |
| `frame.worker.tick50` | 3,960,000 | 55.5 | 4,918 | 436.0 | 26.10 | — | n/a |
| `frame.app.time1d` | 18,000 | 1,455.3 | 297,483 | 810,305 | — | 5.00 | 696,300 |
| `frame.app.time1d` | 3,960,000 | 1,425.0 | 290,319 | 807,272 | — | 5.00 | 686,500 |
| `frame.worker.time1d` | 18,000 | 139.0 | 11,913 | 0.0 | 1,923 | — | n/a |
| `frame.worker.time1d` | 3,960,000 | 139.0 | 11,913 | 0.0 | 1,916 | — | n/a |

Verdict: within budget on every path, counts and time.

Reading it:

- **Every count is flat across a 220x longer session.** The largest move is
  `frame.app.time1d` allocating 2.1 % *less* at the edge. The short-session
  copy bytes on `trade.chart`, `trade.book` and `depth.book` are ordinary
  doublings of still-small buffers falling inside that window.
- **CPU time is flat within the +50 % bound** on every path but one, and that
  one is inside it only because of a cap: `frame.book` reads 244 µs at the
  short session and 311 µs at the edge (+28 %). Its counts are identical, so
  the growth is a walk that allocates nothing: the live half of the heatmap
  projection visits **every retained aggression** each frame
  (`crates/orderflow/src/projection/tiers.rs:89`, `for trade in
  history.aggressions()`, filtered by time). It grows with the session until
  the aggression history's own cap (100,000, or 30 minutes) binds, then stops:
  18,000 retained at the short session, 100,000 at the edge, about 0.8 ns per
  aggression. Not fixed here: aggressions are kept in arrival order, not
  guaranteed time order (`history.rs:683`), so a binary search could skip
  prints and change what the bubbles draw. It is a follow-up in the
  `orderflow` crate, outside this mission's files.
- **Worker-thread rows carry no stopwatch** of their own; their work is the
  counts, and the UI frame that waits on them is timed.
- **Costs that are flat but large**, recorded because the table shows them,
  not because they grow: a depth update allocates 19 times (6.7 KiB), and a
  heatmap projection of 120 bars allocates 4.5 MiB per call on the book
  worker. The median call serves the settled half from its cache; the full
  rebuild behind it, once per projection interval, took about 8 ms in the
  harness's unthrottled loop (the mean at `10b87d90`). Neither moves with the
  session.

## The forming-run fold

The indicator worker walked the live lane's ladder by folding every rung's
prefix from the forming run's first print: O(forming prints) on every drained
batch, so a bar that formed for an hour on a dense tape folded a million
prints sixty times a second. `FormingRun`
(`crates/engine/src/forming_run.rs`, beside `Bar::extend` in the engine, so
the next consumer that needs a forming bar's prefixes reuses it rather than
writing a second fold) now carries the fold: a running bar extended once per appended print and a
checkpoint (the exact fold so far) every `CHECKPOINT_SPACING` = 64 prints. A
walk folds forward from rung to rung and jumps to the checkpoint at or below a
rung when that lies past where it stands, so:

- **the limit** — a walk of *r* rungs over *len* prints folds at most
  `min(len, 63 r)` prints: 4,032 at the 64-rung ceiling however long the bar
  has formed, and never more than the single pass it replaced; appending folds
  each print once. `a_walk_folds_a_bounded_number_of_prints_whatever_the_runs_length`
  holds runs of 1 to 1,000,000 prints to it.
- **memory** — one 120-byte checkpoint per 64 prints, about 2 bytes per print
  held, freed with the run when the bar closes.
- **output identity** — `Bar::extend` is a sequential fold over plain data, so
  a checkpoint is the same intermediate state the old fold passed through.
  `crates/engine/tests/forming_run.rs` keeps the old `lane_prefixes` verbatim as the oracle and
  requires byte-identical prefixes for every rung count 0..=70 on every golden
  trade tape in `crates/engine/tests/fixtures/` (and all of them joined), on a
  600-print synthetic run whose prices, sides, decimal quantities and repeated
  timestamps all move and whose side totals saturate, and on a 20,000-print
  run at ten rung counts up to 1,000 — each after every one of a sequence of
  irregular append batches (1 to 127 prints). The existing ladder and lane
  tests pass unchanged.

Test-first: `dbbd11f7` moved the fold behind `FormingRun` unchanged and
committed the oracle, the identity tests and the limit test ignored and red
(1,000 prints, one rung: 1,000 folds against a budget of 63); `00846de3`
bounded the walk and un-ignored it; `10b87d90` made the walk fold forward
between rungs, after the harness showed the first bound folding 349 prints per
frame on tick:50 where the old single pass folded 27. After the AI review the
type and its tests moved, unchanged in logic, from the indicator worker into
the engine (`quantick-engine`), where `cargo test -p quantick-engine --test
forming_run` runs them below the app.

Folds per frame, from the harness:

| Walk | tick:50, 18k | tick:50, 180k | time:1d, 18k | time:1d, 180k | time:1d, 3.96M |
| --- | ---: | ---: | ---: | ---: | ---: |
| unbounded (`2452e577`), [fold-unbounded.txt](session-length/fold-unbounded.txt) | 27.0 | 27.0 | 18,908 | 180,908 | — |
| bounded (`df40008e`) | 26.1 | 26.1 | 1,930 | 1,889 | 1,916 |

## The tape's reallocation stall (D3)

Q3 measured a ~21 ms UI stall when the tape's `Vec<Trade>` doubles at
2,097,152 prints. At `df40008e`, Q3's harness re-run
([envelope-measure.txt](session-length/envelope-measure.txt)) reads **25.05 ms
at print 2,097,152** (21.36 ms with the footprint on): still there. The
harness reads the same event as work: building a tape live to 3,960,000
prints copies 64.9 bytes per print on average but **117,440,512 bytes in one
reallocation** (2,097,152 × 56). It is one dropped frame, once, about 10.6
hours into two sessions of the mean rate; the next doubling is outside the
envelope.

The harness found a second stall of the same kind and this mission removed
it: `ingest_backfill` filled the tape exactly, so the **first live print
after every load** reallocated the whole loaded session — about 95 MB for a
recovered 1.7 M-print day, the moment the chart went live. The tape is now
reserved at load to twice what it holds, the capacity that push would have
grown it to, and `prepend_history` sizes its joined tape the same way
(`the_first_live_print_after_a_backfill_copies_nothing`, red before the
change: 560,000 bytes copied of a 560,000-byte tape). `trade.chart.backfilled`
reads 0 copy bytes at the edge. The cost, for a pane that goes live, is
nothing: it holds the capacity one print earlier. A pane that never receives
a live print — a paused replay, a closed market, history paged back — keeps
the unused half reserved: one more tape's worth of *committed* memory (about
222 MB at the envelope's edge), in pages never touched, so the working set
does not grow. Unlike reserving the envelope up front, it is proportional to
what was loaded and taken only at a load.

Why the doubling stall is reported rather than fixed here: a contiguous
`Vec` must copy when it outgrows its block, so removing the copy means either
reserving the envelope's 3,960,000 prints (221 MiB of committed memory) in
every pane from its first print, or chunked tape storage. The first trades a
one-frame stall at 10.6 hours for 221 MiB of commit charge per pane from the
first second — duplicated per pane, a resource decision rather than a
behaviour-preserving fix. The second changes `ChartState::trades()` from a
slice, which `tab/history.rs` and `tab/layout.rs` read, outside this
mission's files. Both are named as a follow-up in the PR.

## Frame timing before and after

`APP_HEALTH_SUMMARY` on the WINV26 2026-08-25 replay at speed 60 with the
book, bubbles, footprint and live strip on, release builds of the campaign
base `2452e577` and of `df40008e`, five 45 s runs a side, interleaved, every
store pointed at a scratch directory (`tools/live_envelope/run_replay.ps1`,
summarised by `tools/live_envelope/frame_timing.py`); raw table in
[frame-timing.txt](session-length/frame-timing.txt).

| Side | fps min | frame_avg ms | frame_cpu ms mean (per run) | stdev | worst steady frame | `APP_SLOW_FRAMES` |
| --- | ---: | ---: | --- | ---: | ---: | ---: |
| base | 59 | 16.667 | 1.631 (1.63, 1.64, 1.63, 1.61, 1.64) | 0.012 | 33.36 ms | 0 |
| head | 59 | 16.667 | 1.634 (1.66, 1.63, 1.60, 1.66, 1.62) | 0.026 | 33.45 ms | 0 |

frame_avg is identical and fps never fell below 59. frame_cpu differs by
0.004 ms (0.2 %) against a standard error of the difference of 0.013 ms
(t ≈ 0.31): no measurable change. `worker_deferred` stayed 0 in every run. An
earlier set at `10b87d90`, taken while sibling agents' builds shared the host,
agreed (2.227 against 2.246 ms, t ≈ 0.13); both are in the raw file.

## Gate 6 at one SHA

Gate 6: *scalability claims for supported live workloads have current
measurements, stated rates and bounded-state evidence at the assessed
revision*. The interim assessment blocked it on five findings; each, and what
answers it at `df40008e`:

| Finding (interim assessment, gate 6) | Answered by | Where |
| --- | --- | --- |
| The forming fold is O(forming trades) (`indicator_worker.rs:867`) | bounded to `min(len, 63 r)` folds per walk, byte-identical output, limit and identity tests; the harness holds `frame.worker.time1d` flat from 18,000 to 3,960,000 prints | `crates/engine/src/forming_run.rs`, `crates/engine/tests/forming_run.rs`, [long.txt](session-length/long.txt) |
| Unbounded `std::sync::mpsc` command channels (`indicator_worker.rs`, `orderflow_worker.rs`) | Q3 (#405): `sync_channel`s sized from the envelope, park-and-fold overflow, counted and tested; queue depth re-measured at this SHA | `live_envelope.rs`, [envelope-measure.txt](session-length/envelope-measure.txt) block 4 |
| The tape grows with no cap (`state.rs`) | Q3: stated as `RETAINED_TRADES_PER_PANE` = 3,960,000 with `LIVE_ENVELOPE_EXCEEDED` past it, memory re-measured at this SHA; an evicting cap is Q3's pending product decision; the one doubling stall inside the envelope is above | [live-envelope.md](live-envelope.md), [envelope-measure.txt](session-length/envelope-measure.txt) block 2 |
| No stated or measured live envelope at the SHA | Q3's envelope, its harness re-run here; this page's per-unit work at both lengths | [envelope-measure.txt](session-length/envelope-measure.txt), [long.txt](session-length/long.txt) |
| Measurements not bound to the SHA; `hot_path` bench and `control_idle_dense_replay_benchmark` not gated | the fast variant drives the whole application frame on a dense tape and asserts work budgets in the ordinary `cargo test --workspace` CI step, so every assessed revision measures itself; the timing benchmarks stay manual, and the long variant is on-demand evidence | `session_length_tests.rs`, CI's Test step |

## Reproduce

```sh
# The fast variant (also part of cargo test --workspace):
env -u QUANTICK_BUBBLES cargo test -p quantick-app -- session_length_tests --nocapture --test-threads=1

# The long variant, at the envelope's edge (about 2.5 min once built):
env -u QUANTICK_BUBBLES cargo test --release -p quantick-app \
    session_length_tests::long -- --ignored --nocapture --test-threads=1

# The fold's limit and output identity:
cargo test -p quantick-engine --test forming_run

# Q3's envelope harness, re-run at this SHA:
env -u QUANTICK_BUBBLES cargo test --release -p quantick-app \
    live_envelope_tests::measure -- --ignored --nocapture --test-threads=1
```
