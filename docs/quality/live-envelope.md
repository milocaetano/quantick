# The supported live envelope

The live workload quantick is built, tested and measured for, and what happens
past it. The numbers are code — `crates/app/src/live_envelope.rs` — and this
page renders the same numbers with the measurement behind each one;
`live_envelope::envelope_doc_tests` fails the build when a row here and a
constant there disagree.

Measured on `DESKTOP-BTVJFFR` (Intel Core i5-12400F, 31.8 GB, Windows 11 Pro)
each raw output below names the command that reproduces it and the commit it
was measured at: `measure.txt`, `burst-test.txt` and `frame-timing.txt` at
`5311d6f7` of `feat/live-workload-envelope` (the campaign base `c1b4002e` plus
this branch, the last commit that changed code); `tape-rates.txt` depends only
on the recordings and the script.

## The envelope

A *print* is one MetaTrader tick — the unit the app ingests, which already
folds several B3 deals into one print. *Measured* figures come from
`tools/live_envelope/tape_rates.py` over the thirteen WINV26 sessions and one
WDOU26 session recorded on this host ([tape-rates.txt](live-envelope/tape-rates.txt));
*derived* figures say what they are derived from.

| Constant | Value | Kind | Derivation |
| --- | ---: | --- | --- |
| `SUSTAINED_TRADES_PER_S` | 300 | measured | p99 one-second rate over the WINV26 sessions is 224–286 prints/s; rounded up |
| `BURST_TRADES_PER_S` | 2,000 | measured | busiest single second in any session: 1,882 (WINV26 2026-09-03); rounded up |
| `BURST_TRADES_PER_FRAME` | 512 | measured | busiest 16 ms window (one 60 fps frame): 267 prints (2026-08-17); doubled, rounded up to a power of two. The p99 frame holds 6–7 |
| `MEAN_TRADES_PER_S` | 55 | measured | session means are 41.1–53.4 (WINV26) and 3.5 (WDOU26); rounded up |
| `DEPTH_UPDATES_PER_S` | 1,000 | **derived** | no recording carries a book. Binance publishes `depth@100ms` (10/s); the feed-side depth channel holds 8,192 events; the drain takes up to 2,048 per frame. A hundredfold headroom over Binance, driven synthetically by the burst test |
| `BURST_DEPTH_UPDATES_PER_FRAME` | 2,048 | derived | the per-frame depth drain budget; `crate::tab`'s `BOOK_DRAIN_BUDGET` now reads this constant |
| `SESSION_HOURS` | 10 | measured | first to last print spans 9.47–9.52 h (09:00–18:31 local); rounded up |
| `RETAINED_SESSIONS` | 2 | derived | the day on screen plus the day before, which the replay browser and the MetaTrader session recovery join in front of the live tape |
| `RETAINED_TRADES_PER_PANE` | 3,960,000 | derived | `MEAN_TRADES_PER_S × 3,600 × SESSION_HOURS × RETAINED_SESSIONS` |
| `INDICATOR_COMMAND_QUEUE` | 1,024 | derived | a tick:1 pane sends one closed bar per print plus one forming-bar update: 513 at the peak frame; twice the burst frame |
| `INDICATOR_EVENT_QUEUE` | 4,096 | derived | a peak frame on a tick:1 pane appends up to 512 rows per hosted indicator plus a preview, a lane and an objects event: 515 per indicator, so one peak frame of seven indicators; kept small because the array channel preallocates every slot |
| `REFERENCE_TICKS_PER_BAR` | 50 | derived | the spec the retained-bars figure is stated at: tick:50, the trader's footprint chart and the spec every measurement here runs |
| `RETAINED_BARS_PER_PANE` | 79,200 | derived | `RETAINED_TRADES_PER_PANE / REFERENCE_TICKS_PER_BAR`; a tick:N pane holds `RETAINED_TRADES_PER_PANE / N`, a time pane far fewer |
| `BOOK_COMMAND_QUEUE` | 4,096 | derived | one command per depth event (≤ 2,048) and per print (≤ 512) plus one layout request: 2,561; next power of two, and the trade channel every venue feed already sizes itself to |

The health summary classes the measured ingest against these rates:
`live_rate` is `inside` (≤ sustained trades and ≤ depth rate), `burst`
(≤ burst trades) or `above`. Two scopes, the two rates the summary has: the
trade rate is the window's, summed over every tab, so it can only overstate
one pane's rate; the depth rate is the active tab's, the book the rest of the
line describes. A replay played fast is `above` by design.

## Queues on the live path: cap and overflow policy

