# Chart canvas — full audit report

Read-only analysis of `crates/app/src/{chart,candle_view,price_view,viewport,orderflow_view,orderflow_render,live_strip,bubble_presets,pane,time_header,timezone}.rs`, `orderflow/*`, plus the input/layout code that actually owns the canvas gestures (`app.rs`, `tab.rs`, `toolbar.rs`, `toolrail.rs`, `statusbar.rs`).

One structural note up front: `chart.rs`, `viewport.rs`, `price_view.rs` and `live_strip.rs` are pure, well-tested geometry with no UI in them. Every user-facing gesture on the canvas lives in `pane.rs::handle_navigation` (`pane.rs:1048-1389`) and every pixel is painted from `pane.rs::draw_chart` (`pane.rs:1391-1751`). `orderflow/interaction.rs` is named for *market* interaction (aggression vs. resting liquidity), not pointer interaction — it contains no UI.

## 1. INVENTORY

### Time-axis navigation

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 1 | Chart-body pan (time) | `pane.rs:1256-1266` | Left-drag horizontally anywhere on the candle body | `Viewport::pan_pixels` (`viewport.rs:84`); right edge moves by `dx / candle_width`. Drag right reveals history. |
| 2 | Auto-resume follow | `viewport.rs:96` | Pan back until right edge is within 0.5 bars of newest | `follow = true`; chart re-pins to live. No user action, no announcement. |
| 3 | Pan into empty future | `viewport.rs:18, 91-92` | Drag left past the newest bar | Right edge may travel `FUTURE_MARGIN_BARS = 40` slots past newest. Fixed bar count, not a pixel or viewport fraction. |
| 4 | Chart-body zoom (time) | `pane.rs:1280-1292` | Mouse wheel over the candle body | `viewport.zoom(2^(scroll/300))`. **Anchored to the right edge** (`viewport.rs:63`), not to the pointer. |
| 5 | Time-strip drag zoom | `pane.rs:1326-1335` | Left-drag horizontally on the bottom time strip | `viewport.zoom(exp(dx/120))`, `LANE_ZOOM_DRAG_PX` at `pane.rs:242`. |
| 6 | Time-strip scroll zoom | `pane.rs:1336-1341` | Wheel over the bottom time strip | Same `2^(scroll/300)`. |
| 7 | Zoom clamp | `viewport.rs:14-16` | — | Candle slot clamped to 2 px … 64 px. |
| 8 | Jump to live + reset price | `pane.rs:1276-1279` | **Double-click the chart body** | `viewport.snap_to_live()` **and** `price_view.reset()` — one gesture, two resets, not separable. |

### Price-axis navigation

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 9 | Chart-body pan (price) | `pane.rs:1267-1274` | Left-drag vertically on the candle body | `PriceView::pan` (`price_view.rs:42`); takes the axis out of auto-fit permanently. |
| 10 | Price-gutter drag zoom | `pane.rs:222-228` (`axis_zoom_gesture`) | Left-drag vertically on the right price gutter | `price_view.zoom(exp(dy/150))`, `AXIS_ZOOM_DRAG_PX` at `pane.rs:191`. Drag up compresses the span, down expands it. |
| 11 | Price-gutter scroll zoom | `pane.rs:229-234` | Wheel over the price gutter | `zoom(exp(-scroll/200))`, `AXIS_ZOOM_SCROLL_PX` at `pane.rs:195`. |
| 12 | Price-axis reset | `pane.rs:216-218` | **Double-click the price gutter** | `price_view.reset()` → back to auto-fit. |
| 13 | Auto-fit | `chart.rs:38-63`, `chart.rs:124-135` | Default state | Fits visible bars' high/low with `AUTO_PAD_FRAC = 0.05` (`chart.rs:139`). Falls back to last frame's window, then to the newest bar, so an empty view never blanks. |

### Per-pane y-axis (PR #117)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 14 | Indicator-pane axis zoom | `pane.rs:1380-1388` | Drag / scroll / double-click on that pane's own gutter band | Same `axis_zoom_gesture` as the price gutter. Keyed by `(pane_id, slot)` so a split's two charts don't share a drag. |
| 15 | Pane gutter banding | `app.rs:192-198` | — | Each pane's gutter spans exactly its own height, so a drag can only ever move the pane whose numbers it is over. |

