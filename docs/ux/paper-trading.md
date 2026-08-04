# Paper trading — UX specification

Status: **design target** for the `feat/trade-sim` milestone. Companion of
[quantick UI Design Model](ui-design-model.md) — this feature claims the
zones the growth map already reserved for order/position surfaces; it invents
no new chrome. The deterministic core lives in `crates/sim`
(`quantick-sim`); this document covers how a person meets it.

Design references: TradingView's chart trading (entry/SL/TP as draggable
colored lines, buy/sell at the current price) and NinjaTrader's Chart Trader
(a docked panel with side, quantity and order type next to the chart). Both
patterns are folded into the house grammar instead of copied literally.

---

## 1. What this is, and the honesty contract

A trading *simulator* for practice: buy and sell the charted symbol against
the live tape or a replay session, with protective stop-loss and take-profit
orders, a persisted history of closed trades, and a performance report.

Non-negotiables, inherited from `CLAUDE.md` and the sim crate's contract:

- **Everything is labeled simulated.** The dock tab title, the position
  badge, the status-bar cell and the report window all carry the `SIM`
  marker. No surface may look like a broker connection.
- **Fills come from the tape only.** Market fills at the *next* print;
  a limit fills at its own price when the tape trades at or through it; a
  stop fills at the triggering print (gaps fill honestly worse). No book
  depth, no queue position, no slippage model — and the UI never implies
  otherwise.
- **P&L is shown in points** (price units × quantity), never in currency:
  the workspace has no per-instrument tick value table. Labels read
  `+12.5 pts`, not `R$` or `$`. (An instrument table is future work; until
  it exists a currency number would be an invented number.)
- **What survives:** closed trades, persisted to the history folder.
  **What does not:** open positions and pending orders — they die with the
  session (restart, symbol switch, replay seek) via an explicit, labeled
  flatten. The simulator never pretends continuity it cannot honestly have.

## 2. Zones claimed (design model §11)

| Surface | Zone | Content |
|---|---|---|
| Toolbar | action group | `BUY` / `SELL` market buttons (with quantity), gated like every capability control |
| Right dock | new tab **Trading** | the chart-trader panel: position card, order entry, pending orders, session history |
| Canvas | own layer above drawings | entry / SL / TP / pending-order price lines with gutter chips, draggable |
| Status bar | new cell | `SIM ±N pts` (realized + open), hidden when the simulator has never traded |
| Floating window | `SettingsDialog` mold | the performance report, open–read–close |

Explicitly **not** claimed: the chart's right-click gesture (reserved by the
layer-visibility menu work) and the `AMBER` color (reserved by contract for
provenance — replay/backfill/inferred data). Pending orders use `ACCENT`.

## 3. The Trading dock tab (chart trader)

Top to bottom, grouped by the §3.2 question each block answers:

1. **Header** — "Paper trading — simulated fills from the tape". One line,
   always visible; the didactic anchor of the whole feature.
