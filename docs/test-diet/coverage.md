# Test diet: which `crates/app` test files are redundant by coverage

Measured on `feature/test-diet` at `e77bdfbcb`, Windows, rustc 1.98.0,
cargo-llvm-cov 0.9.1. Report only: no test or production code changed.

## Question

For the 25 largest `crates/app/src/app/tests/*.rs` files plus four test
directories (`orderflow_render/tests`, `pane/tests`, `paper_trading/tests`,
`orderflow_view/tests`): which lines does each one cover that nothing else
covers? A line that only an app test reaches is a reason to keep that test; a
line a domain crate already tests at home is not.

## Method

1. One instrumented build of `quantick-app` and the 11 domain crates
   (`control-handlers`, `drawings`, `paper`, `sim`, `chart`, `orderflow`,
   `sources`, `session`, `workspace`, `engine`, `ui-model`). cargo-llvm-cov
   instruments workspace crates only.
2. **Baseline.** Every test binary of each domain crate runs alone (unit and
   integration tests, no doctests); the kept lines are those in the crate's own
   non-test sources.
3. **Units.** `tools/test_diet/coverage.py` resolves the app's module tree from
   `main.rs` (following `#[path]` and inline modules), lists the test binary's
   2,283 tests, and gives each test to the file that declares it. A top-level
   `app/tests/X.rs` owns its `app/tests/X/` subdirectory; a `#[path]` sibling
   such as `pane_header_tests.rs` (declared inside `retry_readback_tests.rs`) is
   its own file. The four directories each count as one unit. Every other app
   test (1,084) forms a `rest` unit, so "unique" is measured against the whole
   suite, not only the 29 measured units.
4. Each unit runs alone (`--exact` test names, batched) with its own profile,
   exported to lcov. Test sources (`tests/` directories, `tests.rs`,
   `*_tests.rs`) are excluded from every count.
5. **Unique lines** of a unit: app plus domain lines it covers that no other
   unit (`rest` included) and no domain crate's own tests cover.

Test lines: physical lines in the unit's files, comments and blanks included.

### Commands

