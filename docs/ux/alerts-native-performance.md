# Independent native dense replay comparison

The first sections preserve the earlier implementation measurement. The
exact-final `485a77bb` refresh and its provenance are recorded at the end.

On 2026-11-10 an independent agent compiled and ran the scaffold baseline and
the alert observer implementation in separate native Windows processes. The
observed dense replay frame-rate gate passes: all 90 retained health windows
reported 59-60 FPS, with no `APP_SLOW_FRAMES` event in any of the six runs.
The samples do not establish a clean-host causal CPU overhead bound: unrelated
compiler processes remained active on the shared machine.

## Artifacts and scenario

- Baseline: clean detached `839789cbc89c08343bc0ce889f30ea133e75d92b`, separately
  compiled with `cargo build -p quantick-app --features harness` in
  `C:/src/quantick-worktrees/alerts-perf-base`, using its own target directory.
  SHA-256 `441DB7EF98E4DF859A67E843571D07FEDEBEEF32FAD0A972F5632495524A069E`;
  modification time `2026-11-10T08:52:47.3382170Z`.
- Current immutable harness: SHA-256
  `B3294CF3F63975D9163735D9E638AC715D238668DA191D1AA4C51E40B9EE2969`;
  modification time `2026-11-10T08:55:29.7143331Z`. This predates the subsequent
  editor precision/header-only correction; the native observer was unchanged.
  The checkout had uncommitted implementation changes, so HEAD alone is not
  a full source identifier for this binary.
- One synthetic CSV shared by both: 240,000 prints at 5 ms spacing, 4x replay,
  about 800 prints/second and 80 `tick:10` committed closes/second. Tape SHA-256:
  `F80F4E4F8548CE18E82C79D40D364742957000B7D50B10BCD46CF93D048CD5F6`.
- Fresh isolated stores per run, dark theme, 1200x800 logical viewport,
  1800x1200 captured pixels, identical bubbles/configuration, no drawings or
  mirrors staged. Both omit the alert surface hook; current receives one
  no-fire `GreaterThan 1000000 / PerBarClose` rule through admitted `alerts.create`.
- Each current run read back `monitoring`, null reason, and empty history.
  No user stores, live-market trades, orders or notification delivery were used.

Runs alternated baseline/current three times. Each ran at least 40 seconds.
The first five two-second health windows were discarded; the next 15 per run
were retained, giving 45 windows per binary. Percentiles below are nearest-rank
percentiles of the **health-window aggregates**, not per-frame latency percentiles.

## Results

| Metric | Baseline | Current monitoring |
| --- | ---: | ---: |
| FPS range | 59-60 | 59-60 |
| Frame interval median (ms) | 16.668 | 16.667 |
| CPU mean / median (ms) | 6.174 / 6.096 | 6.075 / 6.091 |
| CPU p95 / maximum (ms) | 6.662 / 6.901 | 6.506 / 6.683 |
| Worst-frame window median / maximum (ms) | 17.656 / 32.522 | 17.574 / 18.282 |
| Trades/second mean | 798.429 | 798.371 |
| Worker backlog median / maximum | 10 / 13 | 12 / 19 |
| Parked / deferred / output-blocked maximum | 0 / 0 / 0 | 0 / 0 / 0 |
| Slow-frame event count, whole runs | 0 | 0 |

CPU medians by matched pair were 6.320/6.148, 6.154/5.999 and 6.003/6.144 ms
(baseline/current). There is no observed persistent CPU or frame-rate regression
in these samples. The slightly higher current backlog is reported rather than
hidden; it did not produce parking, deferral, blocked output or reduced throughput.
Replay does not provide a feed-arrival latency figure in these health records.

## Evidence and limits

Raw evidence is outside Git at `C:/src/quantick-worktrees/alerts-perf-evidence/`:
`base-1` through `base-3`, `current-1` through `current-3`, and `comparison.json`.
Every run contains a binary-hashed launch manifest, stderr JSON health log,
compiler inventory at start/end, PID-targeted `capture.png`, and
`health-summary.json`. Current runs also contain admitted `monitoring.json`.
The agent inspected all six screenshots: dense chart/tape data rendered,
the live price remained visible, and the status line showed normal frame rates.
All six owned windows were closed after capture.