2. **Position card** (only when a position is open) — side and quantity
   (`LONG 2`), average entry, open P&L in points (live, colored
   `BUY`/`SELL` by sign), SL and TP values with per-side *clear* buttons,
   and `Close` / `Flatten` buttons. A hint line explains the difference the
   first time both exist ("Close exits the position; Flatten also cancels
   every pending order").
3. **Order entry** — side toggle (Buy/Sell, colored), quantity drag-value
   (default 1), order type (`Market` / `Limit` / `Stop`), optional SL/TP
   offsets, and the action button:
   - `Market` → the button reads `Buy 1 at market` and fires immediately.
   - `Limit`/`Stop` → the button arms **click-to-place**: the crosshair
     shows a price-tag hint ("click a price below the market to rest your
     buy limit"), the next chart click places the order at that price,
     `Esc` disarms. This teaches *where* each order type may sit, because
     the simulator rejects wrong-side placements with advice (§5).
4. **Pending orders** — one row per resting order (`#3 BUY LMT 2 @ 95`),
   with cancel buttons, in the `toolbar.rs` indicator-menu row mold.
5. **Session** — realized points, closed-trade count, and buttons
   `Report…` and `History folder` (reveals the path).

## 4. The chart layer

Painted between the drawings layer and the last-price line, so simulated
orders read above user annotations but never hide the market itself. All
lines span the chart width and end in a gutter chip (the third instance of
the `draw_last_price` chip geometry, so prices never disagree about their
pixel).

| Line | Style | Chip label |
|---|---|---|
| Position entry | solid, `BUY`/`SELL` by side | `SIM LONG 2 @ 103.2` |
| Stop loss | solid, `SELL` red | `SL 97.0 −12.4 pts` |
| Take profit | solid, `BUY` teal | `TP 110.0 +13.6 pts` |
| Pending order | dashed, `ACCENT` | `#3 BUY LMT 2 @ 95.0` |

Interaction, reusing the drawing-drag grammar (`DrawingDrag` mechanics, same
hit radius constants, raw-input reads, gesture-consumption flag so the chart
never pans under a grabbed line):

- SL, TP and pending-order lines are **draggable**; on release the new price
  is submitted as a command and the simulator answers — either the line
  settles at the new price or a rejection toast explains why it snapped
  back ("a long's stop loss must sit below the price it protects…").
- The position entry line is **not** draggable — an average entry is
  history, not an order. Grabbing it is a no-op that still consumes the
  gesture (the `Blocked` pattern).
- Order state lives in the simulator, **not** in `Drawings` — a bar-spec
  change or history rebuild clears annotations, but simulated orders belong
  to the session, not to the bar series.

## 5. Rejections are the curriculum

Every refused command surfaces the sim core's `RejectReason` message as a
toast, verbatim. The messages are written to teach ("a buy stop must sit
above the market (it chases strength) — to buy below the market use a
limit"), so a beginner learns order mechanics by bumping into them, with
zero simulated money lost to a misplaced order. Nothing is silently
clamped or auto-corrected.

## 6. Live and replay: one path

Paper trading works identically on a live feed and a replay session because
the simulator taps the same per-trade ingestion point the bar engine uses.
No capability gate is needed beyond "a feed session is active and has shown
a price" — the Buy/Sell buttons disable themselves with an explanation
until the first print or backfill arrives (§3.4: disabled ≠ hidden).

Timeline honesty on rebuilds:

- **Replay seek** — closed bars cannot be un-closed, so the simulator
  flattens at the last mark, labels the exit `reset`, and a toast says
  "Simulated position flattened — the timeline was rebuilt under it"
  (the drawings-cleared toast's sibling).
- **Symbol or feed switch** — same flatten, because the position's tape is
  gone. History files are per symbol and untouched by switching.
- Backfill and paged history only *seed* the last-seen price; they never
  fill orders — trading against the past would be look-ahead.

## 7. History on disk

Closed trades append to `paper-trades/<SYMBOL>/<session>.csv` (cwd-relative
like every quantick path, `QUANTICK_TRADES_DIR` override), in the
`quantick-trades 1` CSV format defined by `quantick_sim::history` —
append-friendly, self-contained rows, torn tails reported not dropped. The
`<session>` name derives from the first closed trade's venue timestamp, so
the same replay run produces the same file name. Files are created lazily
(no empty files), appended on each close, and never rewritten.

## 8. The performance report

An `egui::Window` in the `SettingsDialog` mold, titled **"Simulated
performance"**, opened from the Trading tab (and Tools menu). Scope combo:
current symbol / all symbols — both read the history folder fresh on open,
so the report always reflects what is actually on disk, this session or any
before it.

Content, from `quantick_sim::PerformanceReport`: net points, trade count,
win rate, profit factor, max drawdown, gross profit/loss, averages and
largest win/loss, long/short split — each metric with a one-line plain
explanation under it (didactic first), and honest blanks: a ratio whose
denominator doesn't exist shows `—`, never `∞`. Unreadable rows found while
loading are counted and disclosed ("2 rows could not be read"), never
silently skipped.

## 9. Out of scope (this milestone)

- Real order routing of any kind.
- Fees, margin, multi-account, per-instrument currency P&L.
- Automated strategies (the growth map's bot row remains future work).
- Persisting open positions across restarts.
