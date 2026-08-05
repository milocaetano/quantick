# Paper trading — full audit report

**Scope:** `crates/app/src/paper_trading.rs` (1716 lines) plus its wiring in `toolbar.rs`, `dock.rs`, `pane.rs`, `statusbar.rs`, `app.rs`, `tab.rs`, and the data surface of `crates/sim`. Read-only analysis; no files modified. The design contract is `docs/ux/paper-trading.md`, and several findings below are places where the implementation silently diverges from it.

---

## 1. INVENTORY — every user-facing element

### 1.1 Toolbar (top panel, always visible)

| # | Element | Location | Trigger / appearance | Effect |
|---|---|---|---|---|
| T1 | `BUY` button (teal `#26A69A`, bold) | `toolbar.rs:484-496` | Always visible in the TRADE group, between HISTORY and the right-hand groups (`toolbar.rs:285-288`) | Pushes `ToolbarAction::PaperBuy` → `PaperTrading::market(Side::Buy)` (`app.rs:1047-1050`) |
| T2 | `SELL` button (red `#EF5350`, bold) | `toolbar.rs:497-509` | Same group, right of BUY | `ToolbarAction::PaperSell` → `market(Side::Sell)` (`app.rs:1051-1054`) |
| T3 | BUY hover text | `toolbar.rs:489-492` | Hover on T1 | "simulated market buy - fills at the next print; quantity and brackets live in the Trading tab" |
| T4 | SELL hover text | `toolbar.rs:502-505` | Hover on T2 | Same wording, sell side |
| T5 | Disabled explanation (both) | `toolbar.rs:493`, `506` | Hover while `paper_ready == false` | "waiting for the first print - there is no market yet" |
| T6 | Overflow `Buy at market (SIM)` | `toolbar.rs:708-714` | Inside the `⋯` menu once the window narrows past the TRADE fold (`toolbar.rs:110-112`) | Same as T1, then closes the menu |
| T7 | Overflow `Sell at market (SIM)` | `toolbar.rs:715-721` | Same | Same as T2 |

There is **no** close, flatten, reverse or cancel control anywhere on the toolbar.

### 1.2 Right dock — strip and entry points

| # | Element | Location | Trigger / appearance | Effect |
|---|---|---|---|---|
| D1 | Trading tab icon (Phosphor `TREND_UP`) | `dock.rs:52`, drawn `dock.rs:183-191` | Fourth of four unlabeled 36 px icons on the always-visible right strip | `Dock::select(DockTab::Trading)` — opens the body, or collapses it if already active (`dock.rs:153-159`) |
| D2 | Strip hover text | `dock.rs:74` | Hover on D1 | "Paper trading — simulated orders, position and history" |
| D3 | Body title `PAPER TRADING · SIM` | `dock.rs:63`, drawn `dock.rs:226-245` | Top of the open body | Static label |
| D4 | Collapse `×` | `dock.rs:236-242` | Right of D3 | Sets `active = None`; hover: "collapse the dock to its tab strip" |
| D5 | View menu → `Paper trading` | `app.rs:1882` (inside the `DockTab` loop at `1878-1890`) | Menu bar | `dock.open_tab(DockTab::Trading)` |

**The dock opens collapsed.** `Dock::new()` sets `active: None` (`dock.rs:124-132`), and I found no persistence of dock state, so every launch starts with the Trading body closed and only the icon strip showing.

### 1.3 Trading tab body — `draw_trading_tab` (`paper_trading.rs:573-593`)

**Header block**

| # | Element | Location | Notes |
|---|---|---|---|
| P1 | Muted small label "Simulated fills from the tape - no broker, points not currency." | `574-578` | Always shown |
| P2 | Its hover text (fill-model explanation) | `579-583` | Explains market/limit/stop fill semantics |

**Position card — `draw_position_card` (`595-694`)**

| # | Element | Location | Trigger / appearance | Effect |
|---|---|---|---|---|
| P3 | "No open position." (muted) | `597` | Whole card is replaced by this line when flat — and the function **returns**, so Close/Flatten do not exist while flat | — |
| P4 | Position line `LONG 1 @ 103.25` | `605-614` | Colored `theme::BUY` for long, `theme::SELL` for short, bold | Static |
| P5 | Open P&L `+2 pts` | `615-620` | Right of P4, colored by sign via `points_color` (`1351-1357`) | Static; only rendered when `mark_price()` is `Some` |
| P6 | P&L hover | `621` | "open profit at the last print, in points (price units × quantity)" | — |
| P7 | `stop loss 90` label | `627` | Only when `position.stop_loss` is `Some` | Static |
| P8 | SL `clear` small button | `628-637`, hover "remove the protective stop" | Next to P7 | `Command::SetBracket { stop_loss: None, take_profit: <unchanged> }` |
| P9 | Muted "stop loss - (drag one on the chart, or use the offsets below)" | `640-646` | When SL is `None` | **Instructional text only — both routes it names are non-functional. See Finding B1.** |
| P10 | `take profit 110` label | `649` | When TP is `Some` | Static |
| P11 | TP `clear` small button | `650-659`, hover "remove the profit target" | Next to P10 | `SetBracket { take_profit: None }` |
| P12 | Muted "take profit - (drag one on the chart, or use the offsets below)" | `662-669` | When TP is `None` | Same problem as P9 |
| P13 | **`Close` button** | `677-684`, hover "exit the position at the next print" | Bottom of the card | `Command::ClosePosition` — queues a market close for the next print |
| P14 | **`Flatten` button** | `685-692`, hover "close the position and cancel every pending order" | Right of P13 | `Command::Flatten` |

