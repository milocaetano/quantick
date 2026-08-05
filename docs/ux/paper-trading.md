# Paper trading — UX specification

Status: shipped by the `feat/trade-sim` milestone, **extended by
`feat/paper-trading-v2`** (drag-to-create brackets, the trades ledger,
closed-trade marks, the filterable report, export). Companion of
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
  otherwise. A protective level attached to a market or stop entry is
  re-checked against the actual fill: if the tape outran it in between,
  the level is dropped and a toast says so — never kept to fire with a
  lying label.
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
| Right dock | tab **Trading** | the chart-trader panel: position card, order ticket, working orders, session strip |
| Right dock | tab **Trades** | the ledger: open position pinned, this session's closed trades, saved history, totals |
| Canvas | own layer above drawings | entry / SL / TP / pending-order price lines with in-plot tags, draggable |
| Canvas | own layer under the live lines | closed-trade marks and connectors (`closed trade marks` in the layer menu) |
| Status bar | new cell | `SIM ±N pts` (realized + open), hidden when the simulator has never traded |
| Floating window | `SettingsDialog` mold | the performance report, open–read–close |

The chart's right-click gesture belongs to the layer-visibility menu; paper
trading joins that menu rather than competing with it — a **trade** section
on top (anchored at the clicked price, on the pane that owns order entry),
and the two paper layer switches among the others. `AMBER` stays reserved by
contract for provenance (replay/backfill/inferred data); pending orders use
`ACCENT`.

## 3. The Trading dock tab (chart trader)

Top to bottom, grouped by the §3.2 question each block answers:

1. **Header** — "Paper trading — simulated fills from the tape". One line,
   always visible; the didactic anchor of the whole feature.
2. **Position block** — always present. Flat, it is one quiet row: `FLAT`
   plus the session's realized points (the answer to "am I in?" costs
   nothing). Open, it is the HUD card's sibling: the `SIM LONG 2` side
   chip, average entry, live open points; a bracket grid showing each
   leg's price and what it pays, with `×` to clear or `Set n pts` to place
   the missing leg straight from the ticket's offset; the `R:R` read when
   both legs exist; and the actions — `× Close`, `⇄ Reverse`, `Breakeven`
   (disabled with its reason until the position is in profit — with no
   fees simulated, break-even is the entry exactly), `Close 50%` (a
   partial close; the rest keeps its average and brackets) and
   `Flatten all`.
3. **Order ticket** — quantity as free decimal text with `−`/`+` steppers
   (`Shift` steps by ten; empty keeps meaning "fix me"), order type as
   three pills (`market` / `limit` / `stop`), optional Stop/Target offsets
   in points, then the entry pair: two full-width `BUY`/`SELL` chips,
   taller than the toolbar's — this is the surface where you commit.
   - `Market` → the buttons disclose consequences (`SELL 5 (reverses to
     short 4)`) and fire at the next print.
   - `Limit`/`Stop` → a press arms **click-to-place**; the armed side
     inverts (quiet fill, its own colour as a ring, `Click a price…`) and
     the other side disables with the reason. `Esc` disarms. The support
     line under the pair narrates the mode.
4. **Working orders** — one row per order: an `ACCENT` dash, `#id`, the
   side in its colour, `LMT 2 @ 95`, and `×` to cancel. Hovering a row
   lifts that order's line on the chart — one hover, two surfaces.
5. **Session strip** — realized points and trade count, `Report…`, and the
   history folder as a full-width quiet button that reveals the path in
   the file manager.

## 4. The chart layer

Painted between the drawings layer and the last-price line, so simulated
orders read above user annotations but never hide the market itself. All
lines span the chart width and end in a gutter chip (the third instance of
the `draw_last_price` chip geometry, so prices never disagree about their
pixel).

The gutter chip carries **the price and nothing else** (still dodging the
last-price row); the words and the controls live in a tag right-anchored
*inside* the plot. The tag grammar is semantic: an order that will fire
wears a solid chip; the position — a fact about the account, not an order —
wears a card (`INSET` fill, hairline border, a side-colour rail).

| Line | Style | In-plot tag |
|---|---|---|
| Position entry | solid 1.5 px, `BUY`/`SELL` by side | card: `SIM LONG 2` + live open pts, hover `×` closes |
| Stop loss | solid 1 px, `SELL` | chip: `SL 97.0 −12.4 pts`, hover `×` clears |
| Take profit | solid 1 px, `BUY` | chip: `TP 110.0 +13.6 pts`, hover `×` clears |
| Pending order | dashed 1 px, `ACCENT` | chip: `#3 BUY LMT 2 @ 95.0`, hover `×` cancels |