Residual toolbar/widget compiler activity limits attribution. These are repeated
matched native samples on a shared host, not a statistical proof of zero overhead.
The performance scenario covers an active price rule and multiple committed
closes per frame; it does not cover active drawing geometry, mirrored sources,
hydration transitions, a manager overlay or notification bursts. Those need
their separate functional/observer evidence and must not be inferred from this
timing result. The independent API consumer report records a durable drawing
selector path, including its honest `OutsideTimeDomain` suspension.

## Exact-final refresh: 485a77bb

Two additional alternating baseline/final pairs were measured against committed
source `485a77bb802255c0dc420137e39997ac7fda1051`, tree
`43a4ed82a0025b6b17b6c4d6fb8f36a3b6f2e0e5`. The immutable final executable was
`quantick-alerts-485a77bb.exe`, SHA-256
`457E82EE5087E63C6DFEB0A12852F0C53F4C29ABA5316526BA456718B4960122`, modified
`2026-11-10T10:50:37.4483233Z`. Its build receipt is
`.artifacts/validation/harness-485a77bb.json`; the verified source manifest is
`.artifacts/validation/repaired-source-manifest.json`, SHA-256
`F623CD371D207C8FE7B268C6F5A30C73E85A4A06BE5EE49372AAAD751A6362A9`.
Both files were copied into each final run's raw evidence directory. The baseline
executable and dense CSV hashes above were verified unchanged. No build was run
for this refresh.

The same isolated configuration, viewport, tape, no-fire rule and sampling method
were reused. Each run retained 15 health windows after five warmup windows,
giving 30 retained windows per binary. Both final runs read back `monitoring`
with null reason and empty firing history. The paired CPU medians were
5.604/5.650 and 5.634/5.646 ms (baseline/final).

| Metric | Baseline refresh | Exact final monitoring |
| --- | ---: | ---: |
| FPS range | 59-60 | 59-60 |
| Frame interval median (ms) | 16.668 | 16.668 |
| CPU mean / median (ms) | 5.677 / 5.622 | 5.666 / 5.648 |
| CPU p95 / maximum (ms) | 6.090 / 6.514 | 6.084 / 6.112 |
| Worst-frame window median / maximum (ms) | 17.804 / 29.050 | 17.764 / 22.378 |
| Trades/second mean | 798.506 | 798.460 |
| Worker backlog median / maximum | 12 / 15 | 12 / 19 |
| Parked / deferred / output-blocked maximum | 0 / 0 / 0 | 0 / 0 / 0 |
| Slow-frame events, whole runs | 0 | 0 |

The exact-final dense replay frame-rate gate passes in these bounded samples.
No material frame-rate or CPU regression was observed; this is not a claim of
zero overhead. Baseline1's launch inventory contained three cargo processes.
Later inventories captured two cargo processes and a rustc process; no owned
compiler ran. Shared-host load therefore still limits causal CPU attribution.
`load-observation.json` records each start/end inventory and explains that the
legacy capture script emits no inventory file when no matching process exists.
These are instant observations, not continuous host-load measurements.

New raw directories under `C:/src/quantick-worktrees/alerts-perf-evidence/` are
`refresh-485a77bb-base-1`, `refresh-485a77bb-final-1`,
`refresh-485a77bb-base-2`, and `refresh-485a77bb-final-2`.
The pooled `comparison.json` and its aggregation script are in the last directory.
All four PID-targeted captures were inspected: chart and tape rendered, live price
remained visible, and the status line showed normal frame rates. Owned PIDs
31336, 46764, 37560 and 43496 were closed; subsequent process lookup confirmed
their absence. Geometry, mirrors, hydration and notification-burst coverage
remain outside this timing scenario, as in the earlier measurement.
