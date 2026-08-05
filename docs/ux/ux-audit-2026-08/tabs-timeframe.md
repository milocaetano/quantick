# Tabs, workspace & timeframe — full audit report

(1/3) — UX AUDIT: Tabs, Workspace Layout & the Timeframe Chart Flow

**Scope:** `crates/app/src/{tab.rs, tabstrip.rs, dock.rs, resample.rs, time_header.rs, state.rs, pane.rs, toolbar.rs, main.rs}`, the tab/layout/bar-type sections of `app.rs`, and `crates/app/config/feeds.toml`. Read-only; no files edited.

---

## 0. Headline

The user's complaint — *"the way the timeframe chart opens feels strange"* — has a concrete, reproducible root cause, and it is **not** primarily about discoverability.

There are two different ways to get time bars in quantick, and **they are not the same feature**:

| | Route A — toolbar `bars → time` | Route B — `File → Layout → Time + Flow` |
|---|---|---|
| Which chart | the **flow pane** (the main chart) | a **second pane**, left half of the canvas |
| Opening interval | **1000 ms (1 second)** — `pane.rs:449` | **60 000 ms (1m)** — `time_header.rs:37` |
| Presets (1m/5m/15m/1h) | **none** — raw ms drag only | yes — `time_header.rs:28-33` |
| Venue candle history (90 days) | **never** — `tab.rs:375-382` | yes |
| Bars on screen at open | ~20 (from a 1000-trade backfill) | ~130 000 |
| Can it fill the window | yes | **no** — capped at 75% (`pane.rs:115`) |

A user who picks the obvious control (`bars → time` in the toolbar, sitting next to tick/volume/dollar/imbalance) gets a **1-second chart with roughly 20 bars and no history**. If they then drag the interval to 60 000 ms hoping for a 1-minute chart, they get a chart with **one bar** — 1000 backfilled trades on a liquid market is seconds to a couple of minutes of tape, and the flow pane has no venue prefix to stand in front of it. That is the "strange".

The good timeframe experience exists, but it is reachable only through a *layout* command in the **File** menu, it always arrives as a **split**, and it can never occupy the whole window.

---

## 1. Inventory — every user-facing control

### 1.1 Tab strip
Drawn inside the menu-bar row (`app.rs:1919-1922`), module `tabstrip.rs`.

| # | Control | Location | Trigger | Effect |
|---|---|---|---|---|
| 1 | Tab chip `SYMBOL · venue` | `tabstrip.rs:62` | left click | `TabAction::Activate(i)` → `app.rs:3251` sets `active_tab` |
| 2 | Chip label amber tint | `tabstrip.rs:59-61` | state: `chip.replaying` | marks a tab playing a recording |
| 3 | Attention dot (3 px amber, chip top-right) | `tabstrip.rs:67-74` | state: `Tab::needs_attention()` (`tab.rs:560`) | background tab reconnecting or asking for user action |
| 4 | Close `×` | `tabstrip.rs:80-90` | click; visible on hover, on the active chip, or always when disabled | `TabAction::Close(i)` → `close_tab` (`app.rs:806`). Tooltip "Close tab (Ctrl+W)"; disabled tooltip "The last tab stays open — a window with no market has nothing to show" |
| 5 | `+` | `tabstrip.rs:94-100` | click | `TabAction::New` → opens SourcePicker (`app.rs:3257`). Tooltip "Open another market (Ctrl+T)" |

**Absent:** rename, drag-to-reorder, middle-click close, right-click context menu, duplicate-tab, `Ctrl+1..9` direct selection. `tabstrip.rs` registers no `context_menu` and no drag sense.

### 1.2 Source picker (`+` dialog)
`egui::Window "Open market"`, `tabstrip.rs:234-359`. Not collapsible, not resizable.

