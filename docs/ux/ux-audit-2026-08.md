# UX audit — August 2026

An atomic usability audit of the whole `crates/app` surface: every tab, tool,
panel, dialog and gesture, carried out by six independent reviewers over the
code on `main`. Each claim carries a `file:line` reference. This document is
the synthesis; the six full per-area reports live in
[`ux-audit-2026-08/`](ux-audit-2026-08/).

**Audit base:** `main @ 2856238` (2026-08-04). PR #120 (chart layer
visibility) merged after the audit snapshot, so findings about the toolbar
layer toggles may be partially addressed — re-verify those before acting.

**Trigger:** user-reported friction — (1) could not close an open simulated
trade and could not tell which trade was open; (2) the way a timeframe chart
opens feels strange; (3) editing indicator properties feels strange.

**Totals:** 136 findings — 12 blockers, 50 majors, 52 minors, 22 nits.

---

## 1. Executive summary

The verdict in one sentence: **quantick's problem is not aesthetics — it is
missing surface.** The design system exists and is disciplined (tokens, amber
reserved for data honesty, capability gating with written reasons on every
disabled control). What is missing is the half of the interface that answers
questions: closing what is open, reading what is on screen, editing what was
added.

Three patterns cut across every area:

1. **Entry controls without exit controls.** BUY/SELL always visible;
   Close/Flatten buried in a dock that opens collapsed. The `bars → time`
   combo visible; the good timeframe chart hidden under *File → Layout*.
2. **The app contradicts itself.** The drawing inspector opens on selection,
   applies live, has tabs — while indicators use a blind dialog that the menu
   occludes. The status bar follows focus; the BARS group ignores it. Layer
   toggles target the flow pane; indicator commands target the focused pane.
3. **Nothing persists.** Tabs, layout, bar spec, drawings, dock, timezone —
   everything resets on every launch. The `ui-state.toml` (§14) already named
   in code comments would resolve half of the "I rebuild my workspace every
   time" pain.

Genuinely strong and not to be regressed: capability gating with written
reasons; the tape cell's arrival-vs-staleness split; gesture-coalesced undo in
drawings; the simulator's didactic rejection messages; amber as a disciplined
provenance-only token.

## 2. Pain point 1 — closing / seeing the open trade

Confirmed on both halves, three blockers. Full detail:
[`paper-trading.md`](ux-audit-2026-08/paper-trading.md).

Root causes:

- **Entry and exit live in structurally different places.** BUY/SELL are on
  the toolbar (`toolbar.rs:484-509`); Close/Flatten exist only inside the
  Trading dock tab (`paper_trading.rs:677-692`), and the dock starts
  collapsed (`dock.rs:124-132`). Nothing bridges the gap — no toolbar link,
  no clickable status cell, no chart menu, no shortcut, no badge on the dock
  icon while a position is open.
- **The position chip is overpainted by the last-price chip** at identical
  x/font (`paper_trading.rs:1300` vs `chart.rs:345,347`, paint order at
  `pane.rs:1709-1714`) — and entry price equals last price at the exact
  moment a market order fills, so the one persistent "you are long" statement
  is born mangled.
- **The status cell cannot distinguish open from flat** (`statusbar.rs:297`).
- **Pressing SELL while long is an undisclosed close-or-reverse**
  (`simulator.rs:601-627`), keyed to a quantity field the toolbar never shows.
- **Bonus blocker:** the UI instructs "drag a stop on the chart, or use the
  offsets below" for an open position — both routes are dead
  (`paper_trading.rs:640-669`); brackets can only be attached before entry.

Redesign (approved direction):

- `✕ Close 1 LONG` on the toolbar whenever a position is open; state-aware
  BUY/SELL labels (`SELL 1 (closes)` / `SELL 5 (reverses to short 4)`).
- A persistent position HUD pinned to the chart's top-left: side, qty, avg
  price, open P&L, Close/Reverse; directional caret when the entry price is
  off-screen.
- Status cell reads `SIM LONG 1 · +2.0 pts` open / `SIM +7.0 pts · flat`
  otherwise, and clicks through to the Trading tab.
- Fix the chip paint-order collision; make the bracket hint true (Apply →
  `SetBracket` on the open position; drag-to-create SL/TP from the entry
  line); a closed-trades ledger in the Trading tab; fill markers on candles.

