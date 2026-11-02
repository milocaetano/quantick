# Theme configuration

Quantick's appearance contract is versioned TOML. The bundled
[Dark](../crates/appearance/themes/dark.toml) and
[Light](../crates/appearance/themes/light.toml) documents use the same parser and
resolver as an authored theme. Start with either file when writing a community
theme. Installation, distribution and a theme picker are outside this foundation.

## Document

`version = 1` and a nonempty `name` of at most 80 characters are required. A name
cannot contain control characters. `dark` selects the base widget appearance and
defaults to `true`. Missing colors and typography properties inherit Dark's
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

Typography tables use `family = "proportional"` or `"monospace"` and `size` in
logical pixels, from 6 through 72 inclusive. Each property can be omitted.
Supported roles are `heading`, `body`, `button`, `monospace`, `small`,
`chart_axis`, `chart_label`, `footprint`, and `drawing`. Family selection uses
bundled fonts; it does not load a path or remove icon fonts and fallback glyphs.

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

`quantick_stores::appearance` stores the source document at a host-supplied path.
Its save operation validates first, writes a temporary sibling and renames it,
then publishes the new state. Validation errors, I/O failures and the session's
saves-off gate leave the active state unchanged. A missing file resolves to Dark;
an invalid existing file is reported without overwriting the user's content.
