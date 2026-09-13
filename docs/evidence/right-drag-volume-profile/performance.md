# Performance evidence

## Rate classification

- Per frame while idle: one small transient-state reconciliation; no drawing
  allocation, trade scan, depth scan, or engine work.
- Per frame while pressed/ready: pointer checks plus one two-anchor measure-tool
  projection and paint.
- Rare: release, dismissal, tab reconciliation, and FRVP conversion.
- Existing FRVP cost: profile calculation remains owned by the registered FRVP
  drawing and runs only after the explicit action.

## Dense replay comparison

Both runs used `origin/main` `d8de9a1b`, the same dedicated target directory,
1400×900 points, the authored 6,000-print BTCUSDT replay at 100×, tick-50 bars,
the same dense bubble preset, isolated storage, and the same capture cadence.
The only candidate-only hook was `QUANTICK_QUICK_RANGE_DEMO=ready`, because the
base does not yet declare that surface.

At approximately 4,000 prints:

| Build/state | FPS | frame average | frame CPU | worst frame |
| --- | ---: | ---: | ---: | ---: |
| `origin/main`, no range | 59 | 16.669 ms | 8.341 ms | 17.515 ms |
| candidate, ready range | 60 | 16.666 ms | 8.665 ms | 17.803 ms |
| candidate, dismissed control | 59 | 16.669 ms | 8.029 ms | 17.237 ms |

The candidate ready state held the presentation target and improved measured
frame average by 0.003 ms. Its 0.324 ms CPU difference from `main` is 1.9% of
one 16.67 ms frame and within the paired-run variation; the same candidate with
the overlay dismissed was below `main`. No run emitted `APP_SLOW_FRAMES`.

The final dense static capture settled at 59 FPS, 16.668 ms average, and 9.600
ms CPU with all 4,000 prints visible. Verdict: **PASS — frame delivery is flat
or better and the feature adds no attributable slow-frame burst**.