| Queue | Cap (code) | When full | Observed by |
| --- | --- | --- | --- |
| Indicator worker commands (`indicator_worker.rs`) | `INDICATOR_COMMAND_QUEUE` = 1,024, `sync_channel` | the sender parks the command on the UI side, in order, and retries on every later send and every frame's `drain_events`; a forming-bar update folds into a parked forming-bar update (its prints appended, its partial and lane budget winning), an input set into a parked set for the same slot, a replay-from-scratch into a parked replay (`fold_parked`); a closed bar never folds. Nothing blocks, nothing is dropped | `ProgressSnapshot.counts.{queued, parked, deferred, coalesced_parked}`; `APP_HEALTH_SUMMARY.worker_{backlog, parked, deferred, coalesced}`; `LIVE_QUEUE_DEFERRED` warning |
| Book worker commands (`orderflow_worker.rs`) | `BOOK_COMMAND_QUEUE` = 4,096, `sync_channel` | parked in order the same way, retried on every send and every frame's `published` / `published_base_grouping`; only a layout request folds into a parked layout request. Prints and depth events keep their order — the parked prints *are* the kept batch the next frame retries — and configuration changes keep theirs, because applying one can prune history the next would not have | same fields |
| Parked commands (UI side) | bounded by the envelope, not by a number: a hard cap would drop prints | above | `worker_parked`, zero inside the envelope |
| Indicator worker events back to the UI (`indicator_worker.rs`) | `INDICATOR_EVENT_QUEUE` = 4,096, `sync_channel`, drained whole every frame | the *worker* waits for the UI to drain (the UI never waits on it); nothing is dropped, and the command queue above parks behind the paused worker | `counts.output_blocked`; `APP_HEALTH_SUMMARY.worker_output_blocked`; `LIVE_QUEUE_DEFERRED` (`output_blocked_since_summary`) |
| Book mailbox | one slot, replaced | — | `mailbox_replacements` |
| Feed trade channel (`crates/feed/src/binance.rs`, `metatrader.rs`, `hyperliquid.rs`) | 4,096, tokio | the venue task awaits (backpressure to the socket) | `feed_arrival_ms` |
| Feed depth channel | 8,192, tokio | as above | `book_queue_len` |
| Depth drain per frame (`tab.rs`) | `BURST_DEPTH_UPDATES_PER_FRAME` = 2,048 | the rest waits for the next frame | `book_queue_len` |

The retry points are per frame for every tab: `drain_tabs` reads every
pane's indicator events each frame, background tabs included. A background
tab's book view reads its mailbox when it syncs, so its parked commands are
retried on that pane's next send — every print or depth event — or when it is
shown; `worker_parked` counts them meanwhile.

`LIVE_QUEUE_DEFERRED` compares each worker's own count with the last summary's, so a busier pane that closed cannot hide a new overflow elsewhere.

A worker that is gone turns every parked command into a counted
`failed_sends` and the existing `INDICATOR_WORKER_DOWN` / `HEATMAP_WORKER_DOWN`
error; nothing vanishes uncounted.

## Retained history

