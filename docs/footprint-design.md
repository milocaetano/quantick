# Footprint (candle tape reading) — design

Status: v1 design, 2026-08-07. Distilled from a three-way team consult: an
order-flow domain survey (ATAS, Sierra Chart, Bookmap, exocharts, Quantower,
Nelogica/Profitchart, Volumetrica), a codebase map of the rendering/zoom/config
mechanics, and a trader/UX panel run against the project's fixed personas.
Decisions and the alternatives they beat are both recorded; the PR body links
here.

## What it is

An optional chart layer that renders the buyer/seller split *inside* each
candle, per price level, with a zoom-driven level of detail: deep zoom shows a
full bid×ask ladder (Profitchart-style), normal zoom shows a textless profile,
far zoom shows only glanceable marks (POC, stacked-imbalance zones). Off by
default; when off, today's rendering is untouched.

Footprint on volume/dollar bars is more comparable than on time bars — every
bar carries the same total volume, so intra-bar distributions can be read
against each other. quantick builds alternative bars natively; this layer is
where that pays off.

## Architecture

```
engine::footprint          app (ChartState)             app (render)
┌────────────────────┐     ┌──────────────────────┐     ┌──────────────────┐
│ FootprintBuilder   │◄────│ feeds trades + bar   │     │ ChartLayer::     │
│  base-grid ladder  │     │ closes (same stream  │     │  Footprint       │
│  per closed bar    │     │ the bar builder eats)│────►│  LOD ladder,     │
│ signals: POC,      │     │ caches BarFootprint  │     │  display grouping│
│  imbalance, stacks,│     │ per closed bar +     │     │  = base × k (int)│
│  extreme ratios    │     │ throttled live bar   │     │                  │
└────────────────────┘     └──────────────────────┘     └──────────────────┘
```

- **Engine owns the ladder** (`engine::footprint`): a pure, deterministic
  accumulator — trades in, per-bar `BTreeMap<i64, FootprintLevel>` out, keyed
  by zero-anchored integer price buckets (`floor(price / base_group)`), plus
  pure signal functions over a finished ladder (POC, diagonal imbalance,
  stacked zones, extreme ratios). No wall clock, no float keys, no HashMap.
  Rationale: "one engine, three consumers" — the backtest runner (next roadmap
  step) and the bot need the same numbers the chart shows; an app-only
  footprint forks that. Rejected: footprint as a `Bar` field (bars are
  equality-compared in golden tests and copied by value everywhere; the ladder
  is opt-in and parallel, driven by the caller who already knows bar
  boundaries).
- **The app feeds it** from the same trade stream that builds bars
  (`ChartState`), never from the aggression store (30-min retention, only
  populated while bubbles are on). Accumulation is lazy: while the layer is
  off — the default — ingestion pays nothing and holds nothing; switching it
  on refolds the retained trades once. Closed bars cache their ladder; the
  live bar re-renders at ~10 Hz, not per print.
- **Capture vs display grouping are separate**, copying the heatmap precedent
  (`EffectiveGrouping`): the engine accumulates on a fixed base grid (the
  instrument tick where the feed reports `price_step`, else the configured
  fallback); the renderer aggregates upward by *integer* multiples `k`
  (snapped to 1/2/5/10/20/25/50/100, hysteresis) so aggregation is exact and
  bars align horizontally — the prerequisite for reading stacked zones across
  bars. Rejected: ATR-based row height (lookback repaints history, breaks
  snapshots) and per-bar anchoring (breaks cross-bar alignment).
- **Level cap, honestly labeled**: a bar whose ladder exceeds the cap doubles
  its group and is flagged `aggregated`; the UI shows the effective grouping
  in the layer legend at all times (data honesty: the row's meaning is never
  silent).

## Level of detail

Effective level = min(candle-width level, row-height level). Before dropping
a level, the renderer first tries a larger display grouping (merging rows
preserves more information than deleting text). Discrete steps with ~15%
hysteresis; no fades (a half-transparent number is illegible and present at
the same time — trader veto).