P13/P14 are **the only dedicated exit controls in the entire application.**

**Order entry — `draw_order_entry` (`696-791`)**

| # | Element | Location | Trigger / appearance | Effect |
|---|---|---|---|---|
| P15 | `BUY` selectable toggle | `698-702` | Teal, bold | Sets `self.side = Buy` |
| P16 | `SELL` selectable toggle | `703-707` | Red, bold | Sets `self.side = Sell` |
| P17 | `qty` label | `708` | Muted | — |
| P18 | Quantity text field | `709` | `TextEdit::singleline`, 48 px, default `"1"` (`155`) | Free text; parsed only at submit (`parse_quantity`, `1059-1070`) |
| P19 | Order-type combo | `710-716` | Options `market` / `limit` / `stop`, lowercase | Sets `self.order_type` |
| P20 | `stop -pts` label + hover | `719-723` | Muted | Hover: "optional protective stop, this many points on the losing side of the entry; empty places no stop" |
| P21 | Stop-offset field | `724` | 48 px text | Applied **only at entry placement** (`parse_bracket`, called from `235` and `524`) |
| P22 | `profit +pts` label + hover | `725-729` | Muted | Hover: "optional profit target…" |
| P23 | Profit-offset field | `730` | 48 px text | Same restriction as P21 |
| P24 | `BUY 1 at market` action button | `739-755` | Only when order type is `market`; text is `"{SIDE} {qty_text} at market"`, colored by side | `self.market(side)` |
| P25 | P24 hover / disabled hover | `750-751` | — | "simulated: fills at the next print of the tape" / "waiting for the first print…" |
| P26 | `Place buy limit on the chart…` button | `767-788` | Only when type is `limit` or `stop`, and nothing armed | Sets `self.armed = Some(ArmedPlacement { side, kind })` |
| P27 | P26 hover / disabled hover | `778-783` | — | "arms a click: the next chart click rests the order at that price (Esc cancels)" |
| P28 | "click the chart at your price…" (accent) | `758-762` | Replaces P26 while armed | Static |
| P29 | `cancel` small button | `763-765` | Right of P28 | `self.armed = None` |

**Pending orders — `draw_pending_orders` (`793-843`)**

| # | Element | Location | Trigger / appearance | Effect |
|---|---|---|---|---|
| P30 | "N market order(s) await the next print" | `801-809` | When queued entries exist | Static |
| P31 | "closing at the next print…" | `810-816` | When a queued close exists (after clicking Close/Flatten) | Static — the only feedback that Close was registered before the next print lands |
| P32 | "No pending orders." | `818-821` | When nothing rests | Static |
| P33 | Order row `#1 buy limit 1 @ 95` | `823-832` | One per resting order | Static |
| P34 | `×` cancel button per row | `833-840`, hover "cancel this order" | Right of P33 | `Command::CancelOrder { id }` |

**Session summary — `draw_session_summary` (`845-865`)**

| # | Element | Location | Notes |
|---|---|---|---|
| P35 | "session: +7 pts realized · 2 closed trade(s)" | `846-850` | The only in-session trade count |
| P36 | `Report…` button | `852-858`, hover "performance metrics computed from the saved history" | Opens the report window; reloads from disk (`871-874`) |
| P37 | Muted "history: paper-trades" | `860-864` | A **non-interactive label**, relative to cwd. The spec (`docs/ux/paper-trading.md:81-82`) called for a `History folder` button |

### 1.4 Report window — `draw_report_window` (`885-937`)

