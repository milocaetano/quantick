# Indicators — full audit report

Scope: adding, configuring, rendering and scripting indicators. Read-only analysis of `crates/app/src/{indicator_panel.rs, indicators/, indicator_render.rs, indicator_worker.rs, pane.rs, toolbar.rs, app.rs}` plus the input/descriptor contracts in `crates/indicators` and `crates/pine`.

**Headline:** the indicator engine is complete and well-tested; the indicator *surface* is one nested icon menu. The project's own UX spec (`docs/ux/ui-design-model.md` §9) describes a chart legend, an Indicators dock tab and an Insert menu — none of the three exist in code. The user's "editing properties feels strange" traces to four concrete, verifiable causes documented in §3.1 (part 2).

---

## 1. INVENTORY — every user-facing element

### 1.1 Entry point (the only one)

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 1 | INDICATORS icon (`CHART_LINE` glyph, 16 px, no text label) | `crates/app/src/toolbar.rs:579-586` | Left-click | Opens the indicator menu. Tinted `theme::ACCENT` when ≥1 indicator is active on the focused pane, `theme::TEXT_MUTED` otherwise (`toolbar.rs:581-585`) |
| 2 | Icon tooltip: *"indicators: add or manage overlay and pane indicators"* | `toolbar.rs:659` | Hover | Only text in the whole UI that names the feature |

The icon lives in the LAYERS group (`toolbar.rs:48-49, 515-516`), immediately beside the bubbles / heatmap / live-strip **toggles**. LAYERS is never folded into the `⋯` overflow (`toolbar.rs:127-137` — the collapse steps never touch it), so the icon is always present. There is **no** menu-bar path: `File`, `View`, `Tools`, `Help` (`app.rs:1773-1918`) contain nothing about indicators. There is **no** keyboard shortcut (the shortcut table is `app.rs:1729-1748`). There is **no** left-rail entry (`toolrail.rs` mentions indicators only inside a test comment).

### 1.2 Inside the INDICATORS menu — "add" section

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 3 | `＋ Add EMA(9) on close` | `toolbar.rs:587-593` | Click | `ToolbarAction::AddEmaIndicator` → `add_native_indicator(SavedKind::NativeEma)` (`app.rs:1001-1003`, `1293-1308`). Hardcoded len 9 / source close (`DEFAULT_EMA_LEN`, `app.rs:68`). Closes the menu |
| 4 | `＋ Add CVD pane` | `toolbar.rs:594-597` | Click | `ToolbarAction::AddCvdIndicator` → native CVD (`app.rs:1004-1006`). Closes the menu |
| 5 | `scripts` section header (11 px, muted, non-interactive) | `toolbar.rs:598-604` | — | Shown only when the library is non-empty |
| 6..N | One button per script, `FILE_CODE` glyph + file name | `toolbar.rs:605-610` | Click | `ToolbarAction::AddScriptIndicator(index)` → `add_script_indicator` (`app.rs:1117-1180`). Closes the menu |

Shipped library (`crates/app/src/indicators/library.rs:24-40`): `ema.pine`, `cvd.pine`, `delta_histogram.pine`, `vwap_cumulative.pine`, `zigzag.pine`, `range_box.pine`, plus every `*.pine` found in the scripts folder.

### 1.3 Inside the INDICATORS menu — "manage" section

One `ui.horizontal` row per active indicator, in add order (`toolbar.rs:615-656`), preceded by a separator when non-empty (`toolbar.rs:612-614`):