Interaction, reusing the drawing-drag grammar (`DrawingDrag` mechanics, same
hit radius constants, raw-input reads, gesture-consumption flag so the chart
never pans under a grabbed line; hover thickens a line, a drag adds the
drawings' halo):

- SL, TP and pending-order lines are **draggable**; on release the new price
  is submitted as a command and the simulator answers — either the line
  settles at the new price or a rejection toast explains why it snapped
  back ("a long's stop loss must sit below the price it protects…"). While
  an SL/TP drags, its tag reads the points distance and the live `R:R`
  against the other leg — the read that turns a drag into a decision.
- **Dragging away from the entry line creates the missing bracket leg** —
  the TradingView gesture: pull to the profit side and a take profit is
  born dashed under the pointer, pull to the losing side for the stop;
  release submits it. Labelled `SL`/`TP` handles appear on hover beside
  the position tag as the clickable alternative (the label removes the
  long/short flip ambiguity). A side whose leg already exists stays
  blocked — that leg's own line is its handle. The entry price itself
  still never moves: with both legs placed, grabbing the line is the old
  `Blocked` no-op.
- Order state lives in the simulator, **not** in `Drawings` — a bar-spec
  change or history rebuild clears annotations, but simulated orders belong
  to the session, not to the bar series.

### Closed-trade marks

Every round trip closed *this session* paints on the chart, under the live
lines: a filled triangle pointing the trade's way at the entry fill, a
diamond at the exit, joined by a faint dotted connector — all in the
**outcome's** colour (win/loss/scratch), direction carried by shape, each
mark ringed in the canvas colour. Marks anchor to the fill price on the bar
holding the fill's venue time. Only the session's trades paint — the tape
on screen proves their fills; rows loaded from earlier sessions stay in the
ledger. The paint caps at the 200 newest visible trades and says so out
loud. Hovering a mark answers with the ledger row's own words; the ledger's
selected trade gains the halo. The `closed trade marks` layer switches all
of it off independently of `paper orders & position` — hiding history must
never hide the position you are in.

## 4b. The Trades ledger (dock tab **Trades**)

The ledger of closed simulated trades: the open position pinned above the
scroll with its live open points, `THIS SESSION` under it, `EARLIER
SESSIONS` (read from the history folder, the live session's own file
excluded) under that — newest first, in fixed-height two-line rows (side
rail, `LONG 2`, `103.25 → 110.00`, signed points; close time · duration ·
exit reason), virtualised so a long history costs nothing. A totals strip
that never scrolls away sums what is listed. Scope pills switch this
symbol / all symbols; a session row's click selects it (Esc clears) and
highlights its round trip on the chart, and a hover control centers the
chart on the trade. Rows from earlier sessions are display-only — their
tape is not the one on screen. The header's download icon exports
everything listed (§7).

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

## 7. History on disk, and the export

Closed trades append to `paper-trades/<SYMBOL>/<session>.csv` (cwd-relative
like every quantick path, `QUANTICK_TRADES_DIR` override), in the
`quantick-trades 2` CSV format defined by `quantick_sim::history` — the
first eight columns are exactly version 1's, then the tape-audit ids
(`entry_agg_id`, `exit_agg_id`) and the excursions (`mae_points`,
`mfe_points`). Version-1 files still load; their missing fields come back
as *unknown* and re-export as empty cells — unknown is not zero.
Append-friendly, self-contained rows, torn tails reported not dropped. The
`<session>` name derives from the first closed trade's venue timestamp, so
the same replay run produces the same file name. Files are created lazily
(no empty files), appended on each close, and never rewritten.

The **export** (the Trades tab's download icon) is a different artifact:
one merged, spreadsheet-facing CSV of everything the ledger lists, written
off the UI thread to `paper-trades/export-<stamp>.csv` — human-readable
UTC stamps beside the venue epochs, a running-equity column, decimals
always with `.` (a pt-BR Excel would reinterpret a comma), symbols as a
column so concatenation survives. The toast answers with the path or the
failure; the journal stays the machine-readable source of truth.

## 8. The performance report

A resizable, non-modal `egui::Window` titled **"Simulated performance"**,
opened from the Trading tab (and the View menu). The filter row picks the
symbol (any folder on disk, or all) and the period — `Today / 7d / 30d /
90d / All` — **measured back from the newest saved trade in scope, never a
wall clock**: the engine has none, and a replayed session's trades may be
years old. The support line states the anchor out loud ("the last 7 days
up to 2026-08-04 (newest saved trade, not the wall clock)").

Three headline tiles answer first — `NET` (the window's one coloured
number), `WIN RATE`, `PROFIT FACTOR` — each with its denominator under it.
Then the realized equity curve by **trade index** (the closing order that
defines the drawdown; calling that axis "time" would misstate the plot):
one quiet line, a diverging fill against the zero baseline, the deepest
drawdown annotated with its own chip, hover snapped to the nearest trade in
the ledger's vocabulary, drawing-only downsampling past a thousand points
that says so. Under it, the metric grid — drawdown, run-up, recovery
factor, gross and averages, payoff, expectancy, sample stddev, streaks
(a scratch breaks both), durations, largest trades, and the excursion
averages with their disclosed denominators ("over 12 of 14 winners") —
plus long-vs-short and per-exit-reason tables. Honest blanks everywhere a
denominator is missing; unreadable files and rows counted and disclosed;
empty states name the filter that caused them and offer to clear it.

## 9. Trading hotkeys and the chart's trade menu

`Shift+B` / `Shift+S` buy/sell at market with the ticket's quantity and
offsets; `Shift+R` reverses; `Shift+F` flattens (close + cancel all);
`Shift+X` cancels every working order without trading. All of them stand
down while any text field owns the keyboard — a capital letter typed into
a symbol box must never become an order.

The pane's right-click menu (on the pane that owns order entry) opens with
a **trade** section anchored at the clicked price: buy/sell at market, and
the resting types that are valid on that side of the market — `Buy limit @
p` below it, `Buy stop @ p` above it, mirrored for sells. The invalid two
stay visible but disabled, wearing the sim core's own rejection text
(disabled ≠ hidden; the curriculum again).

## 10. Out of scope (recorded decisions)

- Real order routing of any kind.
- Fees, margin, multi-account, per-instrument currency P&L.
- Automated strategies (the growth map's bot row remains future work).
- Persisting open positions across restarts. Deliberately kept out even in
  v2: an honest restore would need the same feed and symbol back, a
  `restored — not proven by the current tape` label until the first print,
  and a flatten offer at startup — designed, recorded here, not built.
- Shaded risk bands between entry and SL/TP: considered and rejected —
  the line-and-tag grammar carries the same information without painting
  over the candles.
