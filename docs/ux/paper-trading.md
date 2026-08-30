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
| Pending order | dashed 1 px, `ACCENT` | pill: `BUY LMT 2`; open on hover: `#3 BUY LMT 2 @ 95.0`, `×` cancels |
| A pending order's stop | dashed 1 px, `SELL` | chip: `SL 90.0 −5.0 pts`, hover `×` clears |
| A pending order's target | dashed 1 px, `BUY` | chip: `TP 110.0 +15.0 pts`, hover `×` clears |
| A ladder's working leg | dashed 1 px, `SELL`/`BUY` by role | pill: `SL 1` / `TP 1`; open on hover: `#7 SL 1 @ 97.0`, `×` cancels that leg |
| The aim's projected bracket | dashed 1 px, `SELL`/`BUY` by role | chip: `SL 97.0 · 3 pts · 3 ticks · 1:1`, or `SL 97.0 · 1` per rung under a strategy |

**A working order carries its own bracket.** The legs are *dashed* where
the position's are solid, and the difference is the whole meaning: a
position's stop is an exit that can fire on the next print, while an
order's is a promise that arms the moment the entry fills and never before.
They are measured against the **order's own price**, not the market — the
same reference the simulator validates a bracket against — so `SL 90.0
−5.0 pts` reads "if this fills at 95 and stops at 90, that costs five
points". A leg the chart lets you drop is a leg the venue accepts; the two
cannot disagree about which side of the entry is the protective one.

The grammar is the position's, reused rather than re-invented: hovering the
order's line or its tag reveals the labelled `SL`/`TP` handles for the legs
it does not have yet, pressing one starts a create-drag, and the leg's own
line is its handle once it exists. One function paints both owners
(`draw_bracket_of`), so what a trader learns on a position works on an
order because it is the same code.

**A pending order's tag rests small.** It sits at the right edge of the
plot — over the newest candles, the ones the trader is reading — and stayed
there in full for as long as the order worked, hiding the price action
behind a banner. At rest it now states only what the chart cannot say
otherwise: an order line is `ACCENT` whatever its side, so the words `BUY
LMT 2` are the only place the side, the kind and the size live. The price is
already on the gutter chip and the id only matters once you mean to act on
the order, so both wait for the open form. The tag opens under the pointer
(the line's own grab row, plus the row a clamped tag was pushed into near a
chart edge), while its dock row is hovered — one hover, two surfaces — and
for as long as it is being dragged, since a trader repricing an order is
reading the number they are moving. The `×` exists only in the open form,
and `control_at` asks that same question at press time, so a cancel is never
pressable while unpainted. The position, stop and target tags keep the full
form: there is at most one of each, and none of them is the tag that filled
the screen.

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

### Every visible pane is a trading surface

Order entry follows the **pointer**, not focus. Hold the buy modifier over
any pane on screen — the flow chart, a context chart, one of four in a
split — and the aim paints there and the click places there, with no
focusing click first. A price level is as true on a one-minute context
chart as on the flow chart, and a trader who has just read a level on one
of them should be able to act on it where they read it.

Two things stay where they were. The **position HUD** follows focus, since
there is one card and it must not flicker between panes as the hand crosses
them. And a **drag in flight** stays with the pane it started in: the
grabbed line is read against that pane's price scale, so handing the
gesture to a neighbour mid-drag would reprice the order to whatever that
pane's scale says — a stop that jumps because the hand strayed across a
divider. (`Tab::trading_pane`, a pure function for exactly that reason.)

Every pane already *painted* the orders and the position; only the input
was gated. Nothing about the unsplit case changes: one pane is always the
answer.

### Cmd trading: the aim rides the pointer

Hold the buy modifier (Shift by default; Ctrl sells, both configurable in
the Trading tab) and the chart paints the order the next click would place:
a dashed line from under the cursor out to the axis, the exact snapped
price on the gutter, and a `BUY stop 1` label riding beside the cursor —
a fixed gap to its left, flipping to the right near the left edge, never
under the crosshair it belongs to. Whether the entry rests as a stop or a
limit follows the same validity table the right-click menu uses: above the
mark a buy stops in, below it a buy waits at a limit; a sell mirrors.

