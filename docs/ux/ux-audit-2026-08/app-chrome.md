# App chrome — full audit report

Read-only analysis of everything around the chart canvas: menu bar, toolbar, status bar, tab strip, source picker, replay browser/transport, notice cards, loading indicators, theming, and window chrome.

## 1. INVENTORY

### 1.1 Window chrome & startup

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 1 | Window title `quantick` | `main.rs:146` | startup | Static title. Never reflects feed/symbol/replay state. |
| 2 | Window icon | `main.rs:136-147` | startup | Bundled `assets/icon.png`. |
| 3 | Initial window size 1100×650 | `main.rs:141` | startup | See F-06 — at this width the toolbar already folds 3 groups. |
| 4 | Minimum window size 900×560 | `main.rs:144` | resize | Floor chosen so the drawing rail doesn't clip (documented). |
| 5 | Config load failure → `process::exit(1)` | `main.rs:95-106` | malformed `QUANTICK_CONFIG`/`quantick.toml` | Logs `CONFIG_ERROR` to stderr and exits **before any window opens**. |
| 6 | Startup-selection failure → `process::exit(1)` | `main.rs:107-115` | bad `QUANTICK_DEFAULT_FEED`/`_SYMBOL` | Same: stderr + exit, no window. |
| 7 | Tracing init | `main.rs:63-88` | startup | stderr; `QUANTICK_LOG_FORMAT=json` for NDJSON. |
| 8 | Theme install | `main.rs:158-161`, `theme.rs:75-107` | startup | Phosphor font + dark tokens applied to egui `Visuals`. One theme only. |

### 1.2 Menu bar (zone 1, 28 px — `app.rs:1755-1928`)