| # | Element | Location | Trigger | Effect |
|---|---|---|---|---|
| 7 | Status dot (non-interactive label) | `toolbar.rs:617-628` | — | `WARNING_CIRCLE`/red if errored → `WARNING`/accent if stale → `EYE_SLASH`/muted if hidden → `CIRCLE`/green otherwise. Precedence hardcoded in that order. **No tooltip, no click target** |
| 8 | Indicator label (non-interactive) | `toolbar.rs:629` | — | `descriptor.short_title` or `title` (`indicators/mod.rs:90-95`). Not clickable, no hover |
| 9 | Eye `small_button` | `toolbar.rs:630-640` | Click | `ToggleIndicatorHidden(slot)` → `indicators.toggle_hidden` (`app.rs:1007-1012`). Render-side only, no recompute. Tooltip: *"hide/show without removing (no recompute)"*. **Does not close the menu** |
| 10 | Gear `small_button` | `toolbar.rs:641-647` | Click | `OpenIndicatorSettings(slot)` → builds a `SettingsDialog` from `view.input_values.clone()` (`app.rs:1028-1044`). Tooltip: *"settings (applying recomputes from scratch)"*. **Does not close the menu** |
| 11 | Trash `small_button` | `toolbar.rs:648-654` | Click | `RemoveIndicator(slot)` → immediate removal, UI first then worker (`app.rs:1013-1024`). Tooltip: *"remove this indicator"*. **No confirmation, no undo. Does not close the menu** |

The row carries no colour swatch, no last value, no source/pane indication, and no drag handle.

### 1.4 The settings dialog

`indicator_panel.rs:39-81`, drawn from `app.rs:2901` via `draw_indicator_settings` (`app.rs:1061-1108`).

| # | Element | Location | Notes |
|---|---|---|---|
| 12 | Window titled `Settings — {label}` | `indicator_panel.rs:46` | Label snapshotted at open time (`app.rs:1039`); goes stale if inputs change the title |
| 13 | Window id keyed by slot | `indicator_panel.rs:47` | egui remembers position per slot |
| 14 | Title-bar `✕` | `indicator_panel.rs:48, 77-79` | Maps to `Cancel` — draft discarded |
| 15 | `.collapsible(false)` | `indicator_panel.rs:49` | No collapse |
| 16 | `.resizable(false)` | `indicator_panel.rs:50` | Cannot be resized; **no `ScrollArea`** |
| 17 | 2-column `Grid`, label left / widget right | `indicator_panel.rs:52-60` | One row per declared input |
| 18 | *"this indicator declares no settings"* (muted) | `indicator_panel.rs:61-66` | Shown when `specs.is_empty()` |
| 19 | `Apply` button | `indicator_panel.rs:69-71` | Sends `SetInputs` **and closes** (`app.rs:1093-1105`) |
| 20 | `Cancel` button | `indicator_panel.rs:72-74` | Drops the draft |

No `OK`, no `Reset to defaults`, no per-input revert, no input descriptions/tooltips, no grouping, no tabs.

### 1.5 Property editors — every input widget

Generated purely from `InputSpec` (`crates/indicators/src/input.rs:129-208`). One `match` arm each (`indicator_panel.rs:86-176`):

| # | Input type | Widget | Location | Constraints honoured |
|---|---|---|---|---|
| 21 | `Int` | `DragValue` | `indicator_panel.rs:88-111` | `min`/`max` → `.range()`; `step` → `.speed()`. **`options` ignored** despite the spec documenting a dropdown (`input.rs:148-150`) |
| 22 | `Float` | `DragValue` | `indicator_panel.rs:112-133` | Same; falls back to `DEFAULT_FLOAT_DRAG_SPEED = 0.1` (`indicator_panel.rs:16`). **`options` ignored** |
| 23 | `Bool` | `checkbox(current, "")` | `indicator_panel.rs:134-137` | Empty checkbox text → the label in column 1 is not a click target |
| 24 | `Color` | `color_edit_button_srgba_unmultiplied` | `indicator_panel.rs:138-144` | Full RGBA |
| 25 | `Str` | `text_edit_singleline`, or `ComboBox` when `options` non-empty | `indicator_panel.rs:145-158` | The combo branch is unreachable today — `pine` always emits `options: Vec::new()` (`compile.rs:1441`) |
| 26 | `Source` | `ComboBox` over `SourceId::ALL` | `indicator_panel.rs:159-168` | All 14 series, including flow series (`input.rs:50-65`). **No filtering by `is_price_scaled`** |
| 27 | Fallback | *"(unrenderable input)"* | `indicator_panel.rs:171-174` | Spec/value variant mismatch |