| Level | Candle width | Row height | Draws |
| --- | --- | --- | --- |
| Detailed | ≥ 72 px | ≥ 12 px | bid × ask numbers per row, imbalance highlight, POC, ratio badge at extremes |
| Compact | 40–72 px | ≥ 11 px | one abbreviated delta number per row, imbalance highlight, POC |
| Profile | 18–40 px | ≥ 4 px | textless histogram, POC emphasized, stacked-zone ticks on the edge |
| Marks | 8–18 px | any | POC dot + stacked-imbalance zone marks only |
| Off | < 8 px | — | layer hidden; legend says "footprint: zoom in for detail" |

What survives zoom-out, in order: stacked zones > POC > (opt-in) bar delta
color > everything else. Isolated imbalances die at Profile; digits die below
Detailed/Compact; nothing at Marks is a number. Zone marks from adjacent bars
at the same price coalesce, with a hard cap per screen (beyond it the
effective factor rises and the legend says so).

The LOD ladder is also the performance strategy: Detailed can only exist when
~25 candles fit the viewport (bounded text galleys); Profile/Marks are pure
rectangles batched into meshes. Heavy projection follows the orderflow worker
path, never `update()`.

## The split style (default look)

Adopted from the boss's reference charts (Trinitas/exocharts print and the
Profitchart footprint): each bar draws around a central axis inside the
candle — the **right** side is the total-volume profile in neutral light
gray (the classic silhouette, POC row brightest), the **left** side is a
delta bar per row in the winning side's color ("who won the fight"),
boxed when the row is a diagonal imbalance. Detailed zoom adds the delta
number over the left half. The POC line is **yellow** (user's call; red
would collide with the sell side). The candle itself fades to an outline
box as the zoom crosses from Profile toward Detailed
(`candle_body_fade`): full body at Marks, outline-only at Detailed —
the reference charts' candle-as-a-box, without a hard switch. The classic
sell|buy ladder remains as `style = "ladder"`.

## Signals (v1)

Chosen for real-world use by flow traders; all are context tools, none are
auto-triggers, and none claim standalone statistical edge.

- **Diagonal imbalance**: bid at level *p* vs ask at level *p+1* (diagonal
  because same-price comparison is spread-distorted), ratio ≥ factor AND
  absolute difference ≥ min-qty. Factor default 3.0 (industry converges
  200–400%).
- **Stacked imbalance**: ≥ 3 consecutive same-side imbalances → a zone that
  persists visually to the right of its bar (memory value: retest levels).
- **POC per bar**: highest-volume row.
- **Extreme ratio badge**: dominant/other ratio on the extreme row (the
  Profitchart "9.82 at the low"); reuses the imbalance factor, no extra knob.

Explicitly not built (v1): Nelogica progression/sequential factors (false-
positive generators; stacking covers the useful part), per-candle value area
(session-profile concept, meaningless on one bar's sample — value area now
exists, but over a *range*: `engine::VolumeProfile` folds many bars' ladders
and `value_area(fraction)` expands from the POC two printed rows at a time,
Sierra/CQG style, larger pair wins, exact ties expand downward — the same
tie-toward-lowest rule the POC itself uses; the fixed-range-profile drawing is
its consumer), per-cell heatmap /
delta% coloring everything, trade-count clusters, big-order alerts, financial
volume unit (dollar bars already give that reading), second simultaneous
footprint. Deferred to v2: unfinished auction (must be computed on the base
grid, never the display grid), absorption, delta-divergence marks.

## Configuration

Config follows the versioned-preset pattern; the in-app surface is a small
submenu of the layer entry (trader veto on big dialogs), the full set lives in
the TOML.

Shipped in v1 (`config/footprint.toml`):