### Live lane (reserved band)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 16 | Lane divider resize | `pane.rs:1299-1319` | Drag the divider, grab area ±5 px (`pane.rs:184`) | `resize_live_lane` (`orderflow_view.rs:246`). Cursor becomes `ResizeHorizontal` — **the only cursor change on the entire canvas**. |
| 17 | Lane time zoom (wheel) | `pane.rs:1285-1288` | Wheel while the pointer is right of the divider | `zoom_live_lane`; the candles do not zoom. Boundary from `gesture_hits_lane` (`app.rs:222`). |
| 18 | Lane time-strip zoom | `pane.rs:1344-1362` | Drag / wheel on the time-strip segment under the lane | `zoom_live_lane(exp(dx/120))`. |
| 19 | Lane window readout | `pane.rs:1809-1825` | Always, when a lane exists | Prints `tape · 37.5 s` under the lane. The only readout of what lane zoom is worth. |
| 20 | Lane immunity to pan/zoom | `viewport.rs:388-399` (test) | — | Panning and zooming candles never moves the tape. |

### Crosshair and readouts

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 21 | Crosshair | `pane.rs:2007-2059` | **Only while `Tool::Crosshair` is armed** (`pane.rs:2015`), key `2` (`toolrail.rs:457`) | Vertical + horizontal hairline, plus a price tag on the axis. |
| 22 | Crosshair price tag | `pane.rs:2042-2058` | Same | `{price:.2}` on `theme::TAG_BG`. **No time tag on the x axis.** |
| 23 | Last-price line | `pane.rs:1904-1909` | Always | Dashed 4/4 px across the body, coloured by the carrying bar's direction (`candle_view.rs:35`), alpha 0.55. |
| 24 | Last-price chip | `pane.rs:1913-1924` | Always | Filled chip on the gutter, `{price:.2}`, dark text. |
| 25 | Price gridlines + labels | `pane.rs:1841-1860` | Always | `nice_ticks(lo, hi, 8)` (`chart.rs:295`), label `{tick:.2}`. |
| 26 | Time-strip labels | `pane.rs:1775-1799` | Always | `HH:MM:SS` (`app.rs:354`), step `= (visible/6).max(1)` in **bar indices**, not pixels. No date component. |
| 27 | Timezone selector | `statusbar.rs:328-334` | Combo in the status bar | 38 fixed offsets (`timezone.rs:45-84`), default UTC−03:00 (`timezone.rs:88-92`). Relabels the time axis only; engine stays UTC. |

### Canvas chrome and honesty marks

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 28 | Seam divider | `pane.rs:2069-2092` | Venue prefix present and on screen | Dashed amber rule + `venue` label. |
| 29 | Backfill divider | `pane.rs:2099-2139` | Backfill boundary on screen | Solid amber rule + `backfill` / `live` labels. |
| 30 | Empty-view message | `pane.rs:1732-1740` | No bars in the candle pane | `"no bars in view — double-click to return to the live edge"`. |
| 31 | Connecting message | `pane.rs:1413-1419` | `total == 0` | `"connecting to <symbol> …"`. |
| 32 | L2 status badge | `orderflow_view.rs:498-521` | **Only while `depth_visible()`** (`:501`) | Top-right plate with the capture state. |
| 33 | Status-bar follow hint | `statusbar.rs:309-315` | Only when `!follows_live` | `"history · double-click for live"`. |
| 34 | Status-bar price hint | `statusbar.rs:316-322` | Only when `!price_auto` | `"price: manual · double-click the axis to auto-fit"`. |
| 35 | Focused-pane rule | `tab.rs:1541-1547` | Split layout | 1 px accent line under the focused pane's top edge. |
| 36 | Pane focus by click | `tab.rs:1553-1575` | Any primary press inside a pane | Sets `focus`; read from the raw pointer so the same press that starts a pan also focuses. |
| 37 | Canvas divider | `tab.rs:1582-1599` | Drag between the two panes | Resize, clamped to 25 % minimum each (`pane.rs:115, 162`). |
| 38 | Timeframe chips | `time_header.rs:85-108` | Click 1m/5m/15m/1h, or drag the ms `DragValue` | Time pane interval. |

