# Drawing tools — from five marks to a price-action toolbox

August 2026. A design review of the drawing rail with a UX lead and a
discretionary price-action trader, held against the shipped build. It
extends `docs/drawing-toolbar-ux.md` (which redesigned the *rail chrome*)
with the thing that review deliberately left alone: **which tools exist,
how they look on the candles, and how a trader lives with them.**

Personas are the fixed cast from `.claude/skills/trader-ux-review`: Rafa
(order-flow scalper), Marina (swing / multi-timeframe) and Duda (newcomer).

---

## 1. Where we actually are

The registry ships five tools:

| Tool | Points | Shortcut |
| --- | --- | --- |
| Horizontal line | 1 | `H` |
| Rectangle | 2 | `R` |
| Parallel channel | 3 | `C` |
| Fib retracement | 2 | `F` |
| Fib extension | 3 | `Shift+F` |

**There is no trend line.** For a chartist that is not a gap, it is the
absence of the instrument. Everything else in this document follows from
that one fact: the rail chrome was built well ahead of the toolbox it
holds, and the toolbox never caught up.

Four structural findings came out of the session.

### F1 — The set is not a set

A price-action trader works with three primitives and one measurement:
a **line** (support, resistance, trend, ray into the future), a **zone**
(a rectangle of value, a channel of trend), a **level ratio** (fib), and a
**measurement** (how far, in points and in percent). We ship the zone and
the ratio, half the measurement (none), and none of the line.

> **Rafa:** "I mark support with a horizontal and everything else I hold in
> my head. There is no ruler, so when I want to know if this leg is 0.4% or
> 1.2% I open a calculator. On a fast tape I just don't check."

### F2 — The marks are loud

`DEFAULT_DRAWING_WIDTH_PX = 1.5` with a selection halo `3.5 px` wider and
solid-white 4 px anchor discs. Against a candle body that is often 3–6 px
wide, a 1.5 px annotation competes with the data. TradingView and
MetaTrader both default to a **1 px hairline**; the drawing is a note *on*
the chart, never a second series.

> **Marina:** "My channel is thicker than the candles inside it. It reads
> like a highlighter, not an annotation."

Data honesty applies to visual weight too: user marks are opinion, market
data is fact, and opinion should not out-shout fact.

### F3 — The inspector lands on the evidence

`inspector_placement` scores eight candidates by *least overlap with the
object's bounding box*, tie-breaking toward greater centre distance. The
intent was right; the outcome is that a small object (a horizontal line, a
short trend line) has a small bbox, so "beside it with a 12 px gap" scores
zero overlap and wins — and the panel lands directly on the price action
the trader drew the line to read.

> **Rafa:** "I click the line to change its colour and the window covers the
> exact candles I was looking at. I have to drag it away every single time."

The bug is the objective function. Zero overlap with the *bbox* is not the
goal; **staying out of the way of the read** is. The read is the
neighbourhood of the object, not its bounding box.

### F4 — A drawing is trapped in one pane

`ChartPoint { bar: f32, price: f64 }` anchors to a **bar index**, which is
local to a pane's own series. The 5-minute pane and the tick pane of the
same symbol have completely different bar indices for the same instant, so
a mark drawn on one cannot be expressed on the other — even a horizontal
line, whose price is trivially universal.

> **Marina:** "I draw the trendline on the 5m and switch to ticks to time the
> entry. The line isn't there. I draw it again, badly, and now I have two
> slightly different lines and I don't know which one is the real one."

That last sentence is the real cost: it is not inconvenience, it is
**two versions of the truth**.

---

## 2. Decisions

### D1 — The complete toolbox

Registry order, folded into rail families:

| Family | Tools | Shortcut |
| --- | --- | --- |
| Lines | Trend line | `T` |
| | Ray | `Shift+T` |
| | Extended line | — |
| | Horizontal line | `H` |
| | Horizontal ray | — |
| | Vertical line | `V` |
| | Arrow | — |
| Channels | Parallel channel (rising / falling) | `C` |
| Shapes | Rectangle | `R` |
| | Ellipse | — |
| | Triangle | — |
| Fib | Fib retracement | `F` |
| | Fib extension | `Shift+F` |
| Measure | Ruler | `M` |
| | Price range | — |
| | Date range | — |
| Text | Text note | `A` |

Ray, extended line and horizontal ray *could* be extension flags on the
trend line and the horizontal line. We ship them as named tools anyway:
Duda cannot find a checkbox she does not know exists, and every competing
platform names them. The flags exist too, on the channel, where they are
genuinely a property of one object rather than a different intent.

Deliberately **not** in this pass: pitchfork, regression channel, Gann,
Elliott labels, harmonic patterns, the long/short position tool (the
paper-trading order UI already owns entry / stop / target on the chart).

### D2 — Hairline by default

- Stroke `1.5 → 1.0 px`.
- Fill alpha `24 → 14`.
- Selection anchors: hollow rings in the object's own colour with a light
  core, `3.5 px`, replacing solid-white discs.
- Selection halo: narrower and dimmer — enough to find the object under the
  pointer, not enough to double its weight.
- Labels (fib levels, ruler readout): 10 px, muted, no box unless the label
  sits over candles, where it gets a low-alpha plate for legibility rather
  than a chrome-coloured box.

The width slider keeps its full `0.5 … 6.0` range: this changes the
*default*, not the ceiling. Existing drawings keep whatever width they
were saved with.

### D3 — The inspector goes to a corner

New rule, replacing least-bbox-overlap:

1. Candidates are the **chart corners**, inset by the standard gap, and the
   two **top** corners are tried first.
2. Of those, the one that clears the bbox entirely and whose centre is
   **farthest** from the object wins; an exact tie (a centred object) takes
   the left one.
