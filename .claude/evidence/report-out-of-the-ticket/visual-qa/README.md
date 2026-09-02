# A13 / R15 / G5 — the surfaces before and after

Three surfaces, both builds, captured back to back from the same fixed
journal (83 trades, 3 session files, 2 symbol folders, fixed dates in
2026) so the only variable is the code.

Scene outer, build inner, so a scene's two frames are seconds apart: the
ledger renders trade ages, and two runs an hour apart would differ for
reasons that have nothing to do with this change. Every `QUANTICK_*` store
variable pointed at a per-run scratch directory — sharing one let the
first run write a dock state the next read back, which showed up in the
first comparison as a difference the change had not made.

| surface | hook | health at capture |
| --- | --- | --- |
| report window | `QUANTICK_PAPER_REPORT_AUTOSTART=1` | 59 fps both builds |
| calendar | `QUANTICK_PAPER_CALENDAR=2026-04-15` | 60 / 59 fps |
| ledger | `QUANTICK_DOCK_TAB=trades` + `QUANTICK_LEDGER_SCOPE=all` + `QUANTICK_LEDGER_PAGES=2` | 59 fps both builds |

## Result

Compared over each surface's own rectangle. The rest of the frame is a
live Binance tape and a moving clock, which no two runs can share.

| surface | region | differing pixels | verdict |
| --- | --- | ---: | --- |
| ledger | 340×527 | **0 / 179,180** | **identical** |
| report | 914×628 | 160 / 573,992 (0.028%) | **identical** — see below |
| calendar | 914×628 | 151 / 573,992 (0.026%) | **identical** — see below |

## The 160 and 151 pixels are the tape, and here is the proof

They are not scattered. In the report they sit inside a single 22×9 box at
frame x 536–557, y 158–166; in the calendar, in the two boxes at x 189–557,
y 158–190. That is exactly where the chart's floating indicator legend
prints its live **CVD (script)** reading, which is drawn over the report
window and belongs to the tape, not to the report.

The control run settles it. The **same** build captured twice, minutes
apart, with nothing changed at all:

```
CONTROL (before build, two runs): differing px 79 / 573,992; box x 474..483 y 106..114
```

A build cannot differ from itself. The box the control differs in is
inside the box the before/after comparison differs in, so that band is
live data on both counts. Every pixel of the report, the calendar grid,
the equity curve, the tiles, the trade list and the ledger is identical.

## What the frames show

`report-*-crop.png` — 83 trades, 51% win rate, 2.74 profit factor,
+403/−147, 42 W · 33 L · 8 scratch, the realized-equity curve with its
−11 pts drawdown marker, and the first six rows of the trade list. Same
numbers in both builds, which is the same claim
`the_report_numbers_are_fixed` makes in the test suite, made again through
the window.

`calendar-*-crop.png` — the month grid expanded on 2026-04, the picked day
chipped as `2026-04-15`, the period pills stood down, the report cut to
that day (2 trades, +15 pts, 50% win) and the support line saying
honestly that 81 saved trades sit outside the picked dates.

`ledger-*-crop.png` — the Trades dock scoped to all symbols, two pages
deep, day headers carrying each day's count and net, and the totals strip
reading `83 trades · 50% win · +256 pts`. Byte-identical between builds.

## Not run, and why

`trader-ux-review` was recorded in the goal file as not applicable: no
surface changes, so no trader-severity finding can be introduced, and the
identical-pairs result above is the stronger claim. A UX review here would
be grading `origin/main`.