**Which kind, when you mean a particular one.** The Trading tab's `Place`
selector is `auto` (the shipped default), `limit` or `stop`. This is not a
way to place a stop where a limit belongs: the fill model leaves exactly
one resting kind valid at any price — a buy limit at or above the market
would fill at once, a buy stop at or below it would trigger at once — so
`auto` is right almost always. It is a way to say **which order you came to
place**. The mark moves: a level a hand's breadth above the last price is a
buy stop now and a buy limit two ticks later, and under `auto` the same
click at the same level places a different order depending on when it
lands. Under a stated kind the aim simply **stands down** where that kind
cannot rest — no line, no label, no place — rather than quietly handing you
the other one. A trader who came to buy a pullback is never given a
breakout stop. The tab says which half of the chart is live while a kind is
stated, so the silence is explained before it is met.

**The click places it, anywhere in the plot.** A label that follows the
pointer can never be landed on — move toward it and it moves with you — so
the *held modifier* is the deliberate act and the label is the statement of
what this click will do. Release the modifier and the aim is gone; the
pointer wears the hand cursor for as long as it is up, everywhere, because
everywhere is the target. An overlay `×` still takes the press first: a
cancel under the pointer is never eaten by an order.

**The aim is the last claimant on the canvas.** Its target is the whole
plot, so everything already holding a pixel outranks it, and the aim is
*stood down* there rather than merely refused — no line, no label, no hand
cursor, no place. That is what keeps the promise: the label can never
advertise an order the press will not make. It yields to

- a drawing's **handle**, and the canvas's own chrome — the tape chip, an
  indicator pane's header or divider (`ChartInput::canvas_claimed`, answered
  by the pane exactly as "a tool is armed" is). The default buy modifier is
  Shift, the very key that levels a channel corner mid-drag, so without this
  the drawing gesture would be gone. **Handles only, never a body:** a handle
  is a 12 px target where the two gestures genuinely collide, while a body is
  a region — and some bodies are enormous, since a fixed-range profile claims
  its whole histogram strip on purpose. Yielding bodies left a chart with a
  profile on it with a region where the aim never appeared. Moving a body
  needs no modifier, and a body drag reads Shift every frame, so pressing
  first and then holding it still constrains the move;
- an armed limit/stop from the ticket — an intent already stated, with its
  own hint on screen;
- this module's own furniture: an overlay ✕ or bracket handle, and any
  order/stop/target line a press would grab;
- the layer switch. Hidden means unpainted, and unpainted means untouchable
  — an invisible plot-sized order button is the worst kind of hidden
  control (`ChartInput::layer_visible`).

Sweeping across a drawn line or an order line blinks the aim off for its
grab band, and that band's own cursor comes up instead; nudge clear and the
aim is back.

### A bracket may be a ladder

A bracket used to be one stop and one target. It is now an ordered list of
**parts** — at most four, a bound rather than a preference, because the list
is walked on a per-trade path and its cost should be visible rather than
configured. Each part carries its own stop, its own target and its share of
the entry.

A plain bracket is the degenerate case: one part covering the whole fill.
It arms the position's own pair exactly as it always did, which is what the
port reports and what every venue models — so nothing about the single-pair
trader's chart changed.

**A rung is the order's, not the strategy's.** The named ladder is a
template: once an order rests on the chart it carries a *copy*, and each rung
is a line the trader can haul and a `×` they can clear, one at a time,
leaving every other rung where it is. Nothing is written back to the saved
strategy, so the next order still rests with what they configured. What a
ladder does refuse is the *whole-bracket* handle, where one drag would
replace every rung with a single level — the rungs are reachable
individually instead. After the fill the question does not arise: the rungs
are working orders by then and their own lines already mean "reprice".