3. Only if neither top corner is free, the bottom two on the same rule.
4. Only if all four are fouled — a large object covering the chart — fall
   back to the beside-the-object candidates, least overlap first.
5. Manual position still wins forever, unchanged. The auto-pin rule for
   narrow charts is unchanged.

Top before bottom is **structural, not taste**. A floating panel is
positioned by its top-left corner and grows downwards, and the placement
runs before the panel's real height is known (the Fib level editor is twice
the height of the Style tab). A top corner always has the whole pane to
grow into; a bottom-anchored panel that turns out taller than assumed runs
off the window and silently loses its last rows. Rows a trader cannot reach
read as rows that do not exist — the same failure the old rule had, one axis
over.

> **Duda:** "If it always opens in the same place I learn where to look. It
> jumping around 'intelligently' is what makes it feel random."

Consistency is a feature here, which is why the corner preference is
ordered and deterministic rather than merely "the emptiest area".

### D4 — The channel says what a trader calls it

`ParallelChannelPayload { midline, extend_left, extend_right }`, edited in
a tool-owned inspector tab (`Channel`), exactly the way the Fib level
editor mounts its own tab — no central inspector edits.

- `midline` paints a dashed line at half alpha through the middle of the
  channel. Default **on**: it is the line traders trade off, and TradingView
  users expect it. Existing channels get it via the payload default, which
  is a visible change to an existing object — accepted deliberately and
  called out in the PR body, because a channel without its midline is the
  odd one out, not the baseline.
- `extend_left` / `extend_right` project the three rails past their
  anchors to the chart edge. Default off.
- Naming: hover text becomes *"Rising / falling channel — set the trend
  line, then click the channel width (C)"*. "Parallel channel" stays the
  formal name in the inspector header; the hover is where Duda is standing.

### D5 — The ruler reads in percent

One readout, five numbers, laid out to be read in a glance:

```
+412 pts   +1.83%   82 ticks
17 bars    4m 21s
```

- **Points** — the raw price delta, in the instrument's own units.
- **Percent** — `delta / start_price`, signed. This is the number the
  session asked for and the one every other platform shows.
- **Ticks** — points divided by the instrument tick size, for the WIN /
  WDO traders who size in ticks.
- **Bars** and **elapsed time** — how long the move took, on that pane's
  own bar type.

Sign and colour follow the direction (up / down), never a fixed accent, so
the direction is readable without reading the sign. The readout sits at the
midpoint of the measured leg with a low-alpha plate.

Price range and date range are the same tool with one axis suppressed, and
they share the readout renderer — one implementation of "how do we phrase a
move", not three.

The ruler is a **persistent, selectable drawing** like everything else,
not TradingView's vanishing one-shot. Marina's persistence requirement
beats the novelty, Rafa gets `Esc` / `Delete`, and one interaction model
for every tool is worth more than matching one competitor's quirk.

### D6 — Magnet

A rail toggle. When on, a placed anchor snaps to the nearest of the
open / high / low / close of the bar under the cursor, if that price is
within a pixel threshold of the pointer. Off by default; the rail button
shows its state like the repeat pin does.

This is the difference between a line that *looks* drawn off the swing high
and one that *is*. Every professional platform has it, and price-action
trading is precisely the discipline that depends on it.

### D7 — Show on all charts

`DrawingScope { ThisChart, AllCharts }` on the drawing, default
`ThisChart` — today's behaviour is the default, unchanged.

The mechanism, in one sentence: **a shared anchor stores the market
timestamp captured when it was placed, and every pane reprojects that
timestamp through its own `slot_at_time`.**

- `ChartPoint` gains the market time of the anchor, captured at placement
  from the pane's `slot_open_time`. Price needs no translation — it is the
  same instrument.
- Panes already answer `slot_at_time(ms) -> Option<usize>` across the
  history-prefix seam (`pane.rs`), so the projection is one existing call
  per anchor, not new machinery.
- A shared drawing lives in a tab-level store every pane of that tab
  renders; a `ThisChart` drawing stays exactly where it lives today.
- Toggled from the inspector's **Coordinates** tab, where the anchors
  already are. The object manager marks shared objects so Marina can see at
  a glance which marks are global.

**Scope is the panes of one tab — one symbol, one feed.** Sharing across
tabs would put a BTC line on a WIN chart, and a price level that means
nothing on the instrument it is drawn over is exactly the "data honesty"
failure this repo refuses. If cross-symbol sharing is ever wanted it needs
its own design (percent-anchored levels, or an explicit symbol binding),
not a wider checkbox.

Honesty at the edges, since a timestamp can fall outside a pane's series:

- Before the first bar or after the last: the anchor projects to the
  clamped edge slot, and the drawing is painted at reduced alpha with the
  clamp visible, rather than silently pretending the anchor is on-screen.
- A pane with no bars yet paints no shared drawings — nothing to anchor to.

---

## 3. Acceptance, in persona terms

- **Rafa** can arm a trend line with `T`, measure a leg in percent with
  `M`, and never has an inspector land on the candles he is reading.
- **Marina** draws once on the 5m, ticks *Show on all charts*, and the same
  line — one version of the truth — appears on the tick pane at the same
  instant in market time.
- **Duda** opens the Lines family and reads seven names she recognises,
  finds "rising / falling channel" by hovering the channel slot, and sees
  the ruler answer in percent without knowing what a tick is.

---

## 4. What this does not change

The rail chrome contract of `docs/drawing-toolbar-ux.md` stands: geometry,
docking, badges, stages, the escape stack and the pointer-routing rules are
unchanged. The registry macro stays the only docking point — every tool in
D1 is one implementation file plus one name in the list.