| # | Control | Location | Notes |
|---|---|---|---|
| 6 | Feed combo | `tabstrip.rs:249-266` | lists `config.feeds`; unimplemented providers get a `(soon)` suffix |
| 7 | Symbol combo | `tabstrip.rs:272-278` | corrected to a valid symbol when the feed changes (`ensure_symbol_valid`, `:195`) |
| 8 | "added here" list + per-symbol `×` | `tabstrip.rs:294-315` | removes a user-added symbol; disabled while a tab holds that market |
| 9 | "Add symbol…" field (120 px, hint `WINQ26`) | `tabstrip.rs:321-325` | Enter submits |
| 10 | "Add" button | `tabstrip.rs:328` | trims, takes verbatim, refuses empty (`:208-224`) |
| 11 | Refusal message | `tabstrip.rs:341-343` | amber, under the field |
| 12 | "Open" | `tabstrip.rs:347-349` | `PickerOutcome::Chosen` → `open_tab` (`app.rs:746`) |
| 13 | "Cancel" / window `×` | `tabstrip.rs:350-357` | both cancel |

### 1.3 Menu bar — File (`app.rs:1773-1836`)

| # | Item | Location | Effect |
|---|---|---|---|
| 14 | New Tab… (Ctrl+T) | `app.rs:1774-1783` | opens the source picker |
| 15 | Close Tab (Ctrl+W) | `app.rs:1784-1798` | disabled at one tab, with hover text |
| 16 | **Layout ▸** submenu | `app.rs:1800-1813` | container only |
| 17 | Layout ▸ **Single** | `app.rs:1802` | `set_layout(Single)` (`tab.rs:636`) |
| 18 | Layout ▸ **Time + Flow** | `app.rs:1803` | `set_layout(TimeAndFlow)`; builds the time pane next frame (`tab.rs:670`) |
| 19 | Market Replay… (Ctrl+R) | `app.rs:1815-1824` | |
| 20 | Close Replay | `app.rs:1825-1831` | only present while replaying |
| 21 | Exit | `app.rs:1833-1835` | |

### 1.4 Menu bar — View / Tools / Help
Items 22-33: Hide/Show panels (Ctrl+B) `:1843`; Drawing toolbar ▸ Left/Right/Top/Bottom `:1853-1868`; Hide/Show drawing toolbar `:1874`; L2 settings, Bubble settings, Session, Paper trading `:1878-1888`; Perf readings checkbox `:1890`; Timezone ▸ `:1893-1905`; Tools → Appearance… `:1908`; Help → Replay file format… `:1914`.

Note: `docs/ux/ui-design-model.md` §10 specifies an **Insert** menu (indicators, drawing tools). It does not exist in `draw_menu_bar` (`app.rs:1772-1918`) — indicators live in the toolbar's LAYERS group instead. Doc/code drift.

### 1.5 Toolbar BARS group (`toolbar.rs:362-437`; model wired at `app.rs:946-951`)

| # | Control | Location | Options / range |
|---|---|---|---|
| 34 | `bars` kind combo | `toolbar.rs:370-387` | five entries from `BarKind::ALL` (`state.rs:37`): **tick, volume, dollar, time, imbalance** — labels at `state.rs:47-55` |
| 35 | volume / dollar entries gated | `toolbar.rs:377-384` | disabled without `capabilities.traded_volume`; disabled hover "this source quotes prices but prints no traded volume" |
| 36 | `N trades` DragValue | `toolbar.rs:398-399` | 1..=5000 |
| 37 | `units` DragValue | `toolbar.rs:402-407` | 0.1..=1000, speed 0.1 |
| 38 | `notional` DragValue | `toolbar.rs:410-415` | 1000..=1e9, speed 1000 |
| 39 | **`interval ms` DragValue** | `toolbar.rs:418-426` | 100..=86 400 000 ms, speed 100 — **the only time control on the flow pane** |
| 40 | `target trades` DragValue | `toolbar.rs:429-434` | 2..=5000 |
| 41 | Folded form: parameter merges into combo text | `toolbar.rs:364-368`, `:440-448` | e.g. `time · 60000 ms`; the widget moves to the `⋯` overflow |

**This group always writes to `tab.flow_pane`** (`app.rs:946-951`) regardless of which pane has focus.

