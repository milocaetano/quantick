# Nothing the terminal holds is dropped in silence

Criterion **A3**, and the trader's own question: *"Se existe esses dados no
metatrader, pq não colocar?"*

There were two ways the answer used to be "we had it and threw it away", and
only one of them was the clock window.

## 1. The cap was a span limiter, and it bit

`--backfill-max-ticks` defaulted to **1 000 000**. WINV26 printed **1 525 621**
trades on 2026-08-31 (see [`terminal-probe.md`](terminal-probe.md)). So even
with the window fixed, the trader's own session would have arrived with its
first **525 621 prints** — roughly 09:03 to 11:30 — removed, and the chart would
have looked exactly as complete as a full day.

The default is now **4 000 000**, and it is documented as what it actually is: a
bound on memory, not a decision about how much of the day the trader may see.
It is sized against the measurement rather than chosen — 2.6x the busiest
session on the densest tape this bridge serves. `DEFAULT_BACKFILL_MAX_TICKS` in
`bridge/mt5/quantick_bridge.py` carries that reasoning.

## 2. A cut was reported to a log nobody reads

When the cap does bite the bridge logged `BRIDGE_BACKFILL_TRUNCATED` to stderr
and the chart said nothing. `crates/feed-mt5/src/bridge_log.rs` is the
translator that puts bridge lines in front of the trader, and this code was not
in it — so the one case where the chart is knowingly incomplete was the one case
it did not mention.

It is now an `Attention` report with a next step:

> **WINV26 traded more in this session than quantick opens with — the newest
> 4 000 000 prints are on the chart**
> The rest of the day is still in MetaTrader: press + older to pull it in.
> Nothing was lost — this is a limit on what is held at once.

`BRIDGE_BACKFILL_WALK_FAILED` is reported the same way: a terminal that stops
answering mid-session now says so on the chart instead of leaving a short day
looking like a whole one.

## What is deliberately *not* claimed

The report does not say how many ticks were left behind. Once the cap stops the
walk the bridge has not seen the rest of the session, and the only way to count
what it skipped would be to fetch exactly the memory the cap exists to refuse.
It says what it knows: that a cap did this, how much is on the wire, that the
newest were kept, and where the remainder is.

That is the data-honesty rule applied to the bridge's own limits — the same
reason `stopped_on` is reported as one of `session_edge`, `cap`, `span`,
`terminal_floor`, `terminal_error`, `epoch` or `budget` rather than as a
success flag.

## Proof

- `bridge/mt5/tests/test_session_backfill.py` ::
  `test_a_session_beyond_the_cap_keeps_the_newest_and_says_so` — the cut is
  reported, names the cap, says how much it sent, keeps the newest, and points
  at `load_older`.
- `crates/feed-mt5/src/bridge_log.rs` ::
  `a_cut_session_says_what_arrived_and_how_to_reach_the_rest` — the headline
  carries a grouped count and the next step names `+ older`.
- `crates/feed-mt5/src/bridge_log.rs` ::
  `a_cut_with_no_count_still_says_something_true` — a line from an older bridge
  with no `sending` field still translates into a whole sentence.
- `crates/feed-mt5/src/bridge_log.rs` ::
  `only_the_codes_that_stop_the_bridge_end_the_session` — neither new code kills
  the session; the tape keeps streaming while the trader is told.