From a worktree of `feature/test-diet`, in Git Bash:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
source tools/test_diet/env.sh          # coverage env, CARGO_BUILD_JOBS=2, clears QUANTICK_*
cargo test --no-run $PACKAGES --message-format=json-render-diagnostics > target/test-diet-build.json
python tools/test_diet/coverage.py units target/test-diet-build.json
python tools/test_diet/coverage.py run      # 11 domain baselines, 29 units, rest
python tools/test_diet/coverage.py table    # the tables below
rm -rf target                               # about 27 min to rebuild
```

The instrumented build took 27 min at two jobs; all 41 runs plus the exports
took under 3 min. Every test passed in every run (ignored tests stay ignored).

## Baseline: domain crates under their own tests

| crate | instrumented lines | covered by own tests | own % | covered by app suite | app-only (not own) |
|---|---:|---:|---:|---:|---:|
| `quantick-control-handlers` | 7829 | 4735 | 60% | 6183 | 2547 |
| `quantick-drawings` | 11850 | 10332 | 87% | 6732 | 926 |
| `quantick-paper` | 7146 | 6195 | 87% | 4330 | 587 |
| `quantick-sim` | 2961 | 2896 | 98% | 1147 | 17 |
| `quantick-chart` | 12086 | 11320 | 94% | 5898 | 640 |
| `quantick-orderflow` | 17689 | 16571 | 94% | 11390 | 623 |
| `quantick-sources` | 2540 | 2329 | 92% | 1188 | 103 |
| `quantick-session` | 1694 | 1289 | 76% | 1160 | 245 |
| `quantick-workspace` | 7811 | 6944 | 89% | 6389 | 616 |
| `quantick-engine` | 5062 | 4790 | 95% | 2246 | 164 |
| `quantick-ui-model` | 4577 | 3699 | 81% | 3579 | 658 |

"App-only" is domain code that only the app suite reaches. `control-handlers`
(2,547 lines) is the one crate whose own tests leave a large share to the app.

## Per-file table

Columns: *app lines* / *domain lines*, non-test lines the unit covers;
*domain overlap*, share of those domain lines the domain crate's own tests also
cover; *unique*, lines no other test covers (see Method); *unique vs measured
only*, the same against the 29 measured units without `rest`; *other-crate
unique*, lines in workspace crates outside the 11 (feed, stores, control,
indicators...) no other app test reaches, which the verdict does not count.

| file | tests | test lines | app lines | domain lines | domain overlap | unique | unique vs measured only | unique / covered | unique / test lines | other-crate unique | verdict | most unique lines in |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| `app/tests/control_plane_tests.rs` | 108 | 6704 | 25194 | 20951 | 85% | 711 | 1331 | 1.5% | 10.6% | 601 | keep | app/src/control/gateway/server.rs 172, control-handlers/src/analysis.rs 63 |
| `app/tests/drawings_tests.rs` | 132 | 6414 | 24946 | 22860 | 88% | 327 | 1184 | 0.7% | 5.1% | 81 | slim | app/src/pane/render_registry/dividers.rs 37, app/src/drawings/context_bar.rs 31 |
| `orderflow_render/tests` | 103 | 6337 | 1845 | 3837 | 94% | 213 | 249 | 3.7% | 3.4% | 9 | slim | app/src/orderflow_render/flow_execution.rs 68, app/src/orderflow_render/bubbles.rs 54 |
| `pane/tests` | 103 | 5163 | 3960 | 5329 | 90% | 310 | 357 | 3.3% | 6.0% | 63 | slim | app/src/pane/chart_pane.rs 60, chart/src/pane_series.rs 38 |
| `app/tests/panes_layout_tests.rs` | 88 | 4140 | 22027 | 18424 | 89% | 259 | 550 | 0.6% | 6.3% | 28 | slim | app/src/app/layout_wiring/strip.rs 64, app/src/tab/canvas.rs 45 |
| `paper_trading/tests` | 96 | 3883 | 2670 | 4678 | 91% | 317 | 1296 | 4.3% | 8.2% | 121 | slim | app/src/paper_trading.rs 60, paper/src/desk/gesture.rs 38 |
| `app/tests/paper_trading_tests.rs` | 64 | 3578 | 22847 | 23486 | 89% | 166 | 261 | 0.4% | 4.6% | 108 | slim | control-handlers/src/trade.rs 37, app/src/paper_trading/paint.rs 24 |
| `app/tests/pane_header_tests.rs` | 72 | 3119 | 20714 | 14592 | 88% | 151 | 701 | 0.4% | 4.8% | 16 | slim | app/src/pane_header.rs 72, app/src/pane/chart_pane.rs 12 |
| `orderflow_view/tests` | 44 | 3115 | 2239 | 7193 | 95% | 33 | 66 | 0.3% | 1.1% | 7 | delete-candidate | app/src/orderflow_view.rs 12, app/src/orderflow_view/frame.rs 10 |
| `app/tests/feeds_sources_tests.rs` | 59 | 2850 | 17947 | 15545 | 89% | 90 | 181 | 0.3% | 3.2% | 230 | slim | sources/src/config.rs 19, app/src/control/health.rs 16 |
| `app/tests/retry_readback_tests.rs` | 16 | 2217 | 26972 | 25788 | 86% | 425 | 2054 | 0.8% | 19.2% | 59 | keep | control-handlers/src/layout.rs 114, control-handlers/src/layout/v2.rs 88 |
| `app/tests/tape_navigation_tests.rs` | 31 | 1884 | 20177 | 18490 | 90% | 46 | 217 | 0.1% | 2.4% | 55 | slim | app/src/orderflow_view/frame.rs 10, app/src/orderflow_view.rs 8 |
| `app/tests/layers_tests.rs` | 25 | 1422 | 18327 | 15477 | 89% | 36 | 83 | 0.1% | 2.5% | 6 | slim | app/src/app/chart_layers_wiring.rs 17, app/src/orderflow_view.rs 7 |
| `app/tests/session_length_tests.rs` | 6 | 1355 | 16066 | 12529 | 89% | 6 | 6 | 0.0% | 0.4% | 0 | delete-candidate | app/src/indicator_render.rs 3, app/src/pane/series.rs 2 |
| `app/tests/home_tab_tests.rs` | 28 | 1168 | 17769 | 13426 | 88% | 40 | 85 | 0.1% | 3.4% | 0 | slim | app/src/tabstrip.rs 15, app/src/app/home_wiring.rs 8 |
| `app/tests/workspaces_tests.rs` | 28 | 1098 | 18587 | 15374 | 88% | 24 | 92 | 0.1% | 2.2% | 10 | slim | drawings/src/tools/fib.rs 8, app/src/app/workspace_save_adapter.rs 5 |
| `app/tests/tab_workspace_tests.rs` | 29 | 1057 | 17647 | 14598 | 88% | 146 | 172 | 0.5% | 13.8% | 69 | keep | app/src/app/menu_bar/workspace_rows.rs 110, app/src/app/chart_layers_wiring.rs 21 |
| `app/tests/input_ui_tests.rs` | 25 | 1055 | 18782 | 15775 | 88% | 82 | 92 | 0.2% | 7.8% | 0 | slim | app/src/app/drawing_controller/input.rs 32, app/src/app/drawing_controller/access.rs 20 |
| `app/tests/control_launch_baselines.rs` | 4 | 1011 | 18825 | 12562 | 86% | 60 | 585 | 0.2% | 5.9% | 9 | slim | app/src/app/frame.rs 16, app/src/control/gateway/screenshot.rs 11 |
| `app/tests/chart_view_tests.rs` | 23 | 989 | 18429 | 14852 | 89% | 51 | 317 | 0.2% | 5.2% | 10 | slim | app/src/harness.rs 26, app/src/app.rs 10 |
| `app/tests/indicators_tests.rs` | 19 | 959 | 18360 | 14316 | 88% | 102 | 163 | 0.3% | 10.6% | 86 | keep | app/src/app/layout_wiring/indicators.rs 65, app/src/app/drawing_controller/view.rs 15 |
| `app/tests/stage_order_tests.rs` | 12 | 928 | 21644 | 16315 | 87% | 122 | 477 | 0.3% | 13.1% | 32 | keep | app/src/indicator_panel.rs 28, app/src/app/replay_and_history.rs 17 |
| `app/tests/calendar_history_tests.rs` | 13 | 907 | 15308 | 11907 | 87% | 76 | 81 | 0.3% | 8.4% | 23 | slim | session/src/venue.rs 45, app/src/tab/feed.rs 16 |
| `app/tests/script_editor.rs` | 14 | 786 | 16234 | 10594 | 87% | 112 | 286 | 0.4% | 14.2% | 85 | keep | app/src/app/script_editor_wiring.rs 79, app/src/app/script_reload.rs 23 |
| `app/tests/native_tape_readback_tests.rs` | 4 | 688 | 17078 | 12935 | 88% | 0 | 8 | 0.0% | 0.0% | 0 | delete-candidate |  |
| `app/tests/frame_tail_tests.rs` | 8 | 684 | 17574 | 13371 | 88% | 64 | 70 | 0.2% | 9.4% | 0 | slim | app/src/app/frame.rs 29, app/src/app/frame_tail.rs 22 |
| `app/tests/asset_bubbles_audit_tests.rs` | 7 | 683 | 12670 | 8146 | 86% | 23 | 26 | 0.1% | 3.4% | 105 | slim | app/src/orderflow_view/settings.rs 23 |
| `app/tests/orderflow_tests.rs` | 18 | 682 | 17151 | 13990 | 88% | 28 | 44 | 0.1% | 4.1% | 1 | slim | app/src/pane/context_menu.rs 7, app/src/app.rs 6 |
| `app/tests/drawing_magnet_tests.rs` | 20 | 680 | 16946 | 14726 | 90% | 2 | 20 | 0.0% | 0.3% | 0 | delete-candidate | drawings/src/placement.rs 2 |

### Verdict rule

The brief's threshold, "<2% unique coverage", read as a share of the file's own
coverage (*unique / covered*), puts 26 of 29 files under 2%: any test that builds
a `QuantickApp` and runs a frame covers 12,000 to 27,000 lines of shared startup
and frame code, which swamps the denominator. That reading cannot tell
`session_length_tests.rs` (6 unique lines) from `drawings_tests.rs` (327).

The verdict therefore takes the unique lines bought per test line
(*unique / test lines*): **delete-candidate** under 2%, **slim** from 2% to 10%,
**keep** at 10% or more. Both columns are in the table.

## What this means

| verdict | files | test lines | unique lines they hold |
|---|---:|---:|---:|
| delete-candidate | 4 | 5,838 | 41 |
| slim | 19 | 47,067 | 2,363 |
| keep | 6 | 12,651 | 1,618 |

- **Delete candidates (5,838 test lines, 41 unique lines):**
  `orderflow_view/tests`, `session_length_tests.rs`,
  `native_tape_readback_tests.rs`, `drawing_magnet_tests.rs`. Deleting all four
  loses coverage of 41 lines; the "most unique lines in" column names them, so a
  one-line test elsewhere can take them over first. `orderflow_view/tests` (4)
  and `session_length_tests.rs` (2) also hold `#[ignore]`d benchmarks and long
  variants that did not run here, and the session-length test bounds memory and
  time, which line coverage cannot value; decide those on what they assert, not
  on lines.