## 3. Pain point 2 — how the timeframe chart opens

Full detail: [`tabs-timeframe.md`](ux-audit-2026-08/tabs-timeframe.md).

Root cause: **two routes that are not the same feature, and the obvious one is
broken.**

| | Route A — toolbar `bars → time` | Route B — `File → Layout → Time + Flow` |
|---|---|---|
| Opens at | **1 second** (`pane.rs:449`) | 1 minute, with 1m/5m/15m/1h chips |
| Venue history | **never** (`tab.rs:375-382`); fixing the interval to 60000 ms leaves ~1 bar | 90 days (~130k candles) |
| Full window | yes (but empty) | **never** — always a split, max 75% (`pane.rs:115`) |

Aggravators: opening the split does not focus the new pane
(`tab.rs:636-664`); the BARS group ignores focus and always writes to the flow
pane (`app.rs:946-951`) while the status bar reads the focused pane;
sub-minute intervals silently discard the entire venue prefix
(`resample.rs:24`, `tab.rs:540-542`); chips say "1m" while toolbar/status say
`time(60000ms)`.

Redesign (approved direction):

- **S1 — venue candle history belongs to any pane whose spec is
  `BarSpec::Time`**, not to "the time pane object". This is the real fix.
- Timeframe preset chips in the toolbar (same list as the header); opening
  default 1m, not 1s; human labels everywhere.
- **A third layout: `Time` alone** (full-window timeframe chart).
- Layout moves from File to View with a label that says "timeframe"; opening
  the split focuses the pane it creates.

### The default-bar-type decision

The user suggested time bars as the default. Decision (2026-08-05): **keep
the flow pane as the factory default** — alternative bars are the product's
identity, the flow layers only render on the flow pane, and switching the
default would not fix the broken route (the app would simply *open* on a
one-bar chart). Instead: fix Route A + ship the `Time` layout + persist the
workspace so the user's own choice becomes their default; optionally expose
`default_bars = "time:1m"` in `feeds.toml` as per-feed configuration.

## 4. Pain point 3 — editing indicator properties

Full detail: [`indicators.md`](ux-audit-2026-08/indicators.md).

Four independent root causes:

1. **No legend — the on-screen object is inert.** No overlay legend exists
   (`indicator_render.rs:116`), the pane title has no `Sense`
   (`indicator_render.rs:263-269`). The TradingView convention (double-click
   the name → settings) has nowhere to happen. The project's own spec
   (`docs/ux/ui-design-model.md` §9) describes exactly this legend; it was
   never built.
2. **The menu occludes the dialog it spawns.** The gear does not close the
   menu (`toolbar.rs:641-647`) and egui paints menus above windows.
3. **No live preview, and Apply closes.** Four clicks per tuning attempt
   (`indicator_panel.rs:56-71`).
4. **The app contradicts itself.** The drawing inspector already implements
   the right model (select → inspector → live apply → undo); indicators break
   every part of it.

Also blockers: script errors are computed and never rendered (an errored
indicator just disappears — against the data-honesty rule); the 4th pane
indicator silently does not draw (`indicators/mod.rs:241-246`).

Redesign (approved direction): build the chart legend (error/stale rows
included, double-click opens settings); reuse the drawing-inspector host for
indicator properties with Inputs/Style/Visibility tabs; make `PlotSpec`
editable as a UI-side override; immediate quick wins (close menu on gear,
deliberate dialog position, Apply without closing, delete confirm/undo,
show the scripts folder + rescan).

## 5. Remaining areas — headline findings

- **Drawing tools** ([`drawing-tools.md`](ux-audit-2026-08/drawing-tools.md)):
  blockers — drawings do not survive a restart (no `save()` on the app);
  all chart navigation is dead while a tool is armed. Majors — no trend line,
  no line styles, style neither presetable nor remembered, no right-click
  menu, no delete-all, no magnet/snap, tool shortcuts stay live while the
  rail is hidden.