### Layer toggles — all off-canvas, in the toolbar

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 39 | Bubbles toggle | `toolbar.rs:518-536` | Left-click icon | `set_bubbles_enabled`. Disabled when the feed reports no traded volume (`:524`). Right-click opens the Bubbles dock tab. |
| 40 | Heatmap toggle | `toolbar.rs:538-553` | Left-click icon | `set_depth_visible`; capture keeps running (`orderflow_view.rs:183-198`). Disabled without `book_capture`. Right-click opens the L2 tab. |
| 41 | Live-strip toggle | `toolbar.rs:558-571` | Left-click icon | `live_strip_visible`; never capability-gated (`pane.rs:584-595`). |
| 42 | Live strip content | `orderflow_view.rs:541-643` | Strip visible | Mirrored buy/sell aggression histogram for the forming bar, normalized by its own biggest bucket (`live_strip.rs:91-96`), plus best bid/ask touch lines. |

### Bubble anatomy (one mark, up to ten simultaneous encodings)

`draw_bubble` at `orderflow_render.rs:522-669`. Radius = quantity (`bubble_radius:2252`); fill colour = side (`:192-198`); vertical lean = side, sliding with buy share (`:1300-1305`); halo alpha = normalized size (`:348-350`); hollow ring vs solid disc = buy side below `readable_min_radius` (`:560-562`); pie split = buy share of a closed-bar summary (`:610-623`); consumption front = ate resting liquidity (`:647-656`); impact-ring brightness = matched fraction (`:657-668`); trail = ate liquidity (`:1337-1358`); text = quantity and/or `×N` (`:1382-1405`). Three further channels (sphere shading, rim, separator ring) carry no data. The on-canvas legend (`:1415-1445`) explains at most six things and needs `chart_rect.width() >= 150.0` to draw at all (`:1451`).

### Gestures that do **not** exist

- **No right-click anywhere on the canvas.** `secondary_clicked` appears only at `toolbar.rs:534/551/569` and `toolrail.rs:974`. No context menu on a candle, bubble, axis, drawing or the lane.
- **No hover or click on any order-flow mark.** Every flow draw function takes a bare `&egui::Painter`; the only `egui::Sense` in `orderflow_render.rs` is at line 1529, inside the *settings-panel preview*. No hit-test, no nearest-bubble search, no tooltip.
- **No candle OHLC readout** of any kind — no hover tooltip, no corner legend.
- **No keyboard chart navigation.** Shortcuts are tabs/dock/replay (`app.rs:1729-1748`) and tool arming (`toolrail.rs:454-463`). No arrow-key pan, no `+`/`−`, no Home/End.
- **No middle-drag pan, no horizontal wheel, no Ctrl/Shift wheel modifiers.** Only `raw_scroll_delta.y`.


## 2. FLOWS

**(a) Navigating history and returning to live.** Left-drag right on the candle body; each pixel moves the right edge by `1/candle_width` bars (`pane.rs:1266`). At the default 8 px slot, a full 1200 px drag buys 150 bars. Older trades are fetched separately through the toolbar's HISTORY group, not by reaching the left edge. To return: double-click the chart body (`pane.rs:1276`) — or drag back until the edge falls within half a bar of the newest, which silently re-arms follow (`viewport.rs:96`). There is no button, and the only on-screen hint is a small grey line in the status bar (`statusbar.rs:311`) that appears only after you have already left. When history is prepended or the series is re-cut, the window is held steady by `shift_right_edge` / `reanchor` (`viewport.rs:129, 153`) — this part is handled carefully.

**(b) Zooming time and price.** Time: wheel over the body, or drag/wheel the bottom time strip. Price: drag or wheel the right gutter. The two axes never zoom together and there is no gesture that does both. Time zoom is right-edge-anchored, so the bars under the cursor slide away as you zoom.

