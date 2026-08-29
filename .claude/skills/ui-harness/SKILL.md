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
| `QUANTICK_WINDOW_MAXIMIZED=1` | the window **maximised** on the first frame. Not a larger `QUANTICK_WINDOW_SIZE`: maximising is what the window manager does, and on Windows at a scale factor other than 1 it is the state where the platform hands the client size in physical pixels where points were due — the chart then lays out a third wider than its own surface and the toolbar's layer group, the price axis, the live strip and the dock rail are painted off the edge (`crates/app/src/window_scale.rs`). No size can reach that state, so without this hook the whole class of defect is invisible to anything but a human clicking the title bar. Pair with `QUANTICK_WINDOW_SIZE` to control what the window restores *down* to |
| `QUANTICK_WINDOW_SIZE=WxH` | the size the window opens at, **not floored** — the window itself has no minimum any more, and this hook asks for exactly what it says, degenerate sizes included (`1x1` is a real request, and the state the layout is expected to survive rather than lay out in). Window size decides whether the indicator band has room for its panes and the time axis for its labels, so without this that whole class of defect is invisible to anything but a human dragging a corner. With it plus `QUANTICK_INDICATORS_AUTOSTART`, the collapsed-pane strip is reachable from a fresh launch. |
| `QUANTICK_BOOK_AUTOSTART=1` | L2 heatmap layer |
| `QUANTICK_LIVE_STRIP_AUTOSTART=1` | live strip |
| `QUANTICK_BUBBLES_AUTOSTART=1` | aggression layer (bubbles + live-column footprint) |
| `QUANTICK_FAKE_LATENCY_SPLIT=<arrival>,<source>,<transport>,<hop>` | a **late tape that knows who is late**: the status bar's tape cell replaces the word `arrival` with the guilty hop's name (`MT5 18112 ms`) and the hover carries the numbers behind it. Milliseconds and one word, e.g. `18112,17980,132,MT5`. The hop must be a name the feed can really report — `MT5` or `quantick`, resolved through `LatencyHop::ALL` — so a capture can never claim a state the system has no way to produce. The name only shows above `HIGH_LAG_MS` (5 s), so a value under that photographs the healthy cell instead. It drives the readout through the feed's own latency port and **forges no measurement**: `Tab::trade_arrival_ms`, `APP_HIGH_TRADE_LAG` and the control feed scope are untouched, so a capture run's logs stay distinguishable from a real outage. The state it draws exists only while a real venue is running badly — no setting reaches it and no recording has it, since a replay has no chain to attribute — so without this hook the cell, its hop name and its whole hover breakdown are invisible to anything but a human waiting for a bad day. A malformed value is logged (`FAKE_LATENCY_SPLIT_REJECTED`, which lists the names it would have accepted) and ignored, never silently defaulted: a typo must not photograph the wrong state and call it a pass |
| `QUANTICK_TAPE_STARVE_AFTER_MS=<n>` | a **starved tape**: `n` ms after the first print the tape stops being fed while the book keeps arriving, so the aggression bubbles drift left of the lane's right edge and past its window leave it empty. The state the axis caption under the tape exists for (`last print 6 s back`, then `no print for 41 s`), and one no setting reaches — it is a market condition, a book that keeps changing while nothing prints. Prints are withheld through the feed's own `record_trade`, never forged into the caption, and only from the tape: the candles, the indicators and the simulator keep every print, which is the contrast the capture is of. Pair with `QUANTICK_BOOK_AUTOSTART=1` — with no book there is no second clock and nothing drifts |
| `QUANTICK_BUBBLE_BUDGET=<n>` | how many bubble primitives the whole frame may draw, split between the candles and the tape by the tape's width share. The scripted way to photograph a **folded** mark — the fold is the one bubble state a capture cannot otherwise arrange, because it needs a tape dense enough to exhaust the real budget of 700, which is a market condition and not a setting. `8` folds almost everything; a folded bubble wears a ring and, above 7 px radius, a count under its centre. Calls the same setter the projection reads, so the picture is the picture a busy session gives |
| `QUANTICK_FOOTPRINT_AUTOSTART=1` | candle footprint layer (per-price sell×buy ladder in the candles; detail follows zoom — pair with `QUANTICK_CANDLE_WIDTH` to reach each level) |
| `QUANTICK_CANDLE_WIDTH=<px>` | the zoom, scripted: **pixels per bar**, clamped to the gesture's own 1–256 bounds. One bar is one candle at every zoom (the trust law — nothing ever groups), so `1` is the deepest squeeze: a 1600 px window holds 1600 bars. Footprint LOD by candle width, for the **ladder**: ≥68 Detailed, 33–68 Compact, 10–33 Profile, 6–10 Marks, <6 Off. Each style declares its own Detailed floor (`cluster` ≥126, or ≥68 without its total column) and every floor scales with the panel's `detail_scale`. The gesture's ceiling is 256 px, sized so the widest style is still reachable at the highest `detail_scale` |
| `QUANTICK_PAN_PX=<px>` | a scripted drag on the candles, re-applied every frame: negative pushes the chart left (`-9000` = as far into the projection margin as it goes, i.e. a whole window of empty canvas to draw a channel or a Fibonacci extension into), positive walks back into history. The margin and the way back are states no screenshot can otherwise reach |
| `QUANTICK_FOOTPRINT_PANEL=1` | the footprint settings window open at launch (style, band fineness, **the detail-zoom slider**, imbalance thresholds, POC/badges, and the cluster knobs when that style is picked) |
| `QUANTICK_FOOTPRINT_STYLE=<id>` | which reading the ladder draws: `split` (default), `bidask`, `ladder`, `cluster`, or `auto` — the last picking the richest the zoom can pay for, which makes it the one hook that photographs the *whole* ladder by varying `QUANTICK_CANDLE_WIDTH` alone. Resolved through `FootprintStyle::ALL` — the same registry the panel selector and the TOML read — so a style that exists is reachable here by name; an unknown id is **logged and ignored**, never silently swapped for the default, because a typo in a validation script must not photograph the wrong style and call it a pass. **Mind the floors, which differ per style**: `cluster` writes three quantities a row and needs **≥126 px** of candle (≥68 with `cluster_show_total = false`), `ladder` ≥68, `split` and `bidask` draw from ~10. Below its floor a style hands over and the legend says so (`cluster → bidask`) — so pair `cluster` with `QUANTICK_CANDLE_WIDTH=150` or wider, and remember `detail_scale` multiplies every floor |
| `QUANTICK_PANE_COLLAPSED=1` | the canvas open with its **context column collapsed to its rail** — the 8 px strip and its grip, and the state a trader reaches by dragging the divider past the pane's floor. A capture cannot perform that drag, and the rail is the only affordance that brings the charts back, so without this hook the whole collapsed state is invisible to anything but a hand. Pair with `QUANTICK_LAYOUT=time+flow` or `time+time+flow` — a layout with no context column has nothing to collapse. **Sets the same flag the drag sets**, so the picture is the picture the gesture gives; it does not prove the gesture is reachable, which is what `dragging_the_divider_to_the_edge_collapses_the_column` is for |
| `QUANTICK_LAYOUT_PICKER=1` | the toolbar's **layout popover** open on the first drawn frame — the grid of preset thumbnails behind the LAYOUT icon, which is the fast way between arrangements and the only surface that shows them all at once. A popover is a popup egui owns, so the hook asks for it through the same `open_popup` the click makes rather than faking the surface; it is one-shot, so the first click still closes it. Pair with `QUANTICK_LAYOUT=<preset id>` to photograph the grid with a given cell lit |
| `QUANTICK_LAYOUT_TAB=<name>` | the **layout strip** (above the status bar) open on the named layout tab — switched to when the layouts file has one by that name, created empty when it does not — so a capture can show two layouts side by side across two runs: `QUANTICK_LAYOUT_TAB=levels` on a home that already holds `Layout 1`. Goes through `switch_layout` / `create_layout`, the same calls the strip's click and `+` make |
| `QUANTICK_LAYOUT_DELETE=1` | the strip's **delete confirmation** open on the active layout — the small window naming the layout and how many drawings go with it, with Delete and Cancel. Deleting is the one strip action behind a confirmation (it destroys drawings, on disk too), so a capture cannot reach the window through the context menu's own Delete. Goes through `apply_strip_action(Delete)`, the same call the menu makes; the run's file is untouched unless something presses Delete |
| `QUANTICK_PANE_LAYOUTS=<name,name,name>` | one **layout per pane**, by name, in pane-address order — flow pane first, then the context stack top to bottom (`Layout 1,levels` puts the flow chart on `Layout 1` and the top context chart on `levels`). A name the book lacks is created empty; an empty entry leaves that pane alone. This is the capture of the whole point of per-pane layouts: two charts side by side on two indicator sets, each context header naming its layout and the strip naming the focused pane. Goes through `switch_pane_layout`, the call the strip's click makes for the focused pane |
| `QUANTICK_LAYOUT_RENAME=1` | the strip's **rename box** open on the active layout on the first frame — the in-place text field a double-click opens. Pair with `QUANTICK_LAYOUT_TAB` to name which tab is being renamed |
| `QUANTICK_STYLE_PANEL=1` | the appearance dialog open at launch (candle presets, body mode and colours, **the gap between candles**, outline, wicks, canvas). Behind the toolbar's LOOK button, which a scripted run cannot press |
| `QUANTICK_FOOTPRINT_SETTINGS=<toml>` / `QUANTICK_FOOTPRINT_PRESETS=<toml>` | where the footprint's saved knobs and named presets live. **Always point these at scratchpad files.** Without them a validation run reads — and, the moment it touches a knob, overwrites — the trader's real setups, the same rule `QUANTICK_UI_STATE` carries. |
| `QUANTICK_FOOTPRINT_DEBUG=1` | appends the layer's own inputs to its legend (`[w<candle px> row<base row px> g<capture group> lvl<level> n<ladders>]`). The zoom-boundary bugs were all invisible from outside — this is the chart telling you which number it decided on |
| `QUANTICK_INDICATORS_AUTOSTART` / `QUANTICK_INDICATOR_SCRIPTS_AUTOSTART` | indicator panes / pine scripts |
| `QUANTICK_LEGEND_COLLAPSED=1` | the focused pane's on-chart indicator legend **folded to its count puck** — the healthy rows off the chart, the count and any errored or stale row still on it. Folding is otherwise the chevron on the legend's first row, the View menu entry or Ctrl+L, none of which a scripted run performs. Pair with `QUANTICK_INDICATORS_AUTOSTART=1`, or the pane has no legend to fold. The fold is a **persisted** pane field (it rides in `SavedTab`, like the bar rules), so point `QUANTICK_UI_STATE` at a scratchpad file first: with `save_on_exit` on, a run under this hook otherwise leaves the trader's own legend folded on the next launch |
| `QUANTICK_INDICATOR_SETTINGS=<index>[:<tab>]` | the indicator settings dialog open at launch, on the indicator at `index` among the focused pane's views in add order, and on `inputs` (default) or `style`. Pair with `QUANTICK_INDICATORS_AUTOSTART=1` — `0:inputs` reaches the EMA's parameters, `1:style` the CVD pane's per-plot colour and width. Nonsense is refused rather than guessed, so a typo yields no dialog instead of a capture of the wrong tab. In the app this dialog is opened by double-clicking the indicator — the legend row, the pane header, a collapsed pane's strip, or the plotted line itself — none of which a scripted run can perform. |
| `QUANTICK_REPLAY_DIR` + `QUANTICK_REPLAY_AUTOSTART=1`\|`paused` + `QUANTICK_REPLAY_SPEED` | recorded session playback (deterministic tape → deterministic screen). The folder is **per-run**: without the hook the browser opens on the trader's stored pick, else `Documents/Quantick/replay`, and a run under the hook never rewrites that pick — so pointing a validation run at a scratch folder is safe, unlike the workspace and paper-state files beside it. `1` loads **and plays**; `paused` loads and waits on the first print, which is what a person meets when they open a recording and a state no other hook reaches |
| `QUANTICK_REPLAY_SESSION=<YYYY-MM-DD>` | **which** recording `QUANTICK_REPLAY_AUTOSTART` opens, by day (or by file stem). The scan lists sessions oldest first and a bare autostart takes the first, which is the one day in a folder that can have nothing joined in front of it — so without this the day-before join is a state no capture reaches. A day the folder does not hold opens the browser on its list rather than photographing a different session. Goes through the list's own selection, the same one a click sets |
| `QUANTICK_REPLAY_DAY_BEFORE=0|1` | whether opening a recording joins the **session day before it** (the tape sitting beside it in the same folder) in front of its prints, already played, with the playhead parked on the chosen day's first print. On by default, so a scratch folder holding two consecutive days now opens with both on the chart — pin `0` for a capture that must show one day only. Staged for the run, never written to the workspace, like `QUANTICK_REPLAY_DIR`. Read back through the control plane's workspace summary (`replay_day_before`) |
| `QUANTICK_REPLAY_BROWSER=1` | the Market Replay browser open on **My sessions** — the list of recordings already on disk, with the folder row above it. The window is one menu entry (or Ctrl+R) deep, so this is the only way a capture reaches it |
| `QUANTICK_REPLAY_GET_DATA` | the browser open on its **Get data** tab. `1` takes the chart's own instrument — what clicking the tab does, look-up included — and any other value states the contract outright (`WINQ26`). Both reach the calendar with no clicks; both need a MetaTrader terminal to answer, so the frame is a spinner or a refusal without one |
| `QUANTICK_BUBBLES=<bubbles.toml>` | bubble preset override without touching tracked config |
| `QUANTICK_CHART_LAYERS` | chart layer visibility set: a `version = 1` TOML with a `[layers]` table keyed by layer id (`heatmap`, `bubbles`, `footprint`, `live_strip`, `flow_legend`, `book_status`, `depth_gaps`, `grid`, `last_price`, `crosshair`, `pointer_price`, `pointer_time`, `paper_trading`, `trade_paint`, `drawings`, …). This is also how the canvas *chrome* is reached with no clicks — `flow_legend = false` silences the top-left key, `book_status = false` the top-right badge — and `bubbles = false` with `live_strip = true` is the state that used to blank the strip. `pointer_price` / `pointer_time` are the pointer compass's two axis switches — both ship **on**, so a capture that wants a bare axis has to say so here (each is also one right-click on its own axis, `QUANTICK_CONTEXT_MENU=axis` / `=time`). **Point it at a scratchpad file**: the app writes this file back whenever a switch flips. |
| `QUANTICK_UI_STATE=<toml>` | the saved workspace — the tab strip, each tab's layout/split/focus/bar specs, the dock, the rail, the timezone, the window size. **Always point this at a scratchpad file.** Without it a validation run reads the user's real `ui-state.toml` and overwrites it: the run both inherits yesterday's cockpit and destroys it. Not only on exit — the standing choices in this file are written the moment they are made, so a single click on a rail star or on a replay folder is already a write, autosave off or not. Point it at a path that does not exist to force the configured default; write one by hand to open on an exact arrangement. |
| `QUANTICK_WORKSPACE_SAVE=1` | takes `Workspace → Save workspace` at startup, through the menu entry's own path — the save really happens, so the status-line confirmation is on screen to capture. Pair with `QUANTICK_UI_STATE` pointed at a scratchpad. |
| `QUANTICK_MENU=workspace` | the Workspace menu open on the first drawn frame — the only door to Save, Save as, Export, Open from file, Open recent and Show where it's saved. A menu is a popup egui owns, so there is no state to set that would not be a second way of opening it: the hook delivers the click on the button's own rect, through the app's input path (`raw_input_hook`), exactly as `QUANTICK_CONTEXT_MENU` does. Anything but `workspace` opens nothing rather than the wrong menu. |
| `QUANTICK_WORKSPACE_EXPORT=<path>` | takes `Workspace → Export to file…` at startup, writing the whole cockpit — tabs, chrome, indicators, layers, drawing colours, footprint, added symbols — to `<path>` as one bundle. The path is *given* rather than picked because the OS file dialog is the one thing a scripted run cannot drive; everything past it is the menu entry's own code, so the status-line confirmation is on screen to capture. Point it at a scratchpad path. |
| `QUANTICK_WORKSPACE_IMPORT=<path>` | the same for `Workspace → Open from file…`: really replaces the cockpit from `<path>`, so **point every store env var at scratchpad files first** or the run rewrites the trader's real setup. A bundle that fails its check changes nothing and puts the reason on the status line — which is how the refusal path is captured. |
| `QUANTICK_UI_STATE` and its seven siblings | the cockpit stores now live in `Documents/Quantick/` rather than beside the launch directory (`crate::store_home`), so an **unhooked run reads and rewrites the trader's real cockpit** — not just an empty file in the repo. The full set to point at scratch: `QUANTICK_UI_STATE`, `QUANTICK_LAYOUTS`, `QUANTICK_INDICATORS_STATE`, `QUANTICK_INDICATOR_PRESETS`, `QUANTICK_CHART_LAYERS`, `QUANTICK_DRAWING_PRESETS`, `QUANTICK_FOOTPRINT_SETTINGS`, `QUANTICK_FOOTPRINT_PRESETS`, `QUANTICK_SYMBOLS`. `QUANTICK_LAYOUTS` matters most for the demo hooks: every drawing a `QUANTICK_DRAWINGS_DEMO` / `QUANTICK_TEXT_NOTE` / FRVP / AVWAP / strategy scene places, and every layout `QUANTICK_LAYOUT_TAB` creates, is a layout edit and is written into that file after a second. A store under its own env var is also skipped by the startup rescue, so a QA scratch file is never copied into the home. |
| `QUANTICK_BACKFILL` / `QUANTICK_BOOK_DEPTH` | history paging / depth size |
| `QUANTICK_CONTROL_PANEL=1` | the **Local agent access** window open at launch — status, the read-scope checkboxes for the next connection, the enable/disable button, the connected-clients list with per-client revoke, and the last UI-drain/capture budget line. Otherwise one entry deep in the Tools menu (or the "Agent access: on" status button once enabled), which a scripted run cannot press. Takes the menu entry's own `open_panel` |
| `QUANTICK_CONTROL_ACCESS=1` | observer access **enabled on the first frame** through the panel button's own `enable`: a fresh token and loopback port, and a real descriptor published in the private runtime directory (`%LOCALAPPDATA%\Quantick\control\instances` on Windows), removed again on a clean exit. Pair with `QUANTICK_CONTROL_PANEL=1` to photograph the enabled state; connect a client (the `quantick-control-local` client or the MCP adapter) to photograph the clients list. A run killed mid-way leaves a stale descriptor behind, which discovery reports as an issue rather than an instance |
| `QUANTICK_CONTROL_MARK=<1\|note>` | a **mark** taken on the first frame through the hotkey's own action (`attention.mark.create`, Ctrl+M in the app): the journal gets an `attention.mark.created` event carrying the resolved cursor target — on a first headless frame the pointer is over nothing, so the target reports unavailability honestly — plus the note (`1` = no note). Pair with `QUANTICK_CONTROL_ACCESS=1` and an `events.read` / `quantick_read_events` client to photograph the event; during a replay the mark is also appended to the recording's control trace sidecar (`<session>.control-trace.jsonl`) and re-injected on the next run of that recording |
| `QUANTICK_CONTROL_SCOPES=<ids>` | which scopes the next connection is granted, by ID — the panel's own checkboxes without a hand on the mouse, through the same `configure_scopes` call they write. `all-reads` is the safe default grant, `annotate-tier` the whole annotate tier (chart objects, notifications, sound, scripts), and any comma-separated list of registered permission IDs is honoured (`all-reads,annotate,annotate.chart`). An unregistered ID is refused with a `CONTROL_SCOPE_HOOK_REFUSED` log rather than silently dropped. Pair with `QUANTICK_CONTROL_ACCESS=1`: the profile follows the scopes, so a grant with any annotate scope raises the ceiling to `annotator` and the panel's status line says "reading, and answering on the chart" |
| `QUANTICK_CONTROL_ANNOTATE=<text>` | one **agent-authored label** placed on the first frame at the newest bar, through `annotate.label.create` with an agent actor — the object every attribution surface is photographed from: the "assistant" chip in the object manager row, the `Placed by …` line in the inspector, the robot chip on the context bar, and the "Remove N object(s) placed for you" button the sweep gesture lives on. Pair with `QUANTICK_DRAWING_MANAGER=1` for the list, or select the object for the context bar |
| `QUANTICK_CONTROL_NOTIFY=<popup\|toast\|sound>:<message>` | one **notification** raised on the first frame through `notify.*` with an agent actor: `popup` opens the assistant's window over the chart (title, message, `Sent by …`, Dismiss), `toast` posts to the acknowledgement lane, `sound` asks the platform for its alert (and reports honestly when the build has no audio backend). The rate limit applies to the hook exactly as to a client, so a third call in a burst is refused |
| `QUANTICK_CONTROL_EVIDENCE=<all\|1\|scope,…>[,screenshot]` | one **evidence bundle** captured on the first frame through `evidence.capture` — the very read a connected client calls, via `ControlAccess::invoke_local_read`, so a hooked run and a client run exercise one door. `all` (or `1`) means every registered snapshot scope the configured grant already reaches, taken from the registry rather than a list in the hook; anything else is a scope ID; adding `screenshot` asks for the window to be rasterised as well, which needs `observe.screenshot` in `QUANTICK_CONTROL_SCOPES` and raises the **screenshot notice** in the acknowledgement lane (the visible indicator threat-model O-18 requires, and the surface a capture run photographs). A capture that asked for an image waits up to `CONTROL_EVIDENCE_HOOK_FRAMES` (~2 s) for the window to present, then captures without one and says so in its coverage. The manifest goes to the log as `CONTROL_EVIDENCE_CAPTURED` with the evidence ID, content digest, byte and chunk counts, and how many scopes were captured, omitted, not captured and unavailable — which is what a scripted validation run asserts on. Nothing is written to disk |
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
| `QUANTICK_FRVP_DEMO=stress` | 25 000 one-minute venue candles delivered behind the tape (folded to whatever interval the time pane shows) with one profile over the whole of the **time** pane — the range that used to freeze the app. What it photographs is the *filling* state: a partial histogram with `loading N of M bars` on its status line |
| `QUANTICK_FRVP_FOLD_BUDGET=<bars>` | how much a profile's fold spends per frame (default 1500 bars-worth of map touches). `=1` advances one bar per frame, holding the filling state on screen for as long as a capture needs; a non-positive or unparseable value is refused and the default stands |