- **Chart canvas** ([`chart-canvas.md`](ux-audit-2026-08/chart-canvas.md)):
  blocker — projection caps drop tape data silently (log-only, against the
  CLAUDE.md honesty rule). Majors — zero inspection of any flow mark (a
  bubble with ten encodings answers no questions), crosshair is a mode that
  costs object selection, prices hardcoded to 2 decimals in three places,
  no canvas gesture is discoverable (one cursor on the whole canvas), no
  back-to-live control, overloaded double-click, wheel zoom anchored to the
  right edge instead of the pointer.
- **App chrome** ([`app-chrome.md`](ux-audit-2026-08/app-chrome.md)):
  blockers — a bad config file kills the process before any window opens;
  Binance/Hyperliquid can never escalate to an actionable `Attention` notice.
  Majors — feed/symbol switching destroys the chart with no warning or undo,
  the status bar has no overflow strategy, the default 1100 px window already
  folds three toolbar groups (full toolbar needs 1172 px), two picker idioms
  for the same job.

## 6. Prioritized roadmap

| Prio | Item | Resolves |
|---|---|---|
| P0 | Close/state on toolbar + position HUD + honest status cell + chip-collision fix + dock badge | Pain 1 |
| P0 | Venue history for any `BarSpec::Time` pane + timeframe chips in toolbar + 1m default + human labels | Pain 2 |
| P0 | Indicator chart legend (with visible error/stale) + menu closes on gear + Apply stays open + positioned dialog | Pain 3 |
| P0 | One-to-three-line quick-win batch: cursors on axes and sim lines, jump-to-live chip, ScrollAreas (Trading tab, object manager), tape/bars tooltips, focus-gated Ctrl+W, rail-hidden shortcut gate, hide-all preview guard, persistent Fib errors, delete-all drawings | ~20 findings |
| P1 | **`ui-state.toml` (§14):** tabs, layout, split, focus, bar spec, dock, timezone, window + drawings persisted per (feed, symbol, spec) | Biggest single lever |
| P1 | `Time` layout alone + Layout in View menu + focus follows creation + resolve BARS×focus | Pain 2 structural |
| P1 | Unified inspector (drawings + indicators) with plot Style tab | Pain 3 structural |
| P1 | Brackets on open positions (Apply + drag-to-create) + trades ledger + fill markers | Sim as a practice tool |
| P1 | Crosshair as hover + OHLC readout + bubble inspection tooltip + pointer-anchored zoom + step-aware price decimals | Canvas readability |
| P1 | Config error in a window + Attention path for Binance/Hyperliquid + confirmed market switching | Chrome blockers |
| P2 | Trend line/ray/vertical/text tools; dash/dot line styles + style presets; right-click context menu; magnet/snap | Charting parity |
| P2 | Unified Settings surface (~19 env vars); Insert menu; Indicators dock tab; resizable panes; status-bar overflow plan; live window title; Help → shortcuts | Finish |

## 7. Visual direction — "clean" without changing the soul

The current dark theme is solid. Five principles guide the redesign:

1. **Everything on screen answers questions.** Legend for indicators, tooltip
   for bubbles, OHLC at the crosshair, HUD for the position. If a pixel
   encodes data, the data is reachable with the mouse.
2. **Every gesture announces itself.** The cursor changes over anything
   draggable; hover highlights; interactive things look interactive.
3. **One editing model for the whole app.** Select → inspector appears →
   changes apply live → undo. Drawings already do this; indicators and the
   sim join them.
4. **Dense by default, quiet by choice.** Keep the terminal density, add
   cross-layer control (per-layer opacity/focus) and bubble presets whose
   thresholds are all alive — no encoding that cannot be read.
5. **Honesty even in the cut.** Capped tape gets an amber status cell like
   every other admission; discarded sub-minute history explains itself.

## 8. Methodology

Six independent parallel audits (paper trading, tabs/timeframe, indicators,
drawing tools, chart canvas, app chrome) by separate reviewers over the code
at `main @ 2856238`, without running the app — pixel-level claims are
inferences from constants and are marked as such in the reports. Severities:
Blocker / Major / Minor / Nit; Nielsen heuristics cited per finding. The
per-area reports under [`ux-audit-2026-08/`](ux-audit-2026-08/) contain the
full atomic inventories (every control with `file:line`), the exact user
flows as implemented, all findings, and quick-win vs structural
recommendations.
