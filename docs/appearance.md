# Theme configuration

Quantick's appearance contract is versioned TOML. The bundled
[Dark](../crates/appearance/themes/dark.toml) and
[Light](../crates/appearance/themes/light.toml) documents use the same parser and
resolver as an authored theme. Start with either file when writing a community
theme. Installation, distribution and a theme picker are outside this foundation.

## Document

`version = 1` and a nonempty `name` of at most 80 characters are required. A name
cannot contain control characters. `dark` selects the base widget appearance and
defaults to `true`. Missing colors, heat ramps and typography properties inherit Dark's
version-1 values independently, even when `dark = false`. A light author should
start from the complete Light color fixture rather than merely changing `dark`.

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

`canvas`, `control`, `buy`, `sell`, `candle_bull_outline`, `candle_bear_outline`
and `candle_wick` must be opaque. These roles feed legacy RGB chart style and
preset fields. A nonopaque value is rejected with the semantic field name before
publication; alpha is never silently discarded.

The complete stable color-key list is the `[colors]` table in Dark. Semantic
groups are:

| Keys | Meaning |
| --- | --- |
| `canvas`, `chrome`, `inset`, `control`, `border` | Chart background, fixed surfaces, wells, widgets and borders |
| `text_primary`, `text_muted`, `text_faint`, `text_support` | Labels, secondary text, hints and explanatory text |
| `buy`, `sell`, `poc` | Trade direction and point of control |
| `buy_ink`, `sell_ink` | Readable direction-colored text over the configured underlay |
| `candle_bull_outline`, `candle_bear_outline`, `candle_wick` | Default candle contours and neutral wick |
| `accent`, `pane_mark`, `pane_action`, `off` | Selection, pane headings/actions and manually disabled layers |
| `amber`, `warn`, `rec` | Provenance, errors and recording status |
| `capsule_fill`, `capsule_border`, `capsule_shadow`, `bare_hover`, `float_shadow` | Floating controls, borders, hover and shadows |
| `seam_line`, `seam_label`, `day_rule`, `gap_line`, `gap_label` | Tape provenance, day changes and missing-data marks |
| `tag_bg`, `chip_ink`, `casing` | Axis/tool tags, chip text and readable underlays |
| `draw_violet`, `draw_cyan` | Drawing colors distinct from market direction |
| `loading_title`, `loading_meta`, `loading_faint`, `loading_border` | Loading text, connection dots and outlines |
| `loading_logo_low`, `loading_logo_high` | Cached endpoints for the decorative loading logo |
| `footprint_profile`, `footprint_total_ink`, `footprint_bevel_highlight`, `footprint_bevel_shadow` | Footprint silhouettes, totals and raised cell edges |
| `crosshair_ink` | Crosshair price tag text |

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
drawing text and script object typography remain stored object properties; ambient
roles must not resize an existing drawing.

The `dark` flag also chooses egui's base cursor, disabled text, window shadows and
other widget defaults. Quantick then applies its semantic surface, text, hover,
selection and link colors. Presentation opacity composes with configured alpha;
it does not reinterpret premultiplied channels as straight RGB.

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
The override selects the appearance store path; this checkpoint only reads it,
without automatic writes or file polling. The appearance store is excluded from
workspace bundles. An eventual explicit persistent apply must document whether
it writes this selected path before calling the headless save operation. Use a
scratch copy of a theme for harness runs that exercise persistence, rather than
pointing at an authored or tracked source file.
The application owns its active state, and each egui context caches converted
colors and typography by revision. Repeated frames reuse that cache. Typography
changes update text roles without replacing font definitions, so Phosphor icons
and the bundled fallback chains remain installed.

The [migration inventory](appearance-inventory.md) records completed boundaries
and remaining candidate values. Runtime migration is in progress: orderflow colors/cached typography, enlarged-font layout,
and live inspect/apply still require integration before this feature is complete.

## Authoring example

```toml
version = 1
name = "Ocean"
dark = true

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

Fresh chart candle colors, background and grid inherit independently. Editing
width, opacity or visibility leaves color ownership unchanged; editing a color
makes that field explicit. Choosing a built-in candle preset captures its candle
colors but preserves canvas ownership. Restoring defaults resumes inheritance.
Chart edits retain the existing session-only lifetime unless saved as a named
preset. Saving a named preset captures the whole visible look, including canvas,
without changing ownership of the live chart. Applying that saved preset restores
its explicit snapshot. The stored chart document accepts per-field `ownership`
markers (`theme` or `explicit`); missing legacy markers mean explicit.

New drawings use semantic `accent`, buy/sell marks use `buy`/`sell`, and anchored
VWAP uses `draw_cyan`. The text tool captures the `drawing` font role when created.
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