| # | Element | Location | Notes |
|---|---|---|---|
| R1 | `egui::Window` "Simulated performance" | `891-895` | Non-collapsible, non-resizable, with the standard close `×` |
| R2 | `scope` label | `897` | Muted |
| R3 | `this symbol (BTCUSDT)` selectable | `898-904` | Reloads from disk on change (`931-933`) |
| R4 | `all symbols` selectable | `905-907` | Same |
| R5 | Empty state "No saved trades yet - close a simulated trade and it lands here." | `913-919` | When `trades == 0` |
| R6 | Footer disclaimer (points, not currency) | `922-929` | Muted small |
| R7-R14 | Metric rows, each with a hover explanation | `draw_report_body`, `1125-1184` | net P&L, trades (long/short split), win rate, profit factor, max drawdown, gross profit/loss, avg win/loss, largest win/loss. Honest `—` where a ratio has no denominator |
| R15 | Warning line "N file(s) unreadable, M row(s) skipped" | `1186-1196` | `theme::WARN`, only when > 0 |
| R16 | "N trade(s) across M file(s)" | `1197-1204` | Muted small |

There is **no per-trade list** anywhere in this window — only aggregates.

*(continues in 2/4)*


### 1.5 Chart layer — `draw_layer` (`279-396`), painted by both panes at `pane.rs:1709`

| # | Element | Location | Appearance |
|---|---|---|---|
| C1 | Pending-order line | `286-312` | Dashed (4 px on / 4 px off, `40-42`), `theme::ACCENT` `#8AB4F8`, spanning chart-left to `axis_x` |
| C2 | Pending-order gutter chip | via `draw_price_line:1300-1307` | `#1 buy limit 1 @ 95`, 11 px monospace, dark ink `#0E121A` on an accent-filled rect |
| C3 | Position entry line | `313-334` | Solid, `theme::BUY` for long / `theme::SELL` for short |
| C4 | Entry chip | `319-324` | `SIM LONG 1 @ 103.25` |
| C5 | Stop-loss line | `335-357` | Solid, always `theme::SELL` red |
| C6 | SL chip | `342-346` | `SL 90 -10 pts` (the points figure is the P&L if it fills) |
| C7 | Take-profit line | `358-380` | Solid, always `theme::BUY` teal |
| C8 | TP chip | `365-369` | `TP 110 +10 pts` |
| C9 | Armed-placement hint | `382-395` | 12 px accent text at the chart's **top-left corner**: "click a price to place your buy limit - Esc cancels" |

Every line early-returns when its `y` falls outside the chart rect (`1283-1285`) — off-screen prices draw nothing at all.

### 1.6 Chart interactions — `handle_chart_input` (`417-482`)

| # | Gesture | Location | Behavior |
|---|---|---|---|
| I1 | Armed click | `419-427` | First press inside the chart places the limit/stop at `scale.price_at(pointer.y)`, snapped to the mark's own decimal precision (`snap`, `556-565`) |
| I2 | Grab a line | `430-440` | Press within `LINE_GRAB_RADIUS_PX = 10.0` (`38`) of a line starts a drag |
| I3 | Drag | `443-449` | `drag_price` follows the pointer, clamped to the chart's vertical bounds; the line repaints at the dragged price (`288-292`, `336-341`, `359-364`) |
| I4 | Release | `453-479` | Submits `SetBracket` (SL/TP) or `ModifyOrder`; a rejection snaps the line back and toasts |
| I5 | Hit priority | `line_at`, `487-514` | Pending orders (reverse order) → take profit → stop loss → entry |
| I6 | Entry line = `Blocked` | `510-512` | Consumes the gesture, moves nothing, **sets no cursor** |
| I7 | Escape | `cancel_interaction`, `405-415`; wired at `app.rs:2012` | Cancels the armed placement first, then a grabbed line — one layer per press |

**No hover state, no cursor change, no highlight** exists for any of these lines. `pane.rs:1120` gates the entire cursor-feedback block behind `!paper_gesture`, and `handle_chart_input` never calls `set_cursor_icon`.

**No context menu.** The spec explicitly does not claim right-click (`docs/ux/paper-trading.md:54-56`).

### 1.7 Status bar

| # | Element | Location | Notes |
|---|---|---|---|
| S1 | `SIM +2 pts` cell | producer `status_cell:249-269`; model `app.rs:1716`; rendered `statusbar.rs:297-308` | Monospace, colored by sign (teal/red/muted). Sits in the middle content section, after the bar spec, bar counts and the side-inference note |
| S2 | Its hover | `statusbar.rs:304-307` | "paper-trading P&L (realized + open) in points - simulated fills, not a broker account" |

Returns `None` only while the simulator has never been touched (`250-256`); once any trade closes, the cell shows permanently whether or not a position is open. It is **not clickable**.

### 1.8 Toasts — `draw_toast` (`944-965`)

Bottom-center, lifted 96 px (`TOAST_LIFT_PX`, `45`), 4-second life (`TOAST_MS`, `35`), single slot, newest wins (`show_toast`, `967-972`). Messages:

| Trigger | Location | Text |
|---|---|---|
| Rejection | `983` | `SIM: <RejectReason>` — didactic, verbatim from the sim core |
| Bracket dropped at fill | `984-986` | `SIM: dropped at the fill - <reason>` |
| Entry fill | `987-996` | `SIM fill: buy 1 @ 103.25` |
| Trade closed | `997-1006` | `SIM closed: LONG 1 → +2 pts (manual)` |
| Timeline reset with a position | `212-214` | "SIM position flattened - the timeline was rebuilt under it." |
| Timeline reset with orders only | `216-218` | "SIM orders cancelled - the timeline was rebuilt under them." |
| Bad quantity | `1063-1067` | "SIM: quantity must be a positive number - got `x`" |
| Bad stop offset | `1079-1082` | "SIM: the stop offset must be a positive number of points - got `x`" |
| Bad profit offset | `1087-1090` | Same, profit side |
| Journal write failure | `1053-1055` | "SIM: could not save the trade history - see the log for the path." |

### 1.9 Keyboard

**Escape is the only paper-trading key**, and only for cancelling an armed placement or a grabbed line (`app.rs:2004-2013`). No shortcut opens the Trading tab, closes a position, or flattens. For contrast, the app binds Ctrl+R (replay browser), Ctrl+B (dock), Ctrl+T (new tab), Ctrl+W (close tab), Ctrl+Tab (switch tab) at `app.rs:1730-1747`.

---

## 2. FLOWS — as the code implements them

### (a) Enabling / opening paper trading

There is nothing to enable. The simulator exists per tab from construction (`tab.rs:266`) and ingests every trade unconditionally (`tab.rs:1210`). The toolbar BUY/SELL become enabled the moment `mark_price()` is `Some` — which happens on the first backfilled print via `seed` (`tab.rs:1116`), before any live trade. To reach the panel: click the `TREND_UP` icon, fourth on the right strip, or View → Paper trading.

### (b) Placing a market order

Two entry points, both one click:

1. **Toolbar `BUY`/`SELL`** → `market(side)` (`230-244`). Reads `qty_text` and both offset fields from the Trading tab's form — state the toolbar never displays. On a parse failure the order is silently dropped except for a toast.
2. **Trading tab `BUY 1 at market`** (P24), when the type combo is on `market`.

Either way: `Command::PlaceMarket` queues; nothing happens until the **next print**. The order appears as "1 market order(s) await the next print" (P30) only if the Trading tab is open. On the fill, a toast fires and the entry/SL/TP lines appear on the chart.

### (c) Placing a limit / stop order

Only from the Trading tab, and only via click-to-place:

1. Set the type combo (P19) to `limit` or `stop`.
2. The action button becomes "Place buy limit on the chart…" (P26). Click it → `armed`.
3. The button is replaced by "click the chart at your price…" (P28) plus a `cancel` button (P29), and a 12 px accent hint appears at the chart's top-left (C9).
4. Click anywhere in the flow pane's chart. The price is taken from the pointer's `y`, snapped to the mark's decimal places.
5. A wrong-side placement is rejected, **stays armed**, and toasts advice (`544-550`) — deliberate, so the user clicks again.
6. On success the order rests as a dashed accent line and a row in the pending list.

There is no way to type an exact limit price, and no way to place a limit/stop from the toolbar.

### (d) Seeing the open position

Four surfaces, three of which are conditional:

1. **Chart** — solid colored line at the average entry with a gutter chip `SIM LONG 1 @ 103.25`. Disappears completely if the price scrolls out of view.
2. **Status bar** — `SIM +2 pts`, small monospace, mid-row, indistinguishable from realized-only P&L.
3. **Trading tab position card** — the full picture, but only if the dock body is open on that tab.
4. **Toast** — 4 seconds at the fill, then gone.

### (e) Closing an open trade — every path that exists

| Path | Where | Cost to discover |
|---|---|---|
| `Close` button (P13) | Trading tab, position card | Requires knowing the dock exists, recognizing `TREND_UP`, clicking it |
| `Flatten` button (P14) | Same place, same card | Same |
| **Reversal via the toolbar** — clicking `SELL` while long | `toolbar.rs:497`, netting at `simulator.rs:601-627` | Undocumented in the UI. Closes if quantities match; **opens an opposite position for the remainder if they do not** |
| SL/TP firing | Automatic, on the tape | Not a user action |
| Timeline reset | `on_timeline_reset:199-220` — replay seek, symbol/feed switch, restart | Flattens at the last mark, labeled `reset`, journaled, toasted |
| Tab close | `app.rs:9045-9073` | Ends the session |

There is **no** close/flatten on the toolbar, on the chart, in a context menu, or on a keyboard shortcut.

### (f) Viewing closed trades / history / metrics