**Not editable at all:** plot colour, line width, plot style, offset, marker, fills. These live in `PlotSpec` on the descriptor and can only be changed by editing the `.pine` source. TradingView's entire "Style" tab has no equivalent.

### 1.6 Chart-side rendering

| # | Element | Location | Notes |
|---|---|---|---|
| 28 | Overlay plots on the price chart | `indicator_render.rs:116` (`draw_overlays`), called `pane.rs:1610-1619` | **No legend, no label, no colour key, no value readout, no interaction of any kind** |
| 29 | Overlay draw objects (lines/boxes/labels) | `indicator_render.rs:623+`, called `pane.rs:1622-1631` | Non-interactive |
| 30 | Pane background + top hairline + right rule | `indicator_render.rs:191-208` | — |
| 31 | Pane title, top-left, 11 px muted | `indicator_render.rs:263-269` | `painter.text` only — **no `Sense`, not clickable, not hoverable** |
| 32 | Pane headline value, top-right, monospace muted | `indicator_render.rs:270-278`, `last_value` at `318-330` | Preview value first, else last non-NaN committed |
| 33 | `"{label} — warming up"`, pane centre | `indicator_render.rs:210-219` | Shown while every visible value is NaN |
| 34 | Pane y-axis gridlines + gutter labels | `indicator_render.rs:286-316` | Edge labels suppressed within 7 px (`AXIS_LABEL_EDGE_MARGIN_PX`) |
| 35 | Pane zero line | `indicator_render.rs:227-236` | Only when 0 ∈ range |
| 36 | Warmup gaps (NaN breaks polylines) | `indicator_render.rs:1-10` | Data-honesty rule, correctly implemented |

### 1.7 Pane management

