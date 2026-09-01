# The whole thing, in the actual application

Criteria **A2**, **A4**, **G4** and the `visual-qa` pass. Everything above this
file measures a part; this is the trader's own scenario end to end — the
desktop app, opened on WINV26, against the live MetaTrader terminal.

Run with [`mt5_open.ps1`](mt5_open.ps1) and
[`mt5-config.toml`](mt5-config.toml). Full log: [`mt5-open.log`](mt5-open.log).
Screenshot: [`shots/mt5-session-open.png`](shots/mt5-session-open.png).

## What the run says

```
BRIDGE_TICK_FLOOR_IMPLAUSIBLE  claimed_ms=1788204600002 found_below=…
BRIDGE_BACKFILL_SESSION        count=1525621 first_ms=1788166980233 …
BRIDGE_OPENING_SLICED          total=1525621 opening_now=50000 slices_to_follow=30
MT5_HISTORY_READY              count=49999
MT5_OPENING_PAGE_READY  × 30   … remaining=Some(0)
BRIDGE_OPENING_COMPLETE
```

The status bar reads **`30510+0 bars`** at `tick(50)` — 1 525 500 prints, the
whole 09:03→18:31 session, on a tick chart. That is the ask in R3, in the app.

Note the first line: the implausible tick floor happened again, in this run.
It is not a once-off — see [`whole-day.md`](whole-day.md) — and without the
check this capture would show an empty chart.

## The control run, and what it cost

The first attempt at this capture accidentally became the control, and a better
one than a `main` build would have been. `bridge_command` was a relative path,
which resolves against the app's working directory — not the worktree — so the
**branch's app ran against the main checkout's bridge**. Same binary, same
window, same tape: only the opening block differs.

The trade counts are deliberately not repeated here — they belong to a
particular pair of runs, and a second hand-written copy of a measured number is
how this evidence went stale three times. [`perf.md`](perf.md) generates them
from the logs committed beside it.

| | main's bridge | this branch's bridge |
| --- | --- | --- |
| trades charted | see [`perf.md`](perf.md) | see [`perf.md`](perf.md) |
| bars at tick(50) | ~7 500 | **30 510** |
| oldest print | the clock window | **09:03** (the session) |

The frame cost is measured separately and mechanically: see
[`perf.md`](perf.md), whose tables are generated from committed logs rather
than transcribed. Summary — fps floor 57 → 54 for eight times the trades, no
`APP_SLOW_FRAMES` inside either fill.

**The surface verdicts below were captured on an earlier build** whose opening
slice was 50 000 rather than the shipped 200 000. Slice size changes how long
the fill takes and what it costs in frames — not what any of these five
surfaces draws — so the verdicts stand, but the status-bar row's figures are
that build's and the performance question is answered by `perf.md`, not here.

It is also worth recording *why* the control exists: the first run of this
capture photographed the old behaviour and would have been reported as a pass.
The bridge path in `mt5-config.toml` is absolute now, with a comment saying so.

## Surfaces

| Surface × state | Verdict | Evidence |
| --- | --- | --- |
| Flow chart, MT5 WINV26, whole session loaded | **PASS** | `shots/mt5-session-open.png` — 30 510 bars, axis 18:10→18:22 on the newest, price axis intact, no clipping, footprint legend reads `rows 5 · min qty 92 · side inferred` |
| Status bar under the load | **PASS** | It renders and stays legible while the session lands; `arrival —` because the block is history and carries no live latency, which is the honest reading rather than a fabricated one. The frame figures it happened to show are that run's — the measured ones are in [`perf.md`](perf.md) |
| Book badge with `--no-book` | **PASS** | `book down · bridge_without_depth` and `no book` on the axis — the disabled state explains itself, per the data-honesty rule |
| History menu, `by time` reach | **PASS** | `shots/older-span-menu.png` — chip beside `one page` and `previous session`, `hours of tape per press` = `3 h` from 180 minutes |
| History menu, `previous session` reach | **PASS** | `shots/reach-previous-session-menu.png` — the duration rows are **absent**, so the control appears only under the reach that reads it |

No FAILs. One accepted note: the capture shows the newest bars because that is
where a chart opens; the rest of the session is held and reached by panning,
which the bar count proves and a single frame cannot.