### 1.6 Time pane header (`time_header.rs:66-114`)
Drawn **only while the canvas is split** (`tab.rs:1472-1491`). 24 px strip carved off the top of the time pane (`pane.rs:170-176`).

| # | Control | Location |
|---|---|---|
| 42 | `time` label (faint) | `time_header.rs:84` |
| 43 | `1m` chip (60 000 ms) | `time_header.rs:29` |
| 44 | `5m` chip (300 000 ms) | `time_header.rs:30` |
| 45 | `15m` chip (900 000 ms) | `time_header.rs:31` |
| 46 | `1h` chip (3 600 000 ms) | `time_header.rs:32` |
| 47 | custom interval DragValue, suffix `" ms"` | `time_header.rs:97-108`; tooltip "custom interval for this pane" |

### 1.8 Canvas
| # | Control | Location |
|---|---|---|
| 48 | Draggable divider (4 px rule, 5 px grab margin) | `tab.rs:1582-1604`; clamped to 25%..75% (`pane.rs:115,162`) |
| 49 | Click-to-focus a pane | `tab.rs:1553-1575` (raw pointer press, layer-checked) |
| 50 | 1 px accent focus rule under the focused pane's top edge | `tab.rs:1541-1547` |

### 1.8 Keyboard shortcuts
Declared `app.rs:1729-1748`, consumed `app.rs:3094-3115`.

`Ctrl+T` new tab · `Ctrl+W` close tab · `Ctrl+Tab` next tab · `Ctrl+Shift+Tab` previous tab · `Ctrl+R` replay browser · `Ctrl+B` dock.

**No shortcut exists for:** switching layout, switching bar kind, switching timeframe, jumping to tab N, or focusing the other pane. `handle_tab_keys` is not gated on widget focus (unlike `handle_drawing_keys`, `app.rs:1965`), so Ctrl+W/Ctrl+T fire while a text field has focus.


## 2. Flows

### (a) What the app opens with
1. `config::load()` — `QUANTICK_CONFIG` env path, then `./quantick.toml`, then the embedded `crates/app/config/feeds.toml` (`main.rs:95`).
2. Feed/symbol: `default_feed = "binance"`, `default_symbol = "BTCUSDT"` (`feeds.toml:13-14`), overridable by `QUANTICK_DEFAULT_FEED` / `QUANTICK_DEFAULT_SYMBOL` (`config.rs:31,37`).
3. **Bar spec: `BarSpec::Tick(50)` — hardcoded** at `main.rs:56` (`INITIAL_TICK_SIZE`) and `main.rs:166`. **There is no config key and no env var for the opening bar type.**
4. Layout: `CanvasLayout::Single` (`tab.rs:276`), i.e. flow pane only, no time pane built.
5. Bubbles: `active = "dense tape btc"` in the committed `config/bubbles.toml:9` (working tree currently dirty to `"live lane pie"`). Binance declares no per-feed `bubble_preset`, so the file's `active` stands.
6. Backfill: 1000 trades (`feed/mod.rs:36`, `DEFAULT_BACKFILL_TARGET`), overridable via `QUANTICK_BACKFILL`. At tick(50) that is **~20 bars on screen** at first paint.
7. Dock: visible, collapsed to its 36 px strip, no tab open (`dock.rs:124-131`).

### (b) Getting a time-bar chart today — both routes, click by click

**Route A — toolbar (the discoverable one, and the broken one)**
1. Click the `bars` combo (toolbar, left of centre).
2. Click `time`.
   → `pane.current_spec()` (`pane.rs:502`) returns `BarSpec::Time(self.time_interval_ms)`, and `time_interval_ms` was initialised to **1000** at `pane.rs:449` because the opening spec was `Tick(50)`.
   → **You now have a 1-second chart.**