| # | Element | Location | Notes |
|---|---|---|---|
| 37 | Pane creation | `indicators/mod.rs:266-268` | **Implicit**: a pane appears iff `!descriptor.overlay && !hidden && error.is_none()`. The user never chooses |
| 38 | Pane height | `indicators/mod.rs:23` (`PANE_HEIGHT_FRAC = 0.20`), split at `273-291` | Fixed 20% of chart height each. **No draggable divider, no collapse, no resize** |
| 39 | Pane cap | `indicators/mod.rs:26` (`MAX_PANES = 3`), enforced by `.take(MAX_PANES)` at `241-256` | The 4th pane indicator is **silently not drawn** |
| 40 | Pane y-axis zoom | `pane.rs:1380-1388` → `axis_zoom_gesture` (`pane.rs:208-235`) | Drag/scroll over the pane's gutter zooms its scale; double-click resets to auto-fit. Same feel as the price axis |
| 41 | Pane removal | Only via the trash button (#11) or by hiding (#9) | No per-pane close control |

### 1.8 Script library / Pine loading

| # | Element | Location | Notes |
|---|---|---|---|
| 42 | Scripts folder | `library.rs:18-20` | `QUANTICK_INDICATORS_DIR`, else `./indicators`. Created on first scan (`library.rs:74`) |
| 43 | Library scan | `library.rs:70-89, 107-157` | **Startup only** — a `.pine` file added while the app runs never appears in the menu until restart |
| 44 | Embedded starters | `library.rs:24-40` | 6 scripts, shadowed by a same-named user file (`library.rs:150-154`) |
| 45 | Hot reload | `app.rs:1231-1289` | 1 s mtime poll (`SCRIPT_RELOAD_POLL_INTERVAL`, `app.rs:70`). Success → recompile + replay; failure → `stale` flag, last good version keeps running |
| 46 | Read failure on add | `app.rs:1138-1177` | Builds an error slot so the click is never a no-op |
| 47 | Persistence | `indicators/state_file.rs`, `app.rs:1331-1489` | `indicators-state.toml`, 1 s debounce. **Flow pane of the startup tab only** — indicators on a time pane or a later-opened tab are session-only (documented at `app.rs:1402-1409`) |

There is **no** in-app script editor, no "open scripts folder" command, no "reload library", no `.pine` file picker, and no Pine dialect reference in `Help` (the `Help` menu has one item: replay file format, `app.rs:1913-1918`).

*(continues in 2/3: FLOWS + root-cause diagnosis)*


## 2. FLOWS

### (a) Adding an indicator

1. Locate an unlabelled 16 px line-chart glyph at the right end of the toolbar, sitting among three layer *toggles*.
2. Click → menu opens.
3. Pick `＋ Add EMA(9) on close`, `＋ Add CVD pane`, or a `.pine` file name.
4. Menu closes. The indicator lands on the **focused pane** (`app.rs:999-1000`, `1303`) — the flow pane unless a split canvas is active and the time pane was clicked last.
5. Placement is decided for you: EMA on close → overlay (`native/ema.rs:41`, `overlay: source.is_price_scaled()`); CVD → pane.
6. A pane indicator shows `"CVD — warming up"` until it has non-NaN values.

Zero preview, zero search/filter, zero categorisation, zero description of what a script does before loading it. The library is a flat list of file names. Note: the app does **not** open with EMA+CVD by default — that is gated behind `QUANTICK_INDICATORS_AUTOSTART=1` (`app.rs:630-640`); a fresh user opens with nothing unless `indicators-state.toml` restores a set.

### (b) Editing properties — the flow the user called strange

1. Click the unlabelled toolbar icon.
2. Scroll past the add entries (2 natives + N scripts) to reach the separator.
3. Find the row by its text label.
4. Hit a `small_button` gear roughly 14 px wide.
5. **The menu stays open** — the gear branch never calls `ui.close_menu()` (`toolbar.rs:641-647`), unlike every add branch which does (`toolbar.rs:592, 596, 608`).
6. A floating `egui::Window` appears at egui's automatic cascade position (no `.default_pos`, no `.pivot` — `area.rs:411` `automatic_area_position`), i.e. upper-left of the viewport, far from the right-side toolbar where the click happened.
7. **The still-open menu paints over it**: egui menus use `Order::Foreground` (`egui-0.29.1/src/menu.rs:184`) while `Window` defaults to `Order::Middle` (`area.rs:139`).
8. Edit values. **Nothing on the chart changes** — every widget writes into `dialog.draft` (`indicator_panel.rs:56-57`), a clone taken at open time.
9. Click `Apply` → `SetInputs` to the worker → construct anew, replace, replay from scratch (`indicator_worker.rs:379-412`). **The dialog closes.**
10. To try a different value, repeat from step 1.

Additional behaviours:
- Only one dialog can be open (`app.rs:403`, `indicator_settings: Option<…>`). Opening a second **silently discards the first draft** (`app.rs:1037` overwrites).
- The dialog is app-global with a `TabSlot` target (`app.rs:1062`, `1073-1082`). Switching tabs leaves it floating over an unrelated chart, still editing the other tab's indicator.
- It closes itself if the indicator is removed underneath (`app.rs:1083-1087`).
- Apply retargets nothing: the slot captured at open time wins (`app.rs:1095-1096`).
- If the worker clamps a value (e.g. EMA `len.max(1)`), it honestly mirrors what was bound — but the dialog has already closed, so the user never sees the correction.
- Half the shipped library declares **zero inputs**: `cvd.pine`, `delta_histogram.pine`, `vwap_cumulative.pine`, and the native CVD (`native/cvd.rs:34`). The gear on those opens a window whose entire content is *"this indicator declares no settings"* plus an `Apply` that fires a pointless full recompute.

### (c) Removing an indicator

Toolbar icon → menu → find row → trash `small_button`. Immediate, no confirmation, no undo, no toast. Contrast with drawings, which get a confirmation for locked objects and an Undo toast (`app.rs:1947-1959`).

### (d) Moving an indicator between panes

**Not possible.** Overlay-vs-pane is the descriptor's `overlay` flag, and pane order is add order. The only way to move an EMA off the price chart is to change its `Source` input to a flow series, which flips `overlay` via `is_price_scaled()` (`native/ema.rs:41`) — the indicator vanishes from the chart and reappears as a pane with no explanation. There is no "move to pane", no reorder, no drag.

### (e) Loading a custom `.pine` script

1. Find the folder: `./indicators` beside the working directory, or `QUANTICK_INDICATORS_DIR`. **Nothing in the UI states this path.**
2. Drop the `.pine` file there.
3. **Restart the app** — the scan runs once at startup (`library.rs:70`).
4. Toolbar icon → menu → the file name now appears under `scripts`.
5. Click it. Compile errors produce an error slot; the message goes to the worker and into `view.error`, and is then displayed **nowhere**.
6. Once loaded, saving the file hot-reloads within ~1 s. A save with errors sets `stale` and keeps the last good version running — again with no visible message.

---

## 3.1 ROOT-CAUSE DIAGNOSIS: "editing indicator properties feels strange"

Four independent causes, each with code evidence. Together they fully explain the report.

**Cause 1 — the entry point is not where any user looks.** TradingView's convention is: the indicator writes its name into the chart legend, and you double-click that name. quantick draws **no legend at all** for overlays (`indicator_render.rs:116` paints plots and nothing else), and the pane title is `painter.text` with no `Sense` (`indicator_render.rs:263-269`). The object on screen is inert. The only route is an unlabelled toolbar glyph → menu → 14 px gear. The project's own spec called for exactly the TradingView affordance — *"Legend (chart overlay, top-left) — one row per active indicator: colour dot, short title, last value; eye / gear / remove appear on hover"* (`docs/ux/ui-design-model.md:304-306`) — and it was never built.

**Cause 2 — the menu does not get out of the way.** The gear does not close the menu, and egui paints menus above windows. The user clicks a gear and gets a dialog partly hidden behind the popup they just clicked in, at the opposite corner of the screen from where they clicked. That alone reads as broken.

**Cause 3 — no live preview, and Apply closes.** Every parameter edit is blind: you set 21, click Apply, the window vanishes, and only then do you see the result. Tuning a length — the single most common indicator interaction — costs four clicks and a menu traversal *per attempt*. TradingView applies changes live as you type/drag, keeps the dialog open, and offers OK/Cancel to commit or revert.

**Cause 4 — the app contradicts itself.** quantick already has an excellent property editor: the drawing inspector (`app.rs:2152-2560`). It opens **automatically on selection**, has **tabs** (Style / Extra / Coordinates, `app.rs:2317-2323`), applies **live** with undo coalescing (`app.rs:2346-2352`, `2404-2427`), can be **pinned as a dock panel or floated**, has a title bar with hide/pin/close, and even has a deliberate default position constant (`DRAWING_INSPECTOR_DEFAULT_POSITION`, `app.rs:65`). A user who has edited a trendline in quantick has learned "select the thing → the inspector appears → changes are live". Indicators break every part of that learned model. The strangeness is not abstract — it is measured against the app's own other half.


## 3.2 FINDINGS BY SEVERITY

### BLOCKER

**B1. Indicator errors are computed, tested, and never shown.** `view.error: Option<EvalError>` carries a message with file:line:col and a stable code (`PINE_NO_SECURITY`, etc. — heavily tested at `indicator_worker.rs:725-735`). The UI reads only `.is_some()` (`app.rs:923`); `IndicatorMenuEntry` (`toolbar.rs:210-221`) carries booleans, so the message is structurally unreachable from the toolbar. An errored indicator is filtered out of both render paths (`indicators/mod.rs:237, 267`) and simply disappears from the chart, leaving a red dot inside a menu the user has no reason to open. Same for `stale`, whose message is likewise never rendered. *Heuristic: visibility of system status; help users recognize and recover from errors.* Directly violates the plan's own requirement — *"Error display: full PineError rendering (file:line:col + message + code)"* (`docs/indicator-system-plan.md:574-576`) — and the project's data-honesty rule.

**B2. The 4th pane indicator silently does not exist.** `visible_panes()` ends in `.take(MAX_PANES)` (`indicators/mod.rs:241-246`). The menu shows a healthy green dot, the state file records it, the worker computes it every bar, and nothing draws. No notice, no disabled add entry, no explanation. *Heuristic: visibility of system status.* The cap itself is defensible (`indicators/mod.rs:25-26` calls it "the honest alternative to shrinking panes into unreadability"); the silence is not.

### MAJOR

**M1. No chart legend for overlay indicators.** An EMA overlay is an unlabelled coloured line. Two overlays are two unlabelled lines. There is no way to tell which is which, read a value at the cursor, hide one, or reach its settings. Spec'd at `ui-design-model.md:304-308`, absent from the code. *Recognition rather than recall.*

**M2. Settings are draft-only with no live preview, and Apply closes the dialog.** See Cause 3. `indicator_panel.rs:56-57, 69-71` + `app.rs:1093-1105`. *User control and freedom; flexibility and efficiency of use.*

**M3. The still-open menu occludes the dialog it spawned.** `toolbar.rs:641-647` (no `close_menu`) + egui's `Order::Foreground` menus vs `Order::Middle` windows. *Consistency; aesthetic and minimalist design.*

**M4. Opening a second settings dialog silently discards the first draft.** `app.rs:403, 1037`. No warning, no queueing. *Error prevention.*

**M5. Delete has no confirmation and no undo.** `app.rs:1013-1024`. An indicator with a hand-tuned parameter set is one mis-click from gone; the state file is rewritten 1 s later (`INDICATOR_STATE_SAVE_DEBOUNCE`). Drawings in the same app get a confirmation path and an Undo toast. *User control and freedom; consistency.*

**M6. Plot style is not editable anywhere.** Colour, width, plot type, offset and markers live in `PlotSpec` and can only be changed by editing the `.pine` file. There is no Style tab, and native EMA/CVD have no source file to edit at all. Half of TradingView's settings dialog is missing.

**M7. A new `.pine` file requires an app restart.** `library.rs:70` scans once; the doc comment states this as intentional, but there is no "rescan" command and no message telling the user why their new file is absent. Combined with M8, the custom-script flow is effectively undiscoverable without reading the source.

**M8. The scripts folder is never named in the UI.** `QUANTICK_INDICATORS_DIR` / `./indicators` (`library.rs:18-20`) appear only in code and a log line. No "open scripts folder" action, no path in the menu, no Pine dialect reference under `Help` (which has exactly one entry, `app.rs:1913-1918`). *Help and documentation.*

**M9. Panes cannot be resized, reordered or collapsed.** Fixed 20% each (`indicators/mod.rs:23`, `273-291`). Three panes take 60% of the chart, non-negotiably. Spec'd as *"draggable dividers; a pane collapses to its header. Pane headers carry name + live value + collapse chevron"* (`ui-design-model.md:310-313`) and as a v1 limitation in the plan (`indicator-system-plan.md:552-553`).

**M10. `Source` offers all 14 series with no guidance.** `indicator_panel.rs:159-168` lists `SourceId::ALL` flat. Picking a flow series on an overlay EMA silently relocates it from the price chart to a new pane (`native/ema.rs:41`) — or, if three panes already exist, to nowhere (B2). `SourceId::is_price_scaled()` exists (`input.rs:93-105`) and is not used by the UI to warn or group.

### MINOR

**m1.** `Int`/`Float` `options` silently ignored. `input.rs:148-150` documents *"rendered as a dropdown"*; `indicator_panel.rs:89-96, 113-119` swallow `options` in a `..` pattern. Latent today — `pine` always emits empty options (`compile.rs:1404, 1416, 1441`), which also makes the `Str` combo branch dead — but the doc comment is a lie waiting for the first native that uses it.

**m2.** The dialog cannot scroll or resize. `.resizable(false)` with no `ScrollArea` (`indicator_panel.rs:50-60`). Shipped scripts declare ≤2 inputs so nothing breaks today; a 15-input script puts `Apply` off-screen.

**m3.** Bool label is not a click target. `ui.checkbox(current, "")` (`indicator_panel.rs:136`) leaves a bare ~16 px box.

**m4.** Dialog title goes stale. Snapshotted at open (`app.rs:1039`); `EMA(9)` retitled `EMA(21)` still reads `Settings — EMA(9)`. Masked today because Apply closes.

**m5.** `Apply` on a zero-input indicator triggers a full rebuild for nothing (`app.rs:1093-1105`). Affects 4 of the 8 shipped indicators.

**m6.** Status-dot precedence is invisible (`toolbar.rs:617-628`) — a hidden **and** errored indicator reads as merely errored. No tooltip.

**m7.** The dialog does not follow tab switches (`app.rs:1062, 1073-1082`), so it floats over whatever chart is now on screen.

**m8.** No search or filter in the library list — a folder with 40 scripts becomes a 40-item flat menu (`toolbar.rs:605-610`).

**m9.** Recompute progress is never shown. `IndicatorView::rows()` is commented *"the M4 progress readout reads it live"* (`indicators/mod.rs:74`) but is `allow(dead_code)` outside tests. A rebind over long history shows a frozen plot with no feedback; the plan called for progress (`indicator-system-plan.md:733`).

### NIT

**n1.** The INDICATORS icon is visually identical in weight and placement to the layer toggles beside it, but it is a menu, not a toggle.
**n2.** `＋ Add EMA(9) on close` hardcodes its parameters in the label; no way to add an EMA of a different length in one step.
**n3.** Stale M1/M2 comments describe UI that has since shipped: `toolbar.rs:574-576`, `app.rs:66-67`.

---

## 4. QUICK WINS vs STRUCTURAL

### Quick wins (small, local, high leverage)

1. **Call `ui.close_menu()` in the gear/eye/trash branches** — `toolbar.rs:640, 647, 654`. Three lines. Kills the occlusion problem (M3) outright.
2. **Give the dialog a deliberate position** — `.default_pos(...)` in `indicator_panel.rs:46-50`, mirroring `DRAWING_INSPECTOR_DEFAULT_POSITION` (`app.rs:65`); ideally near the pointer that opened it.
3. **Make Apply not close** — split into `Apply` (send, stay open) and `Close`, or apply live on `.changed()` as the drawing inspector does (`app.rs:2346-2352`). The worker already handles rebind-and-replay atomically, so live apply is close to free; debounce if a rebind over long history proves expensive.
4. **Add `ScrollArea` + `.resizable(true)`** to the dialog (`indicator_panel.rs:50`). Two lines, removes m2 permanently.
5. **Put the error and stale text on the menu row** — widen `IndicatorMenuEntry` (`toolbar.rs:210-221`) from `errored: bool` / `stale: bool` to `Option<String>` and hang it off the status dot with `.on_hover_text`. Fixes the worst of B1 for a handful of lines; the strings already exist on `IndicatorView`.
6. **Disable the pane-adding entries at the cap, with a reason** — `add_enabled(panes < MAX_PANES, …).on_disabled_hover_text("three panes is the maximum — remove or hide one first")`, plus a distinct dot for an over-cap row. Fixes B2 cheaply.
7. **Move the label into the checkbox** — `ui.checkbox(current, title)` with an empty left cell (`indicator_panel.rs:134-137`). Fixes m3.
8. **Confirm or undo on delete** — reuse the existing `DrawingToast`/undo machinery (`app.rs:1947-1959`) for M5.
9. **Render `Int`/`Float` `options` as a combo, or delete the field and its doc claim** (`indicator_panel.rs:88-133` + `input.rs:148-150`). Fixes m1 either way.
10. **Show the scripts folder path in the menu** under the `scripts` header, plus a "Rescan library" item re-running `ScriptLibrary::scan()` (`library.rs:70`). Fixes M7/M8 without a file watcher.
11. **Tooltip or group the source dropdown** by `is_price_scaled()` (`input.rs:93-105`) — "Price" vs "Flow". Partial M10.
12. **Warn when an edit relocates an indicator** — after `SetInputs`, if `descriptor.overlay` flipped, raise the existing notice/toast. Completes M10.

### Structural (deeper redesigns — each already specified in `docs/ux/ui-design-model.md` §9)

**S1. Build the chart legend.** Highest-value change and the direct fix for the reported complaint. One row per active indicator at the chart's top-left: colour dot, short title, last value; eye / gear / remove on hover; **double-click the name opens settings**. Red row for an error (click for full file:line:col + code), amber for stale. Fixes M1, most of B1, the Cause-1 entry-point problem, and gives overlays the identity they lack — in one surface, and the surface TradingView users already know. New module beside `indicator_render.rs`; needs `IndicatorView` to expose `error`/`stale` text and the first plot's `base_color`, both already present.

**S2. Reuse the drawing inspector for indicators.** Instead of a bespoke floating window, mount indicator properties in the same pinnable/floatable inspector host drawings use (`app.rs:2548+`), with tabs **Inputs / Style / Visibility**. Resolves Cause 4 by construction — one property-editing model for the whole app — and unlocks S3 for free. Requires generalising the inspector host from `Drawing` to a small trait; title bar, pin/float, docking and undo-coalescing are already written and tested.

**S3. Make `PlotSpec` editable (the Style tab).** Per-plot colour, width, style and a visibility checkbox, stored as a UI-side style override layered over the descriptor so a hot reload does not stamp on it. The missing half of the settings dialog (M6) and the one thing a Pine-file edit cannot substitute for on native indicators.

**S4. Draggable pane dividers + collapse chevrons + pane headers.** Replace the fixed `PANE_HEIGHT_FRAC` with per-pane fractions persisted in `indicators-state.toml` (or the `ui-state.toml` the design model reserves, `ui-design-model.md:500-501`). Make the pane header a real widget: name + live value + collapse chevron + gear, which also gives pane indicators an in-place settings entry point. Fixes M9 and reinforces S1.

**S5. An Indicators dock tab.** `DockTab` currently has L2 / Bubbles / Session / Trading (`dock.rs:30-43`). Adding `Indicators` gives room for the library browser with descriptions and search, reordering, add-to-pane targeting, and a persistent error log — the "full manager" the spec describes (`ui-design-model.md:307-309`), with the legend as the fast path. Where M7/M8/m8 get proper answers rather than tooltip-sized ones.

**S6. An `Insert` menu.** `Insert → Indicator…` (opens the dock tab), `Insert → EMA / CVD / …` for natives, plus the drawing tools (`ui-design-model.md:317-319, 334`). Discoverability for anyone who never guesses what the line-chart glyph does, and the natural home for a keyboard shortcut. Fixes n1 by giving the feature a labelled path that does not compete with the layer toggles.

**S7. Move-to-pane / reorder.** Let a user send an overlay to its own pane and back, and reorder panes — independent of the `overlay` flag the descriptor declares. Needs a UI-side placement override on `IndicatorView` plus a display-order field; today placement is fully derived (`indicators/mod.rs:266-268`) and the user has no vote (flow (d)).

**Uncertain / needs a running build:** the exact visual overlap between the open menu popup and the settings window (M3) — the egui paint ordering is unambiguous in the 0.29 source, the geometry of the overlap is not.