**(c) Resetting a manually-zoomed y-axis.** Double-click the gutter (`pane.rs:216`). For an indicator pane, double-click *that pane's own* gutter band (`pane.rs:1380`). Double-clicking the chart body also resets price, but drags the viewport to live as a side effect (`pane.rs:1276-1279`). Discovery relies entirely on the status-bar line at `statusbar.rs:318`, which only appears once the axis is already manual.

**(d) Reading order-flow information from a bubble.** There is no flow. A bubble carries up to ten simultaneous encodings and **none of them is queryable**. The only numeric readout is the optional in-bubble label (`orderflow_render.rs:1382-1405`), which requires `radius >= label_min_radius` *and* the text to fit within `1.78 r × 1.45 r`. Everything else — exact quantity, price, timestamp, trade count, what it consumed — is unreachable from the chart.

**(e) Toggling visual layers.** Not on the canvas at all. Three icon buttons in the toolbar's LAYERS group (`toolbar.rs:515-572`); left-click toggles, right-click opens the corresponding dock tab for settings. Bubbles and heatmap are capability-gated with disabled-state explanations, which is good practice. But the toggles always act on the **flow pane's** tape (`app.rs:910-911, 985-995` → `tab.rs:614`), whereas the indicator commands beside them act on the **focused** pane (`app.rs:1007`).

## 3. UX FINDINGS

### Blocker

**F1 — Capped projections drop market data silently.** `max_aggression_primitives = 700` and `max_visible_cells = 12000` (`orderflow/config.rs:810-811`) discard primitives whenever the frame exceeds them, size-ranked (`projection.rs:360`, `1071-1082`). The drop counts *are* computed (`projection.rs:349-356`) and *are* logged — as a `tracing::warn!` with `event_code = "HEATMAP_PROJECTION_CAPPED"` (`app.rs:1648-1662`) — but they never reach the canvas or the status bar. The trader sees a complete-looking tape that is a subset of the flow. This contradicts the project's own non-negotiable rule in CLAUDE.md: *"inferred or incomplete data is labeled as such, never silently patched."* Every other honesty case in this codebase gets an amber mark (`pane.rs:2084`, `2122`, `statusbar.rs:294`); this one gets a log line nobody reads.

### Major

**F2 — Zero inspection of order-flow marks.** (Heuristic: recognition rather than recall; match between system and real world.) The product's whole reason to exist is order-flow reading, and the richest mark on the screen answers no questions. A trader who spots an unusually large bubble cannot learn its size, its price, or when it printed. Evidence: `draw_aggression_bubbles` (`orderflow_render.rs:1286-1407`), `draw_heatmap_background` (`:896-992`), `draw_liquidity_events` (`:1058-1277`) and `OrderflowView::draw_live_strip` (`orderflow_view.rs:532-644`) all take `&Painter` only and cannot register interaction by construction. The projection already carries every field a tooltip would need (`AggressionPrimitive` in `orderflow/projection.rs`, including `agg_ids`, `trade_count`, `first/last_timestamp_ms`, `matched_fraction`) and each primitive already has `x`, `y`, `size` in screen space — the data for a hit-test is sitting there unused.

**F3 — The crosshair is a mode, and an expensive one.** `pane.rs:2015` returns early unless `Tool::Crosshair` is armed. The rail defaults to `Tool::Pointer`, so a fresh chart has **no cursor price readout at all**. Three compounding problems: (i) nothing on screen says the crosshair exists; (ii) at narrow rail widths the Crosshair button folds into the More overflow (`toolrail.rs:1576-1580`), so the only route to a price readout is buried in a menu; (iii) arming Crosshair costs you drawing selection and movement, because that whole branch requires `Tool::Pointer` (`pane.rs:1120`). The user must choose between reading prices and editing objects. Every mainstream charting product treats the crosshair as a hover state, not a tool.