| Option | Default |
| --- | --- |
| `imbalance_ratio` | `3.0` |
| `imbalance_min_qty` | adaptive: p60 of per-row volume over the newest closed bars (stable under pan and live prints), disclosed in the legend; manual override pins it |
| `stacked_count` | `3` |
| `show_poc` | `on` |
| `extreme_ratio_badge` | `on` |

Deferred (auto-only in v1, knobs when someone asks): `mode` (the LOD ladder
always decides; `QUANTICK_CANDLE_WIDTH` scripts the zoom instead),
`cell_content` (bid×ask at Detailed, delta at Compact), `row_grouping`
(always the adaptive integer multiple), `delta_color` (not drawn — the
panel's swing persona vetoed it as default and nobody has asked for the
opt-in yet).

A single min-qty number cannot be global: 20 contracts is right on WIN and
absurd on BTCUSDT — hence the derived default.

## Honesty and gating

- The layer self-disables via `layer_blocked` when the feed reports
  `traded_volume: false` (same reason and mechanism as bubbles — a CFD quote
  stream prints no tape).
- When the aggressor side is *inferred* (MT5 tick rule; replay sessions
  recorded from it), the layer itself carries an "side: inferred" marker in
  its legend — a statusbar note is not enough for a layer whose entire content
  is buyer vs seller. Venue-reported feeds (Binance `is_buyer_maker`,
  Hyperliquid) show no marker.
- Venue-candle prefix bars (backfilled OHLCV without tape) draw no footprint —
  the layer starts where trade-built bars start, and the legend explains the
  absence rather than faking it.

## Zoom extension

`MAX_CANDLE_WIDTH` rises 64 → 160 px so Detailed is reachable (with the
existing scroll-zoom gesture; affected clamp test updated). Row height comes
from the existing manual price-axis zoom; the display grouping adapts so rows
stay readable at any Y span.

## Live bar

Accumulates per trade, repaints at ~10 Hz. Frozen layout: fixed column
widths, tabular numerals, no reflow as digits grow, no layout jump at bar
close.

## UX decisions (from the persona panel)

- Toggle lives in the pane layer menu next to heatmap/bubbles — footprint is a
  representation of the candle itself, not a derived series. Persisted per
  tab like every layer.
- At most three glanceable marks at normal zoom: POC, stacked zones, opt-in
  delta tint. Nothing at Marks level requires reading.
