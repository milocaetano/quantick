# Appearance migration inventory

This is a checkpoint worklist, not a completed migration claim. The scan counts
renderer color constructors/constants and explicit font calls, including tests.
A match needs ownership classification before migration; transparency, geometry,
tests and authored style identities are not automatically theme fields.

Completed through the consumer checkpoint:

- Appearance: bundled Dark fallback, named renderer palette accessors, resolved
  typography and configuration-owned footprint heat fill/ink arrays.
- Chrome/panels: legacy theme tokens now resolve from per-context snapshots across
  menus, rails, headers, tabs, docks, replay, indicators, settings, paper trading,
  calendar, ledger and report panels. Icon buttons resolve their default accent
  at paint time; an explicitly supplied accent stays explicit.
- Chart overlays: axis text and crosshairs, day/seam/gap markers, footprint
  plates/POC/side ink, trade marks, paper overlays and drawing context swatches
  use the snapshot. Footprint row helpers borrow the existing frame snapshot.
- Headless owners: footprint projection/LOD/cache policy from checkpoint 2;
  paper report calendar statistics/selection and virtual ledger rows/pagination
  with their tests. App retains calendar and ledger painters.
- Runtime `app::theme` color constants are gone; the small remaining constants
  module is compiled only for historical Dark regression tests.

Completed in the typography checkpoint:

- Header/rail/loading/report/ticket and chart/indicator text painters use resolved
  role-relative fonts, preserving historical Dark sizes and families. Plot input
  and painting share configured axis reservations. Footprint LOD, row merging and
  fitted text share headless `FootprintText` budgets.
- Loading text/dots/logo and footprint profile/bevel/total ink plus crosshair ink
  have explicit Dark/Light tokens. Logo ramp conversion is cached on changes.
- Feed recovery report/recovery decisions and six tests moved to `feed::notice_report`.
  Footprint fitting and logo color projection live in chart; alpha composition and
  typography/layout policy live in appearance.

Required next migration:

- Direct non-token color literals remain, especially orderflow gradients/bubble
  palettes, caption shadows and overlays.
  Audit ordinary egui cursor, disabled, selection and hovered defaults as well.
- Remaining text candidates: cached FLOW quantity labels, bubble labels, orderflow
  legends/preview/heatmap/tape annotations, indicator picker/preview and menu text.
  Icon glyph dimensions and authored drawing/script text are intentional geometry
  or object properties. Fixed report row/header heights still need a large-font
  layout audit; measured elision is configured, but fixed-height rows may clip at
  extreme supported role sizes. Full text-layout acceptance is not claimed.
- Orderflow/bubble/footprint saved knobs still require per-field inheritance
  provenance. Chart/candle/canvas and new drawing defaults are implemented below;
  authored drawings, scripts, indicators and presets retain their colors.
- Expose discoverable inspect/apply operations, atomically persist before live
  publication, and invalidate any cached painted text/meshes on palette revision.
- Independent Dark/Light visual and performance acceptance remains outstanding.

The following table is the earlier literal scan, retained as a candidate worklist;
its counts have not been presented as a completed semantic audit.

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


## Provenance checkpoint

Chart candle fill/outline/wick, background and grid now resolve inherited fields
through `chart::style::ChartColorOwnership` with a theme/input change gate. UI color
edits mark only the changed field; geometry edits do not freeze colors. Built-in
presets capture candles, saved presets capture the whole look, and persisted legacy
concrete fields are explicit. Session chart edits still require a named preset for
durability, matching the existing workflow.

`drawings::PresetHost` supplies the retained theme for all default creation paths.
Explicit saved style/configuration wins. New text captures size/family; legacy text
keeps proportional font. Translucent saved drawing styles retain premultiplied bytes.
Existing drawn-object and script-authored values remain snapshots.

Still required: orderflow/bubble/footprint ownership and source-presence audit,
orderflow colors and typography cache invalidation, enlarged-font layout fixes,
discoverable atomic inspect/apply, and independent Dark/Light visual/performance
validation. This checkpoint is not full appearance acceptance.

Drawing decoration still needs a separate ambient/authored classification:
`drawings::tools::text::shown` has the empty-note `TEXT_MUTED` placeholder;
`drawings::tool` uses `SELECTION_HALO_COLOR`, `ACCENT` and `SELECTED_ANCHOR_FILL`
for selection; `tools::anchored_vwap` uses `CANVAS` for its anchor core;
`tools::measure_core` uses `BUY`/`SELL` and monospace `READOUT_TEXT_PX`;
`tools::fib` uses monospace `FIB_LABEL_SIZE_PX`; `tools::fixed_range_profile` and
its `paint` module use `CASING` and related text/profile decoration. Their shared
`drawings::palette` aliases remain historical constants. Preserve authored object
colors, level payloads, and existing text while resolving ambient decoration; do
not infer ownership from equal colors or blindly scale existing mark geometry.


## Order-flow cache and typography checkpoint

Regional FLOW fill and opening perimeter use `flow_buy`/`flow_sell`; tape path and
held/retained ink use `tape_path`/`tape_label`; quantity shadows use
`quantity_shadow`. The composition policy remains in orderflow/chart and honors
configured premultiplied alpha. The settings preview now uses configured canvas.
The old app FLOW and shadow constants remain only as exact Dark regression fixtures.
The headless historical `BLUE`/`AMBER` API remains available to existing tests and
callers, but production regional geometry passes configured sides.

FLOW, native-tape quantities, compact legends, depth/gap captions, settings preview
labels and live-strip empty text use the retained ChartLabel role. Measurement and
painting share the same snapshot. FLOW drawing and reusable-run cache keys include
appearance identity; changing color/font cannot reuse stale geometry or placement.
The compact legend reserves measured text height. Preview fixed geometry and the
broader large-font layout audit remain outstanding.

Ownership audit: `stores::footprint_config::FootprintConfig` and footprint presets
contain numeric/layout/switch choices, no persisted RGB fields; footprint paint
already uses semantic theme values. Do not invent a color-provenance marker for a
store that does not persist colors. Native bubble overrides are optional RGB in
`BubbleStyle`; their `Some` values are explicit. HeatmapTheme, default preset
source and saved asset/preset capture still need their coordinated owner seam.
No palette/source inference from equal RGB has been introduced.


## Live configuration boundary

`appearance.state` and `appearance.apply` now expose bounded inspection and
atomic session/durable application. Stores owns the selected destination,
startup fallback, byte limits, source/durability state and write-before-publish
policy. The handler names field-specific validation errors; the existing
cockpit/layout permission and idempotency gates authorize and deduplicate writes.
The adapter publishes chart inputs, drawing defaults and the retained context
palette before returning to the next command or paint. Font definitions remain
installed, text roles update only on typography changes, and unchanged frames
retain the cached palette. See [the live examples](appearance.md#live-inspection-and-atomic-application).

This boundary completes the discoverable live apply gap listed in earlier
checkpoint sections. Remaining acceptance includes the enlarged-font and toolkit
layout audit, ambient drawing decoration, and independent Dark/Light visual,
glyph and performance evidence.
