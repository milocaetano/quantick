# Theme configuration

Quantick's appearance contract is versioned TOML. The bundled
[Dark](../crates/appearance/themes/dark.toml) and
[Light](../crates/appearance/themes/light.toml) documents use the same parser and
resolver as an authored theme. Start with either file when writing a community
theme. ⋮ › Settings › Display › Theme offers Dark and Light and saves the choice. Installation and distribution are outside this foundation.

Light uses neutral soft-gray surfaces and buttons, near-black text, subtle gray grids, solid green bullish candles and solid black bearish candles on conventional Time charts. Light Bookmap (Trail and Tape) uses a slate `#434A52` plot with the established liquidity ramp, bubbles and translucent candles. Light drawing defaults use amber `#B5832B`, buy/sell arrow marks TradingView green `#089981` and red `#F23645`; fixed-range profiles use muted blue-gray. Dark retains its established appearance. Typography, spacing and layout are unchanged.


## Bookmap surface

The optional `[bookmap]` table separates painted Trail/Tape colors from the
application interface and conventional charts. `candle_fill_opacity`,
`[bookmap.colors]` and `[bookmap.footprint_heat]` use the same validated fields as
their root counterparts. Missing surface fields inherit the resolved parent
values; omitting the table preserves existing authored themes. Typography is
shared. The bundled Light document explicitly supplies its slate plot,
amber drawing defaults and blue-gray profiles over the historical Dark market
colors and `0.20` candle opacity, independently of its name or `dark` flag.
`[orderflow]` continues to own liquidity and bubble colors and supports explicit
palette snapshots as before.

Only theme-owned chart fields use the surface values. Legacy global chart settings,
saved candle presets and scripts remain explicit overrides: saving a chart
settings preview captures that preview, and applying such a preset to a chart
can override its Time or Bookmap defaults. No inheritance is inferred from RGB equality.
Light axis gutters, timestamp strips and labels use the root interface palette;
their backing matches the Time canvas. Dark retains authored Bookmap axis colors
and timestamp surfaces. Empty Light panes back the loading text and reconnect
control with the root canvas so the same message stays readable across mixed plots.
Light confines the Bookmap canvas to the plot, including before data arrives;
Dark retains its existing frame. Candle colors and chart marks keep the surface
palette. Paint scopes
are local to one synchronous pane draw and never modify widget visuals or fonts.

## Document

`version = 1` and a nonempty `name` of at most 80 characters are required. A name
cannot contain control characters. `dark` selects the base widget appearance and
defaults to `true`. Missing colors, heat ramps and typography properties inherit Dark's
version-1 values independently, even when `dark = false`. A light author should
start from the complete Light color fixture rather than merely changing `dark`.

Optional `candle_fill_opacity` sets inherited candle body opacity from 0 through 1
inclusive; nonfinite values are errors. Missing values inherit Dark's `0.20`,
independently of `dark`. Light sets `1.0` for solid bodies. Explicit candle
settings, preset body modes, forming-bar dimming and footprint fading retain
their existing behavior.

Unknown keys, unknown versions, invalid colors and invalid font properties are
errors. Configuration is resolved as a whole before publishing it; a failed
change leaves the previous palette, revision and typography intact. Errors name
the field, for example `colors.canvas` or `typography.body.size`. TOML structural
errors include the parser's location and explanation.

Colors accept `"#RRGGBB"` or `"#RRGGBBAA"` in straight sRGB. Alpha is converted
using the same gamma-aware conversion as drawing scenes. The advanced form
`{ premultiplied = [r, g, b, a] }` carries four renderer bytes verbatim. It exists
to preserve historical translucent colors exactly; it must not be used for
ordinary straight-alpha values. Its channels are bytes, not floating-point
fractions, and gamma-aware RGB channels can exceed the alpha byte.

`canvas`, `control`, `chart_grid`, `buy`, `sell`, `candle_bull_outline`, `candle_bear_outline`
and `candle_wick` must be opaque. These roles feed legacy RGB chart style and
preset fields. A nonopaque value is rejected with the semantic field name before
publication; alpha is never silently discarded.

The complete stable color-key list is the `[colors]` table in Dark. Semantic
groups are:

| Keys | Meaning |
| --- | --- |
| `canvas`, `chrome`, `inset`, `control`, `border` | Chart background, fixed surfaces, wells, widgets and borders |
| `toolbar_sheet`, `tab_idle_fill`, `tab_idle_outline`, `tab_band`, `home_glow` | The active market tab and toolbar row as one sheet; an idle tab's block and its 1 pt edge; the active tab's top band; the glow behind the Home Q, its alpha the peak |
| `tab_strip`, `brand`, `replay_ink` | Market tab strip and window title area, the Home wordmark's "tick", and a replaying tab's mark and word ([decision](ux/layout-appearance-2026-11-06/README.md)) |
| `chart_grid` | Inherited chart-grid color, independent of widget surfaces; pane opacity still applies |
| `text_primary`, `text_muted`, `text_faint`, `text_support` | Labels, secondary text, hints and explanatory text |
| `buy`, `sell`, `poc` | Trade direction and point of control |
| `buy_ink`, `sell_ink` | Readable direction-colored text over the configured underlay |
| `candle_bull_outline`, `candle_bear_outline`, `candle_wick` | Default candle contours and neutral wick |
| `accent`, `pane_mark`, `pane_action`, `off` | Selection, pane headings/actions and manually disabled layers |
| `amber`, `warn`, `rec` | Provenance, errors and recording status |
| `flow_surface`, `flow_divider` | Order-flow frame (side headers and axis strip) and divider |
| `flow_band`, `flow_ink`, `flow_band_rule` | Order-flow band fill, its heading (and the rule after it, the status words and the connection chip) and its closing rule |
| `flow_key_lit_dot`, `flow_key_up_ink` | A shown side's key: its glowing dot and white word; the gear's ink while hovered or open |
| `flow_key_unlit_dot`, `flow_key_unlit_ink` | A hidden side's key: its grey dot and muted word |
| `flow_gear_ink`, `flow_key_down`, `flow_key_down_shade` | The bare gear's ink, and the well with its top shade it sinks into while the settings are open |
| `flow_bulb_lit` | The bulb beside the gear while the flow pane's lights are on; at rest it wears `flow_gear_ink` |
| `flow_key_down_rim`, `flow_key_down_ink`, `flow_key_up_top`, `flow_key_up_bottom`, `flow_key_up_lip`, `flow_key_up_edge`, `flow_key_up_shadow` | Accepted legacy v1 keys; the band's keys no longer paint a body |
| `bare_hover`, `float_shadow` | Control hover and floating surface shadows |
| `capsule_fill`, `capsule_border`, `capsule_shadow` | Accepted legacy v1 keys; docked layer capsules no longer paint a separate case |
| `seam_line`, `seam_label`, `day_rule`, `gap_line`, `gap_label` | Tape provenance, day changes and missing-data marks |
| `tag_bg`, `chip_ink`, `casing` | Axis/tool tags, chip text and readable underlays |
| `draw_violet`, `draw_cyan` | Drawing colors distinct from market direction |
| `drawing_default`, `drawing_buy`, `drawing_sell`, `drawing_vwap`, `drawing_profile` | New drawing stroke/text defaults, directional marks, anchored VWAP and fixed-range profile, independent of UI accent and market colors |
| `loading_title`, `loading_meta`, `loading_faint`, `loading_border` | Loading text, connection dots and outlines |
| `loading_logo_low`, `loading_logo_high` | Cached endpoints for the decorative loading logo |
| `footprint_profile`, `footprint_total_ink`, `footprint_bevel_highlight`, `footprint_bevel_shadow` | Footprint silhouettes, totals and raised cell edges |
| `crosshair_ink` | Crosshair price tag text |

Light's `chart_grid` renders the reference `#F2F3F3` at the default grid opacity
over its white `#FFFFFF` canvas. Candle previews and shared axes use the same white.
Dark and Slate retain their existing grid.
Omitting `chart_grid` preserves an explicitly supplied `control` as the grid
default in either root or Bookmap colors; otherwise it inherits the fallback grid.
Explicit pane colors and opacity retain their existing ownership.
Omitting `tab_strip` takes the document's own `inset`; omitting `brand`,
`replay_ink` or a tab token takes the bundled theme of the document's `dark`
mode, so an authored Light file written before these tokens stays Light.
Omitting `flow_band` takes the document's own `flow_surface`, so a file written
before the band had its own fill keeps a band its own flow inks read on.

## Title strip

The optional `[title_strip.colors]` table paints the window's title strip (logo,
`+`, menu and window buttons) in colours other than the root's; missing fields
inherit the resolved root. Bundled Light uses it for the order-flow slate with
Dark's inks, so its logo renders exactly as Dark's. Market tabs keep the root's
colours and inks. A document without the table paints its strip in its own
root colours.