- In-session: one line, `session: +7 pts realized · 2 closed trade(s)` (P35). No list.
- `Report…` (P36) opens the aggregate window, reading fresh from disk. Aggregates only — win rate, profit factor, drawdown, averages. **No individual trade is ever displayed**, despite `sim.closed_trades()` (`simulator.rs:153`) and `history::parse` both returning full per-trade records (side, qty, entry, exit, both timestamps, points, exit reason — `simulator.rs:14-30`).
- The on-disk path is shown as dead text (P37).
- Closed trades are never drawn on the chart.


## 3. UX FINDINGS

### Root cause of the reported failure

The user said: *"I couldn't figure out how to close the open trade, and it wasn't clear which trade was open."* Both halves have concrete causes in the layout, and they compound:

**Why they couldn't close it.** Entry and exit live in structurally different places. `BUY`/`SELL` sit on the toolbar, permanently visible, at `toolbar.rs:285-288`. `Close`/`Flatten` exist only inside `draw_position_card` (`paper_trading.rs:677-692`), which only runs inside `DockTab::Trading` (`dock.rs:216`), and the dock **starts collapsed** — `Dock::new()` sets `active: None` (`dock.rs:126-131`). So a user who has never opened the dock has, on screen, a control that opens positions and no control that closes them. Nothing bridges the gap: the toolbar has no link to the Trading tab, the status bar `SIM` cell is not clickable, the chart lines have no menu, and there is no keyboard shortcut. The one affordance that exists — the fourth icon on the strip, Phosphor `TREND_UP` (`dock.rs:52`) — is an unlabeled upward-trending arrow that reads as "trend" or "indicator", not "my position", and it carries **no badge or state change when a position is open**. A user reasoning by elimination might press `SELL` to close, which works only if the quantity happens to match; otherwise they are now short.
*Violates: #7 Flexibility and efficiency of use; #6 Recognition rather than recall.*

**Why it wasn't clear which trade was open.** The persistent chart evidence is broken by a paint-order collision. `draw_price_line` places the entry chip at `axis_x + 6.0` in `FontId::monospace(11.0)` (`paper_trading.rs:1300-1301`). `draw_last_price` places its chip at `axis_x + AXIS_LABEL_GAP_PX` where `AXIS_LABEL_GAP_PX = 6.0` (`chart.rs:345`) in `AXIS_LABEL_FONT_PX = 11.0` (`chart.rs:347`) — **identical x, identical height** — and it is drawn *after* the paper layer (`pane.rs:1709` then `1713-1714`) with an opaque `rect_filled`. Because a market order fills at the very next print, the entry price *equals* the last price at the moment the position opens. So the chip that says `SIM LONG 1 @ 103.25` is overpainted across its first ~6 characters by `103.25`, leaving something like `…G 1 @ 103.25` — visually just another price tag. The one persistent, chart-anchored statement of "you are long" is mangled precisely when the user most needs it.

Everything else that could have carried the message doesn't:
- The fill toast lasts 4 seconds, bottom-center (`35`, `953-955`), while the user is looking at the chart and the right-hand dock.
- The status cell reads `SIM +2 pts` (`249-269`) — a bare number that appears identically when flat with closed history, and says nothing about side, size or entry.
- The entry line vanishes entirely once the price scrolls out of range (`1283-1285`) — no edge marker, no caret.
- No fill markers are drawn on the candles at all.

---

### Blockers

**B1 — An open position cannot be given a stop loss or take profit, and the UI instructs the user to do exactly that.**
When `stop_loss` is `None`, the card shows "stop loss - (drag one on the chart, or use the offsets below)" (`640-646`; TP twin at `662-669`). Both routes are dead:
- *Drag one on the chart*: `line_at` (`487-514`) only returns `PaperDrag::StopLoss` when `position.stop_loss` is already `Some`. With no stop there is no line, so there is nothing to grab. Dragging cannot **create** a level, only move an existing one.
- *Use the offsets below*: `parse_bracket` (`1075`) is called from exactly two places — `market()` (`235`) and `place_armed()` (`524`). Both are *entry* paths. Typing into the offset fields with a position already open does nothing, and there is no apply button.

`Command::SetBracket` is issued from only four sites (`459`, `465`, `633`, `656`): two drag-releases that require a pre-existing line, and two `clear` buttons that can only set `None`. **The only way to attach protection is to have typed the offsets before entering.** A user who entered from the toolbar (where offsets are invisible) has an unprotectable position and a label telling them otherwise.
*Violates: #1 Visibility of system status; #2 Match between system and the real world; #10 Help and documentation (instructions that are false).*

**B2 — The exit control is not reachable from any surface the user is looking at.** Detailed above. This is the reported failure and it is a genuine dead end, not a discoverability nuisance: nothing on the toolbar, chart, status bar, or keyboard leads to `Close`.
*Violates: #7 Flexibility and efficiency of use.*

