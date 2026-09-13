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
| MetaTrader 5 — B3 (`WIN`, `WDO`) | yes | yes, beside the symbol | the session counter, which the terminal refreshes about every 31 s (measured over a whole session: 592 readings, median interval 31.2 s, 1 500 to 10 000 deals apart); the bridge reads it every poll and stamps every live tick (`deals` in `bridge/mt5/PROTOCOL.md`), so a new reading reaches the chart within one poll of the terminal publishing it |
| MetaTrader 5 — a quoted CFD (`XAUUSD`, `US500`) | not offered | none | the broker prints no deals; the hello says `deal_counter: false` |
| Binance, Hyperliquid | not offered | none | out of scope for now. Binance's aggTrades carry the first and last deal id of every print, so the count would need no recording there — a later mission |
| Replay | not offered | none | later: read the `.deals` file beside an exported tape |

The gate is a capability, never a provider name: `FeedCapabilities::deal_counter`
is true only for a session whose bridge declared it. The bar-kind selector
lists `trades` disabled, with the reason, where it is false — never hidden,
so a pane restored on `trades` before the hello lands is still a kind the
selector can name — and the config loader refuses `trades:N` and
`record_deals` on any provider that is not MetaTrader.

## What the counter can and cannot give

- **Live, yes — at the terminal's resolution.** The counter is a running
  total the terminal keeps and refreshes about every 31 seconds. The feed
  keeps one sample per change (`DealSampler`); the engine (`DealBarBuilder`)
  knows the exact total at each reading and nothing print by print between
  two readings.
- **Between readings, an estimate.** Each print is credited its contracts
  times the *rate* of the last completed window — deals per contract,
  exact for that window — and the running total is re-anchored to the exact
  reading every time one arrives. So the day's total and the number of bars
  are the venue's; where inside a 31-second window each bar closes is an
  estimate, off by the difference between two consecutive windows' rates. A
  bar closes on the first print whose estimated total reaches the next
  multiple of N. A reading that reaches a multiple the estimate had not
  closes the forming bar on the next print; a multiple the estimate closed
  early is not closed twice. The chip and the REC hover say *estimated*.
- **Aligned with ProfitChart in count, not to the deal.** Bars are the
  session's multiples of N, never "N deals since the chart connected": a
  chart that connects at reading 2 300 411 closes its first bar at
  2 302 000. ProfitChart's 2000T cuts on its own per-deal feed; this chart
  cuts the same number of bars, each boundary within a window of it.
- **Backwards, no.** MetaTrader stores only the folded ticks. Prints before
  the first reading, and the prints of the first window (no rate yet), form
  no trades bar; the chart says how many they are, and never guesses.
- **One rule, order-free.** The estimate uses only what came before the
  print — the last completed window's rate and the newest reading strictly
  before it — so readings fed just ahead of their prints (live) and readings
  fed all at once before a rebuild cut identical bars. That is what lets a
  change of N recut the whole day from the recorded readings. A reading
  holds for ten minutes of tape (`READING_MAX_AGE_MS`): a print further
  behind the newest reading before it than that — quantick was down, last
  night's reading under this morning's prints — is *uncounted*, and the chip
  counts it. A counter that stands still while prints keep coming turns REC
  amber after three missed readings (`REC · counter stale 90 s`; `REC ·
  counter stuck at 0` when it never moved, which is what a broker that does
  not report the counter looks like), and with REC off the trades pane's chip
  says the same. A reading lower than the one in force by less than a bar's
  worth of deals is the terminal answering a poll late and changes nothing;
  lower by more is the session restarting, which ends the forming bar.

## Recording

Recording belongs to the **asset**, never to the pane. It writes each reading
to `Documents/Quantick/deals/<SYMBOL>/<YYYY-MM-DD>.deals` (text, delta
encoded; see `crates/app/src/deal_recording.rs`). The directory is
`QUANTICK_DEALS_DIR`, then `[deals] dir` in the config, then that default.

- **REC** beside the symbol starts and stops it. Red while recording, amber
  while the tape moves and the counter does not, grey `RECORDED · day` when
  the readings on screen came from a file, plain `REC` when off. The time on
  the button and the chip is where the open file starts — resumed or written
  this run; the popover also shows the first reading of the run. Readings
  arrive and cut bars whether or not REC is on; the chip says `counting · not
  written to disk` in that case, and what REC adds is the file. Its popover
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
| switch the pane from `tick` to `trades` with REC on since the open | rebuilds the day's bars every N deals from the recorded readings; the recording is untouched |
| change N | the whole day is recut from the same readings and ticks: as many bars as the session's total over N, boundaries estimated within each 31-second window |
| switch to `trades` with REC on since 12:36 | trades bars from 12:36 on; the prints before are counted and reported as *no deal count*, and form no bar |
| switch to `trades` with no count at all | the option is disabled with the reason — press REC, or load a recorded day |
| switch from `trades` back to `tick` | only the drawing changes; REC keeps writing |
| open another B3 symbol | it has its own REC and its own files |
| open a Binance tab | no `trades`, no REC |
| close quantick at 14:00 and reopen at 14:20 | today's file is resumed; the 20 minutes without readings are uncounted prints once they run past what a reading holds for (see *One rule, order-free*), and cutting resumes at the first completed window after 14:20 |
| the bridge stalls for 30 s and catches up in one round | the round's one reading is dated at the round's last tick — it was read after the round was fetched, so it covers every print of it; the stall's prints are credited the rate of the window before it, and the reading re-anchors the total |
| load a recorded day while the live counter is arriving | its readings join the series; the state stays *counting live* (REC off) rather than *recorded*, since the live readings keep cutting — *recorded* is what the chart says once the counter stops |
| leave the app open overnight, or press Reload in the morning | last night's last reading counts nothing of this morning's prints; the session's first reading starts the day |
| press Reload on the feed | every pane is rebuilt from the new session's backfill; the readings it held stay with it, so the morning's prints cut as before |
| switch the tab to another symbol, or open a replay | every pane starts clean: the old market's readings go with its series, and the new market's REC starts on its own default |
| change the display timezone while recording | the open file keeps the day it was named for (its header's `tz_minutes`); the new offset names the next day's file |
| open the tab with no bridge connected | a day recorded earlier is still listed under REC and in the history menu, and still opens; nothing records until a bridge declares a counter |
| press *Stop recording* | the file closes as partial; the day reopens up to where it stopped |
| open the history menu | the recorded days are listed with their coverage; picking one loads its readings, and the tape paged back to that day cuts as trades bars |
| ask by script | `feed.status` carries `deal_recording`; `feed.deal_recording.set` starts or stops it; `trades:2000` is a config spec like `tick:2000`, refused on a feed that is not MetaTrader |

## Follow-ups recorded

- Replay and `tools/mt5/export_session.py` neither read nor write the
  `.deals` file yet — so `quantick-backtest --bars trades:N` refuses the
  kind, saying why, until a headless reader of the recording exists.
- A control-plane call that sets the flow pane's bar spec (`trades:N` as
  much as `tick:N`) does not exist; the toolbar is the only runtime path,
  for every kind.
- Binance's exact per-print deal ids are not used yet.
- The MQL5 bridge carries the same `deals` stamp but was not run here.
