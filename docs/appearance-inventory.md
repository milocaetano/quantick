# Appearance ownership inventory

This inventory classifies production values after the ambient drawing/widget
seam. It is not a completed-feature claim: required residual work is explicit
below. The authoritative key list is the complete bundled Dark document at
`crates/appearance/themes/dark.toml`; Light uses the identical schema.

## Resolved semantic consumers

| Surface | Owner | Keys or role |
| --- | --- | --- |
| Standard widgets | app/theme/chrome; appearance | chrome/control/inset/border/text/accent plus widget_code/warn/error/cursor/shadow/noninteractive/active_border/open_border |
| Header, rail, panels and replay | app painting adapters | body/monospace/small/heading roles, surface/text tokens; replay_well/inset/error and action_ink |
| Gateway initialization errors | app/control/gateway/panel | error_text |
| Canvas/candles | chart/style, stores/chart_style | canvas/control/buy/sell/candle contours, explicit field provenance |
| Plot axes and footprints | appearance/AxisMetrics; chart/footprint_lod; app painters | chart_axis/footprint; row, width, LOD and candle-fade budgets share configured scaling |
| Heat/tape/bubbles/FLOW palette | appearance/orderflow; orderflow/render_style; stores | selected inherited/explicit palette, configured ramp, optional authored RGB overrides |
| FLOW labels/opening text | app/orderflow_render | chart_label, flow_buy/sell, quantity_shadow, text_shadow |
| Drawing selection and placement | drawings/DrawContext and tool; app scene adapter | drawing_selection_halo, canvas/accent, drawing_readout_plate |
| Empty note, VWAP anchor | drawings/tools/text, anchored_vwap | text_muted and canvas; object color remains authored |
| Ruler, Fib and profile annotations | drawings/tools | monospace/body roles, buy/sell/poc/text_muted/casing and drawing_readout_plate |
| Script object labels/marker captions | app/indicator_render | body role, historical proportional 11-pixel reference; authored object colors remain intact |
| Paper rows/table/calendar | paper/report/layout; app report/calendar | shared role scale for virtual rows/baselines/cells/column widths; side/text/surface tokens |
| Indicator picker/preview | app/surfaces | heading/body, modal_scrim/accent |

All local, draft and shared drawing paint contexts borrow the resolved theme.
Gesture-only contexts use the static Dark fallback and do not paint labels.
Appearance resolution/font conversion happens at change boundaries; drawing
objects are not recolored by mutating saved styles.

## Explicit values and non-theme policy

- Chart, drawing and indicator/script payload color fields are authored values.
  Legacy missing provenance stays explicit, even when bytes equal a default.
  Conversion functions in layouts, drawings/style, indicator_panel/legend/render,
  candle_view/candles and orderflow RGB override adapters preserve those bytes.
- Note payload family/size, the inline note editor and context-bar font-size
  previews are authored typography. Script labels/markers have no authored font
  field and therefore use the ambient body role.
- Widget/toolbar overflow icons and vector-icon letters have fixed glyph sizes
  coupled to their hit/outline boxes. They are icon geometry, not ordinary text.
- Transparent/PLACEHOLDER colors, white texture pixels, mask colors and galley
  measurement-only colors are renderer mechanics. Their bytes do not specify a
  visible surface. Orderflow opaque context backing is compositing policy.
- Stroke widths, dash lengths, shape radii, candle widths, layer opacity/fades,
  clip margins and shadow blur/offsets remain geometry or visualization policy.
- `app/theme/palette.rs`, drawing historical palette aliases and explicitly
  cfg(test) constants are Dark regression fixtures, not active theme state.
  A constant is not exempt merely because its file also contains tests.

## Required residual acceptance work

The remaining values below are production matches inspected during this seam;
they must not be treated as authored user choices or as completed migration.

| Owner | Residual |
| --- | --- |
| app/candle_view/style_window.rs::draw_preview | Fixed disabled-background sample, bid/ask bands and aggression bubble colors; distinguish the preview's illustrative backdrop from the authored candles, then give theme-owned decoration exact-Dark defaults |
| app/trade_paint.rs and app/paper_trading.rs plus paper overlays | Fixed selected-trade/drag halo [40,40,40,40]; ambient selection decoration |
| app/orderflow_render/bubbles.rs and flow_execution/flow_runs.rs | Fixed black crown/separator backing; inspect factual edge contrast/compositing versus configurable ambient paint consistently across both cached/live paths |
| app/orderflow_render/tape_path.rs | Retained-tape shade uses fixed black plus PAST_TAPE_SHADE_ALPHA; theme-owned underlay candidate |
| app/toolrail/flyout.rs | Heading RichText explicit size still needs role-relative font inspection |
| app/indicator_render.rs::pane_header_rect and callers | Fixed indicator header geometry must agree with enlarged text and input |
| app/paper_report/{rows,tables,window}.rs and paper_calendar.rs | Larger row/table/calendar geometry is wired, but narrow headline tiles and primary ledger text can still require deliberate elision; the expanded calendar needs bounded scrolling at large roles |
| app/pane_header.rs | Icon-only capsules intentionally keep icon geometry; side-label plate now measures height, but available-height rejection and sample-value/capsule clearance need enlarged-font QA |
| app/orderflow_render/preview.rs | Legend labels measure width and suppress optional tail when it cannot fit; very narrow primary liquidity label/title fitting still needs QA |

Independent Dark/Light/enlarged-family screenshots, glyph checks and matched
hot-path performance evidence remain required. Unit and scene tests are not
visual acceptance. Final delivery must remove this residual section only after
those owners are resolved and verified, then re-run production searches for
`FontId::`, RichText size overrides, `Color32::` and historical palette aliases.