**F4 — Price labels are hardcoded to two decimals, in three places.** `pane.rs:1856` (axis), `pane.rs:1914` (last-price chip), `pane.rs:2045` (crosshair tag). Two failure modes, both live in this product's own markets: on B3 index futures (WIN ≈ 137000, integer ticks) every label wastes three characters on `.00` in a narrow gutter; on a fine-tick instrument `nice_ticks` can return a step below 0.01, and then **two different gridlines print the identical number**. The fix already exists in the codebase and is not being used: `chart::axis_labels` (`chart.rs:365`) derives decimals from the step (`tick_decimals`, `chart.rs:451`), applies unit suffixes, and thins labels to `AXIS_LABEL_MIN_GAP_PX` (`thin_to_fit`, `chart.rs:424`). `draw_price_axis` bypasses all of it and calls raw `nice_ticks` (`pane.rs:1841`). The result is that the **price axis is the only axis in the app without step-aware decimals or collision thinning** — every indicator pane gets better treatment than the price.

**F5 — No gesture on the canvas is discoverable.** (Heuristic: visibility of system status; affordance.) `axis_zoom_gesture` (`pane.rs:208-235`) registers a full `click_and_drag` region over the price gutter and sets **no cursor, no hover highlight, nothing**. Same for the time strip (`pane.rs:1326`). The code is aware of the problem — the lane divider's comment at `pane.rs:178-183` states outright *"the resize cursor is the only thing that says so"* — and that is literally true: the lane divider is the sole element on the canvas that changes the pointer. So the price zoom, the time-strip zoom, the two double-click resets and the lane zoom are all invisible until someone tells you. The status-bar hints (`statusbar.rs:309-322`) are reactive and remote: they appear at the bottom of the window only *after* you have already left auto-fit, and they teach the way out, never the way in.

**F6 — No "back to live" control.** With bars still visible but old, nothing on the canvas offers a return; the empty-view message (`pane.rs:1736`) only fires in the degenerate case where the pane holds no bars at all. Panning back through a long session is a long drag with no scrollbar, no minimap, and no go-to-date.

**F7 — Double-click is overloaded and collides with object editing.** `pane.rs:1276-1279` resets viewport *and* price together, so a user who has carefully framed a price range and just wants to jump to live loses the framing; there is no way to reset time alone. Worse, with `Tool::Pointer` armed a click over a drawing selects it (`pane.rs:1151-1169`) — and no drawing tool implements `double_clicked` anywhere (confirmed: no matches in `crates/app/src/drawings`). So double-clicking a trendline, the natural "open properties" gesture in every charting product, instead **snaps the whole chart to the live edge**.

**F8 — Wheel zoom is anchored to the right edge, not the pointer.** `viewport.rs:63-69`: *"Anchored to the right edge — the newest bar stays put."* When examining a cluster 200 bars back, scrolling to zoom in pushes that cluster off screen. This is the single most-used chart gesture and it fights the user in exactly the situation the product is designed for.

**F9 — Visual overload has no cross-layer control.** With everything on, the canvas paints roughly thirty distinct layer types in one frame. Every de-cluttering mechanism is *within* a layer — temporal clustering (`orderflow/config.rs:30`), dust merge (`:452-462`), primitive caps (`:810-811`), noise floors (`:802-803`). There is no global opacity, no "dim everything but this layer", no focus mode. Layer opacity exists only inside the Bubbles dock tab, three clicks away. There is also no spatial de-confliction at all: no overlap test, no label-collision avoidance, no repulsion — two bubbles at the same price and time simply overdraw, mitigated only by a constant 3.5 px side nudge (`config.rs:384`) and a darker rim. Against the stated goal of a clean look, and with `live lane pie` (spheres, halo 0.20, pie splits, `×N` labels) shipped as the default for both MT5 feeds (`config/bubbles.toml:9`, `crates/app/config/feeds.toml:61-86`), the default state is the busiest one.

**F10 — Ten encodings on one mark, six-row legend, no key for the rest.** `legend_entries` (`orderflow_render.rs:1415-1445`) explains at most six things: liquidity grouping, buy aggression, sell aggression, aligned depletion, unattributed reduction, L2 gap. Nothing on canvas explains the **size scale, the pie split, the hollow ring, the halo, the impact ring, the trail, the side-offset nudge, `×N`, or the live lane itself**. Those explanations exist — as `on_hover_text` on settings widgets (`orderflow_view.rs:1046-1130`) — where a person reading the chart will never see them.

### Minor

