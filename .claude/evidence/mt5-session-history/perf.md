# What the whole session costs to open

Criterion **G4**. Every figure here is a line from a log committed beside this
file. An earlier draft of this evidence quoted the *recovery* frame — the
health summary taken three seconds after the fill ended — as though it were the
load. `delivery-review` caught it. The numbers below are the load.

## The two runs

Both are the same binary, the same window, the same terminal, the same tape.
Only the opening block differs: the control runs the **main checkout's** bridge
(a rolling 12-hour clock window), the branch runs its own (the session).

- Control: [`perf-control.log`](perf-control.log) — `MT5_HISTORY_READY count=375262`
- Branch: [`perf-branch.log`](perf-branch.log) — 1 525 621 prints, 7 opening
  slices, 0 dropped

### Control — 375 262 trades

```
fps=58 avg=16.98 cpu=1.76 worst= 64.35
fps=59 avg=16.68 cpu=0.94 worst= 18.27
fps=59 avg=16.67 cpu=0.85 worst= 18.15
fps=59 avg=16.67 cpu=0.86 worst= 18.33
fps=59 avg=16.67 cpu=0.86 worst= 17.42
fps=59 avg=16.67 cpu=0.87 worst= 17.95
fps=58 avg=17.22 cpu=2.16 worst= 89.08
fps=59 avg=16.67 cpu=2.14 worst= 17.54
```

### Branch — 1 525 621 trades, four times the data

```
fps=59 avg=16.67 cpu=0.99 worst= 31.11
fps=57 avg=17.50 cpu=0.79 worst=145.30
fps=59 avg=16.80 cpu=1.73 worst= 45.10
fps=59 avg=16.81 cpu=2.21 worst= 44.29
fps=57 avg=17.50 cpu=3.02 worst= 89.74
fps=58 avg=17.22 cpu=2.61 worst= 99.43
fps=57 avg=17.50 cpu=2.83 worst=124.06
fps=54 avg=18.47 cpu=3.99 worst=135.83   <- the deepest point of the fill
fps=59 avg=16.67 cpu=1.79 worst= 17.04   <- recovered
```

| | control | branch | |
| --- | --- | --- | --- |
| trades charted | 375 262 | **1 525 621** | 4.1× |
| fps floor | 58 | **54** | −4 |
| frame_avg peak | 17.22 ms | **18.47 ms** | +1.25 ms |
| frame_cpu peak | 2.16 ms | **3.99 ms** | +1.83 ms |
| worst single frame | 89.08 ms | **145.30 ms** | +56 ms |
| `APP_SLOW_FRAMES` | 0 | **0** | — |

**Four times the trades for four fps and one dropped-frame spike, neither of
which crosses the application's own 20 ms slow-frame threshold.** The dip lasts
about eight seconds, once, at open, and the chart is fully interactive
throughout — the first 200 000 prints are on screen a second after launch.

## What the first measurement got wrong, and what fixed it

The first version of this branch sliced the opening block at **50 000** prints,
which is 31 slices for a WINV26 session. Every slice is prepended through
`ChartState::prepend_history`, which re-cuts every bar the chart already holds
— so the work of a fill is the slice count times a growing tape, and 31 of them
cost:

```
fps=49 avg=20.00 cpu=6.43 worst=105.22   <- APP_SLOW_FRAMES
fps=43 avg=22.78 cpu=9.65 worst=138.49   <- APP_SLOW_FRAMES
fps=46 avg=21.39 cpu=7.62 worst=142.70   <- APP_SLOW_FRAMES
```

A floor of **43 fps** and three slow-frame warnings. That is a real regression
against the control's 58, and `visual-qa`'s own rule calls it a FAIL rather than
an environment note.

Raising the slice to **200 000** — 8 slices for the same session — takes the
floor to 54 and the warnings to zero, and costs the trader nothing they asked
for: the *first* paint is the opening block, not a slice, so it still lands in
under a second either way. That is what `DEFAULT_OPENING_SLICE_TICKS` is sized
against, and its doc comment says so.

## The honest residual

The worst single frame is still 145 ms — one prepend of a 200 000-print slice
re-cutting a tape that is already a million long. Fewer, larger slices trade
sustained cost for a taller spike, and this is the far end of that trade. It
happens up to eight times, once, at open. Reducing it further needs the prepend
itself to become incremental rather than a re-cut, which is a change to
`ChartState` and not to this branch.
