# Trades bars and deal recording

Bars cut every N exchange deals — ProfitChart's *Trades* periodicity — on
MetaTrader B3, and the recording that makes them possible.

## Why a tick bar is not that chart

MetaTrader folds every fill an aggressor took at one price into a single
tick and keeps no count per tick. Measured on 2026-09-03 for `WINV26`:

| Source | Count |
| --- | --- |
| `SYMBOL_SESSION_DEALS` (the venue's own session counter) | 5 821 205 |
| ticks with a LAST flag in `CopyTicks` | 1 774 869 |
| volume, sum of ticks vs `session_volume` | 18 707 279 vs 18 707 340 |

The volume matches; the count does not. A `tick:2000` chart counts
MetaTrader ticks, ProfitChart's `2000T` counts deals, and the two cut in
different places — roughly 3.3 deals per tick on the mini index. The `trades`
kind counts what the venue counts.

## Where the count comes from, feed by feed

| Feed | `trades` kind | REC | Count |
| --- | --- | --- | --- |
| MetaTrader 5 — B3 (`WIN`, `WDO`) | yes | yes, beside the symbol | the session counter, read by the bridge every poll (≈ 20 ms) and stamped on every live tick (`deals` in `bridge/mt5/PROTOCOL.md`) |
| MetaTrader 5 — a quoted CFD (`XAUUSD`, `US500`) | not offered | none | the broker prints no deals; the hello says `deal_counter: false` |
| Binance, Hyperliquid | not offered | none | out of scope for now. Binance's aggTrades carry the first and last deal id of every print, so the count would need no recording there — a later mission |
| Replay | not offered | none | later: read the `.deals` file beside an exported tape |

The gate is a capability, never a provider name: `FeedCapabilities::deal_counter`
is true only for a session whose bridge declared it. The bar-kind selector
does not list `trades` where it is false.

## What the counter can and cannot give

- **Live, yes.** The counter is a running total the terminal keeps; the bridge
  reads it after each pump round, so it is at or past every print it stamps.
  The feed keeps one sample per change (`DealSampler`), and the engine joins
  each print to the newest sample at or before it (`DealBarBuilder`). A bar
  closes on the first print whose reading reaches the next multiple of N.
- **Aligned with ProfitChart.** Bars are the session's multiples of N, never
  "N deals since the chart connected": a chart that connects at reading
  2 300 411 closes its first bar at 2 302 000.
- **Resolution is one poll.** Every print of a round carries the same reading,
  so a bar can overshoot by one round's deals — a handful on a normal tape —
  and never carries the overshoot forward.
- **Backwards, no.** MetaTrader stores only the folded ticks. Prints before the
  first reading form no trades bar; the chart says how many they are, and
  never guesses.

## Recording

Recording belongs to the **asset**, never to the pane. It writes each reading
to `Documents/Quantick/deals/<SYMBOL>/<YYYY-MM-DD>.deals` (text, delta
encoded; see `crates/app/src/deal_recording.rs`). The directory is
`QUANTICK_DEALS_DIR`, then `[deals] dir` in the config, then that default.

- **REC** beside the symbol starts and stops it. Red while recording, amber
  while the tape moves and the counter does not, grey `RECORDED · day` when
  the readings on screen came from a file, plain `REC` when off. Its popover
  shows the reading, the start, the file, and offers *Start/Stop recording*,
  *Show as trades*, *Open the folder* and the recorded days.
- **Record by default**: `record_deals = true` on the feed entry (the shipped
  `metatrader-b3` says so) or the Tools menu checkbox, which is saved in the
  workspace and wins over the config. A hand that stopped a recording is not
  restarted by the default.
- **The corner chip** on the flow pane and **the status cell** repeat the
  state, so it is visible with the toolbar folded or the tab in the
  background.

## What happens when

| You… | …and quantick |
| --- | --- |
| switch the pane from `tick` to `trades` with REC on since the open | rebuilds the day's bars every N deals; the recording is untouched |
| switch to `trades` with REC on since 12:36 | trades bars from 12:36 on; the prints before are counted and reported as *no deal count*, and form no bar |
| switch to `trades` with no count at all | the option is disabled with the reason — press REC, or load a recorded day |
| switch from `trades` back to `tick` | only the drawing changes; REC keeps writing |
| open another B3 symbol | it has its own REC and its own files |
| open a Binance tab | no `trades`, no REC |
| close quantick at 14:00 and reopen at 14:20 | today's file is resumed; the 20 minutes without readings form no bars |
| press *Stop recording* | the file closes as partial; the day reopens up to where it stopped |
| open the history menu | the recorded days are listed with their coverage; picking one loads its readings, and the tape paged back to that day cuts as trades bars |
| ask by script | `feed.status` carries `deal_recording`; `feed.deal_recording.set` starts or stops it; `trades:2000` is a config spec like `tick:2000`, refused on a feed that is not MetaTrader |

## Follow-ups recorded

- Replay and `tools/mt5/export_session.py` neither read nor write the
  `.deals` file yet.
- Binance's exact per-print deal ids are not used yet.
- The MQL5 bridge carries the same `deals` stamp but was not run here.