**F11 — Two shipped presets have a dead label switch.** `default` sets `max_radius = 15.0` but `label_min_radius = 16.0` with `show_quantity_labels = true` (`config/bubbles.toml:19, 42`), and `live_lane.radius_scale = 1.0` (`:47`) so the lane cannot rescue it either. No bubble can ever reach the threshold; the toggle does nothing, silently. `dense tape btc` repeats it (`max 14.0` / `label_min 20.0` with `show_trade_count = true`, `:216, 239, 238`), and `3d spheres` sits exactly on the boundary (`16.0` / `16.0`, `:177, 200`). A control that reports "on" and produces nothing is worse than a missing one.

**F12 — Layer toggles ignore pane focus.** `app.rs:910-911` and `985-995` read and write `active_tab().tape()`, which is hard-wired to the flow pane (`tab.rs:614`), while the indicator entries in the same toolbar act on `focused_pane()` (`app.rs:1007`). Focus the time pane, click the bubbles icon: the icon lights, the pane under your eyes does not change. Two adjacent toolbar groups with two different targeting rules.

**F13 — Time axis prints no date and can repeat itself.** `fmt_time` yields `HH:MM:SS` only (`app.rs:354-359`). A time pane can hold 1h bars over months of venue history (`pane.rs:331`), and the axis never says which day. Separately, the label step is `(visible/6).max(1)` in **bar indices** (`pane.rs:1783`) with no pixel-gap thinning; on tick or volume bars several consecutive bars share the same second, so the strip prints the identical timestamp repeatedly. The price axis has `thin_to_fit` for exactly this class of problem (`chart.rs:424`); the time axis has nothing.

**F14 — The live-strip histogram has no scale.** Bars are normalized by the forming bar's own largest bucket (`live_strip.rs:91-96`), so the reference changes continuously and is never shown. Equal widths in two frames mean different quantities, and no number appears anywhere in the strip. Reasonable as a shape-reading device, but currently unlabelled in a codebase that is otherwise strict about labelling.

**F15 — The status badge reports only L2, and vanishes with the heatmap.** `orderflow_view.rs:501` gates on `depth_visible()`. Hide the map while capture is still syncing and the canvas goes quiet about the book entirely. Nothing on canvas ever reports the aggression layer's health.

**F16 — Future margin is a fixed 40 bars regardless of zoom.** `viewport.rs:18`. At 64 px slots that is 2560 px of empty space; at 2 px it is 80 px. The right-hand breathing room changes by a factor of 32 across the zoom range.

**F17 — Minimum zoom-out is a hard wall for tick bars.** `MIN_CANDLE_WIDTH = 2.0` (`viewport.rs:14`) caps a 2000 px window at ~1000 visible bars. A tick-bar session of 100k bars can never be surveyed, and there is no "fit all", no go-to-date and no overview scrollbar to compensate.

### Nit

**F18 — Crosshair has no time tag.** `pane.rs:2042-2058` tags the price axis only.
**F19 — `PriceScale::from_range` substitutes ±0.5 on a degenerate span** (`chart.rs:69-75`). On a 137000-priced instrument that yields an arbitrary one-unit window.
**F20 — `orderflow/interaction.rs` is named for market interaction, not pointer interaction.** A reader looking for canvas input handling opens the wrong file; the real handler is `pane.rs::handle_navigation`.


## 4. QUICK WINS vs STRUCTURAL

### Quick wins