Typography tables use `family = "proportional"` or `"monospace"` and `size` in
logical pixels, from 6 through 72 inclusive. Each property can be omitted.
Supported roles are `heading`, `body`, `button`, `monospace`, `small`,
`chart_axis`, `chart_label`, `footprint`, and `drawing`. Family selection uses
bundled fonts; it does not load a path or remove icon fonts and fallback glyphs.

Custom painters retain their historical hierarchy as fixed reference units: heading
18, body/button 12.5, monospace/drawing 12, small 9, chart axis/label 11, and
footprint 10. A nominal 10-pixel body caption paints at `10 * body.size / 12.5`.
These denominators are historical ratios, independent of the bundled document:
editing Dark's own configured size changes its output just like an external theme.
Configured families apply to both measurement and painting. Axis reservations grow
with their role, and footprint row/width floors and fitted fonts share one budget.
Variable-width footprint digits use a conservative one-em width budget. Authored
note text family/size remain stored object properties. Script label and marker
models do not store typography: their default text uses the body role at the
historical 11-pixel reference size, while their authored colors remain intact.
Fib, measurement and profile annotations use ambient roles; they have no saved
font-size fields.

The `dark` flag chooses egui's base widget geometry and behavior. Semantic
`widget_code`, `widget_warn`, `widget_error`, `widget_cursor`, `widget_shadow`,
`widget_noninteractive`, `widget_active_border` and `widget_open_border` replace
its remaining painted color defaults. Disabled fading, corner radii, stroke
widths, cursor blinking and shadow geometry remain toolkit policy. The inactive
widget border has zero width and no visible color. Presentation opacity composes
with configured alpha rather than reinterpreting premultiplied channels.

`drawing_selection_halo` and `drawing_readout_plate` configure ambient drawing
selection and measurement/placement plates. Existing drawing style colors and
note payload fonts are preserved. Profile POC/side labels, casing, directional
readouts and empty-note placeholders read the current scene's borrowed theme.
`modal_scrim`, `replay_well`, `replay_inset`, `replay_error`, `action_ink`,
`error_text` and `text_shadow` cover modal/replay/error/action decorations.
`trade_selection_halo` covers selected trade marks and dragged paper levels.
`candle_preview_background/bid/ask/buy/sell` (five separate keys) color the
illustrative candle-style preview; its actual candles and enabled canvas retain
the edited chart style. `retained_tape_shade` is ambient missing-history shading
alongside `tape_label`, independent of the selected market palette.

Ledger allocation, virtual scrolling and two-line baselines share one text
scale. Report table columns, calendar cells, pane header reservation/input,
loading chips and preview legend spacing now account for configured text.
Indicator headers share a label/axis-aware height with gesture hit tests and
collapsed strips. Narrow indicator titles, preview titles, report tiles and ledger
readouts visibly elide; report tile hover text retains the complete figure.
The expanded calendar scrolls in both directions within a bounded portion of the
report. Side-label plates reject insufficient measured height; tape keys and FLOW
captions reserve matching enlarged clearance. These layout policies preserve the
normal hierarchy while making practical enlarged roles usable in constrained panes.

## Order-flow palette

`[orderflow.colors]` contains the stable named fields listed in Dark: trade sides,
consumption and depth-only marks, bubble text, gaps, the live boundary, legend
chrome, bid/ask tints, preview grid/path, `crown_backing` and `separator`.
The last two are the full premultiplied decorations, including their opacity;
no second fixed alpha is applied. Live bubbles and cached native FLOW runs read
the same selected palette. Historical choices preserve black alpha 110/170.
Saved palette snapshots created before these keys existed default only those
missing fields to those historical bytes, retaining every saved field. Explicit
new fields survive round trips and never inherit by RGB comparison. Values use the same color syntax as
`[colors]`. `buy`, `sell`, `consumption`, `bubble_text`, `bid_tint` and `ask_tint`
must be opaque because bubble overrides, saved looks and the thermal pipeline
use RGB there; validation rejects unsupported alpha before publication. Other
palette fields preserve premultiplied alpha.

