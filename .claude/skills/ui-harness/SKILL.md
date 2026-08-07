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
| `QUANTICK_INDICATORS_AUTOSTART` / `QUANTICK_INDICATOR_SCRIPTS_AUTOSTART` | indicator panes / pine scripts |
| `QUANTICK_REPLAY_DIR` + `QUANTICK_REPLAY_AUTOSTART=1` + `QUANTICK_REPLAY_SPEED` | recorded session playback (deterministic tape → deterministic screen) |
| `QUANTICK_BUBBLES=<bubbles.toml>` | bubble preset override without touching tracked config |
| `QUANTICK_CHART_LAYERS` | chart layer visibility set |
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
| `QUANTICK_DRAWINGS_DEMO_SHARED=1` | those demo objects marked "show on all charts" — pair with a split layout to see the cross-pane projection |

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