3. To reach 1 minute: drag or click-to-type the `interval ms` DragValue and enter `60000`. At speed 100 (`state.rs:174`) that is a ~600-pixel drag.
   → The chart now holds roughly **one bar**. The flow pane never receives the venue candle prefix: `request_ohlcv_history` returns early when `time_pane.is_none()` (`tab.rs:375-382`), and `refold_history_prefix` installs only onto `self.time_pane` (`tab.rs:534,550`). `pane.rs:412-414` states the invariant outright: *"Only ever non-empty on a time pane."*
   → Bubbles, the heatmap and the live lane keep rendering, because `orderflow` is `Some` for any flow pane by construction (`pane.rs:431`).

**Total: 2 clicks to a wrong chart, plus a numeric entry to reach a chart that is empty.**

**Route B — the split (the good one, and the hidden one)**
1. Click `File`.
2. Hover/click `Layout`.
3. Click `Time + Flow`.
   → `set_layout` (`tab.rs:636`) arms `pending_time_pane` and raises the `rebuilding bars` overlay; the pane is built on the **next** frame (`apply_pending_layout`, `tab.rs:670`), seeded from retained trades, then `request_ohlcv_history` asks for **90 days** of 1-minute candles (`feed/mod.rs:46`, `TIME_HISTORY_SPAN_MS`) — roughly 130 000 bars, ~45 MB per tab per §11.
   → Canvas splits **time left / flow right**, 50/50 (`pane.rs:117`).
   → The time pane opens at **1m** (`time_header.rs:37`) with the 1m/5m/15m/1h chips in its own 24 px header.
   → **Focus is not moved to the new pane.** `set_layout` only forces focus on the way *back* to Single (`tab.rs:648-650`). So the status bar and the toolbar still describe the flow pane until the user clicks the time pane.

**Total: 3 clicks to a correct timeframe chart that occupies at most 75% of the canvas and can never be alone.**

### (c) Changing an existing chart's bar type or timeframe
- **Flow pane:** toolbar `bars` combo + its one parameter. Applied one frame after the selectors settle (`tab.rs:1040-1089`), which debounces a drag to one rebuild per gesture. The viewport is re-anchored by *market time*, not bar index (`tab.rs:1067,1083-1085`), and the pane's drawings are cleared with a toast because bar-index anchors do not survive a re-cut (`tab.rs:324-333`).
- **Time pane:** its own header chips or ms drag (`tab.rs:1479-1489`). A chip click is a local re-fold of candles already held (`resample.rs:41`), never a network round trip. Dropping below 60 000 ms makes the venue prefix vanish silently — `is_foldable` (`resample.rs:24`) refuses anything that is not a whole number of minutes, and `refold_history_prefix` then installs an empty prefix (`tab.rs:540-542`). The only feedback is the `v` segment of the status bar's bar count going to zero (`statusbar.rs:198`).
- **Cross-pane trap:** the toolbar's BARS group writes to `flow_pane` unconditionally (`app.rs:946-951`), while the status bar's spec/bar-count section reads the **focused** pane (`app.rs:1677`).

### (d) Tab lifecycle
- **Create:** `+`, `Ctrl+T`, or File → New Tab… → source picker → Open → `open_tab` (`app.rs:746`) → `adopt_tab` (`app.rs:775`). The new tab **inherits the previous tab's flow-pane bar spec** (`app.rs:789`) but **not** its layout — it always opens `Single` (`tab.rs:276`, asserted at `app.rs:8959-8963`).
- **Close:** `×`, `Ctrl+W`, File → Close Tab. The last tab cannot be closed. Closing flattens the tab's simulated position through `Tab::close` (`tab.rs:1252`), drops the feed handle and workers, and prunes slot bookkeeping (`app.rs:806-850`). **No confirmation, no undo** — drawings, indicators and viewport go with it.
- **Switch:** click a chip, or `Ctrl+Tab` / `Ctrl+Shift+Tab` (wrapping, `app.rs:853`). Background tabs keep draining every frame (`app.rs:3065-3077`); only the active one renders.
- **Duplicates allowed:** the same market may be open twice by design (`app.rs:740-745`).

### (e) Persistence across restarts
**Almost nothing persists.**

