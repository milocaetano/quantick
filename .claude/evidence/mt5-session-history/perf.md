# What the whole session costs to open

Criterion **G4**.

**The numbers below are generated, not typed.** They are the output of
[`summarise_perf.py`](summarise_perf.py) run over the two logs committed beside
it, and reproducing them is one command:

```
python .claude/evidence/mt5-session-history/summarise_perf.py
```

That mechanism exists because this claim was wrong three times, and every time
the same way: figures read off a terminal by hand while a log from a *different*
run sat beside them in the repository. Round two caught a table quoting the
recovery frame as the load; round three caught the corrected table quoting nine
lines of which **none** appeared in the log committed under them. Hand
transcription is the defect, so the transcription is gone.

The script also decides *which* rows are the load, rather than leaving that to
judgement: the fill is bracketed by the log's own `MT5_BACKFILL_START` and
`BRIDGE_OPENING_COMPLETE` markers, and one summary past the close is kept
because a summary describes the two seconds before it.

## The two runs

Same binary, same window, same terminal, same tape. Only the opening block
differs: the control runs the **main checkout's** bridge (a rolling 12-hour
clock window), the branch runs its own (the session).

### Control — the main checkout's bridge — `perf-control.log`

Fill window: `2026-09-01T06:49:19.265940Z` to `2026-09-01T06:49:22.719420Z`; 3 health summaries inside it.
Trades charted (backfill + slices): **185 365**
`APP_SLOW_FRAMES` inside the fill: **0**

```
fps=57  avg= 17.50 cpu= 0.66 worst= 134.32
fps=59  avg= 16.67 cpu= 0.67 worst=  17.38
fps=59  avg= 16.81 cpu= 1.70 worst=  35.31
```

- fps floor **57**
- frame_avg peak **17.50 ms**
- frame_cpu peak **1.70 ms**
- worst single frame **134.32 ms**

### Branch — the session, in slices — `perf-branch.log`

Fill window: `2026-09-01T06:48:57.342414Z` to `2026-09-01T06:49:07.872234Z`; 6 health summaries inside it.
Trades charted (backfill + slices): **1 525 571**
`APP_SLOW_FRAMES` inside the fill: **0**

```
fps=59  avg= 16.81 cpu= 1.20 worst=  43.76
fps=59  avg= 16.81 cpu= 2.13 worst=  37.82
fps=59  avg= 16.94 cpu= 2.13 worst=  64.45
fps=56  avg= 17.64 cpu= 2.91 worst=  89.10
fps=57  avg= 17.36 cpu= 2.47 worst= 105.80
fps=54  avg= 18.33 cpu= 3.73 worst= 127.67
```

- fps floor **54**
- frame_avg peak **18.33 ms**
- frame_cpu peak **3.73 ms**
- worst single frame **127.67 ms**

## Reading it

| | control | branch | |
| --- | --- | --- | --- |
| trades charted | 185 365 | **1 525 571** | 8.2× |
| fps floor | 57 | **54** | −3 |
| frame_avg peak | 17.50 ms | **18.33 ms** | +0.83 ms |
| frame_cpu peak | 1.70 ms | **3.73 ms** | +2.03 ms |
| `APP_SLOW_FRAMES` in the fill | 0 | **0** | — |
| fill lasted | 3.5 s | **10.5 s** | |

**Eight times the trades for three fps, and no slow-frame warning on either
side.** The dip is about ten seconds, once, at open, and the chart is fully
interactive throughout: the first 200 000 prints are on screen within a second
of launch (see [`in-the-app.md`](in-the-app.md)).

**The worst-frame row is deliberately absent from that comparison.** Both runs
peak at a single spike near the first paint — 134 ms on the control, 128 ms on
the branch — so it measures the window opening, not the fill, and comparing the
two would read as the branch being *faster* rather than as noise.

## What an earlier slice size cost

The branch first sliced at **50 000** prints, which is 31 slices for a WINV26
session. Every slice is prepended through `ChartState::prepend_history`, which
re-cuts every bar the chart already holds, so a fill costs the slice count
times a growing tape. At that size the floor was **43 fps** with three
`APP_SLOW_FRAMES` — a real regression against the control, and `visual-qa`'s
own rule calls that a FAIL rather than an environment note.

`DEFAULT_OPENING_SLICE_TICKS` is 200 000 for that reason, and its doc comment
in `bridge/mt5/quantick_bridge.py` says so. It costs the trader nothing they
asked for: the *first* paint is the opening block, not a slice, so it lands in
under a second either way.

## The honest residual

`APP_SLOW_FRAMES` fires **after** the fill in some runs, while the chart holds
1.5 M prints and is being panned and re-cut by the capture harness. That is the
cost of *holding* a session rather than of loading one, it is outside the
window this file measures, and it is not compared against the control — whose
runs are short and never hold that much. Making a 1.5 M-print tape cheaper to
re-cut is a change to `ChartState`, not to this branch.