| History | Cap | Past it |
| --- | --- | --- |
| A pane's tape (`ChartState::trades`, `state.rs`) | none in code; `RETAINED_TRADES_PER_PANE` = 3,960,000 is the envelope | nothing is evicted. `retained_trades` on the health line reports the largest pane; `LIVE_ENVELOPE_EXCEEDED` warns once when a pane crosses the envelope (again if another pane becomes the one past it, or after the tape is back inside), with the excess. An evicting cap is a product decision (below) |
| A pane's bars (`ChartState::bars`) | none in code; `RETAINED_BARS_PER_PANE` = 79,200 at tick:50 is the envelope | nothing is evicted; `LIVE_ENVELOPE_EXCEEDED` carries the pane's bars beside the envelope's |
| Heatmap aggressions and liquidity runs (`crates/orderflow/src/history.rs`, `HeatmapConfig`) | existing caps: 30 min retention, 100,000 aggressions, 500,000 runs, 64 MiB | the oldest leave the canvas; counted, cumulative, as `APP_HEALTH_SUMMARY.heatmap_aggressions_evicted` / `heatmap_runs_evicted` (the active tab's book, like every `heatmap_*` field) |

### Other bounded state on the live path

What else a live pane accumulates or queues, with the cap that holds it. These
caps predate this page and are listed so the inventory is whole:

| State | Cap | Where |
| --- | --- | --- |
| The indicator lane's forming run | the forming bar's own prints plus one checkpoint bar per `CHECKPOINT_SPACING` = 64 prints; freed when the bar closes; the ladder walks at most `MAX_LANE_RUNGS` = 64 rungs, and a walk folds at most 63 prints per rung whatever the run's length ([session-length.md](session-length.md)) | `crates/engine/src/forming_run.rs`, held by `crates/app/src/indicator_worker.rs` |
| Indicator draw objects | `MAX_OBJECTS_PER_KIND` = 500 per kind; the oldest goes, as in Pine | `crates/indicators/src/objects.rs` |
| Trade paint marks | `TRADE_PAINT_LIMIT` = 200 | `crates/app/src/trade_paint.rs` |
| Drawing undo history | `UNDO_HISTORY_LIMIT` = 64 | `crates/app/src/drawings/mod.rs` |
| Footprint ladders | one per closed bar while the footprint is on: part of the pane's retained history above, measured below | `crates/app/src/state.rs` (`FootprintSeries`) |
| Control plane queues, pages and journals | `crates/control/src/limits.rs` and the gateway capacities (`GATEWAY_COMMAND_CAPACITY` = 64, `GATEWAY_STATUS_CAPACITY` = 256, `CONTROL_UI_MAX_STATUS_UPDATES_PER_FRAME` = 32) | `crates/app/src/control/gateway.rs`; tested in `app/tests/control_plane_tests.rs` |

No in-memory journal sits on the market-data path itself.

### Measured at the envelope's edge

One pane, `tick:50`, 3,960,000 prints at the mean rate, from
[measure.txt](live-envelope/measure.txt):

| Footprint | Tape (computed) | Bars (computed) | Working set (OS) | Ingest ns/print, first → last 100k | Slowest single ingest |
| --- | ---: | ---: | ---: | ---: | ---: |
| off | 211.5 MiB | 9.1 MiB | +220.8 MiB | 122 → 101 | 21.4 ms at print 2,097,152 |
| on | 211.5 MiB | 9.1 MiB | +569.5 MiB | 199 → 181 | 22.2 ms at print 2,097,152 |

Heatmap history at its own caps: 99,000 aggressions (6.8 MiB) after 30 minutes
at 55 prints/s; at 300 prints/s the 100,000-aggression count binds after about
5.6 minutes, so the "30-minute" bubble retention is shorter on a busy tape.

Reading the table:

- **Per-print cost does not grow with the session.** The last 100,000 prints
  ingest as fast as the first; the bound is a constant per print.
- **Memory grows linearly, about 56 B per print** (the tape) plus about 90 B
  per print when the footprint is on (its ladders). At the envelope's edge
  that is 221 MiB per pane, or 570 MiB with the footprint — and every pane of a
  tab keeps its own copy of the tape.
- **The tape's growth has one stall per doubling.** `Vec<Trade>` reallocates
  when it doubles; the copy runs on the UI thread. The largest inside the
  envelope is about 20 ms at print 2,097,152 (a 112 MiB copy) — one dropped frame,
  once, about 10.6 hours into two sessions of the mean rate. The next (a
  224 MiB copy, about twice as long) would come at 4,194,304 prints, just
  outside the envelope. Within a single
  session the largest stall is at 1,048,576 prints (a 56 MiB copy). This is
  accepted inside the envelope and named as a follow-up, not a product call:
  chunked tape storage would remove it without evicting anything.

### The product decision this leaves open

No cap that evicts tape or bars is implemented. The numbers a decision needs:
3,960,000 prints per pane at the envelope's edge, 221 MiB per pane without the
footprint and 570 MiB with it, duplicated per pane; a four-pane tab with the
footprint on every pane would hold about 2.3 GB at that edge. Proposed policy,
for the trader to accept or reject: keep the envelope's two sessions whole and,
past `RETAINED_TRADES_PER_PANE`, evict the oldest *session* of tape (never part
of one), keeping its bars; or share one tape per tab instead of one per pane,
which evicts nothing and divides the memory by the pane count.

## Burst test

`crates/app/src/live_envelope_tests/burst.rs`, a plain `cargo test` (about 8
s), drives one pane's live path — `ChartState`, the lane cursor, the real
`IndicatorWorker` hosting `native.cvd`, the real `BookWorker` recording
aggressions and depth — frame by frame at 60 fps of wall time, exactly as the
pane feeds them. Output: [burst-test.txt](live-envelope/burst-test.txt).

- **Inside**: tick:1 (the command-heaviest chart), 300 prints/s for 5 s, then
  2,000 prints/s for 3 s, with 1,000 depth updates/s throughout, then the peak
  frame (512 prints and 2,048 depth updates in one frame). Every print is in a
  bar (each frame is sent once the previous one was admitted, so the
  assertion does not depend on the machine's load; the harness above
  measures how often a frame had to wait), the indicator has one row per closed bar, its last row equals the
  cumulative delta computed independently from the prints, the book retained
  one aggression per print and applied every depth update, and `deferred`,
  `parked` and `coalesced_parked` stay 0 on both workers.
- **Above**: both workers are stalled through the production progress-clock
  port while 40 s of burst-rate frames arrive. Both channels fill to exactly
  their cap, commands park and fold (`deferred` > `parked` > 0,
  `coalesced_parked` > 0), nothing is refused; released, the parked commands
  drain frame by frame and every count above holds again, including the lane's
  forming run, which proves the folded forming-bar updates lost no print.

### Queue depth and keep-up

`measure.txt` block 4 plays tick:1 (one indicator command per print) at 300
prints/s for 20 s and 2,000 prints/s for 5 s, 1,000 depth updates/s
throughout, one frame per 16.7 ms of wall time, and records per second the
deepest each queue got and how many frames found the previous frame not yet
taken by a worker. At most one frame's worth was ever queued (6 and 35
indicator commands, 23 and 52 book commands, against caps of 1,024 and
4,096), nothing parked, and no frame was late after the first (which waits
for the pane's setup commands). The peak frame — 512 prints and 2,048 depth
updates — was sent in under 4 ms on the UI side. The three bounded channels
preallocate 0.2 MiB (indicator commands), 1.0 MiB (indicator events) and
1.3 MiB (book commands) per pane.

## Frame timing before and after

The worker send path changed (bounded channels and a parked-buffer check per
command), so it was measured the way PR #388 measured: `APP_HEALTH_SUMMARY` on
the WINV26 2026-08-25 replay (1.70 M prints) at speed 60 with the book,
bubbles, footprint and live strip on, a release build of the campaign base
`c1b4002e` against a release build of `5311d6f7`, five 45 s runs each,
interleaved, every store pointed at a scratch directory
(`tools/live_envelope/run_replay.ps1`, summarised by
`tools/live_envelope/frame_timing.py`). Raw table and the overflow traces:
[frame-timing.txt](live-envelope/frame-timing.txt).

| Side | fps min | frame_avg ms | frame_cpu ms mean (per run) | stdev | worst steady frame | `APP_SLOW_FRAMES` |
| --- | ---: | ---: | --- | ---: | ---: | ---: |
| base | 59 | 16.666 | 1.900 (1.69, 2.30, 1.92, 1.81, 1.78) | 0.236 | 33.30 ms | 1 |
| head | 59 | 16.666 | 2.046 (2.10, 2.06, 2.08, 2.21, 1.78) | 0.161 | 33.36 ms | 2 |

frame_avg is identical and fps never dropped below 59 on either side. frame_cpu
differs by 0.15 ms (7.7 %); the standard error of that difference over five
runs a side is 0.13 ms (t ≈ 1.1), and the base's own runs span 1.69–2.30 ms, so
the gap is inside the run-to-run noise rather than a measured regression. An
earlier pair of five-run sets at `6b9bd34a` gave 2.567 against 2.621 ms (+0.05).
Every `APP_SLOW_FRAMES` line on both sides is on a run's load. Per print, the
send path allocates less than before, not more: `sync_channel` is an array
preallocated once per pane, where the unbounded channel allocated a block every
31 messages; the parked buffer allocates only while a queue is full.

The replay ran at 4,700 prints/s, `live_rate=above` throughout. In three head
runs of five `worker_deferred` stayed 0. In two (head-3, head-4) the load frame
took 235 and 403 ms while the replay caught up at 18,670 and 17,590 prints/s —
about nine times the burst rate — and the book queue filled: 406 and 566
commands were parked, `LIVE_QUEUE_DEFERRED` said so once, `worker_parked` was
back to 0 by the next summary, and nothing was refused or dropped.
`worker_output_blocked` stayed 0 in every run. That is the overflow policy doing
its job on the real app, above the envelope.

## Reproduce

```sh
# The rates (reads the recordings on this host, standard library only):
python tools/live_envelope/tape_rates.py ~/Documents/Quantick/replay

# The burst test, and the doc-and-code pin:
env -u QUANTICK_BUBBLES cargo test -p quantick-app live_envelope -- --nocapture

# The measurement harness (#[ignore]d; about 30 s once built):
env -u QUANTICK_BUBBLES cargo test --release -p quantick-app \
    live_envelope_tests::measure -- --ignored --nocapture --test-threads=1
```