A **ladder** is what is new. On the fill each part becomes a
one-cancels-the-other pair of working orders on the reducing side:
whichever leg fills closes that part and cancels its sibling, and the parts
the print did not reach carry on untouched. Because the legs are ordinary
working orders they inherit everything orders already have — they show in
the dock's list, they are dragged to reprice, and the `×` cancels one
without touching the other. They take their role's colour rather than
`ACCENT` and say what they do (`SL 1`) rather than which way they trade,
because a leg reading as another entry waiting to fire is the misreading
this surface cannot afford.

Under a ladder the position's own `stop_loss`/`take_profit` answer
**nothing**: there are several stops and no single one of them is true, so
the legs carry the truth and the position declines to pick a rung to stand
for them all.

### Named exit strategies

A ticket row picks a **named exit ladder** — rows of (share of quantity,
gain in ticks, loss in ticks) — and the aim then projects that ladder, every
rung of it, before the click. `<None>` rests a bare order the trader
brackets by hand, which is exactly what the ticket did before this existed.

Ticks only, deliberately: a currency or percentage row needs a tick value
this workspace does not have, and a number the app cannot compute honestly
is one it does not show. The shares must add up to 100%; the editor says so
beside the fields rather than normalising behind the trader's back, and the
last rung takes any rounding so no sliver of a position is left naked.

The editor is a window (`Edit…` beside the selector, or
`QUANTICK_PAPER_STRATEGY_EDITOR=1` on launch) — building a ladder is a job a
trader finishes and closes rather than part of reading the chart. Strategies
and the selection are app-wide and survive a restart in the paper-state
sidecar; a selection naming a strategy the file no longer carries selects
nothing rather than quietly arming its neighbour.

### The ruler: how far, decided before the click

Holding the cmd modifier aims an entry. Rolling the wheel while it is up
walks a projected stop and target out from the pointer, **one tick per
notch, the same distance on both sides**. Equal by construction, so what is
on screen before the click is the trade at 1:1, and the chip states the
distance in points and in ticks.

The question this answers is not "where does the stop go" but "is that
distance worth taking" — asked while the order still costs nothing to
abandon. The distance sticks across aims, and the click places exactly what
the ruler was showing.

One wheel, one meaning at a time: while the ruler spends a frame's travel
the chart's zoom is told to leave it alone, so the same roll never both
widens a bracket and rescales the plot. With a strategy selected the wheel
goes back to the chart — the strategy owns the distances, and two rulers on
one aim would be two answers to one question.

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

Closed trades append to `<trades_dir>/<SYMBOL>/<session>.csv`, in the
`quantick-trades 2` CSV format defined by `quantick_sim::history` — the
first eight columns are exactly version 1's, then the tape-audit ids
(`entry_agg_id`, `exit_agg_id`) and the excursions (`mae_points`,
`mfe_points`). Version-1 files still load; their missing fields come back
as *unknown* and re-export as empty cells — unknown is not zero.
Append-friendly, self-contained rows, torn tails reported not dropped. The
`<session>` name derives from the first closed trade's venue timestamp, so
the same replay run produces the same file name. Files are created lazily
(no empty files), appended on each close, and never rewritten.

**Where that folder is, is configuration — and one click**: the Trading
tab's session strip says it out loud ("trades saved to: …", click to
open) and its folder button opens a picker; the choice applies to every
tab at once, the next close opens a new session file under the new home
(files already written stay put), and it is remembered across restarts in
the `paper-state.toml` sidecar — the added-symbols pattern, so the app
never rewrites the user's hand-commented `quantick.toml`. The shipped
base stays `[paper] trades_dir` in `quantick.toml` (default
`paper-trades`, relative to the working directory), and
`QUANTICK_TRADES_DIR` still overrides everything for one run. The folder is also the feature's **integration port**: the
`quantick-trades` CSV format in `quantick_sim::history` is the contract,
so any producer that writes it there — the future bot runner, a converter,
another tool — appears in the Trades ledger (`EARLIER SESSIONS`), the
report and the export with no extra wiring. The sim crate itself is the
shared engine: a bot embeds the same `Simulator` and emits the same
`ClosedTrade`s, so its trades wear the same mould everywhere by
construction (one engine, three consumers).

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