Landing with the trade-history context goal (`feat/trade-history-context`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_PAPER_CALENDAR=1` | the Simulated performance window with its **month grid expanded** and nothing picked — the state that shows which days hold trades (tinted by the day's net, trade count under the day number) before any filtering happens. Takes the report's own open path, so it stacks with `QUANTICK_PAPER_REPORT_AUTOSTART` rather than racing it |
| `QUANTICK_PAPER_CALENDAR=<YYYY-MM-DD>` | the same, with that **single day** picked and the report already cut to it — the grid also pages to that day's month, so the pick is on screen rather than a month away. A date that is not a real calendar day (`2026-02-30`, `2026-13-01`) is refused and no calendar opens, instead of silently landing on a normalised neighbour |
| `QUANTICK_PAPER_CALENDAR=<YYYY-MM-DD..YYYY-MM-DD>` | a picked **span**, either way round — the two-click range state, which a scripted run cannot otherwise reach because it needs two clicks on two different cells. The support line under the filters reads the span back in words |
| `QUANTICK_LEDGER_SCOPE=<chart\|all\|SYMBOL>` | which instrument's saved history the Trades dock lists: `chart` follows the chart (the default), `all` mixes every folder into one timeline, and any other value is read as a symbol folder name — the way to photograph one market's history while the chart shows another. A folder that does not exist simply lists nothing rather than falling back to the chart, which is the honest answer to "show me WDOFUT" when no WDOFUT was ever traded |
| `QUANTICK_LEDGER_FOLD=1` | every civil day in the ledger **folded shut** — the one-line-per-day read, each keeping its date, trade count and net. Folding is otherwise a click on each day header (or the fold-all control beside the refresh button), so this is the only way a capture reaches it. Pair with `QUANTICK_DOCK_TAB=trades` |
| `QUANTICK_LEDGER_PAGES=<n>` | the Trades dock **past its first page** of saved history: `n` pages of 50 revealed. The only other way there is clicking "show older", so without this the second page and the shrinking "N more saved" count are invisible to a capture. Pair with `QUANTICK_DOCK_TAB=trades` and a `QUANTICK_TRADES_DIR` holding more than 50 saved trades |
| `QUANTICK_PAPER_REPORT_LIST=0` | the report with its **trade list collapsed**. The list is open by default (a curve whose trades are hidden is the confusion the window exists to end), so the hook exists to photograph the collapsed state; any other value leaves it open |

Once merged, move these into the table above.

Landing with the strategy anchors goal (`feat/strategy-anchors`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_STRATEGY_DEMO=1` | a named rectangle over the recent tape (spanning past the newest bar) with a force-bar strategy armed on it — the on-chart state badge, in `armed`. The rectangle covers the chart's middle on purpose: pair with `QUANTICK_CONTEXT_MENU=chart` and the scripted right-click lands *on* it, opening the per-drawing menu (name, rename, the strategy seat, lock/hide/delete) instead of the bare layer menu |
| `QUANTICK_STRATEGY_DEMO=popup` | the same rectangle with the **arming dialog** open over it — preset picker, side, quantity, the force band, the projection multipliers, re-arm, save-preset. The form is the surface a screenshot of "how do I configure the bot" needs |
| `QUANTICK_STRATEGY_PRESETS=<path>` | relocates the strategy bank (`quantick-strategies.toml`), so a validation run seeds or inspects presets without touching the trader's own bank |

Once merged, move these into the table above.

Landing with the signal-alarm goal (`feat/strategy-signal-alarm`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_STRATEGY_DEMO=alarm` | the arming dialog with its **alarm section unfolded** — the checkbox, the on-close/share choice and its share spinner, the once-per-bar/cooldown choice and its seconds, the sound picker — grouped *system* / *standard alarms* / *nature alarms*, with its Test button — the **stop after N s** cut under it, and alarm-only. The scene picks a library clip (`cuckoo`) with the cut ticked, so the row that exists only for a clip is on screen. The section folds itself away while the checkbox is clear (right for a trader, useless for a capture), so `popup` alone can never photograph it; the scene also picks the share gate *and* the cooldown, so the controls that exist only under a choice are all on screen at once |
| `QUANTICK_STRATEGY_DEMO=alarm-sounds` | the same dialog with the **sound picker dropped open** — the three headings (*system*, *standard alarms*, *nature alarms*) and every sound under them, the current one lit. A combo's list exists only while it is open and opening it is a click, so this is the only way a capture sees the grouping at all; the scene opens the popup through egui's own `Memory::open_popup` on the frame the dialog appears, on the id `ComboBox` derives for it (`app.rs`, `draw_alarm_controls`) — if a capture shows the picker closed, that derivation is the first suspect, and the first click elsewhere closes it as it would for a trader |
| `QUANTICK_STRATEGY_DEMO=alarm-badge` | an **alarm-only** instance armed on the region wearing a standing `signal (preview)` mark — the badge that says "this places nothing" and the provisional label together. Both are states a real tape reaches only when a force bar happens to be half-formed, which no capture can wait for, so the scene stages the mark rather than hoping for one. The drawing is left **unselected** on purpose: placing one selects it, and a selection raises the context bar across the region's top edge, which is exactly where the badge paints. **Caveat for photography**: the badge paints at the region's top-left corner and is not clamped into view, while the shared demo region is ±3 % of price — far wider than the few hundred points a tick chart shows — so at the default zoom that corner, and with it the badge, sits off the top of the canvas. The hook reaches the *state* (assert it through the instance, or pan to the corner); it does not frame the badge on its own |
| `QUANTICK_STRATEGY_DEMO=ended-badge` | an armed instance whose region's drawn span no longer reaches the next bar — the badge clause `region ended — stretch it right`. A live tape reaches it only by walking past a hand-drawn right edge, which is minutes of market a capture cannot wait out, so the scene performs the drag: it pulls the demo band's anchors behind the tape and lets the badge answer. Deliberately **not** a staged state — the instance stays armed and its alarm stays live, so the menu's Re-arm reads exactly as it does on a real chart. Same framing caveat as `alarm-badge`: the badge paints at the region's top-left corner, off-canvas at the default zoom |
| `QUANTICK_STRATEGY_DEMO=paused-badge` | an armed instance whose region lost its footing on the series (`off_series`) — the badge clause saying the bot is paused and why. A real chart reaches it only through a re-cut that strands an anchor, which nothing scripted can provoke on cue. Same framing caveat as `alarm-badge` |

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

Landing with the off-tape trade-marks fix (`fix/trade-marks-off-series`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_REPLAY_RESTART_AFTER=<n>` | the replay seek, scripted: once the session has closed `n` round trips, the transport's own **Restart** is pressed. The recording starts over, the trades stay in the ledger (they happened) and their fills are now at instants no bar on screen covers — the one state where a closed-trade mark can be asked to paint against a tape that has not reached it, and the state the marks used to stack up in. Needs a recording playing (`QUANTICK_REPLAY_DIR` + `QUANTICK_REPLAY_AUTOSTART=1`) and trades to close (`QUANTICK_PAPER_DEMO=1` + `QUANTICK_TRADES_DIR` at scratch); on a live feed there is no timeline to seek and the hook simply waits. Consumed once |

Once merged, move it into the table above.

Landing with the toolbar usability goal (`fix/toolbar-usability`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_DRAWING_DRAFT=<anchors>` | the **half-placed** object: that many anchors of the tool armed by `QUANTICK_DRAWING_TOOL` already down, with the pointer parked where the next one would go. A draft's live preview is the whole feedback of a multi-anchor gesture, and it is the only surface that exists *between* two clicks — nothing else reaches it without a hand on the mouse. Clamped to one short of the tool's anchor count, so it always photographs a gesture in flight and never a finished object. `QUANTICK_DRAWING_TOOL=parallel-channel QUANTICK_DRAWING_DRAFT=2` is the channel mid-width, the state the "it draws a straight line" report was about |

Landing with the drawing context bar goal (`feat/drawing-context-bar`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_DRAWING_INSPECTOR=1` | the full settings panel open at launch. Selecting a drawing no longer opens it — it raises the context bar, and the gear on that bar is the one door — so this is the only way to photograph the panel without a click. Pair with `QUANTICK_DRAWINGS_DEMO_SELECT` |

The context bar's *existence* needs no hook: it is up for as long as
something is selected, so `QUANTICK_DRAWINGS_DEMO_SELECT=<tool id>`
*is* the hook that reaches it — and it reaches a different bar per tool,
which is the thing worth photographing, since the bar is built from the
selected object's capabilities. `QUANTICK_DRAWING_TOOL` covers the two new
marks (`arrow-mark-up`, `arrow-mark-down`) and the pencil (`brush`) like any
other registered tool, and `QUANTICK_DRAWINGS_DEMO=1` now places a short
freehand path for the pencil, which declares no anchor count of its own.

Once merged, move them into the table above.

Landing with the popup-position goal (`feat/inspector-position-memory`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_DRAWING_INSPECTOR_POS=<x,y>` | the properties popup **parked where a trader dragged it**, in screen points, through the title-bar gesture's own function. That position is now remembered in the workspace and reused for every drawing selected afterwards, so it is a state a capture has to reach — and a drag is the only thing that sets one, which no scripted run has a hand for. Pair with `QUANTICK_DRAWING_INSPECTOR=1` and `QUANTICK_DRAWINGS_DEMO_SELECT=<tool id>`. Off-screen coordinates are accepted on purpose (`3000,2000` photographs the clamp repairing a position from a bigger monitor); text that is not a point is refused, so a typo shows automatic placement rather than an invented pixel. To photograph the *restore* instead of the drag, hand-write `inspector_position = [x, y]` into the `[chrome]` table of a scratch `QUANTICK_UI_STATE` file |

Once merged, move it into the table above.

Landing with the inverted chart goal (`feat/inverted-chart`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_INVERTED=1` | the chart **upside down** — low prices at the top — through the very setter the axis menu's "Inverted chart" checkbox calls (`PriceView::set_inverted`), on **both panes** of a split layout so no surface is silently audited the right way up. The state is otherwise reached by a long price-gutter drag to the flip threshold (`FLIP_SPAN_FACTOR`, 40 auto-fit spans) plus a second pull, a gesture no scripted run can perform. Every price-mapped layer turns over together — candles, wicks, axis labels, drawings, footprint, heatmap, bubbles, live strip, paper lines, trade paint — so this is the frame that audits all of them at once. Anything but `1` leaves the chart the right way up |
| `QUANTICK_CONTEXT_MENU=axis` | the **price axis's own context menu** (the Inverted chart toggle, and the pointer compass's price half), via the same scripted right-click path as `chart`/`tape` — the click lands on the gutter rect the draw published (`last_price_gutter`), never on a guessed pixel. `scale` is an alias |
| `QUANTICK_CONTEXT_MENU=time` | the **time axis's own context menu** (the pointer compass's time half), the gutter menu's twin. Lands on the *candles'* segment of the bottom strip, published as `last_time_strip` — past the lane divider the strip is the tape's rolling window and carries no menu. `clock` is an alias |
| `QUANTICK_POINTER=<fx>,<fy>` | the **mouse parked over the candles**, both values fractions of the **flow** pane's candle area (the pane `QUANTICK_CONTEXT_MENU` also aims at, so the two hooks agree about which canvas they mean) (`0.5,0.5` its middle, `0.99,0.5` out in the projection margin past the newest bar). Everything that exists only while a pointer is over the chart — the pointer compass's two axis tags, the crosshair, every hover readout — is otherwise invisible to anything but a hand on the mouse. Fractions rather than pixels because the candles' pane moves with the window size, the lane divider and the indicator band: an absolute pair that framed the right bar at one window size frames a different one at the next, and a capture that photographs the wrong bar and calls it a pass is worse than one that photographs nothing. Delivered as a real `PointerMoved` through `raw_input_hook`, re-issued every frame, so the chart does with it what it does with a trader's own mouse. A malformed value is logged (`POINTER_HOOK_REJECTED`) and ignored, never silently defaulted |

Once merged, move these into the table above.

Landing with the parked-context-bar fix (`fix/popup-position-every-tool`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_CONTEXT_BAR_POS=<x,y>` | the selected object's context bar **parked where a trader dragged it by the grip**, in screen points. The bar now keeps that position when the selection moves, so "parked" is a state that outlives one object and therefore one a capture has to be able to reach — and the grip drag that sets it is a gesture no scripted run has a hand for (the ParkedHand rule), exactly as with the popup's title bar. Pair with `QUANTICK_DRAWINGS_DEMO_SELECT=<tool id>`, which is what puts a bar on screen at all. Same *input* contract as `QUANTICK_DRAWING_INSPECTOR_POS`: off-screen coordinates are accepted on purpose (`3000,2000` photographs the host repairing the bar back into the pane the selection lives on, and clear of the live lane, which the parked path is held to exactly as the automatic one is), and text that is not a point is refused, so a typo shows automatic placement rather than an invented pixel. Not the same *lifetime*: the popup's parked point is a workspace field, this one is session state, so there is no `[chrome]` key to hand-write and no restore to photograph — a launch without the hook is the unparked bar. The way back is `ContextBar::clear_manual`, which the grip's double-click calls rather than reimplements — deliberately not Escape, a key a trader presses many times an hour to drop a selection |

Once merged, move it into the table above.

Landing with the paper-trading overhaul (`feat/paper-trading-overhaul`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_CMD_PREVIEW=<buy\|sell>[@<0..1>]` | the cmd-trading preview painted with nobody at the keyboard: the dashed line out to the axis, the side/kind/qty label riding beside the cursor and the gutter price chip. The held modifier is one input a capture run cannot supply (the ParkedHand rule) — and since the label follows the pointer, so is the pointer's **x**: `buy@0.15` parks the aim a sixth of the way in, `sell@0.9` next to the axis, and a bare `buy` keeps the old mid-band park. A stated fraction wins over a real pointer that happens to be over the window; the y still comes from the real hand when there is one. A forced aim **paints but never places** — a run with nobody at the keyboard is holding no modifier, so a stray click during one must not write orders into a journal, and no hand cursor is shown for it. Needs prints on the tape for a mark — pair with a live feed or `QUANTICK_PAPER_DEMO=1`. |
| `QUANTICK_PAPER_ORDER_HOVER=1` | every resting order's in-plot tag in its **open** form (`#3 BUY LMT 2 @ 95.0` with the ✕). At rest the tag is a compact pill and only opens under the pointer, so the full statement is otherwise unreachable from a scripted run — the same ParkedHand problem as above. Pair with `QUANTICK_PAPER_ORDERS`. Since the bracket work it also **parks a hand** on the first order line the pane can show, because the `SL`/`TP` handles are drawn only where a pointer actually is (a pane the hand is not on must not offer a press it will not take). One caveat, and it is the reason a capture sometimes shows tags but no handles: the parked hand only lands on the pane that *takes* paper input — the one under the pointer, else the focused one — so the handles need that pane's price range to contain an order. On BTCUSDT the flow pane autoscales to ~5 bp while `QUANTICK_PAPER_ORDERS` rests its first rung at 6 bp, which puts the order just outside it; the context pane's wider range shows it. There is no hook for *which* pane is focused, so this is the one state here still easier to reach with a real pointer than without. |
| `QUANTICK_PAPER_ORDERS=<rungs>` | rests orders around the mark on the **first print**, 1–4 rungs, each a buy limit below and a sell limit above at 6 bp × rung. This is what makes the in-plot order tag photographable at all: `QUANTICK_PAPER_DEMO=1`'s own order is 220 prints away and sits 0.4 % out — far enough to fall outside the chart's autoscaled price range (a line off-range paints no tag), close enough that a lively tape fills it before the shutter. Both sides per rung is the point: whichever way the tape moves, only one side can fill and a tag survives on screen. Combine with `QUANTICK_PAPER_ORDER_HOVER=1` for the open form, and with `QUANTICK_TRADES_DIR` at scratch. |
| `QUANTICK_PAPER_ORDER_BRACKET=1` | gives every order `QUANTICK_PAPER_ORDERS` rests a protective stop and target at 15 bp either side of the order's own price, so a working order's **bracket** is photographable: two dashed leg lines with their gutter chips and `SL`/`TP` tags, tied to an entry that has not filled. Dashed and not solid on purpose — an order's legs are a promise that arms on the fill, and the layer says which it is. Pair with `QUANTICK_PAPER_ORDER_HOVER=1`, which opens one order's tag and with it the labelled handles for the legs an order does *not* have; the two together hold every state the bracket has in one capture. Reaching either by hand needs a drag from a handle, which a scripted run has no hand for (the ParkedHand rule). |

Once merged, move them into the table above.

Landing with the tape-configuration goal (`feat/tape-own-config`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_TAPE_LAYERS=<list>` | what the **tape** draws, now that it is switched apart from the candles: comma-separated `heatmap`, `bubbles`, `no-heatmap`, `no-bubbles`, or `none` for a bare tape. Each entry calls the setter the tape menu's checkbox calls, and an unlisted layer is left as it was. The state the split exists for — `QUANTICK_BUBBLES_AUTOSTART=1` with `QUANTICK_TAPE_LAYERS=no-bubbles`, or the reverse — is set here directly; use `QUANTICK_CONTEXT_MENU=tape` when the menu itself is what needs photographing |
| `QUANTICK_TAPE_WINDOW=<auto\|90s\|2min\|120000ms>` | how much market time the tape shows: `auto` follows the bars (the default), a duration pins it. Accepts `s`, `m`/`min`, `ms` or bare milliseconds; anything else is refused rather than guessed, so a typo photographs the default instead of an invented window. This is what makes the "bubbles stay visible longer" state capturable at all — otherwise it depends on how fast the bars happen to be closing |

| `QUANTICK_CONTEXT_MENU=<chart\|tape>` | the right-click menu open on that pane, once, on the first frame that has drawn a canvas. The two panes now open **different** menus, so "open the context menu" is no longer one instruction. The click is delivered as a real secondary-button event through the app's own input path (`raw_input_hook`) rather than by reaching into egui's menu state — what opens is exactly what a trader's click opens. `tape` yields nothing on a canvas with no lane, and an unrecognised value opens nothing rather than the wrong pane's menu |

Once merged, move them into the table above.

Landing with the drawing-defaults goal (`feat/fib-defaults-and-inline-text`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_TEXT_NOTE=1` | a note placed in the middle of the window with its **on-chart editor open** — the field where the words will be read, caret already in it, with the selection's context bar beside it. The editor exists only between a placement and the next click elsewhere, so no click-free launch could photograph it; the hook goes through the same two calls a click makes (`place_with` then `begin_inline_text_edit`), never a parallel path |
| `QUANTICK_DRAWING_INSPECTOR_TAB=<style\|extra\|coordinates>` | which tab the properties panel opens on. `extra` is the tool-owned one — a Fib's **Levels** editor, where the ratios, the per-level colours and the two default controls ("Save as default" / "Reset to factory") live. Pair with `QUANTICK_DRAWING_INSPECTOR=1` and `QUANTICK_DRAWINGS_DEMO_SELECT=fib-retracement`. An unknown value leaves the default tab rather than guessing |

Once merged, move them into the table above.

Landing with the feed-recovery goal (`feat/feed-recovery-controls`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_FEED_STALL=connecting\|reconnecting\|silent` | a **stalled feed**, and the two controls that recover it: the notice card carries `Reconnect` and `Reload` (the one that fixes this stall filled, the other beside it, and the caption saying what Reload costs), and the status bar's provenance section carries the same pair — which is the only place they appear on a chart that is *full*, since the card never covers bars. One value per branch of `feed::stall::assess`: `connecting` a first connection that never landed, `reconnecting` a transport that dropped and has not come back, `silent` a socket that stayed open while the terminal behind it stopped sending. Without it these surfaces are unreachable in a capture — every one of them appears only after tens of seconds of a genuinely failing feed, and no setting and no recording produces that. It **forges no measurement**: the feed keeps running, `tape_age_ms` and the connection state are untouched, and the words come from the same three constructors the real judgement uses, so a screenshot shows the sentence a trader would really be reading. Both controls do exactly what they always do when pressed. An unrecognized value is ignored and the real judgement stands, never a silently chosen shape |
| `QUANTICK_FEED_GAP=<ms>` | the **gap seam**: a dashed amber mark, captioned with the silence (`4 min gap`), where a reconnect that kept the timeline left market time no print covers. Placed at the open of the bar halfway through the flow pane's series once there are bars to sit between, so it is on screen at the zoom a capture opens on; every pane resolves it against its own bars, because the gap is anchored in market time rather than in a slot. It goes through `Tab::record_gap`, the same function a real reconnect calls, so the run logs a real `FEED_GAP_MARKED` and the mark is the mark. Reaching it otherwise means breaking a live venue mid-capture and waiting for it to come back. A value under `MIN_MARKED_GAP_MS` (5 s) is refused rather than rounded up — a hook must not photograph a mark the application would not have drawn |

Pair with `QUANTICK_LAYOUT=time+flow` and `QUANTICK_VENUE_HISTORY_DEMO=1` for the split the controls were designed against: a time pane full of the venue's candles beside a flow pane with nothing in it, which is where the card now draws instead of across both.

Once merged, move it into the table above.

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

1. **Own target dir, on a drive with room**: build with
   `CARGO_TARGET_DIR=D:\quantick-agent-target` so the user's running exe is
   never locked and rust-analyzer never poisons fingerprints. It was `F:` until
   that drive stopped existing — check `Get-PSDrive -PSProvider FileSystem`
   before trusting this line, and pick the drive with free space: `C:` runs
   into single-digit gigabytes with a few worktrees on it, and a build that
   dies of ENOSPC looks like a compile error until you read the message.
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

Landing with the MT5 older-history goal (`feat/mt5-load-older`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_LOAD_OLDER=<pages>` | the toolbar's `+ older` button **pressed**, that many times, once the chart has bars to page back from. The button's point is what happens after the click — the prints prepended in front of what is already drawn — so a capture that can only photograph the enabled button proves the affordance exists and nothing about whether it works. Goes through `Tab::request_older_history`, the very function the click calls, so a hooked run drives the loading indicator too. Fires one page per frame and waits for each reply: the feed serves one request at a time, so pressing them together would photograph the refusal path instead of the feature. Waits up to `LOAD_OLDER_HOOK_FRAMES` (~10 s) for a first block, then gives up and logs `LOAD_OLDER_AUTOSTART_GAVE_UP` rather than hanging a capture run on a bridge that never connected. On MetaTrader it needs a bridge that declares `history_paging` (`bridge/mt5/quantick_bridge.py`, not the Expert Advisor); on a feed that cannot page, each press is answered empty and the chart is unchanged |
Once merged, move it into the table above.

Landing with the one-week candle default (`fix/frvp-candles-window-and-history`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_LOAD_OLDER_CANDLES=<spans>` | the history menu's `+ older candles` entry **pressed**, that many times, once the opening span has landed. A chart now opens on one week of venue candles (`feed::TIME_HISTORY_SPAN_MS`) and reaches the quarter a week at a time, so "what does a deep chart look like" is a state no capture reaches without a hand on the menu. Goes through `Tab::request_older_ohlcv_history`, the function the menu entry calls, so a hooked run drives the loading indicator and the prepend too. One span per frame, and it waits: the tab serves one candle request at a time. Spends `LOAD_OLDER_CANDLES_HOOK_FRAMES` (~60 s at 60 fps) across the whole run — much larger than the trade twin's, because a span is several slices of several pages and the reach is documented in thirteen of them — and every waiting frame costs a tick, so a venue that never answers gives up and logs `LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP` with the reason rather than hanging the run. The trade twin is `QUANTICK_LOAD_OLDER` — two records, two capabilities, two hooks: a feed can serve candle history without paging its tape |

Once merged, move it into the table above.

Landing with the load-older outcome (`fix/history-reach-speaks`):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_HISTORY_NOTE=<ending>` | the **outcome** of a `+ older` press, in the loading lane where the spinner was — the one line that tells a trader their press reached nothing. Named by the ending's own log token and resolved through `CampaignEnd::from_action`: `nothing_coming_back` (the venue answered empty and the run gave up), `venue_exhausted` (the record is spent), `page_budget_spent` / `print_budget_spent` / `span_cap_covered` (stopped on a budget, press again), `nothing_charted`. `reach_met` raises nothing and says so in the log — a press that worked has the chart as its answer. Raised through `Tab::raise_history_note`, the same call a settled run makes, so the picture is the picture a refusing venue gives. Without this the surface is invisible to a capture: on any feed a validation run can arrange, the reach either lands its session or the source declares it cannot page and the button never takes a press. An unknown token raises no note rather than the wrong one |

Once merged, move it into the table above.

## Reading the running app through the control plane

A screenshot shows what a window looks like. It does not say what the
application *believes* — which market, which revision, how late the tape is,
whether a control is disabled and why. The control plane answers that in
structured data, and an assertion against it is worth more than an assertion
against pixels: it does not move when a colour does.

Prefer this over reading a capture whenever the question has a structured
answer. Keep the screenshot for the questions only pixels can answer — clipping,
font, composition, "does this read".

**The fixture.** Launch with local access enabled and the scopes the read
needs, per the table above:

```powershell
$env:QUANTICK_CONTROL_ACCESS = "1"
$env:QUANTICK_CONTROL_SCOPES = "all-reads,observe.evidence,observe.screenshot"
```

**The client.** `quantick-mcp` is a STDIO MCP server; feeding it JSON-RPC lines
is a complete client, no extra tooling. It discovers the running instance
itself and never starts one.

Build it first — step 1 of the launch workflow builds `quantick-app` only, and
the adapter is a separate binary in the same target directory:

```powershell
$target = "D:\quantick-agent-target"     # the same one the launch used
cargo build -p quantick-mcp
$mcp = Join-Path $target "debug\quantick-mcp.exe"
```

```powershell
$lines = @(
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"ui-harness","version":"1"}}}',
  '{"jsonrpc":"2.0","method":"notifications/initialized"}',
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"quantick_get_scene","arguments":{}}}'
) -join "`n"
$lines | & $mcp --profile observer
```

Every answer is one JSON line on stdout; `result.structuredContent` is the
capability's own result, and `result.isError` with a `control.*` code is a
refusal you can branch on. Useful calls:

| Ask | Call |
| --- | --- |
| What is on screen, by name | `quantick_get_scene` |
| Which market, which bars, which layout | `quantick_get_snapshot` with the scopes |
| Is the frame healthy, is the tape late | `quantick_get_diagnostics` |
| What changed since I looked | `quantick_read_events` / `quantick_wait_for_change` |
| Everything at one instant, hashed | `quantick_capture_evidence` |

**Evidence bundles.** `quantick_capture_evidence` freezes the named scopes, the
events around them and the effective configuration into one hashed bundle and
answers with a manifest. Read it back with `quantick_invoke` on
`evidence.read`, page by page, and concatenate the base64 chunks: the bytes are
the bundle's canonical JSON and their SHA-256 is the manifest's
`content_digest`. Two fields decide whether an assertion is sound:

- `coverage` — what the capture left out, and why, as codes. A scope you did
  not name is in `omitted_scopes`; a field the application could not fill is in
  `unavailable_fields` with the JSON Pointer that finds it. `complete` is never
  true, and a capture never pretends to be the whole session.
- `screenshot.capture_revision` — equal to the bundle's own `capture_revision`,
  which is what makes `screenshot.control_regions` trustworthy: each named
  control's rectangle in the image, in physical pixels, with `within_image`
  saying whether the window was clipping it. That is the pair a visual defect
  is diagnosed from — the picture plus the names.

Without a client on the socket, `QUANTICK_CONTROL_EVIDENCE` takes the same
capture from a launch and logs the manifest as `CONTROL_EVIDENCE_CAPTURED`.
Bundles live in memory for fifteen minutes, are cleared when access is turned
off, and are never written to disk.

Landing with the history-reach goal (*a load-older press that reaches the previous session*):

| Hook | Reaches |
| --- | --- |
| `QUANTICK_HISTORY_REACH=<token>` | pins how far one `+ older` press reaches, overriding what the workspace saved: `page` (one request of the page size — the press every release before this one had) or `previous-session` (keep asking until the tape reaches past the market's last close plus a lead into the session before it). The tokens come from `HistoryReach::ALL`, the same list the history menu is drawn from, so a hook can reach every reach a trader can. A token this build does not know is refused out loud (`HISTORY_REACH_HOOK_UNKNOWN`) and leaves the current reach alone — a silent fallback would look like a press ignoring the run it was told to make. Pair it with `QUANTICK_LOAD_OLDER=1` to photograph a run in flight: with `previous-session` a single press keeps paging, so the loading indicator stays up across several replies |
| `QUANTICK_HISTORY_REACH` note for `QUANTICK_LOAD_OLDER` | with `previous-session` set, **one hooked press is one run**, not one request: the hook waits on the same loading task the run holds, so `QUANTICK_LOAD_OLDER=3` is three runs and not three pages. The `+ older` button itself is drawn disabled while a run is in flight (a press during one does nothing, and a live button that swallows it reads as broken), so a capture of the button mid-run photographs the greyed state and its reason — which is the state to photograph |
| `QUANTICK_VENUE_LEAD_IN=1` | pins the View → *venue candles on charts cut by trades* switch on. Off by default and off for anything but `1`, because that is the whole point of the switch: a tick, volume, dollar or imbalance chart has always opened holding only the prints this session saw, and nothing goes in front of them unasked. On, the venue's own 1-minute candles are installed unfolded in front of a chart cut by trades — the only state in which a tick chart shows yesterday, and one no capture reaches without a hand in the View menu. Reaches `Tab::set_venue_lead_in`, the function the checkbox calls, so a hooked run refolds exactly as a click does |

Once merged, move these into the table above.

## Adding a new hook

New surface → new `QUANTICK_*` env hook in the same commit: read the var next
to the existing autostart block in `crates/app/src/app.rs`, call the same
function the manual toggle calls, default off. Then add one row to the
registry table above. That row is part of the feature's definition of done.