- Tooltips name things in full ("Point of Control — price with the most
  volume in this bar") for newcomers; Detailed mode carries a bid | ask
  header so numbers are not misread as prices.
- Legend always states the current state, including effective grouping and
  the "zoom in" hint when the layer is on but below its readable threshold —
  an on-but-invisible layer must explain itself or it reads as broken.

## v2 — the style registry, and the ladder's contrast floor

Status: 2026-08-20. Written after a colour/contrast pass and an order-flow
domain consult, both prompted by the same report: the second style "looks
visibly bad".

### The ladder was broken, and it was a composition bug

The `ladder` had been given the half of the split's treatment that *removes*
its background (`candle_body_fade` hands the candle's interior to the layer)
without the half that *replaces* it (the `canvas_backdrop()` plate, which was
gated on `style == Split`). Its contrast floor was therefore whatever three
unrelated settings happened to leave behind:

| Candle preset | buy number | sell number |
| --- | --- | --- |
| `OrderFlow` (default) | 5.14:1 | 4.57:1 |
| `Glass` | 4.46:1 ❌ | 4.07:1 ❌ |
| `OutlineOnly` | 5.97:1 | 5.13:1 |
| `Classic` | 2.23:1 ❌ | 2.18:1 ❌ |

Two of the four failed WCAG AA, and the default passed by 0.07. Switching
candle *appearance* — a choice with nothing to do with this layer — silently
cost the trader the footprint's numbers. Three more defects sat in the same
branch: the POC's 0.18 wash dropped the most-read row from 6.77:1 to 4.37:1
and its full-width line struck through both number columns (the split refuses
that line deliberately; the ladder had never been told), the stacked-zone wash
was painted *after* the cells and so tinted the digits along with their
background, and the delta totals strip was withheld from the style entirely.

**Not the cause, though it was the first suspicion**: the depth map does not
bleed through the ladder in the default state — `pane.rs` already clears the
heat behind each candle's high–low span. The plate is justified by robustness
instead: against the candle preset, against `background_enabled = false`
(which makes that clear paint transparent and clear nothing), and against the
bucket bands at a bar's extremes, which extend past the cleared span.

The rule the fix encodes: **a style whose content is digits owns its own
floor; a style whose content is geometry does not need one.** A bar's length
reads the same over any background. A number does not degrade gracefully.

### `FootprintStyle` is a registry

Four `if style ==` tests for two styles scales to two. The enum now carries
`ALL`, `id`, `label`, `hover`, `detailed_quantity_columns`,
`candle_treatment`, `plate`, `draws_own_rows`, `fallback` and
`detailed_legend`; the panel iterates it, the TOML and
`QUANTICK_FOOTPRINT_STYLE` resolve through the same `from_id`, and the draw
code dispatches instead of branching. Adding a style is a registry edit.

### The two new styles

- **`bidask`** — both sides at absolute size, mirrored on one shared scale
  (the larger *side*, never the row total, or a one-sided row would draw at
  the same width as a balanced one). No digits, so it lives from Profile up
  and fills the 10–33 px band where the number styles cannot go. It answers
  what the split cannot: 400×380 and 40×20 share a delta and are not the same
  market.
- **`cluster`** — the reference chart's boxed ladder: `bid | ask | total` per
  row, each bar in its own cased box with a border, the candle moved out into
  a lane at the left of the slot rather than sitting behind the box, and a
  raised-cell relief at the deepest zoom. It declares its own Detailed floor —
  ~126 px with its total column, ~68 without, because the floor counts the
  furniture (the candle lane, the box padding, the gutters) and not only the
  digits — and hands over to `bidask` above it, announced in the legend as
  `cluster → bidask`.

  The zoom ceiling rose 160 → 256 px to keep that floor reachable at the top of
  the `detail_scale` range. A style the registry offers and no zoom can draw is
  a broken promise, and
  `every_style_is_reachable_at_some_zoom_and_every_detail_scale` is what makes
  the next style with a wider floor fail loudly instead of quietly.

### `auto` — the zoom picks the reading

Not a fifth look, a switch. The layer already changed *how much* it says as the
zoom moves — full numbers, one delta, a textless shape, then marks. `auto`
changes *which reading* says it, so a single wheel walks the whole ladder:
three columns up close, two below that, and a shape once digits stop fitting.

It is a chain in the registry (`AUTO_CHAIN`), walked richest-first, and the
first link the candle can pay for wins. Adding a style to the walk is one entry
there — the same registry edit adding a style anywhere else already is. The
chain ends on a style that needs no digits at all, so `auto` never falls
through: at one pixel a candle it still answers with something.

Two properties are pinned by test, because both are the reason to use it:
the walk is **monotonic** (zooming in never buys a poorer reading — checked
across the whole zoom range, a pixel at a time), and it always resolves to a
concrete style rather than to itself. The legend names what it landed on, so
"why is my chart different" is answered on the chart.

It is offered, not defaulted. A trader who never opened this window keeps the
split they have always had.

### The heat ramp, and the band it steps over

Six quantised steps, never a gradient: rounding a float into a colour every
frame is how a pixel moves between two identical frames, the depth map already
owns the continuous-gradient channel on this screen, and steps can be counted.

The ramp measures **rank, not ratio** — where a cell falls in the distribution
of the ladders currently on screen, cut at fixed percentiles.

That was learned the expensive way, and it is worth writing down because the
wrong answer is the intuitive one. The first build divided each cell by the
95th percentile of side volume over the newest closed bars. Two things were
wrong with it, and a screen capture measured both:

- **The wrong bars.** "Newest closed bars of the series" is not "what is on
  screen": the colours moved with where the replay's live edge happened to
  be, not with what the trader was looking at.
- **The wrong statistic.** Per-cell volume is heavily skewed and the shape of
  that skew changes with the market, so one denominator cannot serve both a
  quiet stretch and a busy one. Measured, ratio-to-p95 put **47% of cells in
  the top step**: the brightest colour on screen was also the most common one,
  which leaves nothing for it to stand out against. An earlier variant had the
  mirror defect, with nearly everything on the floor.

Percentile cuts fix both ends by construction — the busiest cells are always
the top step and the quiet ones always the floor, in any regime — and the cuts
are deliberately uneven, because most rows are ordinary and the bright steps
are worth spending on the tail. The test
`the_heat_ramp_spreads_over_a_skewed_distribution` guards it against a skewed
fixture, not a uniform one: a ramp that only behaves on uniform data proves
nothing about a tape.

The step lightnesses are **not evenly spaced**, and that is the design. There
is a band of lightness where no available ink reaches 4.5:1 — the light ink has
run out of headroom, the dark one has not gained it — so the ramp steps over
it, and the largest jump in the scale is exactly where the ink flips. The
irregularity is the most distinguishable boundary in the scale.

What guards it is `both_ink_boundaries_are_forced_by_contrast`, and it guards
the *contract* rather than the construction: it proves neither boundary can
move a step in either direction without dropping something under 4.5:1. So a
well-meant "smooth out the ramp" fails loudly rather than quietly, and it fails
on the property that matters instead of on a number that happens to be true.

The two sides' ramps are **isoluminant by construction**. Under deuteranopia
their hues collapse, and what still separates bid from ask is the *position*
of the column — constant, and independent of the data. Luminance carries the
ordinal reading, which works for everyone. The derived requirement: **the
columns may never swap places, not even by config.**

Rejected from the reference chart: its **orange** (collides with `AMBER`,
reserved for provenance, and sits a few degrees from `POC`) and the
**light blue** of its total column (collides with `ACCENT`). The total column
carries a neutral grey silhouette instead of a heat step — the same silhouette
the split style draws — which is what lets it live on a single ink with no
flip rule at all.

### The relief

Two mesh quads per cell (a light top edge, a dark base), `Detailed` only.
Fixed alphas rather than values derived from the fill: derived ones would
scale the highlight with the base and fade it out at exactly the ends of the
ramp where it has to carry the relief. Fixed, the two are complementary — on
dark steps the white edge works and the shadow vanishes, on light steps the
reverse — and the better of the two never falls under +11 L\*, four times the
just-noticeable difference. The left and right faces are not drawn: in a field
of cells that already touch sideways they are the least informative of the
four, and they cost 57% more.

Rects, never strokes: a stroke goes through the tessellator's feathering, and
feathering is precisely what blurs a one-pixel edge into nothing.

### Reaching it

The layer had lived only in the pane's right-click layer menu — a
representation of the candle itself, two levels deep in a gesture, while three
lesser layers each had a toolbar button. It now sits in the toolbar's LAYERS
group: left-click toggles, right-click opens the settings window, the same
language its neighbours speak. Its glyph is the grid of cells it draws, per
the group's alphabet rule.

### Still not built

The stacked-zone mark **does not outlive the bars that formed it**, despite a
code comment that claimed it did (now corrected). Real persistence needs a
level memory with a death rule — a stacked imbalance dies on a print through
the far edge, absorption dies on a *close* through it, because a wick that
pierces and returns is the defender holding — and the domain consult
recommends building it once for three producers: stacked zones, absorption
(WP-12) and naked POCs. That is its own package.

Also specified and not built here: the three-lane footer (volume-or-duration,
delta with delta %, max/min delta), unfinished auction, single prints, and
intra-bar max/min delta. **Session delta is deliberately not among them**: it
is the integral of the lane above it drawn in the form that hides its own
information, and on a tick-rule instrument it accumulates classification error
for the whole session and prints the sum to four digits.