1. **Surface the capping (F1).** The counters already exist in `book.dropped_*` (`app.rs:1657-1659`). Add an amber status-bar cell — `"tape capped · 340 prints hidden"` — beside the existing `side_note` (`statusbar.rs:290-296`), which is the established pattern for this exact kind of admission.
2. **Route the price axis through `chart::axis_labels` (F4).** Replace the raw `nice_ticks` + `{tick:.2}` at `pane.rs:1841-1859` with the existing `axis_labels(lo, hi, height)`. Decimals become step-derived and labels get thinned. Then feed the same derived decimal count into the last-price chip (`:1914`) and the crosshair tag (`:2045`) so all three agree. No new machinery, and it fixes the duplicate-label bug outright.
3. **Set cursors on the interactive axis bands (F5).** `axis_zoom_gesture` already has the `Response`; add `ResizeVertical` on hover over the price gutter and `ResizeHorizontal` on the time strip, mirroring what the lane divider already does at `pane.rs:1310-1312`.
4. **Add a "jump to live" chip (F6).** Render it at the right end of the time strip whenever `!viewport.follows_live()`; one click calls the existing `snap_to_live()`. The state is already computed and already plumbed to the status bar (`app.rs:1717`).
5. **Fix the dead label thresholds in the shipped presets (F11).** Lower `label_min_radius` below each preset's `max_radius` in `config/bubbles.toml`, or clamp it in `apply_to` (`bubble_presets.rs:97-103`) so an impossible threshold can never ship.
6. **Add a date to the time axis (F13).** Print a day marker on the first label after a date change, in the style the seam divider already uses.
7. **Point the layer toggles at the focused pane, or grey them out when the focused pane has no tape (F12).** `app.rs:985-995`; a `ChartPane` with `orderflow: None` is already the explicit signal.
8. **Draw the crosshair time tag (F18).** `slot_open_time` (`pane.rs:536`) already answers the question; add a tag on the time strip mirroring the price tag's geometry.
9. **Split the chart-body double-click (F7).** Reset time only; leave price to its own gutter double-click, which is already documented in the status-bar hint at `statusbar.rs:318`.

### Structural

1. **A hover-inspection layer for flow marks (F2, F10).** Each `AggressionPrimitive` already carries `x`, `y`, `size` in screen space plus `quantity`, `trade_count`, `first/last_timestamp_ms` and `matched_fraction`. Build a per-frame spatial index of the projected primitives and hit-test the pointer against it, then show a tooltip with the exact figures. This turns ten opaque visual channels into readable data and would retire most of F10 at the same time. The main design question is where it lives — the flow renderers are painter-only by contract, so the hit-test belongs in `OrderflowView` beside the published frame rather than inside `orderflow_render.rs`.
2. **Make the crosshair a hover state, not a tool (F3).** Decouple `draw_crosshair` from `toolrail.tool()` and drive it from `hover_pos` (already maintained at `pane.rs:1074`), with an explicit setting to turn it off. Keep a "magnet" or snapping mode as the tool if the rail slot is wanted. This alone removes the crosshair-vs-selection trade-off and the overflow-menu burial.
3. **Pointer-anchored zoom (F8).** `Viewport::zoom` needs an anchor argument: keep the bar under the cursor fixed by adjusting `right_bar` as `candle_width` changes. Small, self-contained, and fully unit-testable in `viewport.rs` where the existing zoom tests already live.
4. **An OHLC readout.** A corner legend that follows the crosshair (symbol, O/H/L/C, volume, delta) is the standard companion to F3 and is missing entirely today.
5. **Cross-layer visual control (F9).** A per-layer opacity or a "focus this layer" affordance in the LAYERS group, so the trader can pull the heatmap back without turning it off. Currently the only lever is binary on/off in the toolbar or a slider three clicks deep in a dock tab.
6. **Long-history navigation (F17).** A time-axis overview strip, a go-to-date control, or a "fit visible session" command. Today a 100k-bar tick session is only traversable by dragging.
7. **Time-axis label thinning (F13).** Port the pixel-gap logic from `chart.rs::thin_to_fit` to the time strip so labels are spaced by pixels rather than bar counts, and so repeated timestamps collapse.

## One note on what is working well

The layout contract is genuinely good and worth preserving: `plot_split` (`app.rs:181`) carves the regions once so the input handler and the renderer can never disagree about a boundary; `axis_zoom_gesture` (`pane.rs:208`) is a single implementation shared by the price gutter and every indicator pane, so no two axes can drift apart in feel; interaction ids are namespaced per pane (`pane.rs:492`) so a split's two charts cannot share a drag; and the empty-view fallback chain in `price_window` (`chart.rs:124-135`) means a re-cut series never produces a blank frame. The capability gating on the bubbles and heatmap toggles, complete with `disabled_explanation` text (`toolbar.rs:529, 546`), is exactly the right pattern — it just needs to reach the canvas findings above.