- **Persisted:** indicator slots — kind, hidden flag and input values — for the **flow pane of the first tab only** (`app.rs:1400-1489`), debounced, in the file at `state_file::default_path()`. User-added symbols persist to `quantick-symbols.toml` (`symbols_file.rs`). Bubble presets persist to `config/bubbles.toml` via the panel's save.
- **Not persisted:** open tabs, active tab, per-tab feed/symbol, **canvas layout**, split fraction, focused pane, bar spec, timeframe, dock width and active dock tab, viewport position, drawings.

`tab.rs:207-210` and `app.rs:383-385` both name this explicitly as the deferred §14 `ui-state.toml` question. Every restart returns to Binance/BTCUSDT/tick(50)/Single.


## 3. UX findings

### BLOCKER-1 — `bars → time` produces a 1-second chart, then an empty one
**Evidence:** `pane.rs:449` (`time_interval_ms = 1_000` default) + `pane.rs:502` + `tab.rs:375-382` (no venue prefix off the time pane) + `feed/mod.rs:36` (1000-trade backfill).
**Heuristic:** *Match between system and the real world*; *Help users recognize, diagnose and recover from errors*.
The word "time" in a bar-type list, on a charting app, promises a timeframe chart. It delivers 1-second bars, and correcting the interval to a real timeframe empties the chart because the flow pane has no history source that reaches back further than 1000 trades. **This is the user's reported complaint.** Nothing on screen explains why a 1-minute chart shows one bar.

### BLOCKER-2 — a plain, full-window time chart is not reachable at all
**Evidence:** `CanvasLayout` has exactly two variants (`tab.rs:47-54`); `MIN_PANE_FRACTION = 0.25` (`pane.rs:115`) caps the time pane at 75%; `time_header::draw` is only called when `split` is true (`tab.rs:1462,1472-1491`).
**Heuristic:** *User control and freedom*; *Flexibility and efficiency of use*.
The chart with presets and 90 days of history can never be the only chart. A user who wants "just a 5-minute chart" must accept a permanently split canvas with a flow chart they did not ask for taking at least a quarter of the window.

### MAJOR-3 — the good timeframe entry point is filed under **File → Layout**
**Evidence:** `app.rs:1800-1813`.
**Heuristic:** *Recognition rather than recall*; *Match between system and the real world*.
"File" is where documents live; layout is a view concern, and the View menu right beside it already owns toolbar docking, panels and timezone. Worse, neither label — "Single", "Time + Flow" — contains the words *timeframe*, *time bars*, *minutes* or *candles*. A user hunting for a timeframe chart will scan the toolbar's `bars` combo (Route A, broken) and never open File.

### MAJOR-4 — focus lies: the toolbar's BARS group ignores it
**Evidence:** `app.rs:946-951` writes to `tab.flow_pane` unconditionally; `app.rs:1677` reads the focused pane for the status bar; `tab.rs:1541-1547` paints a focus accent.
**Heuristic:** *Consistency and standards*; *Visibility of system status*.
Every other focus-following surface follows focus — status bar content, indicator targeting, drawing chrome. BARS does not. Concretely: click the time pane (accent moves, status bar reads `time(60000ms)`), then change the toolbar's `bars` combo to `volume` — **the chart on the right changes** and the status bar keeps reading `time(60000ms)`. §11 does state "two selectors, two panes, no modes" as the intent, but the intent needs a visual anchor the current chrome does not provide: the BARS group is nowhere near the flow pane and carries no indication of what it governs.

### MAJOR-5 — opening the split does not focus the pane it just created
**Evidence:** `set_layout` (`tab.rs:636-664`) sets focus only in the `Single` branch.
**Heuristic:** *Visibility of system status*.
The user explicitly asked for the time chart; the chrome keeps speaking for the other one. Status bar, indicator insertion and the drawing rail all still act on the flow pane until an extra click lands on the new pane.

### MAJOR-6 — the two time controls speak different languages
**Evidence:** `time_header.rs:29-33` ("1m", "5m", "15m", "1h") vs `toolbar.rs:418-426` and `state.rs:147` (`time({ms}ms)`).
**Heuristic:** *Consistency and standards*.
The time pane offers human timeframes; the toolbar and the status bar both speak raw milliseconds. A user who learned "1m" in one place has to know it is 60000 in the other. The status bar renders `time(60000ms)` even when the chart came from clicking the chip labelled `1m`.

