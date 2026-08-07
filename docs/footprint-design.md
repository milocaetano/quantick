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
(session-profile concept, meaningless on one bar's sample), per-cell heatmap /
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