**B3 — The open-position chip is overpainted by the last-price chip at the exact moment a position opens.** Detailed above; `paper_trading.rs:1300-1307` vs `pane.rs:1713-1714` with `chart.rs:345,347`.
*Violates: #1 Visibility of system status.*

---

### Major

**M1 — The status cell cannot distinguish "open" from "flat."** `status_cell` (`249-269`) sums realized and open P&L into one figure and returns `Some` as soon as anything has ever happened. `SIM +12 pts` after a closed winner and `SIM +12 pts` with an open winner are byte-identical. The one always-visible paper surface answers the wrong question.

**M2 — Pressing `SELL` while long is an undisclosed close-or-reverse.** `simulator.rs:601-627`: selling 5 while long 2 closes 2 (reason `Reversal`) and **opens a short 3** (confirmed by the crate's own test at `simulator.rs:1058-1077`). The toolbar `SELL` button uses `qty_text` from the Trading tab (`230-244`), which the toolbar never shows. A user who cannot find `Close` will reach for `SELL`; whether that closes the trade or flips it depends on a number they cannot see. No preview, no warning, no confirmation.
*Violates: #5 Error prevention; #1 Visibility of system status.*

**M3 — The entry line creates an invisible 20 px band where the chart silently refuses to pan.** `line_at` returns `PaperDrag::Blocked` within 10 px of `avg_price` (`510-512`); `handle_chart_input` returns `true`; `pane.rs:1263` excludes the pan. Locked *drawings* at least set `CursorIcon::NotAllowed` (`pane.rs:1239-1241`), but the paper path sets no cursor — the whole cursor-feedback block is gated behind `!paper_gesture` (`pane.rs:1120`). The result is a horizontal stripe across the chart, near the current price, where dragging does nothing and nothing explains why.
*Violates: #1 Visibility of system status; #9 Help users recognize and diagnose.*

**M4 — SL/TP/order lines are draggable with zero affordance.** Same root cause as M3: no hover highlight, no cursor change, no handle. Drawings get `Move` and `ResizeNwSe` cursors on hover (`pane.rs:1134-1148`); paper lines get nothing. The only mention of draggability is inside the false hint of B1.
*Violates: #6 Recognition rather than recall.*

**M5 — An armed limit/stop placement is inert while any drawing tool is armed, and its hint never goes away.** `handle_navigation` returns early when `handle_drawing_placement` reports an armed tool (`pane.rs:1057-1059`), so `paper.handle_chart_input` never runs. The chart-corner hint (`382-395`) and the panel's "click the chart at your price…" (`758-762`) keep instructing, while every click draws a trendline. Compounding it, `cancel_interaction` sits **above** the drawing draft in the escape stack (`app.rs:2012` vs `2016`), so one Escape silently kills the paper arm when the user meant to cancel the drawing.
*Violates: #1 Visibility of system status; #3 User control and freedom.*

**M6 — Lines are drawn on both panes but only interactive on one.** `pane.rs:1709` paints the paper layer for every pane; `tab.rs:1525` sets `paper_owns_input = side == PaneSide::Flow`. In a split layout the time pane shows pixel-identical SL/TP lines that cannot be grabbed, with nothing marking them read-only.
*Violates: #4 Consistency and standards.*

**M7 — The position disappears from the chart when its price scrolls out of view.** `draw_price_line` early-returns (`1283-1285`) with no edge indicator. Combined with M1, a user who pans away has no on-screen evidence a position exists.

---

### Minor

**m1 — No closed-trade list exists anywhere.** `sim.closed_trades()` (`simulator.rs:153`) and `history::parse` both return complete records — side, quantity, entry, exit, both timestamps, points, exit reason (`simulator.rs:14-30`) — and every one of those fields is discarded before reaching the screen. The user sees a count (`846-850`) and aggregates (`1113-1205`). They cannot answer "what were my last three trades and why did they close?", which is the core question a practice simulator exists to answer.

**m2 — No fill markers on the candles.** Nothing shows *where* an entry or exit happened. The data is present.

**m3 — The Trading tab body does not scroll.** `dock.rs:204-218` places the tab body directly into the `SidePanel`; the L2 tab supplies its own `ScrollArea` (`orderflow_view.rs:1160`) but the Trading branch (`dock.rs:216`) does not. With several pending orders, the session summary and `Report…` button are clipped with no scrollbar and become unreachable.

**m4 — The history path is dead text.** `860-864` renders `history: paper-trades` as a muted label — relative to the working directory, not selectable, not clickable. The spec asked for a `History folder` button (`docs/ux/paper-trading.md:81-82`).

**m5 — The report is only reachable from inside the Trading tab.** `docs/ux/paper-trading.md:155` promises "opened from the Trading tab (and Tools menu)". I found no menu entry — the only opener is P36, two levels behind a collapsed dock.

**m6 — Quantity is free text validated at submit.** `TextEdit::singleline` at `709`; `parse_quantity` toasts on failure (`1059-1070`). The spec called for a DragValue (`docs/ux/paper-trading.md:70`). Typing `1,5` or `1.` fails only when the button is pressed — and if pressed from the *toolbar*, the field that caused the failure isn't even on screen.
*Violates: #5 Error prevention.*

**m7 — The toolbar buttons never disclose the quantity or brackets they will use.** `toolbar.rs:489-492` says "quantity and brackets live in the Trading tab" — pure recall. A user who set qty 10 and a 5-point stop an hour ago gets no hint from the button they are about to press.

**m8 — Single-slot toast, 4 s, bottom-center.** `show_toast` (`967-972`) overwrites; `TOAST_MS = 4_000`; anchored `CENTER_BOTTOM, -96 px` (`953-955`). A fill and a `BracketDropped` in the same frame means one message is lost — and the dropped-bracket case is exactly the one that must not be missed, since it silently leaves the position unprotected.

**m9 — `Close` vs `Flatten` is explained only on hover.** Two same-weight buttons side by side (`677-692`). "Flatten" is trading jargon. The spec asked for a hint line when both exist (`docs/ux/paper-trading.md:67-69`); it was not implemented.

**m10 — Paper lines ignore the user's candle colors.** `draw_layer` uses the `theme::BUY`/`theme::SELL` constants (`600-603`, `353`, `376`), while `draw_last_price` reads `chrome.style.candles.bull_outline` (`pane.rs:1895-1899`). A user who recolors their candles gets a long-entry line that no longer matches their bulls.

---

### Nits

**n1 — Three label registers in one panel.** `side_word` → `"buy"` (`1322`), `side_word_upper` → `"BUY"` (`1329`), `position_word` → `"LONG"` (`1336`). The pending row reads `#1 buy limit 1 @ 95`, the market button reads `BUY 1 at market`, the toggle reads `BUY`, the chip reads `SIM LONG 1 @ …`.

**n2 — `stop -pts` is ambiguous** (`719`) — parses as "stop minus points" as easily as "stop offset in points".

**n3 — A short position shows two red lines.** The entry is `theme::SELL` (`600-603`) and the stop is always `theme::SELL` (`353`); only the chip text disambiguates. Color-only coding also fails for red-green deficiency (`#26A69A` vs `#EF5350`).

**n4 — No confirmation on `Flatten`.** Defensible for a simulator, but it is the most destructive control in the panel and sits one pixel-gap from `Close`.

**n5 — The report scope label reads `this symbol ()` before a symbol is set** (`902`, with `symbol` initialized empty at `149`). `set_symbol` runs every frame from `tab.rs:1103-1104`, so this is likely unreachable in practice.

---

### What the implementation gets right

Worth preserving through any redesign: rejections carry didactic, actionable text straight from the sim core (`983`, `docs/ux/paper-trading.md:113-121`); disabled controls explain themselves rather than hiding (`toolbar.rs:493`, `751`); the fill model's honesty is stated in the panel header and its hover (`574-583`); timeline resets flatten loudly and journal the exit rather than pretending continuity (`199-220`); unreadable history rows are counted and disclosed rather than dropped (`1186-1196`); and ratios without denominators show `—`, never `∞` (`1142`, `1149`).


## 4. RECOMMENDATIONS

### Quick wins

1. **Fix the chip collision (B3).** Either paint the paper layer *after* `draw_last_price` (swap `pane.rs:1709` and `1713-1714`), or offset the paper chips — right of the last-price chip's width, or vertically by one chip height when the two `y` values are within a chip's height of each other.
2. **Put `Close` on the toolbar (B2).** Add `paper_position: Option<PositionSummary>` to `ToolbarModel` and a `ToolbarAction::PaperClose`. While a position is open the TRADE group becomes `BUY | SELL | ✕ Close 1 LONG`. All the state already flows to `app.rs:960`.
3. **Make the toolbar buttons state-aware (M2).** While long 1, label the sell button `SELL 1 (closes)` and the buy button `BUY 1 (adds)`; while long 1 with qty 5 in the form, `SELL 5 (reverses to short 4)`. Turns the hidden footgun into a readable one.
4. **Badge the dock strip icon and make the status cell clickable.** An accent dot on `TREND_UP` while a position is open (`dock.rs:183-191`), and `dock.open_tab(DockTab::Trading)` on a click of the `SIM` cell (`statusbar.rs:297-308`).
5. **Rewrite the status cell (M1).** `SIM LONG 1 · +2.0 pts` when open, `SIM +7.0 pts · flat` otherwise. One string change in `status_cell` (`249-269`) plus the model tuple.
6. **Make the false hint true (B1).** Ship an `Apply` button beside the offset fields that issues `SetBracket` against the open position — the command exists and takes absolute prices; `parse_bracket(side, position.avg_price)` already computes them. Until that lands, the text at `640-646` and `662-669` must stop naming two routes that do not work.
7. **Give the paper gesture a cursor (M3, M4).** Have `handle_chart_input` (or a companion `hover_kind`) report `ResizeVertical` over a draggable line and `NotAllowed` over the entry, and let `pane.rs` set it before the `!paper_gesture` gate at `1120`.
8. **Wrap the Trading tab body in a `ScrollArea`** (`dock.rs:216`), matching the L2 tab (m3).
9. **Add `Report…` to the View menu** next to the dock-tab entries at `app.rs:1878-1890` (m5).
10. **Add a flatten shortcut** (e.g. Ctrl+Shift+F) beside the existing bindings at `app.rs:1730-1747`, and name it in the `Flatten` tooltip.
11. **Make the history path a button** that opens the folder, and show the absolute path on hover (m4).
12. **Swap the quantity field for a `DragValue`** (m6) so invalid text is unrepresentable; keep the offsets as text so empty can mean "none".
13. **Disarm the drawing tool when a paper placement is armed** (M5), and make the escape stack cancel whichever mode is currently *visible* rather than a fixed order.

### Structural redesigns

**S1 — A persistent position HUD pinned to the chart.**
The single change that fixes both halves of the user's complaint without opening the dock. A compact opaque pill anchored to the chart's top-left (below the armed hint), present whenever a position is open and independent of scroll position:

> `SIM LONG 1 @ 103.25 · +2.0 pts` `[✕ Close]` `[⇄ Reverse]`

Behavior: opaque to the pointer like the drawings inspector; `Close` issues `ClosePosition` directly; `Reverse` issues a market order at twice the position size with a one-line confirmation; hovering the pill highlights the entry line and, when the entry price is outside the visible range, replaces the price with a directional caret (`▲ 103.25 above`) so M7 is answered too. Clicking the pill body opens the Trading tab for the full card.

**S2 — Chart-native bracket creation and management.**
Extend the existing drag grammar so it can *create* levels, closing B1 in the vocabulary the docs already promised. Hovering the entry line reveals two small handles in the gutter; dragging one downward from a long entry creates a stop loss, upward creates a take profit (mirrored for shorts), with a live preview chip showing the resulting points risk. Hovering an existing SL/TP line reveals an inline `✕` that clears it — the same action as the card's `clear` button, at the place the user is already looking. Every drop still routes through `SetBracket` and still gets refused with advice on a wrong-side placement, so the didactic contract is unchanged.

**S3 — A trades ledger in the Trading tab.**
Below the session summary, a scrollable table: time, side, qty, entry, exit, reason, points — one row per `ClosedTrade`, current session above the on-disk history, colored by sign. Clicking a row scrolls the chart to span `opened_ms..closed_ms` and highlights that round trip. This is the surface that turns a P&L counter into a practice tool, and every field it needs is already parsed and then thrown away (m1).

**S4 — Fill markers on the candles.**
Small triangles at each entry and exit (price × time from `ClosedTrade`), joined by a faint line for the round trip, colored by outcome. Answers "where did I get in, and why did it close there?" — the question the whole feature exists for. Gate it behind a LAYERS toggle so it obeys the house rule that every visual layer has one.

**S5 — Resolve the two-pane ownership split (M6).**
Either let both panes drag (the simulator is per-tab and a price level is as true on the 5-minute context as on the flow chart — which is the reasoning `pane.rs:1703-1708` already gives for *drawing* on both), or render the context pane's lines visibly read-only: dimmed stroke, no chip. The current state, identical pixels with different behavior, is the one option that teaches the user nothing.

## 5. Confidence and caveats

Everything above is grounded in the code as it stands on `main`; the app was not run, so pixel-level claims are inferences from constants:

- **B3** rests on `axis_x` being the same value in both calls (it is), both offsets 6.0, both fonts 11 px. The overlap is certain; its *visual severity* was estimated, not measured.
- **B2's "dock starts collapsed"** rests on `Dock::new()` (`dock.rs:124-132`) and the absence of any restore path on `Dock`.
- **M2's reversal arithmetic** is confirmed by the crate's own test (`simulator.rs:1058-1077`).

**Top three, if only three things ship:** (1) `Close` on the toolbar whenever a position is open; (2) the chip-collision fix so `SIM LONG 1 @ …` is legible at the moment of entry; (3) the status cell stating side and size, not just a points figure. Those three, together, are the reported bug.

