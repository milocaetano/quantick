# G3 / G4 — performance impact, declared

## Every touched path, by rate

| path | rate | what this change did to it |
| --- | --- | --- |
| `ReportState::draw_window` and everything under it | **per-frame, while the report window is open** | relocated verbatim; no algorithmic change |
| `ReportState::draw_trades_tab` and its row painters | **per-frame, while the Trades dock is on** | relocated verbatim; no algorithmic change |
| `ensure_report_view` (the cut, `EquityWalk::of`, `PerformanceReport::from_trades`) | **rare** — on a filter, timezone or history change, guarded by the freshness check that was already there | relocated verbatim; the guard is unchanged |
| `DayIndex::build` | **rare** — keyed on `(generation, source, timezone)`, as before | unchanged |
| `load_history` | **rare** — a folder read on open, refresh, retarget or import | unchanged |
| `PaperTrading::settle`, `handle_events`, `journal` | **per-trade** | `handle_events` now calls `report.journal_changed(&env)` where it read `self.report_open` and called `self.reload_report()`. Same test, same reload, one struct field deeper |
| the chart layer, order entry, brackets, the ruler | **per-frame / per-trade** | untouched |

## Why the per-frame paths are flat

This is a relocation, not a rewrite. Every render function moved
byte-for-byte apart from `self.<field>` becoming `self.<field>` on a
smaller struct and `self.symbol` becoming `env.symbol`. The two caches
that make these surfaces cheap are intact and were not touched:

- the **equity walk** is still cut once when the view is cut, not walked
  per frame (`EquityWalk::of`, called from `ensure_report_view`);
- the **day index** is still keyed and rebuilt only when its inputs move,
  so an open calendar still costs a fixed 42 cells per frame however long
  the history is;
- the **ledger totals** are still summed with the folder read, not on the
  frame;
- the ledger still virtualises through `ScrollArea::show_rows` and still
  builds a bounded number of rows — `the_ledger_builds_a_bounded_number_of_rows_however_deep_the_history`
  moved with it and passes.

## What the seam costs per frame

`report_env!` expands at each call site into five borrows and one
`open_row()`, which is at most three reads of the venue and a
`PositionSummary` — the same three reads the ledger did inline before,
now done once at the call rather than once inside the draw. It is built
twice per frame at most (the dock tab and the window), and only for a tab
that is drawing. Nothing is cloned into it: every field is a borrow, which
is why it is a borrowed struct rather than an owned one.

## Measurement

`APP_HEALTH_SUMMARY`, last line of each capture run, on a live Binance
tape with the surface open. Same journal, same machine, runs interleaved
so a scene's two readings are seconds apart:

| scene | build | fps | frame_avg_ms | frame_cpu_ms |
| --- | --- | ---: | ---: | ---: |
| report | before | 59 | 16.668 | 2.378 |
| report | **after** | 59 | 16.668 | **2.191** |
| calendar | before | 60 | 16.667 | 2.202 |
| calendar | **after** | 59 | 16.675 | **2.909** |
| ledger | before | 59 | 16.667 | 1.891 |
| ledger | **after** | 59 | 16.678 | **1.769** |

Every run sits on the 60 fps vsync cap, so `frame_avg_ms` is pinned at
16.67 by the cap and carries no signal. `frame_cpu_ms` is the number with
room to move, and it goes both ways: the report and the ledger came out
slightly cheaper after the move, the calendar slightly dearer.

**Read that as noise, not as a result.** These are single ~10-second
samples against a live tape whose trade rate is not controlled between
runs; the spread within one build is the same order as the spread between
builds. What the readings do support is the negative claim the gate asks
for: nothing fell off the frame cap, and no surface moved by the kind of
margin a real regression produces. A change that made the report cost more
per frame would show as a dropped cap or a frame_cpu in the tens of
milliseconds, and none of the six runs does.

The positive claim rests on the code rather than the clock: this is a
relocation with the caches intact (above), and the golden test proves the
arithmetic underneath is unchanged.
