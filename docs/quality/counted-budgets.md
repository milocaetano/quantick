# Counted hot-path budgets

Each hot-path crate holds its own paths to heap-work budgets in the ordinary
`cargo test`: allocations, bytes allocated and bytes a reallocation may copy,
per unit, read by the counting allocator `quantick_engine::work_meter`
(re-exported as `quantick_chart::work_meter`). Each test binary installs it
as its `#[global_allocator]`. Counts do not move with the host's load, so no
assertion needs a stopwatch.

| Crate | Consts in | Paths |
| --- | --- | --- |
| engine | `crates/engine/tests/hot_path_budget.rs` | `push_into` per trade for tick, volume, dollar, time, imbalance (trades, volume, dollar), renko, deals |
| orderflow | `crates/orderflow/tests/hot_path_budget.rs` | `record_trade` per print, `handle_depth_event_at` per update, interleaved with the prints that move the clock and close bars, `project_at` per frame, each counted frame a rebuild (a projection interval apart, past the 220 ms cache), a live FLOW frame per print (regional and tick spheres) |
| chart | `crates/chart/tests/hot_path_budget.rs` | per drawn frame: `project_styled_flow_executions` (worker subtracted), `FlowPresentation::styled` + `place_flow_chrome` |

`app`'s `session_length_tests` already hold `trade.chart`, `trade.book`,
`depth.book`, `frame.book`, `frame.app` and `frame.worker` through the app
binary, against session length; these budgets do not replace them.

Each const states its calibration and the SHA it was measured at
(`ff300983`, `3f9380bd`, `d5b0f95f`, `d91a4c29` and `53266e52`, the branch
heads at measurement); every budget is at most twice its calibration. Two
are tighter or looser for a stated reason: `orderflow.frame.book` declares
its bytes at 1.46x so that a copy of the liquidity history per rebuild
crosses it, and `orderflow.depth.book` measured zero copy bytes and allows
64 per update. Every engine builder measured zero, so every engine budget is
zero; a red engine test names the first trade whose push did heap work.

## Each budget fails on a plausible regression

Injected one at a time, the test run, then reverted; none is in the tree.

| Budget | Injection | Measured per unit (budget) |
| --- | --- | --- |
| `engine.push.*` (all nine) | a `Vec` holding the trade, built in `Bar::extend` | about 1 alloc, 55 bytes per trade, renko 0.68 / 38 (0 / 0); tick names trade 5001 (agg_id 5001), 1 alloc, 56 bytes |
| `orderflow.trade.book` | a `String` formatted per print in `record_trade` | 1.033 allocs (0.06), 17.1 bytes (16) |
| `orderflow.depth.book` | the archived runs collected into a fresh `Vec` per update in `apply_delta` | 3,318,360.6 bytes (12,288), interleaved with prints |
| `orderflow.frame.book` | the liquidity history cloned per `project_at` | 10,179,414 bytes (8,388,608) per rebuild |
| `orderflow.trade.flow.spheres` | the worker's memo bypassed (`project` for `project_cached`) | 1,160.9 allocs (500), 813,756 bytes (196,608) |
| `orderflow.trade.flow.regions` | a second, discarded full projection per publication | 2,318.7 allocs (2,000), 1,620,875 bytes (1,572,864) |
| `chart.frame.flow_session` | the visible tape cloned per frame | 329,186 bytes (160) |
| `chart.frame.flow_plan` | a `String` formatted per region in the plan | 832.4 allocs (6) |

Two injections that did not fail are worth knowing: a deep copy of each
published FLOW frame (+137 allocations a print on the regional frame) stays
inside both FLOW budgets, and bypassing the memo changes nothing on the
regional frame, whose untyped reference never uses it. A budget catches what
roughly doubles a path's heap work (on the frame's bytes, what adds about
half), not every extra allocation.

## Measured, not fixed

The regional FLOW frame allocates about 4 MB per five-print frame over a
6,000-print window (1,161 allocations a print), because an untyped reference
is projected whole every frame; the tick-sphere frame, which keeps its memo,
costs about a quarter of that. Budgeted at today's cost.

## What counted budgets cannot catch

A CPU slowdown that allocates nothing: a loop that walks retained history
without copying it, a quadratic sort, a cache that stops hitting but rebuilds
into reused buffers. Those show only on a clock. Timing stays in the
`#[ignore]` tests run by name (`flow_live_bench.rs`, the long
`session_length_tests` variant), in `cargo bench -p quantick-engine`, and in committed
measurements under `docs/quality/`, taken on an idle host; the one taken
with these budgets is `docs/quality/frame-timing-234852f6/frame-timing.txt`
(base and head about one base standard deviation apart, measured beside
the trader's live session, not idle).