### MAJOR-7 — sub-minute intervals silently delete 130 000 bars of history
**Evidence:** `resample.rs:24` (`is_foldable`), `tab.rs:540-542` (installs an empty prefix), `statusbar.rs:198` (the `v` count drops to 0).
**Heuristic:** *Visibility of system status*; the project's own **Data honesty** rule.
The ms drag accepts values down to 100 (`state.rs:159`). Dragging from 60000 to 59900 throws the entire venue prefix away. The refusal is honest in intent — the code refuses to invent a fold — but nothing on screen says *why* the chart just lost three months. `OHLCV_INCOMPLETE` is logged (`tab.rs:481`); this case logs nothing at all.

### MAJOR-8 — no config or env control over the opening bar type
**Evidence:** `main.rs:56,166`; `config.rs` exposes only `QUANTICK_CONFIG`, `QUANTICK_DEFAULT_FEED`, `QUANTICK_DEFAULT_SYMBOL`.
**Heuristic:** *Flexibility and efficiency of use*.
Feed and symbol are configuration on principle (`feeds.toml` header says so). The bar type — arguably the more personal choice — is a constant in `main.rs`. Combined with zero workspace persistence, every launch is tick(50) forever.

### MINOR-9 — closing a tab is instant and irreversible
**Evidence:** `app.rs:806-850`; `tabstrip.rs:88-90`.
**Heuristic:** *Error prevention*; *User control and freedom*.
The `×` sits a few pixels from the chip label and appears on hover. One click discards that tab's drawings, indicator slots, viewport and simulated position (flattened and journaled, but gone). No confirmation, no undo, no reopen-closed-tab.

### MINOR-10 — hover-revealed `×` shifts the whole strip
**Evidence:** `tabstrip.rs:77-91` — the button is only added to the layout when `show_close`.
**Heuristic:** *Aesthetic and minimalist design* / stability of the pointing target.
Hovering chip 1 inserts a button into the horizontal layout, pushing chips 2..n and the `+` to the right. The `+` moves out from under a pointer travelling toward it. Reserving the space (or always drawing the `×` faintly) removes the jitter.

### MINOR-11 — no way to reorder, rename or duplicate a tab
**Evidence:** `tabstrip.rs:53-102` registers no drag sense and no context menu.
Duplicates are explicitly allowed (`app.rs:740-745`), which makes "two views of one book" a supported workflow with two identical chips and no way to tell them apart or to reorder them. "Duplicate this tab" — the natural gesture for that workflow — does not exist; the user must re-pick the market in the source picker.

### MINOR-12 — the time pane header likely clips at the minimum pane width
**Evidence:** `time_header.rs:21` (24 px strip), `:25` (8 px horizontal padding), `:84-108` — label + four chips + a DragValue showing up to `86400000 ms`; `pane.rs:115` allows the pane down to 25% of the canvas; `main.rs:145` allows a 900 px window.
At 900 px with the dock strip (36 px) and the tool rail open, a 25% time pane is roughly 200 px, against ~250 px of header content. **Not visually verified** — flagged from geometry, needs a screenshot at minimum window width.

### MINOR-13 — `Ctrl+Tab` and `Ctrl+W` are not focus-gated
**Evidence:** `app.rs:3094-3115` has no `ctx.memory(|m| m.focused().is_some())` guard, unlike `handle_drawing_keys` (`app.rs:1965`).
Typing in the source picker's "Add symbol…" field with `Ctrl+W` held (or any app that maps Tab navigation) can close a tab. Also: whether `Ctrl+Tab` survives egui's own Tab focus-navigation handling is **not determinable from the code** and needs a live test.

### NIT-14 — bar-kind labels are lowercase and unqualified
`state.rs:47-55`: `tick`, `volume`, `dollar`, `time`, `imbalance`. `time` is the only one whose meaning changes with a unit the combo does not show. `time` reading `time (1s)` or `time (1m)` inline would cost nothing.