| # | Item | Location | Trigger | Effect |
|---|---|---|---|---|
| 9 | **File → New Tab…** | `app.rs:1776` | click / `Ctrl+T` | Opens `SourcePicker`. |
| 10 | **File → Close Tab** | `app.rs:1784-1798` | click / `Ctrl+W` | Closes active tab; disabled on last tab with explanation. |
| 11 | **File → Layout → Single** | `app.rs:1802` | click | `set_layout(Single)`. |
| 12 | **File → Layout → Time + Flow** | `app.rs:1803` | click | Builds/reveals the time pane (deferred one frame, shows `BarRebuild` spinner). |
| 13 | **File → Market Replay…** | `app.rs:1815-1824` | click / `Ctrl+R` | Opens the replay browser window. |
| 14 | **File → Close Replay** | `app.rs:1825-1831` | click (only rendered while replaying) | Returns to the live feed. Entry vanishes when not replaying. |
| 15 | **File → Exit** | `app.rs:1833` | click | `ViewportCommand::Close`. No confirmation. |
| 16 | **View → Hide/Show panels** | `app.rs:1838-1852` | click / `Ctrl+B` | Toggles the whole dock. Label flips with state. |
| 17 | **View → Drawing toolbar → Left/Right/Top/Bottom** | `app.rs:1853-1868` | click | Re-docks the drawing rail; `selectable_label` marks current. |
| 18 | **View → Hide/Show drawing toolbar** | `app.rs:1869-1877` | click | Toggles the rail. |
| 19 | **View → L2 settings** | `app.rs:1878-1888` | click | `dock.open_tab(L2)`. |
| 20 | **View → Bubble settings** | idem | click | `dock.open_tab(Bubbles)`. |
| 21 | **View → Session** | idem | click | `dock.open_tab(Session)`. |
| 22 | **View → Paper trading** | idem | click | `dock.open_tab(Trading)`. |
| 23 | **View → Perf readings** (checkbox) | `app.rs:1890` | click | Shows/hides fps + frame ms + trade count on the status bar. Default **off** (see F-09). |
| 24 | **View → Timezone → (38 offsets)** | `app.rs:1893-1905` | click | Sets `self.tz`; scroll area capped at 280 px. Duplicate of the status-bar combo (#68). |
| 25 | **Tools → Appearance…** | `app.rs:1908` | click | Opens the candle/canvas style window. |
| 26 | **Help → Replay file format…** | `app.rs:1914` | click | Opens the replay browser **with the format section expanded**. |

### 1.3 Tab strip (shares the menu row — `tabstrip.rs:53-102`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 27 | Tab chip `SYMBOL · venue` | `tabstrip.rs:56-65`; label built in `tab.rs:710-716` | click | Activates that tab. Amber text while that tab is replaying (`tabstrip.rs:59-61`). |
| 28 | Attention dot (amber, 3 px) | `tabstrip.rs:67-74`; predicate `tab.rs:560-564` | background tab reconnecting or holding an `Attention` notice | Painted top-right of the chip. Suppressed on the active tab. |
| 29 | Chip close `×` | `tabstrip.rs:77-91` | hover / active / last-tab | Closes the tab; disabled on the last tab. |
| 30 | `+` button | `tabstrip.rs:94-100` | click / `Ctrl+T` | Opens the source picker. Tooltip "Open another market (Ctrl+T)". |

### 1.4 Source picker dialog (`tabstrip.rs:234-359`, driven by `app.rs:3118-3163`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 31 | Window "Open market" | `tabstrip.rs:243-247` | `Ctrl+T` / `+` / File → New Tab | Non-collapsible, **non-resizable**, no anchor. |
| 32 | Feed combo | `tabstrip.rs:249-266` | click | Sets draft feed; unimplemented providers labelled `"… (soon)"` (`config.rs:60`). |
| 33 | Symbol combo | `tabstrip.rs:272-278` | click | Sets draft symbol; auto-corrected by `ensure_symbol_valid` when the feed changes. |
| 34 | "added here" list + `×` per symbol | `tabstrip.rs:283-316` | click | Removes a user-added symbol from the catalog. Disabled while a tab shows it. |
| 35 | "Add symbol…" field + **Add** | `tabstrip.rs:319-338` | Enter / click | Trimmed, taken verbatim, added to the catalog, persisted to `quantick-symbols.toml`, opened in a new tab. |
| 36 | Refusal line | `tabstrip.rs:341-343`, set from `app.rs:3155-3159` | failed `config.validate()` | Red small text under the field; dialog stays open. |
| 37 | **Open** / **Cancel** | `tabstrip.rs:346-353` | click | Opens the chosen market in a new tab / dismisses. Window `×` also cancels. |

### 1.5 Toolbar (zone 2, 44 px — `toolbar.rs:265-317`)

**SOURCE group** (`toolbar.rs:320-358`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 38 | `feed` label + feed combo | `toolbar.rs:342-349` | click | Writes `tab.feed_id`; feed switch applied later in the frame via `maybe_switch_feed` (`tab.rs:889`) — **resets the chart**. |
| 39 | `symbol` label + symbol combo | `toolbar.rs:350-357` | click | Writes `tab.symbol`; same reset path. |
| 40 | Amber replay label (replaces 38+39) | `toolbar.rs:321-330` | while replaying | `source: <session>` in amber, hover shows file path + side source (`app.rs:896-907`). No combos while replaying — deliberate. |

**BARS group** (`toolbar.rs:362-437`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 41 | `bars` kind combo (Tick/Volume/Dollar/Time/Imbalance) | `toolbar.rs:370-387` | click | Sets `pane.kind`; rebuild deferred to `apply_spec_changes` (`app.rs:2986`) with a `BarRebuild` spinner. Size-based kinds disabled without `traded_volume`, with the reason on hover. |
| 42 | Bar parameter (`N trades` / `units` / `notional` / `interval ms` / `target trades`) | `toolbar.rs:395-437` | drag / type | Sets the spec parameter. Folds into the kind combo as `tick · 50` when collapsed. |

**HISTORY group** (`toolbar.rs:452-477`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 43 | `+ older` button | `toolbar.rs:454-460` | click | `request_older_history()`. Disabled without `history_paging`: "this feed only streams forward…". |
| 44 | Caret `▾` menu → page size drag (500–50 000) | `toolbar.rs:461-477` | drag | Trades pulled per click. |
| 45 | "N trades backfilled so far" | `toolbar.rs:476` | — | Read-only counter inside the caret menu only. |

**TRADE group** (`toolbar.rs:483-510`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 46 | **BUY** (teal, bold) | `toolbar.rs:484-496` | click | Simulated market buy at the Trading tab's quantity. Disabled until the sim has seen a price. |
| 47 | **SELL** (red, bold) | `toolbar.rs:498-509` | click | Mirror of #46. |

**LAYERS group** (right-aligned — `toolbar.rs:515-572`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 48 | Indicators menu (chart-line glyph) | `toolbar.rs:577-660` | click | Accent-coloured when any indicator is active. Contains: Add EMA(9), Add CVD pane, script list, and per-indicator rows with status dot, eye, gear, trash. |
| 49 | Bubbles toggle (three-circles, BUY accent) | `toolbar.rs:518-536` | left / right-click | Toggles the layer / opens the Bubbles dock tab. Disabled without `traded_volume`. |
| 50 | Heatmap toggle (fire, ACCENT) | `toolbar.rs:538-553` | left / right | Toggles display only (capture keeps running) / opens the L2 tab. Disabled without `book_capture`. |
| 51 | Live-strip toggle (wall, ACCENT) | `toolbar.rs:558-571` | left / right | Toggles the strip / opens the **L2** tab (same target as #50 — see F-12). Never capability-gated. |

**LOOK / PANELS / OVERFLOW**

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 52 | Appearance icon (paint brush) | `toolbar.rs:303-309` | click | Toggles the style window; `active()` while open. |
| 53 | Panels icon (sidebar) | `toolbar.rs:294-300` | click | `dock.toggle_visible()`; tooltip names Ctrl+B. |
| 54 | Overflow `⋯` | `toolbar.rs:664-728` | click when anything folded | Holds Appearance, Show/hide panels, Load older + page size, Buy/Sell at market (SIM), bar parameter — in the fixed §6 order. |
| 55 | Collapse planner | `toolbar.rs:127-144` | resize | Folds in order: LOOK → PANELS → HISTORY → TRADE → param → feed-name-to-initial. Symbol and LAYERS never fold. |


### 1.6 Status bar (zone 7, 28 px — `statusbar.rs:203-363`)

**Provenance (left)**

| # | Segment | Location | Content / colour |
|---|---|---|---|
| 56 | State dot (8 px) | `statusbar.rs:227-232` | Faint = connecting, WARN = reconnecting, BUY = live, AMBER = replay (`statusbar.rs:46-53`). |
| 57 | State word | `statusbar.rs:233-237` | `connecting` / `reconnecting` / `live` / `replay`. |
| 58 | Venue name | `statusbar.rs:238` | Feed display name, or the literal `"recording"` while replaying (`app.rs:1686-1690`). |
| 59 | Symbol (monospace) | `statusbar.rs:239-243` | — |
| 60 | Tape cell | `statusbar.rs:255-263`, text in `tape_text` (`statusbar.rs:167-186`) | `arrival 230 ms` / `stale 12 s` (age > 10 s, `metrics.rs:27`) / `10× 45%` replaying / `arrival —`. Turns WARN past `HIGH_LAG_MS` (5 s) or when stale. **No tooltip anywhere on this cell.** |

**Content (middle)**

| # | Segment | Location | Content |
|---|---|---|---|
| 61 | Bar spec | `statusbar.rs:268-272` | e.g. `tick(50)`. |
| 62 | Bar progress | `statusbar.rs:273-280` | e.g. `37/50 ticks`, amber, hover "how far the forming bar is from closing". |
| 63 | Bar counts | `statusbar.rs:281-289`, `bars_text:190-199` | `240+61 bars`, or `26000v+240+61 bars` with a venue prefix. **No tooltip explaining the split.** |
| 64 | Honesty label | `statusbar.rs:290-296`; source `tab.rs:1332-1359` | Amber: `side: inferred (tick rule)`, `side: not recorded`, `prints: quote-derived`. Hover carries the full disclosure. |
| 65 | Sim P&L cell | `statusbar.rs:297-308` | `SIM ±N pts`, coloured by sign, hover "…simulated fills, not a broker account". Absent until the simulator is touched. |
| 66 | "history · double-click for live" | `statusbar.rs:309-315` | TEXT_FAINT, only when the viewport is detached. |
| 67 | "price: manual · double-click the axis to auto-fit" | `statusbar.rs:316-322` | TEXT_FAINT, only when the price axis is manual. |

**Machinery (right, right-to-left)**

| # | Segment | Location | Content |
|---|---|---|---|
| 68 | Timezone combo | `statusbar.rs:328-334` | 38 fixed offsets; default UTC−03:00 (`timezone.rs:88-92`). The bar's only control. |
| 69 | Clock glyph | `statusbar.rs:335` | Decoration for #68. |
| 70 | fps · frame ms · cpu ms | `statusbar.rs:347-356` | Only when `show_perf`; WARN past `SLOW_FRAME_MS` (20 ms). |
| 71 | `N trades` | `statusbar.rs:358-362` | Only when `show_perf`. |

### 1.7 Notice card (`notice_card.rs`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 72 | Card body (420 px, clamped to chart) | `notice_card.rs:164-219` | `should_draw` (`notice_card.rs:72-78`) | `Attention` → always. `Working`/`Reconnecting` → only while `bars == 0`. `Connected`/`Clear` → never. |
| 73 | Severity dot + border | `notice_card.rs:181-197` | — | AMBER for `Attention`, TEXT_MUTED for progress. |
| 74 | Headline + next step | `notice_card.rs:199-210` | — | Wrapped to the clamped width; step only exists on `Attention`. |
| 75 | **Try again** button | `notice_card.rs:214-218` → `app.rs:3038-3042` → `tab.restart_feed` (`tab.rs:1427`) | click | Respawns the live feed and restarts book capture. Present only on `Attention`. |
| 76 | Actual notice texts | `feed/mt5_bridge.rs:227-232, 365-371, 474-479, 493-499`; `feed/metatrader.rs:192-195, 256-258, 368, 487-493` | — | MetaTrader path is rich and actionable. **Binance/Hyperliquid only ever emit `Working`/`Reconnecting`/`Connected`** (`feed/binance.rs:154-161`, `feed/hyperliquid.rs:141-148`). |

### 1.8 Loading (`loading.rs`)

| # | Element | Location | Trigger | Label |
|---|---|---|---|---|
| 77 | `History` | `loading.rs:45-47, 67` | backfill / load-older / source reset | "loading history…" |
| 78 | `BarRebuild` | `loading.rs:48-49` | bar kind/param change, time-pane build | "rebuilding bars…" |
| 79 | `BookSync` | `loading.rs:50-51`; mirrored `app.rs:2998` | book connecting/buffering/resyncing | "syncing order book…" |
| 80 | `ReplaySession` | `loading.rs:52-53`; mirrored `app.rs:2997` | session parsing on a worker | "loading replay session…" |
| 81 | `VenueHistory` | `loading.rs:54-55` | candle history in flight | "loading venue history…" |
| 82 | Overlay backdrop | `loading.rs:166-212` | any active task | Stacked rows, amber spinners, centred at the top of the chart, 150/255 black backdrop. |
| 83 | `inline()` spinner | `loading.rs:158-161` | embedded | Used by the replay browser for the folder dialog and session load. |
| 84 | Counting semantics | `loading.rs:103-127` | — | `begin`/`end` counted; `restart` collapses to 1; `set_active` level-triggered. Saturating — cannot underflow into "loading forever". |

### 1.9 Replay browser (`replay_view.rs:280-540`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 85 | Window "Market Replay" | `replay_view.rs:287-293` | `Ctrl+R` / File / Session tab / Help | Centre-anchored, resizable, 560×460. |
| 86 | Folder text field | `replay_view.rs:320-327` | Enter | Rescans. Seeded from `QUANTICK_REPLAY_DIR`. |
| 87 | **Browse…** | `replay_view.rs:329-331` | click | Native folder dialog on a worker thread (`replay_view.rs:235-252`) — never blocks a frame. |
| 88 | Refresh `↻` | `replay_view.rs:332-338` | click | Rescans. |
| 89 | "waiting for the folder dialog" spinner | `replay_view.rs:340-344` | while the picker is out | Inline loading row. |
| 90 | Session list | `replay_view.rs:349-419` | click / double-click | Selects / selects+loads. Shows label + size + per-entry notes. |
| 91 | Empty states | `replay_view.rs:359-383` | no library / no sessions | "Choose the folder holding your recorded sessions." / "No sessions in this folder." + format hint. |
| 92 | Problems collapsing header | `replay_view.rs:421-457` | rejected files | Per-file subject + detail + advice; auto-opens when no session loaded. |
| 93 | Format help link | `replay_view.rs:459-491` | click | Toggles a scrollable monospace dump of `format::FORMAT_HELP`. |
| 94 | Speed chips (Start at) | `replay_view.rs:508-515` | click | Sets the opening speed. |
| 95 | "and play" checkbox | `replay_view.rs:517-518` | click | Autoplay vs open paused. |
| 96 | **Play session** (amber fill) | `replay_view.rs:525-536` | click | Loads on a worker; disabled with "Pick a session from the list first". |
| 97 | Error frame | `replay_view.rs:496-505` | parse failure | Red-tinted panel with `{e}\n{advice}`. |

### 1.10 Replay transport (30 px, above the status bar — `replay_view.rs:544-633`)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 98 | Skip-back | `replay_view.rs:560-566` | click | `Restart`. Tooltip "Back to the first print". |
| 99 | Play/Pause (amber glyph) | `replay_view.rs:568-582` | click / **Space** | Reads live status before deciding, so a click can't invert a stale state. Space only when nothing has focus (`replay_view.rs:629-631`). |
| 100 | `REPLAY` badge | `replay_view.rs:584, 655-668` | — | Amber pill, black text. |
| 101 | Session label | `replay_view.rs:585` | — | — |
| 102 | Session clock `HH:MM:SS` | `replay_view.rs:586-590`, `clock_text:725-728` | — | In the **session's own timezone**, not the app's `tz` (see F-13). |
| 103 | Speed chips | `replay_view.rs:592-597` | click | `SetSpeed`. |
| 104 | Seek track | `replay_view.rs:618-622, 677-721` | drag / click | Emits **one** seek per gesture on release. |
| 105 | `N / M prints` | `replay_view.rs:607-615` | — | Space-grouped thousands (`thousands:735-745`). |
| 106 | Close `×` | `replay_view.rs:600-606` | click | Back to the live feed. |

### 1.11 Dock, theming, shared widgets

| # | Element | Location | Notes |
|---|---|---|---|
| 107 | 36 px dock tab strip (L2, Bubbles, Session, Trading) | `dock.rs:173-192` | Always visible when the dock is visible; clicking the active tab collapses. |
| 108 | Dock body, width remembered per tab | `dock.rs:194-220, 162-164` | Clamped 280–360 px. |
| 109 | Session tab | `dock.rs:250-290` | Duplicates the replay entry point (#13) and Close Replay (#14). |
| 110 | Design tokens | `theme.rs:20-52` | CANVAS/CHROME/INSET/CONTROL/BORDER/TEXT×3/BUY/SELL/ACCENT/AMBER/WARN/TAG_BG/TEXT_SUPPORT. Amber reserved for provenance honesty. |
| 111 | Tooltip delay 350 ms | `theme.rs:79` | Prevents a trail of labels on a rail sweep. |
| 112 | `IconButton` four-state grammar | `widgets.rs:106-141, 217-267` | disabled > pressed > active > hover > idle; disabled = 40 % opacity **plus** a hover explanation; focus ring on top. |
| 113 | Appearance window "candle appearance" | `candle_view.rs:115-164` | Presets, live preview, body/outline/wick/live/canvas sections, "restore Order flow defaults", footer promising redraw-only. |

## 2. FLOWS

### (a) First launch

1. `config::load()` → env override, then `./quantick.toml`, then the compiled-in `feeds.toml`. **Any parse error kills the process before a window exists** (`main.rs:95-115`).
2. The live feed spawns **before** the window (`main.rs:134`), so backfill is already in flight at first paint.
3. First frame: menu bar + one tab chip `BTCUSDT · Binance` + toolbar + empty canvas + status bar reading `● connecting  Binance  BTCUSDT  arrival —`, `tick(50)`, `0+0 bars`.
4. Because `bars == 0`, the notice card shows `Working { "connecting to Binance" }` — headline only, no button.
5. Backfill lands as one `Backfilled` batch; the overlay shows "loading history…" while it is out. `FeedNotice::Connected` flips the dot to green and clears the card.
6. **Nothing about window state persists.** eframe is built with `default-features = false` and no `persistence` feature (`crates/app/Cargo.toml:29-34`), and `impl eframe::App` (`app.rs:2860`) defines only `update` — no `save`. Window size/position, dock state, open tabs, timezone, perf toggle and candle style all reset every launch. Only indicator state, added symbols, bubble presets and drawing presets have their own sidecar files.

### (b) Switching feed and symbol

- Two independent combos in the toolbar (#38, #39). Writing either mutates `tab.feed_id`/`tab.symbol` immediately; `maybe_switch_feed` (`tab.rs:889-935`) runs later in the same frame, respawns the feed and resets every pane.
- Picking a feed that doesn't offer the current symbol silently retargets it: `ensure_symbol_valid` (`app.rs:969-973`, `tab.rs:720-724`) snaps to the feed's first symbol. **No confirmation, no undo** — and the reset discards loaded history and drawings (`note_overlay_cleared`).
- During replay the combos are replaced by the amber session label, so a stale selection can't respawn a live feed under the recording.
- The picker (#31-37) is the *other* path, and it opens a **new tab** instead of switching — two different mental models for "choose a market".

### (c) Starting a replay session

`Ctrl+R` / File → Market Replay… / dock Session tab → browser window → folder (env-seeded, typed, or **Browse…**) → scan (`library::scan`) → list + a collapsible "N file(s) were not loaded" with per-file advice → pick speed + autoplay → **Play session** → parse on a worker (inline spinner + `ReplaySession` overlay row) → on success the window closes and `ReplayAction::Open` fires; on failure the window stays open with `{error}\n{advice}` in a red frame.

Once playing: the SOURCE group becomes the amber session label, the status dot turns amber, the tape cell becomes `10× 45%`, a 30 px transport appears above the status bar, and the tab chip's label turns amber. Exit via the transport `×`, File → Close Replay, or the Session tab's Close replay.

### (d) Connection health and data honesty

Four independent surfaces, and they genuinely say different things:

- **Dot + word** — transport state, driven by explicit `Connected`/`Reconnecting` notices rather than inferred from trade arrival (`feed/mod.rs:296-309`).
- **Tape cell** — `arrival` is a frozen observation from when the newest print landed; `stale N s` is wall-clock minus the newest event's timestamp, recomputed every frame. This is the only thing that catches a socket that stays open and delivers nothing (`statusbar.rs:157-186`, `metrics.rs:18-27`).
- **Honesty label** — amber, in the middle section: `prints: quote-derived` beats `side: inferred` (`tab.rs:1328-1352`).
- **Notice card** — the only surface that says *why* and offers a fix, and only MetaTrader populates it.

Capability gating is consistently by capability, never by provider name: `traded_volume` disables the volume/dollar bar kinds and the bubble layer; `book_capture` disables the heatmap; `history_paging` disables `+ older`. Every disabled control keeps its glyph at 40 % and explains itself on hover.

### (e) Settings / preferences

There is no preferences surface. Settings are scattered across: the appearance window (candles/canvas), the dock tabs (L2, bubbles, trading), the View menu (perf, timezone, rail dock), the status-bar timezone combo, and **~19 environment variables** (`QUANTICK_CONFIG`, `_DEFAULT_FEED`, `_DEFAULT_SYMBOL`, `_BACKFILL`, `_BOOK_DEPTH`, `_REPLAY_DIR`, `_REPLAY_AUTOSTART`, `_REPLAY_SPEED`, `_SYMBOLS`, `_INDICATORS_DIR`, `_INDICATORS_STATE`, `_INDICATORS_AUTOSTART`, `_INDICATOR_SCRIPTS_AUTOSTART`, `_BUBBLES`, `_BOOK_AUTOSTART`, `_LIVE_STRIP_AUTOSTART`, `_BUBBLES_AUTOSTART`, `_DRAWING_PRESETS`, `_TRADES_DIR`, `_LOG_FORMAT`). None are discoverable from the UI.


## 3. UX FINDINGS

### Blocker

**F-01 · A bad config file kills the app with no visible message.** `main.rs:95-115` — both `config::load()` and `apply_startup_selection_from_env()` log to stderr and `process::exit(1)` before `run_native`. Launched from a desktop shortcut or a file-manager double-click, the app flashes nothing and dies; the user has no way to learn that `quantick.toml` line 14 is malformed. The app already owns exactly the right widget for this — `FeedNotice::Attention` + the notice card's "Try again". *Heuristic: visibility of system status; help users recognize and recover from errors.*

**F-02 · Binance and Hyperliquid can never ask for help.** `feed/binance.rs:154-161` and `feed/hyperliquid.rs:141-148` only ever call `connection_notice`, which yields `Working`/`Reconnecting`/`Connected` (`feed/mod.rs:296-309`). A wrong symbol, a geo-blocked endpoint, a DNS failure or a 451 all present identically: `● reconnecting` forever, a `Reconnecting` card that disappears the moment one bar exists, and **no Try again button** (the button only exists on `Attention`, `notice_card.rs:148-151`). *Heuristic: error recovery; help users diagnose.*

### Major

**F-03 · Changing feed or symbol silently destroys the chart, with no warning and no undo.** `toolbar.rs:343-357` writes straight through `&mut` borrows; `tab.rs:889-935` then respawns the feed and resets every pane, discarding loaded history and clearing drawings (`app.rs:2986-2989`). Two adjacent combos in the busiest part of the toolbar, one click apart, both destructive. *Heuristic: user control and freedom; error prevention.*

**F-04 · Changing the feed can silently change the symbol.** `ensure_symbol_valid` (`app.rs:969-973`) snaps the symbol to the feed's first entry when the new feed doesn't offer the current one. Switching from `metatrader-b3 / WINQ26` to Binance lands you on `BTCUSDT` with no acknowledgement. *Heuristic: visibility of system status.*

**F-05 · The status bar has no overflow strategy.** `statusbar.rs:211-221`: a single `horizontal_centered` row. The middle section can carry up to seven items simultaneously (#61-67), including a 46-character hint. The module's own comment concedes the risk (`statusbar.rs:291-293`) but nothing enforces it — no eliding, no clipping, no priority order, no equivalent of the toolbar's `collapse_plan`. At 900 px with a detached viewport, a manual price axis, a sim position and perf readings on, the timezone combo is the first thing to go. *Heuristic: consistency; aesthetic and minimalist design.*

**F-06 · The shipped default window size already folds three toolbar groups.** `CollapsePlan::FULL.width()` = **1172 px** (`toolbar.rs:35-51, 97-120`), against a default inner width of 1100 (`main.rs:141`) minus 16 px margin = 1084 px available. The planner folds LOOK, then PANELS, then HISTORY before it fits. Out of the box, Appearance, Show/hide panels and Load older all live behind `⋯`. At the 900 px minimum, TRADE and the bar parameter fold too. Nobody sees the designed toolbar without resizing first. *Heuristic: recognition rather than recall.*

**F-07 · Two different picker idioms for the same job.** The toolbar has bare feed+symbol combos that *switch the current tab* (destructively). The `+` picker has labelled combos, an add-symbol field, a remove list, and Open/Cancel — and it *opens a new tab*. Same catalog, same two fields, opposite outcomes, no cross-reference. The picker is also the **only** place a symbol can be added or removed. *Heuristic: consistency and standards.*

### Minor

**F-08 · The tape cell has no tooltip.** `statusbar.rs:255-263` — `arrival 230 ms`, `stale 12 s` and `10× 45%` all render in the same slot with no hover text. Same for the bar-count cell (#63): `26000v+240+61 bars` is unexplained.

**F-09 · Perf readings default off, and the toggle is buried.** `View → Perf readings` (`app.rs:1890`) is the only path. The app logs `APP_SLOW_FRAMES` when frames exceed 20 ms (`app.rs:1601-1614`), so the app knows it is struggling while the user sees nothing on screen.

**F-10 · Duplicate timezone pickers with no shared affordance.** `app.rs:1893-1905` (scrolling menu) and `statusbar.rs:328-334` (combo) both enumerate the same 38 offsets. Neither is searchable; neither shows city names.

**F-11 · Replay has four entry points and three exits.** Entries: `Ctrl+R`, File → Market Replay…, Session dock tab, Help → Replay file format…. Exits: transport `×`, File → Close Replay, Session tab → Close replay. Defensible for discoverability, but the Session dock tab is almost entirely a shortcut to a window that already has a global shortcut.

**F-12 · The live-strip toggle right-clicks into the L2 tab.** `toolbar.rs:570` — same target as the heatmap toggle (`toolbar.rs:552`). The tooltip promises "right-click for settings", and the tab that opens is titled `L2 · LIQUIDITY MAP`.

**F-13 · The replay clock and the chart's time axis can disagree.** `replay_view.rs:725-728` renders the transport clock in the session's own recorded timezone, while the time axis uses the app-wide `tz` (default UTC−03:00). Two visible clocks showing different times for the same instant, with nothing labelling either.

**F-14 · The `+ older` counter is hidden behind a caret menu.** `toolbar.rs:476` — "N trades backfilled so far" only appears after opening the `▾` menu.

**F-15 · The window title never changes.** `main.rs:146` — always `quantick`. With multiple tabs, an alt-tab or taskbar preview says nothing about which market, which venue, or whether a recording is playing.

**F-16 · No keyboard shortcut reference and no About.** Help contains one item. Shortcuts are discoverable only via individual tooltips or menu items, and several — Space for play/pause, double-click to re-follow live — appear in no menu at all.

### Nit

**F-17 · Window title casing is inconsistent.** `"Market Replay"`, `"Open market"`, `"candle appearance"`, `"Drawn objects"` — four windows, three casing conventions.
**F-18 · Two different close glyphs.** Dock header and tab chips use Latin-1 `×`; the replay transport uses Phosphor `icons::X`.
**F-19 · Mixed hyphen conventions in copy.** Tooltips use ASCII hyphens while notices use em/en dashes.
**F-20 · Toolbar labels are lowercase, menu items are sentence case.** `feed`, `symbol`, `bars` against `New Tab…`, `Market Replay…`; the `⋯` menu mixes both.

## 4. QUICK WINS vs STRUCTURAL

### Quick wins

1. **Show config errors in a window** (F-01). Instead of `process::exit(1)`, launch `run_native` with a minimal error viewport carrying the parse error and the resolved config path. ~30 lines in `main.rs`.
2. **Give Binance and Hyperliquid an `Attention` path** (F-02). The variant, the card and the Try-again wiring all already exist; the reconnect loops just need a retry-count threshold that upgrades `Reconnecting` to `Attention` with the last transport error and a next step. Mirror `mt5_bridge.rs:474-479`.
3. **Tooltip the tape and bar-count cells** (F-08). Two `.on_hover_text()` calls; the prose already exists verbatim in the source comments.
4. **Widen the default window to ~1200 px, or re-measure the width constants** (F-06). Add a test pinning `CollapsePlan::FULL.width() <= default_inner_width - 16.0`.
5. **Move "N trades backfilled" out of the caret menu** (F-14).
6. **Live window title** (F-15): `SYMBOL · venue — quantick`, plus a `REPLAY` prefix, via `ViewportCommand::Title` whenever the chip label changes (`tab.rs:710-716` is already the one place that recomposes it).
7. **Fix the live-strip right-click target** (F-12) — either a dedicated dock tab or a tooltip that names L2 honestly.
8. **Help → Keyboard shortcuts** (F-16): a static window listing the ten bindings already defined in `app.rs:1729-1748`.
9. **Copy pass** (F-17/19/20): one casing convention for window titles, em dashes everywhere.

### Structural

1. **A collapse plan for the status bar** (F-05). Port the `CollapsePlan` idea from `toolbar.rs:97-144`: rank the middle-section items by importance (spec > honesty label > counts > progress > sim P&L > navigation hints), measure, and drop the tail into a `⋯` hover or elide it. Contained entirely in `statusbar.rs`.
2. **Make market switching non-destructive, or make it ask** (F-03/F-04). Options: (a) confirm before a switch that would discard history and drawings, with the count in the prompt; (b) unify on the picker — toolbar combos open a new tab like `+`, in-place switching behind an explicit action. Option (b) also resolves F-07.
3. **UI-state persistence.** Enabling eframe's `persistence` feature + `save()` covers window geometry; a `ui-state.toml` sidecar — the pattern the codebase already uses four times — covers open tabs, dock state, timezone, perf toggle and candle style. `app.rs:384` already carries the §14 marker.
4. **One settings surface.** ~19 env vars and no preferences dialog. A Tools → Settings window grouping data (backfill depth, book depth, replay folder), paths (indicators dir, trades dir, symbols file) and display (timezone, perf) — reading/writing the same sidecar as #3.
5. **Unify the source-selection model** (F-07). One picker component used by both the toolbar (as a popup) and `+`, with the mode (switch this tab / open a new tab) as an explicit choice inside it.

## 5. What is genuinely strong (don't regress it)

Capability gating is by capability rather than provider name throughout, and every gated control stays visible at 40 % opacity with a written reason instead of disappearing (`widgets.rs:106-141`). The arrival-vs-staleness split in the tape cell is a real insight most trading apps get wrong. The notice card's "never cover a working chart" rule is enforced by a tested predicate (`notice_card.rs:72-78`). Amber is disciplined as a provenance-only token and never used decoratively. The replay seek commits once per gesture for a stated performance reason. And every chrome module ships an off-screen layout test that catches duplicated widget ids and panicking widgets before they reach the chart.

