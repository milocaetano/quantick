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
| `QUANTICK_CANDLE_WIDTH=<px>` | the zoom, scripted: candle slot width in pixels, clamped to the gesture's own 2–160 bounds. Footprint LOD by width: ≥72 Detailed, 40–72 Compact, 18–40 Profile, 8–18 Marks, <8 Off |
| `QUANTICK_FOOTPRINT_PANEL=1` | the footprint settings window open at launch (style, band fineness, imbalance thresholds, POC/badges) |
| `QUANTICK_FOOTPRINT_SETTINGS=<toml>` / `QUANTICK_FOOTPRINT_PRESETS=<toml>` | where the footprint's saved knobs and named presets live. **Always point these at scratchpad files.** Without them a validation run reads — and, the moment it touches a knob, overwrites — the trader's real setups, the same rule `QUANTICK_UI_STATE` carries. |
| `QUANTICK_FOOTPRINT_DEBUG=1` | appends the layer's own inputs to its legend (`[w<candle px> row<base row px> g<capture group> lvl<level> n<ladders>]`). The zoom-boundary bugs were all invisible from outside — this is the chart telling you which number it decided on |
| `QUANTICK_INDICATORS_AUTOSTART` / `QUANTICK_INDICATOR_SCRIPTS_AUTOSTART` | indicator panes / pine scripts |
| `QUANTICK_INDICATOR_SETTINGS=1` | the indicator settings dialog (sliders + live preview), opened for the first indicator once its inputs arrive from the worker. Pair with an autostart hook (or a seeded state file) that loads an indicator |
| `QUANTICK_INDICATOR_PRESETS=<toml>` | where the settings dialog's named input presets live. **Point at a scratchpad file**; seeding it is how the preset picker is photographed populated |
| `QUANTICK_INDICATORS_STATE=<toml>` | where the persisted indicator set lives. **Always point this at a scratchpad file** — without it a validation run reads (and, after an Apply, rewrites) the trader's real `indicators-state.toml`. Seeding it is also how a specific input state (a layer toggled off, a tuned window) is photographed without a click |
| `QUANTICK_REPLAY_DIR` + `QUANTICK_REPLAY_AUTOSTART=1` + `QUANTICK_REPLAY_SPEED` | recorded session playback (deterministic tape → deterministic screen) |
| `QUANTICK_BUBBLES=<bubbles.toml>` | bubble preset override without touching tracked config |
| `QUANTICK_CHART_LAYERS` | chart layer visibility set: a `version = 1` TOML with a `[layers]` table keyed by layer id (`heatmap`, `bubbles`, `footprint`, `live_strip`, `flow_legend`, `book_status`, `depth_gaps`, `grid`, `last_price`, `crosshair`, `paper_trading`, `trade_paint`, `drawings`, …). This is also how the canvas *chrome* is reached with no clicks — `flow_legend = false` silences the top-left key, `book_status = false` the top-right badge — and `bubbles = false` with `live_strip = true` is the state that used to blank the strip. **Point it at a scratchpad file**: the app writes this file back whenever a switch flips. |
| `QUANTICK_UI_STATE=<toml>` | the saved workspace — the tab strip, each tab's layout/split/focus/bar specs, the dock, the rail, the timezone, the window size. **Always point this at a scratchpad file.** Without it a validation run reads the user's real `ui-state.toml` and, on exit, overwrites it: the run both inherits yesterday's cockpit and destroys it. Point it at a path that does not exist to force the configured default; write one by hand to open on an exact arrangement. |
| `QUANTICK_WORKSPACE_SAVE=1` | takes `Workspace → Save workspace` at startup, through the menu entry's own path — the save really happens, so the status-line confirmation is on screen to capture. Pair with `QUANTICK_UI_STATE` pointed at a scratchpad. |
| `QUANTICK_BACKFILL` / `QUANTICK_BOOK_DEPTH` | history paging / depth size |
| `QUANTICK_TRADES_DIR` | paper-trading journal location (point at scratch) |

Landing with the drawing-toolbar goal (`feat/drawing-toolbar-pro`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_DRAWING_TOOL=<tool id>` | opens with that tool armed — any id in `DRAWING_TOOLS` (`trend-line`, `ray`, `measure`, `text`, …) |
| `QUANTICK_DRAWING_MAGNET=1` | the magnet on (anchors snap to the bar's OHLC) |
| `QUANTICK_DRAWINGS_DEMO=1` | one of every registered drawing placed on the flow pane once it has bars, the last one selected so the inspector is on screen too |
| `QUANTICK_DRAWINGS_DEMO=bands` | the same set, plus a level on each indicator pane's own value and a diagonal across it — the band projection under test. Pair with `QUANTICK_INDICATORS_AUTOSTART=1` |
| `QUANTICK_DRAWINGS_DEMO_SHARED=1` | those demo objects marked "show on all charts" — pair with a split layout to see the cross-pane projection (which is now also where they can be grabbed, moved and deleted) |
| `QUANTICK_DRAWINGS_DEMO_RECUT=1` | re-cuts the bars under the demo objects after placing them, and adds one mark anchored before the loaded history — the two states a timeframe switch produces: every mark re-anchored onto the new bars, and one faded "off series" |
| `QUANTICK_DRAWINGS_MANAGER=1` | opens the object manager, which is where the "off series" and "other market" badges are read — and the only place a mark clamped off the visible window can be found at all |
| `QUANTICK_DRAWINGS_DEMO_SELECT=<tool id>` | selects that tool's demo object and centres the viewport on it. Selection is what puts an object's handles on screen, so this is the only way to photograph the grab points of a tool that is not last in the registry (`parallel-channel` for its corner and rail handles) |
| `QUANTICK_FRVP_DEMO=1` | one fixed-range volume profile placed on the flow pane once it has bars. When the pane carries a venue history prefix the range straddles the seam, so the partial-coverage label ("profile from N of M bars") is on screen — the honesty surface this hook photographs |
| `QUANTICK_FRVP_DEMO=compare` | two adjacent profiles over the same stretch of liquidity map, one per over-heatmap mode (outline vs always-fill) — the silhouette decision's before/after in a single frame |

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

Landing with PR #127 (paper trading v2): `QUANTICK_DOCK_TAB`
(`l2|bubbles|session|trading|trades`), `QUANTICK_PAPER_REPORT_AUTOSTART=1`,
`QUANTICK_PAPER_DEMO=1` (scripted deterministic trade sequence). Once merged,
move them into the table above.

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