`[orderflow].ramp` has nine opaque colors, by default at the intensity positions
`0, .09, .22, .38, .55, .70, .83, .93, 1`; `ramp_stops` places them elsewhere,
from 0 to 1, each past the one before. `buy_rim` and `sell_rim`, written
together, draw flat bubbles ringed in them. Missing colors or the ramp inherit
Dark independently. `heat_gamma` (0.1 to 5) raises each heat cell's intensity
to that power, on top of the look's own gamma, before the ramp reads it; below
1 it lifts weak liquidity. `heat_fade_below` (0.01 to 1) paints every cell at or
above that intensity in its full ramp colour, fading in linearly below it.
Without it a cell's opacity follows its intensity, which suits a ramp that starts
at a dark ground. Both are absent from Dark and from Light's paper and island.

## Lit flow surface

The optional `[lit_flow]` table is the order-flow pane with its lights on, picked
by the bulb beside the band's gear (`orderflow.settings.lights.set`). Its
`[lit_flow.colors]` and `[lit_flow.orderflow]` derive from the root theme, not
from `[bookmap]`, so drawings, VWAP and plot inks are the root's paper colors.
Its order-flow palette outranks a palette the pane picked for the island. A theme
without the table offers no bulb; the bundled Light authors it, Dark does not.
Light's `[lit_flow.orderflow]` sets `heat_gamma = 0.7` and `heat_fade_below = 0.1`,
the mock's mapping, so weak and mid book levels read as grey on the white plot
instead of washing out. `candle_casing_px` lays each candle on a plate that wide in `casing`; rimmed
bubbles wear a white halo about three pixels wide in `separator`. Heat cells, preview and legend sample the same resolved
ramp; the live strip and bubble controls read the same selected palette.

Fresh order-flow settings, the shipped `default` preset, and the four shipped
feed-declared startup looks (BTC Spot, BTC perpetual, live lane pie and mini index
regions) explicitly inherit application appearance. A preset's `palette = { source = "inherited" }`
is source metadata, not a rule inferred from its name or RGB values. Files
without this marker retain historical Bookmap. Other historical palette choices
remain explicit. A manual preset selection captures its inherited palette at
selection time; a named save captures the visible palette in the saved look.
Ordinary asset/settings capture retains ownership, so radius-only edits keep
inherited colors. Optional RGB overrides remain explicit even when equal to a
historical default. Ownership participates in saved asset equality and survives
reload, tab adoption and the asset save switch.

## Footprint heat

`[footprint_heat]` defines `buy`, `sell`, and `ink`, each an array of exactly six
colors, from quietest to busiest. Missing arrays inherit Dark independently.
The renderer looks up the resolved arrays; it does not derive heat colors or
contrast per cell. Dark preserves the historical six steps and their three ink
weights. Light supplies contrasting heat and ink arrays. Authors should check
all six ink/fill pairs for readable contrast.

The bundled Dark document is complete: every color, font property and heat array
is required by its complete-document validation. It is parsed once into an
immutable fallback through the same resolver used by authored documents.

At startup the host reads `QUANTICK_THEME=<toml>` when set, otherwise
`appearance.toml` from the normal cockpit directory. The explicit path also
isolates appearance state during harness runs and is excluded from cockpit rescue.
Before a builtin choice is saved, the override selects the appearance store for
both startup reads and an explicit source apply with `persist: true`; otherwise
that store is `appearance.toml` in the normal cockpit directory.

⋮ › Settings › Display › Theme and `appearance.apply` with `builtin` save the complete bundled
source to a sibling named `<selected filename>.choice.toml`. Startup prefers this
sibling when present, and subsequent durable source applies write that active
choice store. The original selected document is preserved, including an authored
`QUANTICK_THEME` file. A session-only builtin choice creates no file. Removing the
choice sibling while the app is closed restores startup from the original source.
Invalid or unwritable candidates leave the previous source and destination intact.
The choice shows Dark or Light only for the exact bundled source (or initial Dark
fallback); authored copies, even with the same name or colors, show Custom theme.
Startup also recognizes the four exact earlier native Light choices, including
the dark Bookmap version, allowing LF or CRLF line endings, and resolves them to
current Light. The exact earlier native Dark choice also retains its builtin
identity after adding the profile token, with identical rendered colors.
These migrations apply only to the choice sibling; edited copies
and authored base files keep their source. Startup leaves the file untouched;
the next explicit save writes the current source through the normal write gate.
Custom source remains available through the existing API; the native menu only
selects the two builtins. Both builtins keep the established fonts, sizes, layout
and spacing; Light changes the colors. Font overrides remain an authoring feature,
with modest variations supported and verified, rather than a new typography UI. Remote callers never choose
a path. Session applies write nothing; no automatic writes or external file
polling occur. Appearance remains excluded from workspace bundles. A durable source
apply intentionally replaces the active destination. Use an isolated selected
path for harness runs; its choice sibling is also part of that isolated store.
The application owns its active state, and each egui context caches converted
colors and typography by revision. Repeated frames reuse that cache. Typography
changes update text roles without replacing font definitions, so Phosphor icons
and the bundled fallback chains remain installed.