- **Slim (47,067 test lines, 2,363 unique lines):** each file's unique lines
  sit in one or two production files (last column). Keep the tests that reach
  those, drop the rest; the 85% to 95% domain overlap says most of what remains
  re-runs domain code its own crate already tests.
- **Keep (12,651 test lines):** the control-plane, retry/readback, workspace
  menu rows, indicator layout, stage order and script editor files carry the
  app's only coverage of gateway, layout handler and wiring code.
- **Upper bound, not a plan:** deleting every delete-candidate and slim file
  would drop 52,905 test lines (81% of the 65,556 measured) and 2,404 covered
  lines. Line coverage does not see assertions: two tests that walk the same
  lines but check different outcomes look redundant here.

## Limits

- Line coverage only; no branch or assertion weight.
- `other-crate unique` lines (feed, stores, control, indicators, ...) are not in
  the verdict because those crates' own tests were not measured.
  `feeds_sources_tests.rs` (230), `control_plane_tests.rs` (601),
  `paper_trading/tests` (121) and `asset_bubbles_audit_tests.rs` (105) hold the
  most; check them before deleting.
- llvm-cov reported mismatched data for 1 function in `quantick-chart` and 48 in
  `quantick-engine` while merging their integration-test binaries; those
  functions' counters are dropped, which can only understate the baseline and so
  overstate an app file's unique lines.
- Domain crates were built with the features the app unifies onto them (for
  example `quantick-drawings/test-support`), not their standalone feature set.
- The `surfaces` modules declared through `surface_catalog!` are not walked by
  the module resolver; their tests fall to `rest`, which is where they belong.
- One app-binary profile (`crates/app/default_*.profraw`, about 26,000 counts)
  was written outside any run's profile path while the baselines ran and is in
  no unit's numbers; its origin was not traced.