### NIT-15 — doc drift: the Insert menu specified in §10 does not exist
`docs/ux/ui-design-model.md:334` lists **Insert** — indicators, drawing tools. `draw_menu_bar` (`app.rs:1772-1918`) has File, View, Tools, Help only.

## 4. Should time bars be the default? — the honest tradeoff

**The user's suggestion is directionally right about the symptom and wrong about the cure.**

### What genuinely argues for it
- **First-run comprehension.** tick(50) on BTCUSDT with a 1000-trade backfill is ~20 bars covering seconds of tape. That is a thin, unfamiliar first chart even for a flow trader — it looks like the app failed to load.
- **The time pane is the only chart with real history.** 90 days of venue candles vs. 1000 trades. On depth of data alone, the timeframe chart is the better default.
- **It is the shared vocabulary.** Every user arrives from a platform where "chart" means "timeframe chart", and orientation ("where is price, what happened today") is a timeframe question.

### What argues against it, and I find this decisive
- **It contradicts the product's stated identity.** `CLAUDE.md`: *"Real-time alternative bar charts… Build bars, show bars, expose bars to code."* `tab.rs:50-52` calls `Single` *"The flow pane alone — quantick's default and its identity."* Opening on a 1-minute candle chart makes quantick indistinguishable from TradingView at first glance and buries the one thing it does that others do not.
- **The flow layers are the point, and they need the flow pane.** Bubbles, heatmap and the live lane render only on the flow pane (`pane.rs:351-354`, §11). Defaulting to time bars either abandons them at startup or forces them onto a time chart the design explicitly keeps them off.
- **It would not fix the reported bug.** The user's pain is that Route A yields a 1-second, historyless chart. Making the *startup* spec `Time(60000)` leaves that trap exactly where it is — and adds a new one: the flow pane still gets no venue prefix (`tab.rs:375-382`), so quantick would now **open** on a 1-minute chart with one bar. That is strictly worse than today.

### Recommendation
**Do not change the default chart type. Fix the two structural defects instead**, then let the user's own choice persist:

1. **Give the flow pane the venue prefix whenever its spec is `BarSpec::Time`.** This is the real bug. `request_ohlcv_history`'s `time_pane.is_none()` guard should become "no pane in this tab is cutting by time", and `refold_history_prefix` should fold onto every time-cutting pane. Once done, `bars → time` yields a real chart, and everything downstream stops feeling strange.
2. **Persist the workspace** (§14 `ui-state.toml`): tabs, layout, split fraction, focus, bar spec. Then a user who prefers timeframe charts gets one on every launch without the project changing its identity — the correct resolution of "make it the default", because it makes it *their* default.
3. If a shipped default is still wanted after that, expose it as **configuration** (`default_bars = "time:1m"` in `feeds.toml`, alongside `default_feed`) rather than moving the hardcoded constant. That respects the file's own stated principle and lets the B3/MT5 presets differ from the BTC one.

## 5. Quick wins vs. structural

### Quick wins (localized, low blast radius)