The [appearance inventory](appearance-inventory.md) records production boundaries
and classified exclusions. Independent screenshots, glyph checks and matched
performance evidence are delivery checks; source and unit tests do not substitute
for visual acceptance.

## Authoring example

```toml
version = 1
name = "Ocean"
dark = true
candle_fill_opacity = 0.35

[colors]
canvas = "#101B27"
chrome = "#152331"
text_primary = "#DCEAF4"
accent = "#68C8EE"
draw_cyan = "#40D9CC"
capsule_fill = "#101B27D0"

[typography.body]
size = 13.0

[typography.chart_axis]
family = "monospace"
size = 12.0
```

An AI author can copy the complete Dark or Light document, preserve `version`,
change semantic values, then call the same validation boundary as the host. The
Rust entry point is `quantick_appearance::ResolvedTheme::parse`; it performs no
file access. `Appearance::apply` owns one application's active palette and
monotonic change revision. Identical text is not reparsed; semantically identical
documents do not invalidate rendering. A color-only change does not invalidate
typography. Renderers retain an immutable resolved palette at their boundary and
read tokens by index.

## Saved customizations

Theme inheritance is explicit provenance. An explicit color equal to yesterday's
default is still explicit tomorrow. Legacy styles with no provenance marker are
explicit. Saved presets, existing drawings and authored script colors are not
reclassified by comparing their RGB bytes to a theme. Changing the default only
affects values whose owner records theme inheritance.

Fresh chart candle colors, fill opacity, background and grid inherit independently.
Editing width or visibility leaves ownership unchanged; editing a color or fill
opacity makes that field explicit. Choosing a built-in candle preset captures its
candle colors and opacity but preserves canvas ownership. Restoring defaults resumes inheritance.
Each chart owns its candle and canvas settings. Double-clicking a candle opens
that chart's editor, whose caption identifies the chart and says "Only this
chart". The editor keeps that target when focus changes; closing or removing
the chart cannot redirect edits into a neighbour. Order Flow and every Time
chart remain independent across tabs, markets and themes.

Pane styles and their ownership markers travel with the saved workspace and
restore on restart. Older workspaces without pane styles use the existing
global style as their initial values. Inherited Time colors resolve against the
main theme; inherited Order Flow colors resolve against its Bookmap surface.
Saving a named preset captures the whole visible look, including canvas,
without changing ownership of the live chart. Applying that saved preset restores
its explicit snapshot. The stored chart document accepts per-field `ownership`
markers (`theme` or `explicit`); missing legacy markers mean explicit.

New drawings use semantic `drawing_default`, buy/sell marks use
`drawing_buy`/`drawing_sell`, anchored VWAP uses `drawing_vwap`, and fixed-range
profiles use `drawing_profile`. Light supplies amber strokes and text (green and
red for buy/sell marks), translucent
fills derived from those strokes, and a white `drawing_readout_plate`. Its profile
source remains `#7A8EC2`, calibrated to the earlier muted blue-gray histogram:
the reference's approximately `#B1BCD9` fill already includes the existing 0.55
value-area row opacity over `#F5F5F5`. The profile stays in the chart and retains
its row geometry, value-area weights, POC and double-cased outline over heatmaps.
Dark retains historical defaults.
Missing drawing keys independently inherit Dark; for existing authored themes,
an omitted `drawing_profile` uses an explicitly supplied `drawing_default` on
the same surface, preserving their previous new-profile default. Changing UI `accent` does not
change new drawing colors. The text tool captures the `drawing` font role when created.
Saved tool style, configuration and named default presets take precedence in that
order; existing objects never follow subsequent theme changes. Text payloads store
family and size; a legacy payload without family retains proportional text.
Drawing defaults store opaque colors as legacy RGB hex and translucent colors as
`color_premultiplied` bytes, preserving configured alpha without a lossy conversion.

`quantick_stores::appearance` stores the source document at a host-supplied path.
Its save operation validates first, writes a temporary sibling and renames it,
then publishes the new state. Validation errors, I/O failures and the session's
saves-off gate leave the active state unchanged. A missing file resolves to Dark;
an invalid existing file is reported without overwriting the user's content.


## FLOW geometry and tape annotations

