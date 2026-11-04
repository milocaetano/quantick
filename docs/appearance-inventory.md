# Appearance ownership inventory

This inventory classifies production painting and default-creation paths after
the source migration. Visual/performance acceptance is recorded separately. The authoritative key list is the complete bundled Dark document at
`crates/appearance/themes/dark.toml`; Light uses the identical schema.

## Resolved semantic consumers

| Surface | Owner | Keys or role |
| --- | --- | --- |
| Standard widgets | app/theme/chrome; appearance | chrome/control/inset/border/text/accent plus widget_code/warn/error/cursor/shadow/noninteractive/active_border/open_border |
| Header, rail, panels and replay | app painting adapters | body/monospace/small/heading roles, surface/text tokens; replay_well/inset/error and action_ink |
| Order-flow pane identity | chart/flow_band; app pane/header adapters | flow_surface/divider/ink/band_rule and flow_key_*; measured text and shared scaled band/side-header geometry |
| Gateway initialization errors | app/control/gateway/panel | error_text |
| Canvas/candles | chart/style, stores/chart_style | canvas/control/buy/sell/candle contours, explicit field provenance |
| Plot axes and footprints | appearance/AxisMetrics; chart/footprint_lod; app painters | chart_axis/footprint; row, width, LOD and candle-fade budgets share configured scaling |
| Heat/tape/bubbles/FLOW palette | appearance/orderflow; orderflow/render_style; stores | selected inherited/explicit palette, configured ramp, optional authored RGB overrides |
| FLOW labels/opening text | app/orderflow_render | chart_label, flow_buy/sell, quantity_shadow, text_shadow |
| Drawing selection and placement | drawings/DrawContext and tool; app scene adapter | drawing_selection_halo, canvas/accent, drawing_readout_plate |
| Empty note, VWAP anchor | drawings/tools/text, anchored_vwap | text_muted and canvas; object color remains authored |
| Ruler, Fib and profile annotations | drawings/tools | monospace/body roles, buy/sell/poc/text_muted/casing and drawing_readout_plate |
| Script object labels/marker captions | app/indicator_render | body role, historical proportional 11-pixel reference; authored object colors remain intact |
| Candle-style preview | appearance; app/candle_view/style_window | candle_preview_background/bid/ask/buy/sell; authored candle/canvas fields remain explicit |
| Trade selection / paper drag | appearance; app trade/paper painters | trade_selection_halo |
| Bubble crown / separator | appearance/orderflow; selected orderflow palette | crown_backing/separator; legacy snapshots default missing properties to historical bytes |
| Retained-tape shade | appearance; app/orderflow_render/tape_path | retained_tape_shade, ambient alongside tape_label |
| Indicator headers | appearance/AxisMetrics; chart/indicator_views/pane_layout | shared label/axis height, collapsed strip floor, paint and hit rectangles; title/value fitting |
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

## Constrained layout policy

- Report rows use one role scale for allocations, virtual scrolling and baselines.
  Primary head text fits beside the P&L/source; overlong P&L and day/session results
  visibly elide only when they cannot fit alone. Optional route/tag text yields first.
- Headline tiles bound caption/value/unit/subline to their cell; hover retains the
  complete content. The calendar has a bounded two-axis scroll area.
- Indicator titles and values have disjoint width budgets; collapsed strips and
  expanded header hit areas grow with the larger chart-label/axis role.
- Side-label plates require their full measured height and width. Tape key and
  FLOW caption clearances scale with their header; sample values clear the enlarged
  capsule reservation. Icons retain their geometric boxes.
- Preview title/liquidity labels elide within available width; optional legend
  entries yield when they cannot fit. The synthetic scene's geometry lives in
  headless orderflow configuration and the live paint vocabulary is reused.

## Final bounded scan classification

The scan includes multiline constructors and inline `cfg(test)` modules, rather
than excluding a file because its name contains a painter or tests. Inspected
production matches in app and drawings fall into the following classes:

| Match | Classification |
| --- | --- |
| Color conversion from candle/drawing/script payloads and saved override arrays | Explicit authored data adapters |
| Drawing palette aliases and legacy orderflow palette literals | Historical compatibility/default fixtures; current inherited defaults resolve through the theme |
| `drawings/fib.rs`, `footprint_render.rs`, `loading.rs`, `trade_paint.rs`, `orderflow_view.rs`, `flow_perimeter.rs` inline tests | Regression inputs below explicit test boundaries |
| FLOW one-pixel white texture; day-label measurement white; transparent/PLACEHOLDER values | Neutral renderer/measurement mechanics |
| Sphere white highlight / black shading endpoints | Lighting transform of the selected palette's authored side color, controlled by visualization shading parameters |
| Body/axis/label reference sizes | Relative hierarchy through configured roles, not fixed final font sizes |
| Fixed icon glyph size, vectors, spinner diameter, sample radii and dash widths | Geometry; no ordinary text inherits icon sizing |
| Note payload font and size picker samples | Explicit authored typography; script object default fonts are ambient because their model has no font fields |

The complete Dark contract and explicit Light core-key inventory are tested.
Owner coverage pins preview alpha parity, palette legacy/snapshot preservation,
font-aware pane allocation, narrow enlarged ledger geometry and drawing scenes.
Independent Dark/Light/enlarged-family screenshots, glyph checks and matched
hot-path performance evidence remain required for delivery. Unit/scene tests
establish source behavior, not visual acceptance.
