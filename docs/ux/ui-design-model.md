# quantick UI Design Model

Status: **implemented — migration phases 1–5** (status bar, toolbar regroup,
icon set, right dock, tool rail); phase 6 lands with the indicator milestones.
§2 documents the UI as it was *before* this model, kept as the audit record.
Scope: the desktop app's chrome — where buttons, menus, icons, panels and tools
live today, and where they will live as the app grows into indicators, drawing
tools and beyond. This document is the reference for every future UI change:
a new feature first finds its home here, then gets built.

The companion images in [`img/`](img/) are part of the spec, not decoration.
Everything in `CLAUDE.md` applies unchanged; this model adds UI placement rules
on top of it.

---

## 1. Why now

The app's UI grew feature by feature: each capability added one more control to
a single toolbar row, one more side panel, or one more floating box painted
over the candles. That was the right cost/benefit while the app was proving
its engine. Two upcoming features break the pattern:

- **Indicators** (`docs/indicator-system-plan.md`) need a manager panel,
  per-indicator settings, overlay legends, sub-panes and error surfaces.
- **Drawing tools** need an exclusive-selection tool palette and object
  editing conventions.

Bolted onto the current chrome, each would add checkboxes to an already
overloaded row and panels to an already crowded left edge. The model below
reorganizes the shell **once**, so that everything after it docks into a
reserved zone instead of inventing a new surface.

## 2. Today's UI — audit