| Fix | Where | What |
|---|---|---|
| **QW1 — Timeframe presets in the toolbar** | `toolbar.rs:417-427` | Replace the bare `interval ms` DragValue with the same 1m/5m/15m/1h chips the time pane has, plus the drag as the custom escape hatch. Reuses `time_header::PRESETS` — one list, two surfaces, as the module already documents. Kills the "1 second" surprise. |
| **QW2 — Sane opening interval for `bars → time`** | `pane.rs:449` | Change the flow pane's default `time_interval_ms` from `1_000` to `time_header::DEFAULT_INTERVAL_MS` (60 000). One constant. Note this alone is *not* enough — without the prefix fix (S1) it produces a one-bar chart, so ship QW2 and S1 together. |
| **QW3 — Human timeframe labels everywhere** | `state.rs:147`, `toolbar.rs:445` | Format `BarSpec::Time` as `1m` / `5m` / `1h` when the interval is a round unit, falling back to ms otherwise. Status bar and toolbar then agree with the chips. |
| **QW4 — Move Layout to the View menu** | `app.rs:1800-1813` | Cut the submenu from File, paste into View next to "Drawing toolbar". Relabel "Time + Flow" as "Time + Flow (timeframe chart)" or add a hover text naming what it opens. |
| **QW5 — Focus the pane you just opened** | `tab.rs:636-650` | `set_layout(TimeAndFlow)` sets `self.focus = PaneSide::Time`. The user asked for that chart; the chrome should speak for it. |
| **QW6 — Say why the venue history vanished** | `tab.rs:540-542` | When a time pane's interval drops below a minute, emit a notice/badge instead of an empty prefix in silence. The honest refusal deserves an honest explanation. |
| **QW7 — Reserve the close-button width** | `tabstrip.rs:77-91` | Always allocate the `×` slot; render it transparent when not hovered. Removes strip jitter. |
| **QW8 — Focus-gate the tab shortcuts** | `app.rs:3094` | Add the `ctx.memory(focused().is_some())` guard `handle_drawing_keys` already uses. |
| **QW9 — Label the BARS group's target while split** | `toolbar.rs:363` | While the canvas is split, render the group's caption as `bars (flow)` instead of `bars`. One string, and MAJOR-4's trap becomes visible. |

### Structural (design decisions, real work)

**S1 — Venue candle history belongs to any time-cutting pane, not to "the time pane".**
Today the 90-day prefix is keyed to *which pane object* it is (`tab.rs:375-382`, `tab.rs:534`, `pane.rs:412-414`), not to *what the pane is showing*. The correct rule is capability-shaped, matching how the rest of the app gates on `FeedCapabilities` rather than on identity: **a pane whose spec is `BarSpec::Time` with a foldable interval gets the prefix.** This is the single highest-value change in this audit — it is what makes the toolbar route produce a real chart, and it is a prerequisite for any "time bars by default" discussion.

**S2 — A third layout: `Time` alone.**
`CanvasLayout` needs a variant where the time pane owns the whole canvas (`tab.rs:47-54`), with the header strip drawn in Single-time mode too (`tab.rs:1462`). This is what "I just want a 5-minute chart" actually means, and it is currently unrepresentable. Cost: `draw_canvas` must handle a single *non-flow* pane, and the flow-layer toolbar toggles must disable themselves when no flow pane is on screen — which the capability-gating pattern already supports.

**S3 — Workspace persistence (`ui-state.toml`, §14).**
Open tabs, active tab, per-tab layout, split fraction, focus, and per-pane bar spec. `tab.rs:207-210` and `app.rs:1400-1409` both already name this as the blocker for persisting anything beyond the first tab's flow-pane indicators. It converts "should time bars be the default?" from a product argument into a user preference, and it removes the daily re-setup tax that likely contributed to the original complaint.

**S4 — Decide whether BARS follows focus, and make the answer visible.**
Two defensible resolutions: (i) BARS follows focus like everything else, and the time pane's header becomes a redundant convenience; or (ii) BARS stays bound to the flow pane, and the toolbar says so (QW9) while the time pane's header gains equal visual weight. The current state — focus-following status bar over a focus-ignoring control — is the one option that is indefensible.

**S5 — Tab strip affordances: reorder, duplicate, reopen-closed.**
Duplicate-tab in particular is the missing gesture for the explicitly-supported "two views of one book" workflow (`app.rs:740-745`); today it requires re-picking the market in a dialog, and it does not inherit the source tab's layout or drawings.

## 6. What could not be determined from code

- Whether `Ctrl+Tab` actually reaches `handle_tab_keys` or is swallowed by egui's focus navigation (`app.rs:3099`). Needs a live run.
- Whether the time pane header clips at the minimum pane width (MINOR-12). Geometry suggests yes at a 900 px window; needs a screenshot.
- Real-world timing of the 90-day / ~130 000-candle fetch on opening the split — the `loading venue history` overlay covers it (`loading.rs:72`), but how long the user stares at it per venue is unmeasured here.

