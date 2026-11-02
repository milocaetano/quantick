# Appearance migration inventory

This is a checkpoint worklist, not a completed migration claim. The scan counts
renderer color constructors/constants and explicit font calls, including tests.
A match needs ownership classification before migration; transparency, geometry,
tests and authored style identities are not automatically theme fields.

Completed in the current seam:

- Appearance: complete bundled Dark fallback and font defaults resolved from TOML;
  configuration-owned footprint heat fill and ink arrays with Dark parity tests.
- App theme adapter: per-context color/typography cache; base egui chrome widgets
  and standard text roles; no per-frame parsing or font-definition replacement.
- Footprint cluster heat: cached palette read once per layer, fill and ink lookups.
- Chart: footprint heat percentile cuts/step decisions and width/fade arithmetic.
- Stores: configured footprint dressing, LOD caches and pane footprint state.

Required next migration:

- Replace transitional `appearance::palette`/`app::theme` constants with resolved
  palette snapshots across chrome, panels, axes, grids, candles and text. Remove
  runtime literal bypasses when their callers are migrated. Keep historical
  authored preset bytes explicit.
- Inventory orderflow gradients, bubble palettes, aggression/footprint colors and
  saved asset/preset paths. Uncustomized defaults inherit; saved/authored palettes
  remain explicit with per-field provenance. A width-only candle edit must not
  freeze background or color inheritance.
- New drawing defaults follow the theme unless a saved user default exists;
  existing drawing styles retain their authored colors. Treat indicator/script
  plot colors as authored identities, including built-in studies.
- Match each font consumer's current size/family before assigning a role. Explicit
  icon-font sizes and fitted quantity layout budgets need deliberate handling.
- Expose inspect/apply through the discoverable control contract; validate and
  persist before live publication. Add provenance roundtrip/switch tests.

Scan candidates at this checkpoint:

| File (first match line) | Matches |
| --- | ---: |
| `crates/app/src/app/tests/candle_aggression_tests.rs:305` | 1 |
| `crates/app/src/app/tests/chart_view_tests.rs:654` | 2 |
| `crates/app/src/app/tests/drawings_tests.rs:2332` | 1 |
| `crates/app/src/app/tests/flow_candle_paint_tests.rs:152` | 13 |
| `crates/app/src/app/tests/layers_tests.rs:923` | 1 |
| `crates/app/src/app/tests/native_tape_readback_tests.rs:341` | 3 |
| `crates/app/src/app/tests/pane_header_tests.rs:955` | 1 |
| `crates/app/src/candle_view/candles.rs:120` | 1 |
| `crates/app/src/candle_view/style_window.rs:260` | 5 |
| `crates/app/src/control/gateway/panel.rs:146` | 1 |
| `crates/app/src/drawings/context_bar.rs:846` | 3 |
| `crates/app/src/drawings/fib/levels_tab.rs:346` | 2 |
| `crates/app/src/drawings/fib.rs:101` | 2 |
| `crates/app/src/drawings/style.rs:31` | 1 |
| `crates/app/src/feed_notice.rs:329` | 1 |
| `crates/app/src/footprint_panel.rs:85` | 5 |
| `crates/app/src/footprint_render/bar.rs:394` | 7 |
| `crates/app/src/footprint_render.rs:438` | 8 |
| `crates/app/src/indicator_legend.rs:544` | 1 |
| `crates/app/src/indicator_panel.rs:369` | 1 |
| `crates/app/src/indicator_render.rs:130` | 10 |
| `crates/app/src/layout_picker.rs:203` | 1 |
| `crates/app/src/layout_strip.rs:244` | 1 |
| `crates/app/src/layouts.rs:152` | 2 |
| `crates/app/src/loading.rs:14` | 15 |
| `crates/app/src/orderflow_panel.rs:172` | 1 |
| `crates/app/src/orderflow_render/bubbles.rs:97` | 6 |
| `crates/app/src/orderflow_render/constants.rs:18` | 3 |
| `crates/app/src/orderflow_render/flow_execution/flow_labels.rs:34` | 2 |
| `crates/app/src/orderflow_render/flow_execution/flow_runs.rs:23` | 1 |
| `crates/app/src/orderflow_render/flow_execution.rs:151` | 3 |
| `crates/app/src/orderflow_render/flow_perimeter.rs:48` | 4 |
| `crates/app/src/orderflow_render/heatmap.rs:110` | 4 |
| `crates/app/src/orderflow_render/legend.rs:89` | 3 |
| `crates/app/src/orderflow_render/preview.rs:124` | 7 |
| `crates/app/src/orderflow_render/tape_path.rs:18` | 4 |
| `crates/app/src/orderflow_render/tests/dot_radii_tests/display_scale_tests.rs:93` | 1 |
| `crates/app/src/orderflow_render/tests/dot_radii_tests.rs:288` | 1 |
| `crates/app/src/orderflow_render/tests/flow_cache.rs:143` | 16 |
| `crates/app/src/orderflow_render/tests/flow_execution.rs:148` | 16 |
| `crates/app/src/orderflow_render/tests/flow_live_bench.rs:354` | 2 |
| `crates/app/src/orderflow_render/tests/flow_stage_profile.rs:64` | 2 |
| `crates/app/src/orderflow_render/tests/mod.rs:367` | 11 |
| `crates/app/src/orderflow_render/tests/tape_only_tests.rs:40` | 2 |
| `crates/app/src/orderflow_render/tests/tape_path_tests.rs:126` | 1 |
| `crates/app/src/orderflow_render/tests/tape_price_tests.rs:38` | 4 |
| `crates/app/src/orderflow_render.rs:50` | 2 |
| `crates/app/src/orderflow_view/frame.rs:441` | 4 |
| `crates/app/src/orderflow_view/tests/dense_frame_performance.rs:85` | 1 |
| `crates/app/src/orderflow_view/tests/native_split_tests.rs:156` | 1 |
| `crates/app/src/orderflow_view/tests/pending_tests.rs:157` | 1 |
| `crates/app/src/orderflow_view/tests/recorded_frame_performance.rs:192` | 1 |
| `crates/app/src/orderflow_view/tests/tape_frame_tests.rs:54` | 3 |
| `crates/app/src/orderflow_view/tests/tape_memory_tests.rs:52` | 2 |
| `crates/app/src/orderflow_view.rs:308` | 6 |
| `crates/app/src/pane/hint.rs:22` | 2 |
| `crates/app/src/pane/layer_painters.rs:376` | 1 |
| `crates/app/src/pane/menu_renderer.rs:91` | 1 |
| `crates/app/src/pane/painting.rs:23` | 4 |
| `crates/app/src/pane/render_registry/axes.rs:76` | 6 |
| `crates/app/src/pane/render_registry/days.rs:67` | 2 |
| `crates/app/src/pane/render_registry/dividers.rs:61` | 3 |
| `crates/app/src/pane/render_registry/drawings.rs:30` | 3 |
| `crates/app/src/pane/strategies.rs:301` | 1 |
| `crates/app/src/pane/tests/mod.rs:242` | 10 |
| `crates/app/src/pane/tests/native_split_tests.rs:84` | 2 |
| `crates/app/src/pane/tests/tape_only_tests.rs:76` | 2 |
| `crates/app/src/pane_header.rs:102` | 12 |
| `crates/app/src/paper_calendar.rs:317` | 4 |
| `crates/app/src/paper_chrome.rs:49` | 1 |
| `crates/app/src/paper_report/curve.rs:107` | 10 |
| `crates/app/src/paper_report/ledger.rs:363` | 1 |
| `crates/app/src/paper_report/rows.rs:191` | 8 |
| `crates/app/src/paper_report/sessions.rs:295` | 4 |
| `crates/app/src/paper_report/tables.rs:99` | 5 |
| `crates/app/src/paper_trading/paint.rs:218` | 3 |
| `crates/app/src/paper_trading/paint_ctx.rs:121` | 7 |
| `crates/app/src/paper_trading/ticket.rs:66` | 1 |
| `crates/app/src/paper_trading.rs:149` | 1 |
| `crates/app/src/pointer_compass.rs:151` | 3 |
| `crates/app/src/replay_get_data.rs:881` | 4 |
| `crates/app/src/replay_view.rs:818` | 6 |
| `crates/app/src/surfaces/chart_settings/candles.rs:209` | 1 |
| `crates/app/src/surfaces/drawing_chrome/inline_editor.rs:205` | 2 |
| `crates/app/src/surfaces/drawing_chrome/inspector.rs:561` | 2 |
| `crates/app/src/surfaces/indicator_picker.rs:84` | 3 |
| `crates/app/src/surfaces/indicator_preview.rs:80` | 1 |
| `crates/app/src/theme/chrome.rs:23` | 1 |
| `crates/app/src/theme/ink.rs:13` | 1 |
| `crates/app/src/theme/palette.rs:8` | 1 |
| `crates/app/src/theme/resolved.rs:31` | 2 |
| `crates/app/src/theme.rs:56` | 2 |
| `crates/app/src/time_header.rs:130` | 2 |
| `crates/app/src/toolbar.rs:612` | 1 |
| `crates/app/src/toolrail/band.rs:64` | 2 |
| `crates/app/src/toolrail/flyout.rs:244` | 4 |
| `crates/app/src/toolrail.rs:860` | 2 |
| `crates/app/src/trade_paint.rs:62` | 6 |
| `crates/app/src/widgets.rs:270` | 2 |

Total: 346 matches in 99 files.
