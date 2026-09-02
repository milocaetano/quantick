# Performance — the harness hook owner

The gate this answers is `arch-review`'s: **every touched path classified by
rate, and a hot path proved flat rather than believed flat.**

**Classification.** The twenty-three hooks are read at three rates. Most are
**rare** — parsed once in `QuantickApp::new` and consumed by the first frame
that can honour them. Five are **per-frame**: `QUANTICK_CANDLE_WIDTH` and
`QUANTICK_PAN_PX` re-apply through `apply_scripted_view` every frame,
`QUANTICK_POINTER` is re-delivered as a real pointer event every frame, and the
three budgeted hooks (`QUANTICK_LOAD_OLDER`, `QUANTICK_LOAD_OLDER_CANDLES`,
`QUANTICK_HISTORY_NOTE`) tick a counter every frame. None is per-trade or
per-depth: nothing in `harness.rs` is reached from the feed or the book.

**What changed on those paths.** A field read on `QuantickApp` became a method
call on a struct one level in. No allocation, no lock and **no `clone()` at
all** — the module contains none.

It did, briefly. The first draft answered "is the drawings demo still owed?"
by handing back the request, which owns a `String`, so a hook waiting for bars
allocated sixty times a second to say "not yet". The first review round caught
it, and the shape it left is the one to copy: a `bool` peek
(`drawings_demo_armed`) for the question asked every frame, and a `take` for
the one frame that acts.

**What was measured.** `APP_HEALTH_SUMMARY`, seven samples per launch, newest
taken, across 32 scenes captured on both an `origin/main` control build and this
branch — the same matrix `visual-qa.md` describes, from the same launches.

| scene | fps (main → branch) | frame_avg ms | frame_cpu ms | frame_worst ms | slow-frame lines |
| --- | --- | --- | --- | --- | --- |
| `avwap` | 59 → 59 | 16.668 → 16.667 | 1.84 → 1.77 | 17.02 → 17.16 | 0 → 0 |
| `context-menu-chart` | 60 → 59 | 16.666 → 16.667 | 1.89 → 1.80 | 17.30 → 17.09 | 0 → 0 |
| `drawing-draft` | 60 → 60 | 16.667 → 16.667 | 1.66 → 1.88 | 16.98 → 16.96 | 0 → 0 |
| `drawings-bands` | 59 → 59 | 16.667 → 16.668 | 1.66 → 1.77 | 24.48 → 17.01 | 0 → 0 |
| `drawings-demo` | 60 → 60 | 16.666 → 16.665 | 1.51 → 1.45 | 17.25 → 17.46 | 0 → 0 |
| `drawings-recut` | 59 → 60 | 16.667 → 16.667 | 1.67 → 1.61 | 17.14 → 17.15 | 0 → 0 |
| `drawings-shared` | 60 → 59 | 16.666 → 16.667 | 1.48 → 1.45 | 17.21 → 16.96 | 0 → 0 |
| `footprint-zoom` | 59 → 60 | 16.667 → 16.666 | 1.96 → 1.76 | 17.16 → 17.19 | 0 → 0 |
| `frvp` | 59 → 59 | 16.667 → 16.670 | 1.72 → 1.65 | 17.14 → 17.24 | 0 → 0 |
| `frvp-compare` | 59 → 60 | 16.667 → 16.664 | 1.70 → 1.69 | 17.16 → 17.20 | 0 → 0 |
| `indicator-settings` | 59 → 59 | 16.668 → 16.670 | 1.86 → 1.88 | 16.92 → 17.11 | 0 → 0 |
| `layout-picker` | 59 → 59 | 16.667 → 16.668 | 1.68 → 1.64 | 17.33 → 17.32 | 0 → 0 |
| `maximized` | 59 → 59 | 16.667 → 16.667 | 1.78 → 1.72 | 17.21 → 17.13 | 0 → 0 |
| `menu-workspace` | 59 → 59 | 16.668 → 16.667 | 1.71 → 1.85 | 17.25 → 17.15 | 0 → 0 |
| `pan-left` | 59 → 60 | 16.667 → 16.666 | 1.57 → 1.52 | 17.04 → 17.14 | 0 → 0 |
| `pointer` | 59 → 59 | 16.667 → 16.667 | 1.69 → 1.77 | 17.00 → 16.90 | 0 → 0 |
| `r-avwap` | 60 → 59 | 16.666 → 16.667 | 8.33 → 8.58 | 18.54 → 18.94 | 0 → 0 |
| `r-drawing-draft` | 59 → 60 | 16.670 → 16.665 | 8.15 → 8.18 | 17.30 → 18.13 | 0 → 0 |
| `r-drawings-bands` | 59 → 59 | 16.667 → 16.667 | 8.01 → 7.22 | 17.84 → 18.33 | 0 → 0 |
| `r-drawings-demo` | 59 → 59 | 16.667 → 16.667 | 6.88 → 7.14 | 17.83 → 17.78 | 0 → 0 |
| `r-drawings-recut` | 59 → 60 | 16.668 → 16.666 | 8.80 → 8.91 | 16.96 → 16.96 | 0 → 0 |
| `r-drawings-shared` | 59 → 59 | 16.667 → 16.667 | 8.33 → 7.57 | 17.92 → 18.24 | 0 → 0 |
| `r-frvp` | 59 → 59 | 16.667 → 16.668 | 8.57 → 8.22 | 17.71 → 17.61 | 0 → 0 |
| `r-frvp-compare` | 59 → 59 | 16.668 → 16.668 | 8.34 → 8.53 | 17.45 → 17.63 | 0 → 0 |
| `r-history-note` | 59 → 59 | 16.667 → 16.668 | 7.97 → 8.73 | 17.92 → 18.80 | 0 → 0 |
| `r-replay-restart` | 59 → 59 | 16.667 → 16.668 | 8.09 → 8.50 | 18.36 → 18.69 | 0 → 0 |
| `r-strategy-armed` | 60 → 60 | 16.664 → 16.605 | 8.14 → 8.63 | 17.53 → 19.27 | 0 → 0 |
| `r-strategy-popup` | 59 → 59 | 16.667 → 16.667 | 8.51 → 8.20 | 21.15 → 18.51 | 0 → 0 |
| `settings-autostart` | 59 → 59 | 16.667 → 16.667 | 1.99 → 1.96 | 17.10 → 16.89 | 0 → 0 |
| `strategy-popup` | 60 → 59 | 16.666 → 16.667 | 1.73 → 1.98 | 17.23 → 17.23 | 0 → 0 |
| `venue-history` | 60 → 60 | 16.666 → 16.666 | 1.58 → 1.61 | 17.29 → 17.01 | 0 → 0 |
| `venue-history-part` | 59 → 60 | 16.667 → 16.664 | 1.59 → 1.59 | 17.28 → 20.14 | 0 → 0 |

- **main**: 32 scenes; fps min 59, mean 59.25; frame_avg mean 16.6670 ms; frame_cpu mean 4.136 ms; worst frame 24.48 ms; APP_SLOW_FRAMES lines 0
- **branch**: 32 scenes; fps min 59, mean 59.34; frame_avg mean 16.6649 ms; frame_cpu mean 4.149 ms; worst frame 20.14 ms; APP_SLOW_FRAMES lines 0
