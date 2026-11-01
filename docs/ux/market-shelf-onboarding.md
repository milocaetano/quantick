# Market Shelf: first run and opening more charts

Status: design approved by the trader on 2026-10-07, not built yet. This page is
the brief to implement from; the two prototypes beside it show the behaviour.
[`window-menu-and-workspace-tabs.md`](window-menu-and-workspace-tabs.md)
(2026-10-31) extends it: the tab strip moves into the title bar, the text
menu bar becomes a `⋮` menu, every tab carries a workspace, tab tear-off by
drag enters scope, and the palette gains Bars and Workspace selectors. Where
the two disagree, the newer page wins.

- [`first-run-concepts.html`](first-run-concepts.html) — the five first-run
  concepts a design panel shortlisted. The trader picked **Market Shelf**.
- [`market-shelf-prototype.html`](market-shelf-prototype.html) — clickable
  prototype of the chosen flow: Home tab, the `+` "Open a market" palette,
  My markets, pin and remove. Prices, trades and venue symbol lists in both
  files are simulated.

## The brief

No guided tour. A new user lands, sees what markets exist, picks one and
watches a smooth live chart the first time. Crypto is the suggested start
(BTC and ETH on Binance, Bybit later). From there the user must be able to
open more charts and shape the Home page themselves.

## How it was decided

Eight designers each proposed one concept from a different angle (launcher
cards, chart open on launch, command palette, recipes, market pulse grid,
live preview picker, newcomer and B3 trader). A panel of five roles (visual
designer, interaction designer, B3 day-trader, crypto scalper, engineer
costing it in egui) merged them into five finalists:

| Concept | Clicks to a live chart | Cost |
|---|---|---|
| Live on Launch (panel's pick) | 0 | S |
| Type a Ticker | 1 key | M |
| **Market Shelf (trader's pick)** | 1 | L |
| Live Preview Picker | 0 to see, 1 to open | L |
| Recipe Shelf | 1 | M |

A second round of four designers then worked out how to open more charts
from the shelf: the `+` button, the Home tab's life after the first chart,
markets beyond the starter cards, and several charts at once.

## The design

### Home tab

- Home is a pinned tab, always first, with a house icon and the label "Home".
  It never closes (`Ctrl+W` on Home does nothing) and never shrinks to an
  unlabelled glyph: the first prototype did that and the trader could not
  find the way back.
- Rows, top to bottom:
  1. **My markets** — markets the user pinned (★, in pin order), then recent
     ones (open first, then recently closed, "Closed 2m ago"), at most 8, and
     a dashed **+ Add to Home** card at the end. Empty state: "Pin the
     markets you watch every day".
  2. **Start here · crypto, no account** — Binance BTCUSDT and ETHUSDT, large
     cards that turn compact once My markets holds three entries.
  3. **Hyperliquid** — BTC, ETH, HYPE, SOL, ZEC.
  4. **Coming soon · Advanced** — Bybit (dashed, cannot open), "B3 via MT5 ·
     Set up (5 min)", "Replay · recorded sessions".
  5. Status bar: one entry per venue ("Binance live", "MT5 not set up") and
     the hint "Ctrl+T search any market · Ctrl+Shift+H back here".
- A card already open in a tab shows an **Open** badge; clicking it focuses
  that tab instead of opening a duplicate. Ctrl+click or middle-click opens
  in the background.
- Hover a card to get **☆** (pin to My markets) and **×** (remove from Home).
  × works on any card, starter cards included; "Show hidden markets (n)"
  brings starter cards back. Removing from Home never removes a market from
  search.
- Sparklines run only while Home is visible.

### `+` / `Ctrl+T`: "Open a market"

- A centred palette over the current chart, not a jump to Home: whoever
  presses `+` came to type a ticker. It replaces today's feed + symbol
  dialog (`surfaces/source_picker.rs`) and keeps its add-symbol path and
  catalog refusal rule (a refusal stays on screen under the field).
- Home and the palette read **one headless list of markets** (market, venue,
  price, trades/s, status), so the two views cannot disagree.
- Empty query: "Start here", "Recent", "Hyperliquid".
- Typing groups results by asset with one row per venue. Grouping uses an
  explicit `asset` key per configured symbol; nothing is merged by guessing
  (WIN vs WINV26 must never fall under one heading by accident).
- Row types and copy:
  - live market: `BTCUSDT · Binance · 38 trades/s · 64,212.5`, plus
    `open · tab 2` when already open;
  - venue market not added: `Not in My markets · Enter to add and open`
    (no price until it streams);
  - MT5 waiting: `Waiting for bridge · attach the Quantick EA to WIN$N in
    MetaTrader`; MT5 closed: `Market closed · last trade 18:29 · 5,412.0`;
  - replay: `REPLAY · Recorded Mon 6 Oct 2026 · 09:00–17:55 · 412k trades`;
  - Bybit: `Coming soon · not available yet`, cannot be chosen;
  - no match: `No market matches "XYZ"` then `Add XYZUSDT to Binance…`.
- Every row has ☆ to pin it to Home. "+ Add to Home" opens the same palette
  titled "Add to Home", where Enter pins instead of opening.
- Keys: `↑↓` choose, `Enter` open (or focus the open tab), `Ctrl+Enter`
  background tab, `Tab` cycles the bar type (tick:50, volume:5, time:1m),
  `Esc` close. `Shift+Enter` is reserved for "open in new window".
- Footer link "Browse all in Home ⌂".

### Several charts at once

Tabs first; a tab moved to its own OS window next; a multi-market split
inside one tab only if snapped windows prove clumsy in real use. A tab's
panes all belong to one market today, so a split means per-pane feeds and a
workspace schema change.

### Named commands

Every action is reachable without a mouse: `market.palette.open`,
`market.open {feed, symbol, bar, background}`, `catalog.add {feed, symbol}`,
`home.show`, `home.pin`, `home.unpin`, `home.hide`, `tab.focus_index`,
`tab.focus_next`, `tab.close`, later `tab.move_to_window`.

## Build order

Each phase is one PR that is usable on its own.

| Phase | What ships | Cost |
|---|---|---|
| 1 | Home as the pinned house tab; card opens or focuses a tab; Open badge | S |
| 2 | "Open a market" palette on `+` / `Ctrl+T`, on the shared market list | M |
| 3 | Home your way: pin, remove, + Add to Home, My markets, recents; the Home list is saved with the workspace, not in `feeds.toml`. Second launch restores tabs; with none, it opens on Home | M |
| 4 | Markets beyond the cards: venue symbol lists fetched by the feed crates, add-and-open, MT5 status rows, replay sessions by date | M |
| 5 | Move a tab to its own window (second monitor, or snapped side by side) | M |
| Later | Multi-market split in one tab | L |

Not now: drag to reorder My markets, tab tear-off by drag, linked crosshair
across markets, spread charts.

## Rules this design must keep

- Data honesty: replay always says REPLAY; a stale feed says stale; a market
  that has not streamed shows no price or sparkline.
- Bybit appears only as "coming soon" until a `feed-bybit` exists.
- The shared market list is UI-free, so it belongs in a headless crate
  (`workspace` is the likely home) with its tests, not in `app`.