`colors.flow_buy` and `colors.flow_sell` own regional FLOW sectors and opening
perimeters. Both accept alpha, composed into the existing factual emphasis and
opaque context backing by the headless order-flow painter policy. `tape_path`
owns the quiet execution path, `tape_label` its held/retained annotations, and
`quantity_shadow` the shadow under FLOW and native-tape quantities. Each accepts
alpha independently. Their bundled Dark values preserve historical output.

FLOW and native-tape quantities, legends, depth captions, preview labels and live
strip empty text use `typography.chart_label`, including fitting and measurement.
FLOW caches retain one immutable appearance adapter and include its change identity
in both drawing and reusable-run keys. A theme/font change rebuilds the affected
drawing; warm frames reuse it. Label galleys are laid out against that retained
snapshot each frame so the font atlas can safely change. The compact legend grows
its row reservation to the measured text height.


## Live inspection and atomic application

Discover `appearance.apply` version 1 and the `appearance.state` snapshot scope
with `control.describe`. Inspection uses the existing `observe` and
`observe.workspace` permissions. Applying requires the cockpit profile and
`cockpit` and `cockpit.layout` permissions, including session-only changes. It never enters replay
control traces.

These are complete capability inputs (the MCP adapter supplies its normal
request envelope). Read first with `snapshot.read`, version 1:

```json
{"scopes":["appearance.state"]}
```

The scope value contains `revision`, `origin`, `source`, `durable`,
`writes_enabled`, `load_failed` and `choice` (`dark`, `light`, or `custom`). `source` is the active original TOML, or the
complete bundled Dark TOML at fallback. `origin` is `bundled_dark`, `session`, or
`selected_store`; no selected filesystem path is disclosed. `durable` means the
last successful load/save established this source, not that an external file
watcher has confirmed it still exists. `load_failed` records startup fallback
from an invalid/unreadable file and clears on a successful explicit save.

Choose and remember a builtin with `appearance.apply`, version 1:

```json
{"builtin":"light","persist":true}
```

`builtin` accepts `dark` or `light`. Exactly one of `builtin` and `source` is
required. The default `persist: false` remains session-only for either input.
A persistent builtin selection uses the same protected sibling as ⋮ › Settings › Display › Theme.

Apply a session candidate with `appearance.apply`, version 1:

```json
{"source":"version = 1\nname = 'Ocean'\n[colors]\naccent = '#55BBCC'","persist":false}
```

Apply and save that same source to the selected destination:

```json
{"source":"version = 1\nname = 'Ocean'\n[colors]\naccent = '#55BBCC'","persist":true}
```

Both return `{ "changed": boolean, "state": <appearance.state value> }`.
`changed` and `revision` track resolved theme changes; formatting-only changes
can update source or persistence status without rebuilding palette caches.
A save after a session apply still writes, even when the source text matches.
A fresh explicit save also repairs a removed destination. The save uses a
distinct temporary sibling even for a selected `.tmp` filename, refuses an
occupied staging file, and publishes only after writing and renaming succeed.
The session's saves-off gate refuses the entire persistent apply.

The JSON schema limits source to 65,536 Unicode characters. Load and runtime
apply additionally enforce 65,536 UTF-8 bytes before parsing, so multibyte input
can hit the byte limit sooner. A whole candidate must pass version, color,
opacity and typography validation. `control.invalid_request` with
`details.code = "invalid_theme"` names `details.field`; persistence failure uses
`details.code = "persistence_failed"`, `details.field = "persist"` and a bounded
error kind without local paths. Refusal preserves active source, revision,
fonts, palette, chart/default owners and the prior destination file.

Supply the normal request-envelope idempotency key for retries on the same
connection. A successful keyed retry returns the original result and performs
no second mutation or disk write. After reconnecting or expiration of the key's
retention window, inspect `appearance.state` to reconcile; a new unkeyed durable
apply intentionally attempts the write again. Publication synchronizes the
context palette, inherited chart inputs and new-drawing defaults before another
command or paint can observe the candidate. Order-flow rendering reads that
same cached appearance snapshot; warm frames never parse TOML or reinstall font
definitions.

Candle API geometry and opacity fields use canonical decimal strings (for
example, `"gap_px": "4.5"`), matching the control transport. RGB channels remain
integer bytes. Read the complete `effective_style`, edit the desired fields and
submit that document. A feed/symbol switch or replay transition retains the
pane-owned style and retires its open editor sitting; reopen that chart
explicitly before editing again.
