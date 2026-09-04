# PR 2 observer performance evidence

> **Archaeology, not current state.** This document records what was true
> when it was written and is kept for the reasoning it carries. For what
> has shipped, ask the registry — see [Precedence](../README.md#precedence).

| Environment | Value |
| --- | --- |
| Date | 2026-08-19 |
| Baseline | `origin/main` at `f22805ce2e114cf7592fc3a6a4fd37989ef80357` |
| Candidate | `feat/control-observer` working tree |
| Host | Intel Xeon E5-2680 v4, Windows 10 22H2 x86-64 |
| Toolchain | Rust and Cargo 1.97.1, MSVC target, workspace optimized test profile, `CARGO_INCREMENTAL=0` |

## Idle-frame method

The ignored `control_idle_dense_replay_benchmark` test is compiled into both
the baseline and candidate from identical source. It creates the same headless
`QuantickApp`, installs 8,000 historical trades, warms up for 30 frames, and
then measures 600 real application frames while delivering 64 live trades per
frame. The window is 1,400 by 900 pixels. No observer request is pending and no
projection registry is constructed.

After both binaries were built, they were executed in alternating baseline /
candidate order for five paired samples. This avoids comparing a cold process
with a warm one. The table reports the median of those five samples; each side
therefore covers 3,000 measured frames and 192,000 live trades.

| `APP_HEALTH_SUMMARY`-equivalent field | `origin/main` | Candidate | Delta |
| --- | ---: | ---: | ---: |
| `frame_cpu_ms` | 0.653476 ms | 0.655126 ms | +0.25% |
| frame p99 | 0.897800 ms | 0.893300 ms | -0.50% |
| `frame_worst_ms` | 1.515000 ms | 1.433600 ms | -5.37% |
| `trades_per_s` | 97,468.559 | 97,198.654 | -0.28% |

The fixture uses deterministic historical timestamps, so its absolute
`feed_arrival_ms` is intentionally meaningless and is not compared. The
arrival workload itself is identical on both sides.

The observed differences are smaller than run-to-run variance and show no
measurable idle-frame regression. This matches the structural result: outside
tests, `standard_registry` and `capture` are referenced only inside
`crates/app/src/control/`; `QuantickApp::draw_frame` and `eframe::App::update`
do not construct, lock, allocate for, or call the observer. PR 3 will add the
bounded request-time dispatcher.

## On-demand capture calibration

`observer_core_capture_p99_stays_within_the_ui_budget` builds an app with 2,000
historical bars, warms the registry for 25 captures, and measures 500 coherent
captures of all seven initial scopes. The measurement includes revision
projection and owned DTO construction on the calling thread. It excludes JSON
serialization, which begins only after the owned `SnapshotCapture` leaves the
UI thread.

Repeated measurements put p99 between **22 and 28 microseconds**, with the
worst capture between **24 and 29 microseconds**. `CONTROL_UI_BUDGET_US` is
therefore calibrated from its 1,000 microsecond opening cap to **250
microseconds**, approximately nine times the higher observed p99. Later
snapshot modules must preserve the same test or paginate their data before
docking.

## Reproduction

```powershell
$env:CARGO_TARGET_DIR = 'D:\cargo-target\quantick-control-observer'
$env:CARGO_INCREMENTAL = '0'
cargo test -p quantick-app app::tests::control_idle_dense_replay_benchmark -- --ignored --exact --nocapture --test-threads=1
cargo test -p quantick-app app::tests::observer_core_capture_p99_stays_within_the_ui_budget -- --exact --nocapture --test-threads=1
```

Run the idle benchmark on `origin/main` and the candidate from the same host,
and alternate process order when collecting multiple samples.