## 9b. The same three things, without a hand

`CLAUDE.md`'s *operable without a hand* rule, for the one class of
capability that had no registry entry at all: `trade.order.place`,
`trade.order.bracket` and `trade.order.cancel`, joined by
`trade.strategy.select` and `trade.ruler.set` — the two that shape what the
*next* order will carry rather than changing one that exists. Every result
in the family reports which ladder is armed and where the ruler stands, so
a caller that placed an order knows which protection it just bought. Each
is a named call with
an actor in its signature, answering with the venue's own refusal text and
with every working order read back, plus the mark the call landed against
— every refusal here is a statement about a price relative to the market,
so the market comes with it.

`place` states its kind rather than inferring one. The chart's aim can
infer because it has a pointer and a market under it; an action has
neither, and an action whose meaning depends on where the market is at the
instant it lands is one nobody can replay.

They sit behind their own effect and their own permission, whose only
ceiling is a `trader` profile **nothing hands out**. `annotate`'s own
description promises it never affects a position, so a trade cannot borrow
it, and deciding which connection may trade is a decision about a real
account rather than a detail of the change that carved the tier out. Today
the gateway refuses them before dispatch and the in-process operator — a
hotkey, a harness hook, a test — reaches them normally.

## 9c. Where the orders actually go

Since the venue port (`quantick-trading`), this host holds a
`Box<dyn TradingVenue>` rather than a `Simulator`. Orders, brackets,
positions, fills and round trips are facts about trading, not about
simulation, so they live in that crate and the deterministic paper
simulator is *one implementation* of the port. The chart's gestures build
`OrderIntent`s; `Command::dispatch` is the one door for callers that
already speak `sim::Command` (the strategy kernel, the backtest harness).

Nothing about the honesty contract moves: every venue constructed here is
still the simulator, every surface still says `SIM`, and P&L is still in
points. What the port buys is that the day a broker implements it, none of
the surfaces above learn a second vocabulary — and the permission that
would guard it is already carved out (§9b).

## 10. Out of scope (recorded decisions)

- Real order routing of any kind.
- Fees, margin, multi-account, per-instrument currency P&L.
- ~~Automated strategies (the growth map's bot row remains future work).~~
  **Revoked** by the strategy-anchors work: an armed instance on a chart
  drawing now fires `PlaceMarket` through this same funnel (journal,
  toasts, ledger, report — no separate path). The bot never trades
  against an open position and every way it stops watching is a named
  badge state; see `docs/ux/strategy-anchors.md`. Real routing stays out.
- Persisting open positions across restarts. Deliberately kept out even in
  v2: an honest restore would need the same feed and symbol back, a
  `restored — not proven by the current tape` label until the first print,
  and a flatten offer at startup — designed, recorded here, not built.
- Granting any connection the `trade` permission. The tier exists and the
  gateway enforces it; which profile may hold it is a decision about a real
  account, deliberately left for the change that brings one.
- A market order from the aim. On the mark exactly nothing can rest, and
  that thin band is where a trader's pointer naturally sits — firing a
  market entry from it would turn a hover into a fill. Market stays on the
  buttons and the hotkeys, which are unambiguous.
- **Currency and percentage rows in a strategy.** Other platforms offer
  them; this one does not, because the workspace has no per-instrument tick
  value and a converted number would be a guess wearing a currency sign.
  Ticks are what the chart can prove.
- **A per-rung stop offset, and a nested trailing-stop strategy.** Recorded,
  not built: neither is expressible in a ladder that arms a fixed level per
  part.
- Shaded risk bands between entry and SL/TP: considered and rejected —
  the line-and-tag grammar carries the same information without painting
  over the candles.
