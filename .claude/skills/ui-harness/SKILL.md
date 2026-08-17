---
name: ui-harness
description: How an agent drives and observes the quantick desktop app without a human clicking — env-var hooks to reach every UI surface, the screenshot capture workflow, and the rule that every new surface must register a hook. Use when launching the app for validation, capturing screenshots, adding a new UI surface, or when another skill (visual-qa, trader-ux-review) needs to see the app.
---

# UI harness — drive the app without a mouse

The contract that makes autonomous visual work possible:

> **Every user-visible surface (panel, layer, tab, popup, demo flow) must be
> reachable from a fresh launch via environment hooks alone — zero clicks.**

A PR that adds a surface without a hook leaves that surface untestable by
agents; that is a Should-fix in review. Hooks follow the existing family:
`QUANTICK_<SURFACE>_AUTOSTART=1` reuses the exact code path of the manual
toggle — never a parallel activation path — and defaults to off, so a hook
never changes behaviour for a user who did not set it.

## Hook registry

Verified on `main` (grep `env::var` in `crates/app/src` to re-verify — code
is the source of truth, this table is the index):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_CONFIG=<toml>` | full feed/symbol config override (use a scratchpad toml; **never** edit the user's root `quantick.toml`) |
| `QUANTICK_DEFAULT_FEED` / `QUANTICK_DEFAULT_SYMBOL` | startup feed/symbol |
| `QUANTICK_WINDOW_SIZE=WxH` | the size the window opens at, floored at the app's own 900x560 minimum. Window size decides whether the indicator band has room for its panes and the time axis for its labels, so without this that whole class of defect is invisible to anything but a human dragging a corner. With it plus `QUANTICK_INDICATORS_AUTOSTART`, the collapsed-pane strip is reachable from a fresh launch. |
| `QUANTICK_BOOK_AUTOSTART=1` | L2 heatmap layer |
| `QUANTICK_LIVE_STRIP_AUTOSTART=1` | live strip |
| `QUANTICK_BUBBLES_AUTOSTART=1` | aggression layer (bubbles + live-column footprint) |
| `QUANTICK_FOOTPRINT_AUTOSTART=1` | candle footprint layer (per-price sell×buy ladder in the candles; detail follows zoom — pair with `QUANTICK_CANDLE_WIDTH` to reach each level) |
| `QUANTICK_CANDLE_WIDTH=<px>` | the zoom, scripted: **pixels per bar**, clamped to the gesture's own 1–160 bounds. One bar is one candle at every zoom (the trust law — nothing ever groups), so `1` is the deepest squeeze: a 1600 px window holds 1600 bars. Footprint LOD by candle width: ≥63 Detailed, 33–63 Compact, 10–33 Profile, 6–10 Marks, <6 Off — all four scaled by the panel's `detail_scale` |
| `QUANTICK_PAN_PX=<px>` | a scripted drag on the candles, re-applied every frame: negative pushes the chart left (`-9000` = as far into the projection margin as it goes, i.e. a whole window of empty canvas to draw a channel or a Fibonacci extension into), positive walks back into history. The margin and the way back are states no screenshot can otherwise reach |
| `QUANTICK_FOOTPRINT_PANEL=1` | the footprint settings window open at launch (style, band fineness, **the detail-zoom slider**, imbalance thresholds, POC/badges) |
| `QUANTICK_STYLE_PANEL=1` | the appearance dialog open at launch (candle presets, body mode and colours, **the gap between candles**, outline, wicks, canvas). Behind the toolbar's LOOK button, which a scripted run cannot press |
| `QUANTICK_FOOTPRINT_SETTINGS=<toml>` / `QUANTICK_FOOTPRINT_PRESETS=<toml>` | where the footprint's saved knobs and named presets live. **Always point these at scratchpad files.** Without them a validation run reads — and, the moment it touches a knob, overwrites — the trader's real setups, the same rule `QUANTICK_UI_STATE` carries. |
| `QUANTICK_FOOTPRINT_DEBUG=1` | appends the layer's own inputs to its legend (`[w<candle px> row<base row px> g<capture group> lvl<level> n<ladders>]`). The zoom-boundary bugs were all invisible from outside — this is the chart telling you which number it decided on |
| `QUANTICK_INDICATORS_AUTOSTART` / `QUANTICK_INDICATOR_SCRIPTS_AUTOSTART` | indicator panes / pine scripts |
| `QUANTICK_INDICATOR_SETTINGS=<index>[:<tab>]` | the indicator settings dialog open at launch, on the indicator at `index` among the focused pane's views in add order, and on `inputs` (default) or `style`. Pair with `QUANTICK_INDICATORS_AUTOSTART=1` — `0:inputs` reaches the EMA's parameters, `1:style` the CVD pane's per-plot colour and width. Nonsense is refused rather than guessed, so a typo yields no dialog instead of a capture of the wrong tab. In the app this dialog is opened by double-clicking the indicator — the legend row, the pane header, a collapsed pane's strip, or the plotted line itself — none of which a scripted run can perform. |
| `QUANTICK_REPLAY_DIR` + `QUANTICK_REPLAY_AUTOSTART=1` + `QUANTICK_REPLAY_SPEED` | recorded session playback (deterministic tape → deterministic screen). The folder is **per-run**: without the hook the browser opens on the trader's stored pick, else `Documents/Quantick/replay`, and a run under the hook never rewrites that pick — so pointing a validation run at a scratch folder is safe, unlike the workspace and paper-state files beside it |
| `QUANTICK_REPLAY_BROWSER=1` | the Market Replay browser open on **My sessions** — the list of recordings already on disk, with the folder row above it. The window is one menu entry (or Ctrl+R) deep, so this is the only way a capture reaches it |
| `QUANTICK_REPLAY_GET_DATA` | the browser open on its **Get data** tab. `1` takes the chart's own instrument — what clicking the tab does, look-up included — and any other value states the contract outright (`WINQ26`). Both reach the calendar with no clicks; both need a MetaTrader terminal to answer, so the frame is a spinner or a refusal without one |
| `QUANTICK_BUBBLES=<bubbles.toml>` | bubble preset override without touching tracked config |
| `QUANTICK_CHART_LAYERS` | chart layer visibility set: a `version = 1` TOML with a `[layers]` table keyed by layer id (`heatmap`, `bubbles`, `footprint`, `live_strip`, `flow_legend`, `book_status`, `depth_gaps`, `grid`, `last_price`, `crosshair`, `paper_trading`, `trade_paint`, `drawings`, …). This is also how the canvas *chrome* is reached with no clicks — `flow_legend = false` silences the top-left key, `book_status = false` the top-right badge — and `bubbles = false` with `live_strip = true` is the state that used to blank the strip. **Point it at a scratchpad file**: the app writes this file back whenever a switch flips. |
| `QUANTICK_UI_STATE=<toml>` | the saved workspace — the tab strip, each tab's layout/split/focus/bar specs, the dock, the rail, the timezone, the window size. **Always point this at a scratchpad file.** Without it a validation run reads the user's real `ui-state.toml` and overwrites it: the run both inherits yesterday's cockpit and destroys it. Not only on exit — the standing choices in this file are written the moment they are made, so a single click on a rail star or on a replay folder is already a write, autosave off or not. Point it at a path that does not exist to force the configured default; write one by hand to open on an exact arrangement. |
| `QUANTICK_WORKSPACE_SAVE=1` | takes `Workspace → Save workspace` at startup, through the menu entry's own path — the save really happens, so the status-line confirmation is on screen to capture. Pair with `QUANTICK_UI_STATE` pointed at a scratchpad. |
| `QUANTICK_MENU=workspace` | the Workspace menu open on the first drawn frame — the only door to Save, Save as, Export, Open from file, Open recent and Show where it's saved. A menu is a popup egui owns, so there is no state to set that would not be a second way of opening it: the hook delivers the click on the button's own rect, through the app's input path (`raw_input_hook`), exactly as `QUANTICK_CONTEXT_MENU` does. Anything but `workspace` opens nothing rather than the wrong menu. |
| `QUANTICK_WORKSPACE_EXPORT=<path>` | takes `Workspace → Export to file…` at startup, writing the whole cockpit — tabs, chrome, indicators, layers, drawing colours, footprint, added symbols — to `<path>` as one bundle. The path is *given* rather than picked because the OS file dialog is the one thing a scripted run cannot drive; everything past it is the menu entry's own code, so the status-line confirmation is on screen to capture. Point it at a scratchpad path. |
| `QUANTICK_WORKSPACE_IMPORT=<path>` | the same for `Workspace → Open from file…`: really replaces the cockpit from `<path>`, so **point every store env var at scratchpad files first** or the run rewrites the trader's real setup. A bundle that fails its check changes nothing and puts the reason on the status line — which is how the refusal path is captured. |
| `QUANTICK_UI_STATE` and its seven siblings | the cockpit stores now live in `Documents/Quantick/` rather than beside the launch directory (`crate::store_home`), so an **unhooked run reads and rewrites the trader's real cockpit** — not just an empty file in the repo. The full set to point at scratch: `QUANTICK_UI_STATE`, `QUANTICK_INDICATORS_STATE`, `QUANTICK_INDICATOR_PRESETS`, `QUANTICK_CHART_LAYERS`, `QUANTICK_DRAWING_PRESETS`, `QUANTICK_FOOTPRINT_SETTINGS`, `QUANTICK_FOOTPRINT_PRESETS`, `QUANTICK_SYMBOLS`. A store under its own env var is also skipped by the startup rescue, so a QA scratch file is never copied into the home. |
| `QUANTICK_BACKFILL` / `QUANTICK_BOOK_DEPTH` | history paging / depth size |
| `QUANTICK_TRADES_DIR` | paper-trading journal location (point at scratch — and note the absent-default is now the user's documents folder, `Documents/Quantick/paper-trades`, so an unhooked run touches real history) |
| `QUANTICK_PAPER_STATE=<toml>` | the paper sidecar: the picked journal folder and the cmd-trading settings. **Point at a scratchpad file** — an unhooked run reads and rewrites the trader's real `paper-state.toml`, and startup consolidation may clear their stored folder pick. |
| `QUANTICK_DOCK_TAB=<l2\|bubbles\|session\|trading\|trades>` | the dock open on that tab — `trading` is the ticket (with the CMD TRADING block), `trades` the ledger |
| `QUANTICK_PAPER_REPORT_AUTOSTART=1` | the Simulated performance window (Source/Period filter rows, typed period, import button) |
| `QUANTICK_PAPER_DEMO=1` | scripted deterministic trade sequence — real journaled trades for every paper surface; pair with `QUANTICK_TRADES_DIR` at scratch |

Landing with the drawing-toolbar goal (`feat/drawing-toolbar-pro`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_DRAWING_TOOL=<tool id>` | opens with that tool armed — any id in `DRAWING_TOOLS` (`trend-line`, `ray`, `measure`, `text`, …) |
| `QUANTICK_DRAWING_MAGNET=1` | the magnet on (anchors snap to the bar's OHLC) |
| `QUANTICK_TOOL_FAVORITES=<tool ids>` | comma-separated tool ids pinned as rail favorites — the starred section at the tool end of the rail, one button per id in the given order (`parallel-channel,fib-retracement`). Same restore path as the workspace file, and restoring writes nothing back — but a star *clicked* during the run is written to `QUANTICK_UI_STATE` on that frame, so point that at a scratchpad before driving the flyout |
| `QUANTICK_TOOLBOX_DOCK=<left\|top\|bottom>` | docks the rail against that edge, so the horizontal band and its left/right chevrons are reachable without editing the workspace file |
| `QUANTICK_TOOLBAR_SCROLL=<px\|end>` | parks the scrolling tool band at that offset (`end` = the far end). Only mid-travel shows both chevrons live at once, and a screenshot cannot click an arrow to get there. Pair with a short `QUANTICK_WINDOW_SIZE` — the band only exists between 489 and 633 px of rail extent (`docs/drawing-toolbar-ux.md` §2.8) — and with `QUANTICK_TOOL_FAVORITES` to reach the state where pins spill into the band |
| `QUANTICK_TOOLBOX_FLYOUT=<family id>` | that family's flyout open on the first frame (`lines`, `fib`, `marks`, `brush`, `shapes`, `measure`) — the rows with their favorite stars, the surface where pinning and unpinning happen |
| `QUANTICK_DRAWINGS_DEMO=1` | one of every registered drawing placed on the flow pane once it has bars, the last one selected so the inspector is on screen too |
| `QUANTICK_DRAWINGS_DEMO=bands` | the same set, plus a level on each indicator pane's own value and a diagonal across it — the band projection under test. Pair with `QUANTICK_INDICATORS_AUTOSTART=1` |
| `QUANTICK_DRAWINGS_DEMO_SHARED=1` | those demo objects marked "show on all charts" — pair with a split layout to see the cross-pane projection (which is now also where they can be grabbed, moved and deleted) |
| `QUANTICK_DRAWINGS_DEMO_RECUT=1` | re-cuts the bars under the demo objects after placing them, and adds one mark anchored before the loaded history — the two states a timeframe switch produces: every mark re-anchored onto the new bars, and one faded "off series" |
| `QUANTICK_DRAWINGS_MANAGER=1` | opens the object manager, which is where the "off series" and "other market" badges are read — and the only place a mark clamped off the visible window can be found at all |
| `QUANTICK_DRAWINGS_DEMO_SELECT=<tool id>` | selects that tool's demo object and centres the viewport on it. Selection is what puts an object's handles on screen, so this is the only way to photograph the grab points of a tool that is not last in the registry (`parallel-channel` for its corner and rail handles) |
| `QUANTICK_FRVP_DEMO=1` | one fixed-range volume profile placed on the flow pane once it has bars. When the pane carries a venue history prefix the range straddles the seam, so the partial-coverage label ("profile from N of M bars") is on screen — the honesty surface this hook photographs |
| `QUANTICK_FRVP_DEMO=compare` | two adjacent profiles over the same stretch of liquidity map, one per over-heatmap mode (outline vs always-fill) — the silhouette decision's before/after in a single frame |

Landing with the strategy anchors goal (`feat/strategy-anchors`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_STRATEGY_DEMO=1` | a named rectangle over the recent tape (spanning past the newest bar) with a force-bar strategy armed on it — the on-chart state badge, in `armed`. The rectangle covers the chart's middle on purpose: pair with `QUANTICK_CONTEXT_MENU=chart` and the scripted right-click lands *on* it, opening the per-drawing menu (name, rename, the strategy seat, lock/hide/delete) instead of the bare layer menu |
| `QUANTICK_STRATEGY_DEMO=popup` | the same rectangle with the **arming dialog** open over it — preset picker, side, quantity, the force band, the projection multipliers, re-arm, save-preset. The form is the surface a screenshot of "how do I configure the bot" needs |
| `QUANTICK_STRATEGY_PRESETS=<path>` | relocates the strategy bank (`quantick-strategies.toml`), so a validation run seeds or inspects presets without touching the trader's own bank |

Once merged, move these into the table above.

Landing with the progressive venue-history goal
(`feat/progressive-venue-history`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_VENUE_HISTORY_DEMO=1` | a venue candle prefix in front of the bars cut from prints, delivered through the feed's own reply path — the finished seam divider, deterministic and with no venue involved. The seam otherwise needs a real venue to serve a real quarter of history, which no scripted capture can wait on |
| `QUANTICK_VENUE_HISTORY_DEMO=partial` | the same prefix with the run left open: the mid-load frame progressive delivery exists to produce — part of the history drawn, the "loading venue history" indicator still up. It lasts a few seconds once, at a moment nothing controls, so this hook is the only way to photograph it |
| `QUANTICK_PROGRESSIVE_HISTORY=1` / `=0` | pins the View → progressive venue history switch for the run, overriding what the workspace saved. Anything else is refused rather than guessed, so a typo leaves the trader's own setting alone |

Once merged, move these into the table above.

Landing with the anchored VWAP goal (`feat/anchored-vwap`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_AVWAP_DEMO=1` | one anchored VWAP on the flow pane, anchored ~40 bars back with the 1σ and 2σ band pairs on — the band stack, its layered fills and the anchor marker in a single deterministic frame. The tool itself is registry-driven, so `QUANTICK_DRAWING_TOOL=anchored-vwap` arms it, `QUANTICK_DRAWINGS_DEMO=1` includes it, `QUANTICK_DRAWINGS_DEMO_SELECT=anchored-vwap` selects it (context bar + anchor handle), and `QUANTICK_DRAWING_INSPECTOR=1` opens the settings panel whose VWAP tab holds source and bands |

Once merged, move it into the table above.

Landing with the toolbar usability goal (`fix/toolbar-usability`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_DRAWING_DRAFT=<anchors>` | the **half-placed** object: that many anchors of the tool armed by `QUANTICK_DRAWING_TOOL` already down, with the pointer parked where the next one would go. A draft's live preview is the whole feedback of a multi-anchor gesture, and it is the only surface that exists *between* two clicks — nothing else reaches it without a hand on the mouse. Clamped to one short of the tool's anchor count, so it always photographs a gesture in flight and never a finished object. `QUANTICK_DRAWING_TOOL=parallel-channel QUANTICK_DRAWING_DRAFT=2` is the channel mid-width, the state the "it draws a straight line" report was about |

Landing with the drawing context bar goal (`feat/drawing-context-bar`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_DRAWING_INSPECTOR=1` | the full settings panel open at launch. Selecting a drawing no longer opens it — it raises the context bar, and the gear on that bar is the one door — so this is the only way to photograph the panel without a click. Pair with `QUANTICK_DRAWINGS_DEMO_SELECT` |

The context bar itself has no hook of its own and needs none: it exists for
as long as something is selected, so `QUANTICK_DRAWINGS_DEMO_SELECT=<tool id>`
*is* the hook that reaches it — and it reaches a different bar per tool,
which is the thing worth photographing, since the bar is built from the
selected object's capabilities. `QUANTICK_DRAWING_TOOL` covers the two new
marks (`arrow-mark-up`, `arrow-mark-down`) and the pencil (`brush`) like any
other registered tool, and `QUANTICK_DRAWINGS_DEMO=1` now places a short
freehand path for the pencil, which declares no anchor count of its own.

Once merged, move them into the table above.

Landing with the paper-trading overhaul (`feat/paper-trading-overhaul`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_CMD_PREVIEW=<buy\|sell>` | the cmd-trading preview painted with nobody at the keyboard: the dashed y-locked line, its clickable side/kind/qty label and the gutter price chip, parked mid-chart for that side. The held modifier is the one input a capture run cannot supply (the ParkedHand rule). Needs prints on the tape for a mark — pair with a live feed or `QUANTICK_PAPER_DEMO=1`. |

Once merged, move it into the table above.

Landing with the tape-configuration goal (`feat/tape-own-config`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_TAPE_LAYERS=<list>` | what the **tape** draws, now that it is switched apart from the candles: comma-separated `heatmap`, `bubbles`, `no-heatmap`, `no-bubbles`, or `none` for a bare tape. Each entry calls the setter the tape menu's checkbox calls, and an unlisted layer is left as it was. The state the split exists for — `QUANTICK_BUBBLES_AUTOSTART=1` with `QUANTICK_TAPE_LAYERS=no-bubbles`, or the reverse — is set here directly; use `QUANTICK_CONTEXT_MENU=tape` when the menu itself is what needs photographing |
| `QUANTICK_TAPE_WINDOW=<auto\|90s\|2min\|120000ms>` | how much market time the tape shows: `auto` follows the bars (the default), a duration pins it. Accepts `s`, `m`/`min`, `ms` or bare milliseconds; anything else is refused rather than guessed, so a typo photographs the default instead of an invented window. This is what makes the "bubbles stay visible longer" state capturable at all — otherwise it depends on how fast the bars happen to be closing |

| `QUANTICK_CONTEXT_MENU=<chart\|tape>` | the right-click menu open on that pane, once, on the first frame that has drawn a canvas. The two panes now open **different** menus, so "open the context menu" is no longer one instruction. The click is delivered as a real secondary-button event through the app's own input path (`raw_input_hook`) rather than by reaching into egui's menu state — what opens is exactly what a trader's click opens. `tape` yields nothing on a canvas with no lane, and an unrecognised value opens nothing rather than the wrong pane's menu |

Once merged, move them into the table above.

Landing with the tape-switch goal (`feat/tape-chart-switch`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_TAPE=<on\|off>` | whether the tape is on the canvas at all — the chip in the canvas's top-right corner. `off` reserves no band, so the candles take the whole canvas and there is nothing to right-click for `QUANTICK_CONTEXT_MENU=tape`; the tape's two layer switches are untouched, so `on` returns the tape that was switched off. Calls the setter the chip calls; anything but `on`/`off` leaves the tape alone rather than guessing |

Once merged, move it into the table above.

For a screen that represents the user's real setup, enable the trio:
`QUANTICK_BOOK_AUTOSTART` + `QUANTICK_BUBBLES_AUTOSTART` +
`QUANTICK_LIVE_STRIP_AUTOSTART`, with a preset from `config/bubbles.toml`
(never bare defaults).

## Launch and capture workflow

1. **Own target dir**: build with `CARGO_TARGET_DIR=F:\src\quantick-agent-target`
   so the user's running exe is never locked and rust-analyzer never poisons
   fingerprints.
2. **Fresh exe, proven fresh**: `cargo build -p quantick-app` immediately
   before capturing, then compare the exe `LastWriteTime` against your last
   edit. `cargo test` green does **not** imply the exe was rebuilt.
3. **Launch via PowerShell `Start-Process`** with hooks set and
   `RUST_LOG=quantick=info`, stderr to a log file. A bash background job
   produces a window whose GL surface never presents (pure-white captures).
4. **Capture by PID, never by window title**: use
   `heatmap-design-ref/capture_window.ps1` (PrintWindow with
   PW_RENDERFULLCONTENT) adapted to filter by the PID you launched — title
   matching grabs the wrong window when other instances or editors are open.
5. **Gate on health before trusting a capture**: `APP_HEALTH_SUMMARY` prints
   every 2 s. `fps≈59 / frame_avg≈16.7` → surface presents, capture is real.
   `fps≈19 / frame_avg≈52 / frame_cpu≈3` → occluded or idle desktop, capture
   will be blank; wait for fps ≥ 50 in the log and recapture. Blank capture
   is an environment state, not a render regression — run a `main` control
   build before blaming the change.
6. **Verify by pixel when the eye can be fooled**: read the PNG (e.g.
   `System.Drawing`) to count/locate marks, match dash signatures, or compare
   two frames; use `readable_min_radius` from `config/bubbles.toml` as the
   "too small to read" reference.
7. **Be a guest on the desktop**: never fight the user — if the window gets
   minimized or the mouse is active (`GetLastInputInfo` idle ≈ 0), stop
   driving, keep the evidence you have. Do not inject input with `SendInput`.
   Never bind a second MT5 listener on the user's port (9100) — use
   `QUANTICK_CONFIG` with an alternate `listen_addr`. Close every instance
   you opened when done.

## Adding a new hook

New surface → new `QUANTICK_*` env hook in the same commit: read the var next
to the existing autostart block in `crates/app/src/app.rs`, call the same
function the manual toggle calls, default off. Then add one row to the
registry table above. That row is part of the feature's definition of done.