![Today's UI, annotated](img/01-current-audit.svg)

Inventory, from code:

| Surface | Kind | Code |
|---|---|---|
| Menu bar (`File`, `Help`) | top panel | `app.rs::draw_menu_bar` |
| Controls row (feed, symbol, bar type + param, history paging, candle style button, heatmap toggle + L2 panel button, bubbles toggle + panel button, perf toggle) | top panel, one wrapped row | `app.rs::draw_controls` |
| L2 heatmap settings | **left** side panel | `orderflow_view.rs::draw_l2_panel` |
| Aggression bubbles settings | **left** side panel | `orderflow_view.rs::draw_bubbles_panel` |
| Candle style editor | floating window | `candle_view.rs::draw_style_window` |
| Market Replay browser | floating window (Ctrl+R) | `replay_view.rs::draw_browser` |
| Replay transport | bottom panel (while replaying) | `replay_view.rs::draw_transport` |
| Timezone picker | floating area, pinned bottom-right | `app.rs::draw_timezone_selector` |
| Header line (symbol · spec · bar counts · mode) | text painted on chart | `app.rs::draw_header` |
| Perf overlay (fps, frame ms, lag, trades) | box painted on chart | `app.rs::draw_overlay` |
| History loader | spinner painted on chart | `app.rs::draw_history_loader` |
| Backfill/live divider | amber line on chart | `app.rs::draw_backfill_divider` |

Findings (numbered in the image):

1. **The row answers six unrelated questions.** Source, bar spec, history,
   appearance, layers and diagnostics share one `horizontal_wrapped` row. It
   wraps unpredictably and grows linearly with features.
2. **Panels multiply leftward.** Every settings panel is its own
   `SidePanel::left`; two open at once cost the chart ~400 px.
3. **Four surface conventions coexist.** Docked panels, floating windows, a
   pinned area and painted overlays — with no rule for which a new feature
   should pick.
4. **No icon system.** Three ad-hoc emoji glyphs (`⟲`, `🎨`, `📈`) with no
   shared geometry or states.
5. **Status is scattered across four corners** of the canvas, occluding
   candles — there is no single line to glance at.
6. **Nothing is reserved for what is coming.** Indicators, drawings and
   alerts have no zone; each would extend findings 1–5.
7. **The transport is an island** with its own bottom-panel conventions.

What today's UI gets **right**, and the model keeps:

- Affordances gate on `FeedCapabilities`, never on provider names.
- Amber consistently marks "not live" (backfill divider, replay).
- Data text is monospace; chrome text is proportional.
- The chart owns almost the whole window; chrome is thin.

## 3. Principles

1. **The chart buys every pixel.** Fixed chrome has a budget (§5); any new
   chrome must fit an existing zone or displace something inside it.
2. **Group by question, not by feature.** *What am I looking at?* (source,
   bars) · *what is drawn on it?* (layers) · *how do I act on it?* (tools) ·
   *how healthy is it?* (status). A feature splits across groups rather than
   forming its own cluster.
3. **The amber thread.** `#F0B90B` is reserved for provenance honesty: replay,
   backfill boundary, inferred data, stale scripts. It is never decoration.
   This is the UI expression of the project's data-honesty rule — and the one
   deliberate visual signature of the app.
4. **Disabled ≠ hidden.** A capability the venue lacks renders at 40% opacity
   and explains itself on hover. Only a mode change (replay) may swap chrome.
5. **Docked is persistent, floating is transient.** Anything a trader keeps
   open while trading docks (settings, managers). Anything opened, used and
   closed floats (browsers, pickers, error details).

## 4. Design tokens

Palette — extracted from `app.rs` / `style.rs`, now named:

| Token | Value | Role |
|---|---|---|
| `bg/canvas` | `#131722` | chart canvas (user-configurable) |
| `bg/chrome` | `#171B26` | menu, toolbar, dock, status bar |
| `bg/inset` | `#10141D` | sub-panes, wells |
| `bg/control` | `#232936` | buttons, combos, inputs (also grid lines) |
| `border` | `#2E3648` | panel and control borders |
| `text/primary` | `#D2DAE2` | labels, values (today's `OVERLAY`) |
| `text/muted` | `#96A0AF` | secondary labels (today's `MUTED`) |
| `text/faint` | `#6E7887` | hints, disabled (today's `CROSSHAIR`) |
| `buy` | `#26A69A` | bull candles, buy flow, healthy/live dot |
| `sell` | `#EF5350` | bear candles, sell flow |
| `accent/overlay` | `#8AB4F8` | indicator overlay default (first plot) |
| `honest/amber` | `#F0B90B` | not-live provenance only (§3.3) |
| `warn` | `#FF6347` | threshold breaches, script errors |
| `tag/bg` | `#373F50` | tooltips, axis price tag |

Type:

- **Data is monospace** (`Consolas`/platform mono): prices, counts, times,
  lag, speeds. Sizes 10–12 px.
- **Chrome is proportional** (egui default): labels, menus, buttons. Sizes
  11–13 px.
- No third family; weight (400/600) is the only emphasis tool.

Icons (§6–§7 use them):

- 16 px glyph on a 28×28 px hit target (rail: 20 px on 30×30).
- Stroke 1.5 px, outline style; **filled/tinted = active**, outline = idle.
- States: idle `text/muted` → hover `text/primary` on `border` fill → active
  glyph in layer accent on 22% tint → disabled 40% opacity + hover
  explanation.
- Source: a vetted icon font (e.g. Phosphor via `egui-phosphor`) or in-house
  tessellated set — **no more ad-hoc emoji in chrome**. Emoji remain fine
  inside free text (menus, tooltips).

## 5. The shell — nine zones

![Zone model](img/02-shell-zones.svg)

| # | Zone | Size | Owns |
|---|---|---|---|
| 1 | Menu bar | 28 px | rare actions, discoverability, shortcuts (§10) |
| 2 | Context toolbar | 44 px | source · bars · history · layers · look · panels (§6) |
| 3 | Tool rail | 44 px | chart-acting tools: cursor, crosshair, drawings (§7) |
| 4 | Chart canvas | elastic | candles, flow layers, overlay legends, drawings |
| 5 | Sub-panes | ≤ 40% of 4, max 3 | pane indicators: CVD, delta… (§9) |
| 6 | Time axis | 24 px | time labels, x-zoom drag |
| 7 | Status bar | 28 px (+30 transport) | provenance · content · machinery (§8) |
| 8 | Dock | 280–360 px, collapsible to 36 | tabbed settings panels (§7) |
| 9 | Price axis | 64 px | price labels, y-zoom drag |

Vertical chrome while live: 28+44+24+28 = **124 px** — 4 px more than today's
120 px (menu 26 + controls 36 + time 22 + header/overlays ≈ 36 painted over
candles). In exchange, nothing is painted over the candles anymore except the
legend and honest markers.

**The rule that keeps this stable:** a new feature must claim a slot inside
zones 1–9. If it genuinely cannot, the shell — not the feature — is what gets
redesigned, in this document first.

## 6. Toolbar

![Toolbar model](img/03-toolbar-model.svg)

Left, *what am I looking at*:

- **SOURCE** — feed combo, symbol combo (from config, as today). During
  replay the group is replaced by the amber session label; you cannot pick a
  venue without closing the recording first (existing behaviour, kept).
- **BARS** — bar-kind combo + its one parameter drag (as today).
- **HISTORY** — one `+ older ▾` split button; the page size moves into its
  menu. Gated by the `history_paging` capability.

Right, *what is drawn on it*:

- **LAYERS** — one icon toggle per visual layer: heatmap, bubbles,
  indicators (future). Left-click toggles the layer; a small caret (or
  right-click) opens the layer's dock tab. **Adding a layer adds one icon**,
  not a checkbox + button pair as today.
- **LOOK** — opens the appearance dialog (candles, canvas, grid).
- **PANELS** — shows/hides the dock.
- **⋯ overflow** — collapse target; the toolbar never wraps. Collapse order:
  LOOK → PANELS → HISTORY → bar parameter merges into the kind combo → feed
  name shrinks to its initial. Never folded: the symbol and LAYERS.

Leaving the toolbar: the perf checkbox (a *reading*, moves to the status
bar §8) and both panel-opening buttons (the dock's tab strip replaces them).

## 7. Tool rail and dock

![Tool rail and dock](img/04-tool-rail-dock.svg)

**Tool rail (left, 44 px).** Exclusive selection; exactly one tool armed;
`Esc` always returns to Pointer. Ships now with Pointer and Crosshair —
2 slots that already earn their place — and gives drawing tools their
permanent home the day they land:

| Tool | Key | Notes |
|---|---|---|
| Pointer | `Esc`/`1` | pan/zoom/select — today's default interaction |
| Crosshair | `2` | today's hover crosshair becomes a mode |
| Trend line | `T` | two anchors, **bar-index coordinates** (Pine-compatible) |
| Horizontal level | `H` | one price, extends both ways |
| Zone (rectangle) | `R` | supply/demand boxes |
| Measure | `M` | Δprice, Δbars — and Δdelta: an order-flow-native measure |
| Note | `N` | text pinned to bar + price |
| *(footer)* Magnet | | snap anchors to nearest bar OHLC |
| *(footer)* Lock | | drawings ignore edits until unlocked |

Footer slots are toggling *modifiers*, body slots are exclusive *tools*.
Object deletion happens on the selected object (`Del` / context row) — no
global "clear all" icon within reach of a slip.

User drawings persist per symbol; coordinates are bar-index + price, the same
x-model the indicator plan locks for scripts, so replay seeks and history
prepends shift them correctly (`viewport.shift_right_edge` already models
this).

**Dock (right, 280–360 px).** One tabbed panel replaces today's stack of
left `SidePanel`s: tabs **L2**, **Bubbles**, **Indicators**, **Session**.
Tab strip (36 px) stays visible when collapsed. Rules:

- A layer's *toggle* lives in the toolbar; its *settings* live in its tab.
  Opening a tab never toggles the layer — looking is not enabling.
- One tab open at a time; the chart pays the dock width once, not per panel.
- Width draggable, remembered per tab.
- Panel contents migrate as-is (palette, grouping, intensity…); only their
  home changes.
- Floating windows remain for transient tasks only: session browser (from
  the Session tab / Ctrl+R), appearance dialog, script error details.

Why right, when today's panels are left: the rail claims the left edge
(tools want to be near the pointer's resting side), settings are read-mostly,
and the dock sits beyond the price axis where the eye rarely travels
mid-trade.

## 8. Status bar and transport

![Status bar](img/06-status-bar.svg)

One 28 px line, three sections, replacing the perf overlay, the floating
timezone pill and the mode text painted over candles:

- **Left — provenance:** state dot (green live · amber replay · red stalled),
  venue, symbol, feed lag.
- **Middle — content:** bar spec, bar counts (`240+61 bars` keeps today's
  backfilled+live split), honesty labels such as `side: inferred (tick rule)`
  for feeds without true aggressor sides (the MT5/B3 case). Indicator
  recompute progress borrows this section, then yields it back.
- **Right — machinery:** trades ingested, fps + frame time, timezone picker
  (the bar's only control, kept at the far edge).

A reading that breaches its threshold turns `warn` — the layout never moves.
While a recording plays, the **transport strip** (30 px) appears directly
above the status line: play/pause, speed, seek bar, session label, close —
all amber-marked. The transport is thus part of the status system, not an
island.

The chart canvas keeps only: overlay legend (§9), the amber backfill divider,
the history-loading spinner, and the L2 connection badge — things that mark
*positions or data on the chart itself*.

## 9. Indicator surfaces

![Indicators](img/05-indicators-panes.svg)

Where the indicator plan (`docs/indicator-system-plan.md` §4) meets the
shell:

- **Legend (chart overlay, top-left)** — one row per active indicator:
  colour dot, short title, last value; eye / gear / remove appear on hover.
  Red row = script error (file:line + code, click for details); amber row =
  `stale — edit has errors` (last good version still running). The legend is
  the *fast path*; the full manager is the **Indicators dock tab** (add,
  reorder, library browser, input settings generated from `InputSpec`).
- **Sub-panes** — pane indicators stack between chart and time axis: own
  auto-fit y-scale, shared x-axis, shared crosshair. Max 3; draggable
  dividers; a pane collapses to its header. Pane headers carry name + live
  value + collapse chevron.
- **Honest gaps** — warmup NaN renders as a gap in the plot, never a
  fabricated value. This is a rendering *rule*, stated here so no future
  "fix" interpolates it away.
- **Insert menu** — `Insert → Indicator…` (opens the dock tab),
  `Insert → EMA / CVD / …` for natives. Drawing tools also list under
  `Insert` with their shortcuts, mirroring the rail.
- **Paint order** (extends the contract in `candle_view.rs`): heatmap →
  grid → script draw-objects → candles → overlay plots → user drawings →
  bubbles → legend/badges. Scripts draw under user drawings; both under
  bubbles — execution evidence always wins.

## 10. Menus and shortcuts

Menus stay shallow; they exist for discoverability and shortcuts, never as
the only path:

- **File** — Market Replay… (`Ctrl+R`), Close Replay, Exit.
- **View** — dock show/hide (`Ctrl+B`), each dock tab, sub-pane
  collapse-all, perf readings on/off, timezone.
- **Insert** — indicators (natives + library), drawing tools (§9).
- **Tools** — appearance dialog, future: script library folder, alerts.
- **Help** — replay file format…, future: Pine dialect reference,
  shortcut list.

Shortcut map (reserved now so nothing collides later):
`Esc/1` pointer · `2` crosshair · `T/H/R/M/N` drawing tools · `Del` delete
selected drawing · `Ctrl+R` replay browser · `Ctrl+B` dock · `Space`
play/pause while replaying · `+`/`−` replay speed. Single letters stay free
of modifiers (no text inputs live on the chart).

## 11. Growth map

Where future features land — decided now, so they never claim new chrome:

| Future feature | Toggle/entry | Settings | Canvas presence | Status |
|---|---|---|---|---|
| Indicators (M1–M5) | LAYERS icon + Insert menu | dock tab Indicators | legend rows, overlays, sub-panes | recompute progress |
| Drawing tools | tool rail + Insert menu | selected-object popover | drawings layer | object count (View) |
| Alerts | on indicator/level context | dock tab Alerts | triggered marker on bar | armed count + last fired |
| Bot / strategy monitor | LAYERS icon | dock tab | order/position markers | connection + P&L cell |
| Second chart / layouts | File → Layout | — | split canvas (zones 4–6 duplicate per chart; zones 1–3, 7–9 stay singular) | per-chart provenance |
| Watchlist | — | dock tab | — | — |

## 12. Migration plan

Each phase is one small PR, passing the four checks, shippable alone, no
engine changes anywhere. Suggested order — cheapest confidence first:

1. **Status bar** (`app/src/statusbar.rs`) — move perf readings, timezone,
   mode/counts line off the canvas; delete `draw_overlay`,
   `draw_timezone_selector`, and the header's status half. Transport strip
   restyles into it (`replay_view.rs` keeps ownership).
2. **Toolbar regroup** (`app/src/toolbar.rs`) — extract `draw_controls`,
   regroup per §6 with the overflow rule; history becomes a split button.
   No behaviour change, only placement; capability gating untouched.
3. **Icon set** — adopt the icon font/set, replace emoji glyphs, implement
   the four button states as a small widget helper (`app/src/widgets.rs`).
4. **Dock** (`app/src/dock.rs`) — tabbed right dock; migrate L2 + bubbles
   panel bodies unchanged; retire the two `SidePanel::left`s; Session tab
   hosts the replay browser entry.
5. **Tool rail skeleton** (`app/src/toolrail.rs`) — Pointer + Crosshair as
   real modes with the exclusive-selection + `Esc` grammar, reserving the
   geometry drawing tools will fill.
6. **Indicator surfaces** — land with indicator milestones M1/M4 (worker,
   legend, panes, dock tab), already knowing their home.

Phases 1–3 are pure relocation and can precede indicator M1; phase 4 should
land before the Indicators tab is needed; phase 5 anytime; phase 6 rides the
indicator plan.

## 13. Open questions

- **Persistence of chrome state** (dock width, active tab, rail tool,
  sub-pane heights): piggyback on the indicator plan's
  `indicators-state.toml` or a sibling `ui-state.toml`. Decide at phase 4.
- **Multi-chart** (§11) doubles zones 4–6 per chart; the model supports it,
  but splitter behaviour and per-chart toolbars are deliberately unspecified
  until a real need exists.
- **Themes**: tokens (§4) make a light theme *possible*; it is a non-goal
  until someone asks for one.
