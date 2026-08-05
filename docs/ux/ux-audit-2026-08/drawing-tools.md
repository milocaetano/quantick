# Drawing tools — full audit report

Scope: `crates/app/src/drawings/**`, `crates/app/src/toolrail.rs`, and the wiring in `crates/app/src/app.rs`, `crates/app/src/pane.rs`, `crates/app/src/tab.rs`. Read-only; no files were modified. Reference spec: `docs/drawing-toolbar-ux.md`.

## 1. INVENTORY

### 1.1 Drawing tools (the whole registry)

Registered in one macro list at `crates/app/src/drawings/mod.rs:393-399`. Five entries, four rail slots (the two Fib tools fold into one family slot).

| # | Name | id | File | Icon | Points | Shortcut | Fill? | Extra tab | Payload |
|---|---|---|---|---|---|---|---|---|---|
| 1 | Horizontal line | `horizontal-line` | `drawings/horizontal_line.rs:11` | `MINUS` | 1 | `H` | no | — | `NoPayload` |
| 2 | Rectangle | `rectangle` | `drawings/rectangle.rs:10` | `RECTANGLE` | 2 | `R` | yes | — | `NoPayload` |
| 3 | Parallel channel | `parallel-channel` | `drawings/parallel_channel.rs:20` | `PARALLELOGRAM` | 3 | `C` | yes | — | `NoPayload` |
| 4 | Fib retracement | `fib-retracement` | `drawings/fib_retracement.rs:11` | `ROWS` | 2 | `F` | no | "Levels" | `FibPayload` |
| 5 | Fib extension | `fib-extension` | `drawings/fib_extension.rs:11` | `ROWS_PLUS_TOP` | 3 | `Shift+F` | no | "Levels" | `FibPayload` |

Non-drawing modes on the same rail: **Pointer** (`toolrail.rs:98`, key `1`, also the Esc fallback) and **Crosshair** (`toolrail.rs:99`, key `2`).

Notably absent from the registry: trend line (diagonal), vertical line, ray, arrow, text/note/callout, measure/ruler, long-short position, pitchfork, elliott/wave, price range, brush. The parallel channel's first two clicks *are* a trend line, but there is no way to keep only that.

### 1.2 Tool rail controls

Rail geometry: 44 px thick (`toolrail.rs:18`), 32 px button hit target, 18 px glyph (`widgets.rs:29`, asserted at `widgets.rs:332-333`). Four docks (Left default, Right, Top, Bottom — `toolrail.rs:133-139`).

**Leading cluster** (`toolrail.rs:560-589`):

| Control | file:line | Trigger | Effect |
|---|---|---|---|
| Grip (6-dot glyph) | `toolrail.rs:608` | drag | Docks the rail at the nearest window edge on release (`nearest`, normalised distance, `toolrail.rs:151`). Live drop-preview band painted at `toolrail.rs:1128`. `Esc` mid-drag aborts and keeps the dock (`toolrail.rs:647`). Cursor Grab→Grabbing. |
| Pointer button | `draw_button`, `toolrail.rs:857` | click / `1` / `Esc` | Arms Pointer (select, move, pan, zoom). |
| Crosshair button | same | click / `2` | Arms crosshair mode; the crosshair lines + axis price tag only paint in this mode (`pane.rs:2015`). |
| Separator hairline | `toolrail.rs:674` | — | Decoration. |
| Per-tool button ×3 | `toolrail.rs:857` | click / letter key | Arms that drawing tool. Active state = accent marker on the edge facing the window border (`marker_edge`, `toolrail.rs:173`). |
| Fib family split button | `draw_family_slot`, `toolrail.rs:909` | left-click body | Arms the last-armed member, or member[0] before any use (`family_member`, `toolrail.rs:900`). |
| Fib family caret zone | `toolrail.rs:934-940`, click test at `:970` | click in the 10×10 px bottom-right corner, **or** right-click anywhere on the button | Opens the family flyout. |
| Draft badge | `paint_draft_badge`, `toolrail.rs:876` | automatic | Shows `2/3` on the armed multi-point tool while a draft is open. Occupies the same corner as the caret and suppresses its glyph (`:941`). |
| More menu (`DOTS_THREE`) | `draw_more_menu`, `toolrail.rs:714` | click | Lists exactly what the current stage swallowed, by name + shortcut. At Minimal it also carries repeat / hide-all / lock-all as text items. |

**Trailing cluster** (`toolrail.rs:593-602`, laid from the far end backwards):

| Control | file:line | Trigger | Effect |
|---|---|---|---|
| Repeat pin (`ARROW_CLOCKWISE`) | `toolrail.rs:701` | click | Toggles: keep the drawing tool armed after an object completes. Default off = one-shot back to Pointer (`pane.rs:899`). |
| Hide-all eye | `toolrail.rs:832-854` | click | `set_all_hidden` — a *layer* over each object's own eye; restoring preserves per-object state (`mod.rs:778`, test `mod.rs:1143`). One undo entry. |
| Lock-all | `toolrail.rs:808-830` | click | `set_all_locked` on every object, one undo entry, never deletes (`mod.rs:793`). |
| Separator | `toolrail.rs:674` | — | Decoration. |
| Objects button (`LIST`) + count badge | `toolrail.rs:779` | click | Toggles the object manager. Badge = `drawings.items().len()` of the **focused pane**. |

**Responsive staging** (`stage_for`, `toolrail.rs:285`): Full ≥ 417 px, Compact 345–416 px (only Pointer, Crosshair, the armed tool, More survive), Minimal < 345 px (Crosshair and the globals fold into More). Pure function of extent, hysteresis-free.

**Menu path**: View → Drawing toolbar → Left/Right/Top/Bottom (radio), plus Show/Hide drawing toolbar — `app.rs:1853-1877`.

### 1.3 Inspector — title bar (`draw_inspector_title_bar`, `app.rs:2152`)

| Control | file:line | Trigger | Effect |
|---|---|---|---|
| Grip glyph + title | `app.rs:2174-2196` | drag (floating only) | Moves the window; sets `inspector_moved` so automatic placement stops. |
| Title bar double-click | `app.rs:2240` | double-click | Clears `inspector_moved`, re-runs automatic placement. |
| Eye | `app.rs:2229` | click | Hide/show *this* drawing. |
| Pin | `app.rs:2212` | click | Switches between floating window and right-side dock panel; sets `inspector_pin_touched` so the auto-pin rule stops firing. |
| Close (X) | `app.rs:2201` | click | Clears the selection (keeps the drawing). |

### 1.4 Inspector — body (`drawing_inspector_body`, `app.rs:2263`)

| Control | file:line | Effect |
|---|---|---|
| "Lock drawing" / "Unlock drawing" | `drawings/action_bar.rs:30` | Toggles lock. Textual by contract. |
| "Delete drawing" + `Del` shortcut hint | `drawings/action_bar.rs:37` | Requests delete through the one command path (`request_delete_selected`, `app.rs:1947`). |
| Locked / hidden explanatory lines | `app.rs:2278-2293` | Status text only. |
| "Delete locked drawing?" + Cancel / Delete anyway | `app.rs:2294-2305` | Only shown when the object is locked and a delete was requested. |
| Tab: Style | `app.rs:2313` | — |
| Tab: *extra* (Fib only, "Levels") | `app.rs:2316` | Mounted by the tool, central code never learns its contents. |
| Tab: Coordinates | `app.rs:2319` | — |
| Style → colour swatch | `app.rs:2349` | `color_edit_button_srgba` on `style.color`. |
| Style → "line width (px)" slider | `app.rs:2353` | 0.5 … 6.0 (`mod.rs:24-25`). |
| Style → "fill opacity" slider | `app.rs:2363` | 0 … 160, only for tools where `supports_fill()` (rectangle, channel). |
| Coordinates → per-anchor `bar` DragValue | `app.rs:2383` | Labels A/B/C/D (`app.rs:2376`), speed 0.25 bars/pt. Disabled when locked. |
| Coordinates → per-anchor price DragValue | `app.rs:2389` | Speed = visible range / 200 (`PRICE_DRAG_STEPS`, `app.rs:102`). |

### 1.5 Fib "Levels" tab (`drawings/fib.rs:687`) — the only tool-owned inspector

Built-in preset buttons (`fib.rs:701-709`); custom-preset combo + Apply / Delete / "Set as default" (`fib.rs:714-752`); preset-name field + "Save preset" with an inline "Overwrite? Yes/No" confirmation and "Clear default" (`fib.rs:753-789`). Per level row (`fib.rs:798-882`): visible checkbox, ratio text field (accepts `.618`, `0,618`, `61.8%` — `parse_ratio`, `fib.rs:381`), custom label field, colour override + `x` to inherit, "fill" to next visible level, `×` delete (disabled at one level). Then "+ Add level" / "Sort" / "Restore" (`fib.rs:899-918`), band-opacity slider, labels-mode combo, label-position combo, extend combo, "log scale" checkbox (disabled with hover reason when any anchor price ≤ 0), and "Invert A ↔ B" (`fib.rs:922-974`).

### 1.6 Object manager (`draw_drawing_manager`, `app.rs:2687`)

Non-modal window "Drawn objects", opens beside the rail in all four docks (`manager_target_position`, `app.rs:2665`). Listed **top-most first**, matching hit-test order (`app.rs:2721`). Per row: selectable label `"{tool name} {index+1}"` (weak when hidden), inline "locked"/"hidden" tags, and right-aligned **Hide/Show · Lock/Unlock · Front · Delete** small buttons (`app.rs:2741-2767`). Footer: **Show all** and **Unlock all** (`app.rs:2771-2778`). Selecting a row also centres the viewport on the object's mean bar (`app.rs:2783-2794`).

### 1.7 Selection visuals and handles

Painted by the wrapper, never by the tool (`DrawingTool::paint`, `mod.rs:311-341`): a halo pass underneath at `width + 3.5 px` in premultiplied ~16 % white (`mod.rs:37`), then the object in its own colour, then white anchor discs of radius 4 px with an accent ring — **only when unlocked** (`pane.rs:1974`, "a locked object shows no resize handles: the affordance would lie").

Hit radii: stroke/body select radius **10 px** (`pane.rs:41`), anchor grab radius **12 px** (`pane.rs:43`), drag-to-place threshold **4 px** (`pane.rs:45`).

### 1.8 Drag behaviours (`pane.rs:1119-1250`)

- **Anchor drag** — press within 12 px of an anchor of the selected object (selected object wins ties, `drawing_anchor_at`, `pane.rs:1014`) → `DrawingDrag::Anchor`, moves that one point, clamped to the history rect, cursor `ResizeNwSe`.
- **Body drag** — press on the stroke/interior → `DrawingDrag::Translate`, rigid translation of every anchor.
- **Blocked** — press on locked geometry → `DrawingDrag::Blocked`, cursor `NotAllowed`, nothing moves.
- One drag = one undo entry (`begin_gesture` at press, `commit_gesture` at release, `pane.rs:1188/1248`).
- Chrome gate: a press under a floating panel never grabs the drawing beneath (`over_chrome`, `pane.rs:1099`); a drag that *started* on canvas keeps running across panels.

### 1.9 Keyboard map

| Key | Where | Effect |
|---|---|---|
| `1` / `2` | `toolrail.rs:454-458` | Pointer / Crosshair |
| `H` `R` `C` `F` `Shift+F` | declared per tool, dispatched `toolrail.rs:460` | Arm that tool |
| `Esc` | escape stack, `app.rs:2009-2027` | rail drag → paper interaction → delete confirmation → cancel draft (+ disarm) → deselect → Pointer |
| `Backspace` (during placement) | `app.rs:2031` | Drop the last draft anchor; the last one cancels the draft |
| `Delete` / `Backspace` (otherwise) | `app.rs:2034` | Delete the selected drawing |
| `Ctrl+Z` / `Ctrl+Y` / `Ctrl+Shift+Z` | `app.rs:1994-1996` | Undo / redo (drawings only, 64 entries, `mod.rs:30`) |
| `Alt+L` | `app.rs:1997` | Toggle lock on the selection |
| `Alt+H` | `app.rs:1998` | Toggle hide on the selection |
| `Ctrl+D` | `app.rs:1999` | Duplicate, offset +2 bars, unlocked, becomes the selection (`mod.rs:655`) |
| `←→↑↓` (+`Shift` = ×10) | `app.rs:1986-1989`, applied `app.rs:2064-2079` | Nudge: 1 bar horizontally, 1 px worth of price vertically; one undo entry per press |
| `Alt+click` on chart | `pane.rs:1157` | Cycle down the z-order through overlapping objects, wrapping |

All chart shortcuts are suspended while any widget holds keyboard focus (`app.rs:1965`, `toolrail.rs:447`).

### 1.10 Magnet / snap

**None.** `drawing_point_at` (`pane.rs:779`) converts a pixel to a raw fractional bar and a raw scale price. No snap to OHLC, to bar centres, or to the instrument's tick size — even though the codebase already has tick snapping for paper trading (`paper_trading.rs:556`).

### 1.11 Preset persistence

`PresetStore` (`drawings/presets.rs:43`) writes a versioned TOML (`quantick-drawing-presets.toml`, env override `QUANTICK_DRAWING_PRESETS`). Stores, per tool id, named **payload** exports plus one "default for new objects". A future file version starts empty rather than guessing (`presets.rs:63-71`). Overwriting requires explicit consent (`presets.rs:134`).


## 2. FLOWS (as implemented today)

**(a) Pick a tool and place.** Click the rail button or press its letter → the tool is armed. `handle_drawing_placement` (`pane.rs:807`) then owns the pointer inside the *history* rect (chart minus the live lane). Cursor becomes a crosshair glyph. Each **press** places one anchor (`pane.rs:846-852`); if this was the first press of a fresh draft and the pointer travelled ≥ 4 px before release, a second anchor lands on release (`pane.rs:861-870`). So:

- Horizontal line (1 pt): one press. Lands on mouse-**down**, not on release.
- Rectangle / Fib retracement (2 pts): click-click **or** press-drag-release.
- Parallel channel / Fib extension (3 pts): press-drag-release for the baseline, then one more click; or three clicks.

On completion the object is appended, becomes the selection (`mod.rs:626-632`), and the tool reverts to Pointer unless the repeat pin is on (`pane.rs:896-903`). A live preview follows the pointer using the hovered anchor (`pane.rs:1978-2001`). A new object's payload is seeded from the user's default preset if one is set; its **style is always the stock default** (`mod.rs:618`).

**(b) Select.** Pointer armed → click. Topmost hit within 10 px of the geometry wins (`drawing_at`, `pane.rs:920`, walks `.rev()`); `Alt+click` walks downward through the stack. A click on nothing deselects. Hover pre-feedback: `Move` over a body, `ResizeNwSe` over an anchor of the selection, `NotAllowed` over locked geometry (`pane.rs:1124-1150`). Selecting also opens the inspector, since the inspector is a pure function of the selection.

**(c) Edit properties.** Only through the inspector. It opens floating, auto-placed by an eight-candidate least-overlap algorithm clamped inside the chart pane (`inspector_target_position`, `app.rs:2485`), or pinned as a right-side panel. On a chart narrower than 1180 px a fresh selection opens **pinned** (`app.rs:2597-2607`). Colour/width/fill live in the Style tab; a whole slider or colour gesture coalesces into one undo entry via `inspector_edit_baseline` → `record_edit_of` (`app.rs:2415-2427`, `mod.rs:559`).

**(d) Move / resize.** Body drag translates; anchor drag (12 px grab) moves one point; arrows nudge; the Coordinates tab edits bar and price numerically. Locked objects reject all four.

**(e) Delete.** Four triggers — inspector button, `Delete`/`Backspace`, manager row "Delete", and the forced path — all funnel through `Drawings::delete_selected` (`mod.rs:714`). Unlocked → deletes and raises an 8-second toast with an Undo button (`app.rs:1951`, `TOAST_UNDO_MS` = 8000). Locked → `NeedsConfirmation`, and the "Delete locked drawing?" prompt appears **in the inspector body**. **There is no delete-all / "remove drawings" command anywhere** — the rail's globals are hide-all and lock-all only.

**(f) Presets.** Fib tools only. Built-in ratio sets are one-click. Custom presets: type a name → "Save preset" → if the name exists, an inline "Overwrite? Yes/No" appears. "Set as default" makes new objects of that tool start from it. Presets carry the payload only — **never colour, width, or fill**.

**(g) Persistence.**
- **Across restarts: no.** `impl eframe::App for QuantickApp` defines only `update` (`app.rs:2860-2867`); there is no `save`. Drawings live in `ChartPane::drawings` in memory. Rail dock, rail visibility, the repeat pin and the inspector pin state are equally volatile — the spec acknowledges this and defers it to an unbuilt `ui-state.toml` (`docs/drawing-toolbar-ux.md` §3.5). By contrast, indicators *do* persist (`indicators/state_file.rs`, saved at `app.rs:1487`), and drawing *presets* persist.
- **Across bar-type / timeframe / feed switches: destroyed.** `clear_overlay` (`tab.rs:324`) wipes items, draft, selection **and the entire undo history** (`mod.rs:673-681`). The user gets a toast — "Drawings cleared - the bars were rebuilt under them." — deliberately with **no Undo button**, because nothing can bring them back (`app.rs:1500-1513`).
- **Across tab switches: preserved** (per-tab, per-pane state).

## 3. UX FINDINGS

### Blockers

**B1 — Chart markup does not survive a restart.**
`app.rs:2860-2867` has no `save`. Every trendline, level and Fib a trader draws is gone when the app closes. This is the single most damaging gap: it makes the drawing tools unusable for the multi-session workflow they exist for, and it is *inconsistent inside the same app*, since indicators (`indicators/state_file.rs`) and drawing presets (`drawings/presets.rs`) both persist. Heuristic: user control and freedom; consistency and standards.

**B2 — All chart navigation is dead while a tool is armed.**
`pane.rs:1058`: `if self.handle_drawing_placement(...) { return; }` returns before the chart's `interact`, the scroll-zoom branch, the double-click reset, and the axis handles. With any drawing tool armed you cannot pan, cannot scroll-wheel zoom, cannot drag either axis, cannot double-click to reset. Placing the second anchor of a channel on a bar that is currently off-screen is impossible without disarming, panning, and re-arming — losing the draft to the Esc that disarms. TradingView keeps wheel-zoom and scroll live throughout placement. Heuristic: flexibility and efficiency of use.

### Major

**M3 — No trend line.** The registry (`mod.rs:393-399`) ships five tools and omits the diagonal trend line, the most-used object in technical analysis, plus vertical line, ray, arrow, text/note and measure. The parallel channel's first two clicks draw exactly the missing primitive but cannot be stopped there (`required_points() == 3`, `parallel_channel.rs:36`). For an order-flow charting app this reads as an unfinished tool set rather than a deliberate "small and focused" scope.

**M4 — No line style.** `DrawingStyle` is `{ color, width_px, fill_alpha }` (`mod.rs:407-412`). No dashed, no dotted. Traders routinely encode meaning in dash style (projected level vs confirmed level); here every object is a solid line. The Fib anchor guides *are* dashed but with a hardcoded constant (`fib.rs:39`) the user cannot reach.

**M5 — Style is neither preset-able nor remembered.** `place_with` always assigns `DrawingStyle::default()` (`mod.rs:618`) — the same light blue, 1.5 px, for every tool and every object. Presets export only the tool **payload** (`DrawingPayload::export_preset`, `mod.rs:53`), so:
- The three `NoPayload` tools (horizontal line, rectangle, channel) have **no preset capability at all** — `export_preset` returns `None`, and they render no extra tab.
- Even for Fib, "Set as default" cannot carry a colour.
The result: every new drawing must be recoloured by hand, one at a time, and a chart with ten objects is monochrome. Heuristic: recognition over recall; user control.

**M6 — No right-click context menu on a drawing.** The only `secondary_clicked` handlers in the whole app are the rail family slot (`toolrail.rs:974`) and three toolbar layer buttons (`toolbar.rs:534/551/569`). Right-clicking an object — the primary edit path every TradingView user reaches for first (Settings / Clone / Remove / Visual order / Lock / Hide) — does nothing. Every action requires the inspector or a memorised chord. Heuristic: match between system and the real world; consistency with platform conventions.

**M7 — No way to clear all drawings.** Grep for `Delete all` / `Remove all` / `Clear all` / `delete_all` across `crates/app/src` returns nothing. The rail offers hide-all and lock-all (`toolrail.rs:807`), the manager offers "Show all" and "Unlock all" (`app.rs:2771-2778`), but removing a marked-up chart means deleting objects one by one. TradingView's "Remove drawings" is a single command.

**M8 — A new drawing is invisible when hide-all is engaged, but its preview is not.**
`is_visible` returns false while `all_hidden` (`mod.rs:519-521`), yet the draft preview at `pane.rs:1978` is painted **unconditionally**. So with hide-all on, the user sees the rubber-band preview follow the cursor, clicks to complete — and the object vanishes. Nothing warns them; the rail's eye is merely in its active state. This will read as "the tool is broken". Heuristic: visibility of system status.

**M9 — Tool shortcuts stay live while the rail is hidden.**
`ToolRail::handle_keys` (`toolrail.rs:446`) has no `visible` check and is called unconditionally each frame (`app.rs:2891`), while `draw` returns early when hidden (`toolrail.rs:478`). Pressing `H` with the toolbar hidden arms the horizontal-line tool with **no on-screen indication anywhere** (the status bar shows no armed tool — grep for `tool` in `statusbar.rs` returns nothing), and the next click on the chart drops a line instead of selecting or panning. This directly contradicts the invariant the codebase itself asserts in `toolrail.rs:1244` (`hiding_the_toolbox_cannot_leave_an_invisible_drawing_tool_armed`) — the test covers `toggle_visible` but not the keyboard path.

**M10 — No crosshair or price readout while placing.**
`draw_crosshair` returns immediately unless `Tool::Crosshair` is armed (`pane.rs:2015`). While placing a Fib or a horizontal line there are no guide lines and no axis price tag; the user is eyeballing the level against the price gutter. Every charting platform shows the crosshair *during* drawing precisely because that is when precision matters most.

**M11 — No magnet/snap of any kind.** `drawing_point_at` (`pane.rs:779`) yields raw fractional bar and raw pixel-derived price. A horizontal line placed at a swing high will sit a few cents off it, and the price is not even snapped to the instrument's tick size, though the machinery exists next door (`paper_trading.rs:556`). For a level-marking workflow this is the difference between a usable line and a decorative one.

**M12 — Fib validation messages flash for one frame.**
In `draw_levels_tab`, `error` is a frame-local (`fib.rs:793`) set inside `response.lost_focus()` / `.changed()` branches and rendered at `fib.rs:896-898`. The next frame the branch is false and the label disappears. "Ratios read like 0.618 or 61.8%.", "That ratio already exists." and "At least one level stays visible." are therefore visible for roughly 16 ms — effectively never. The user sees their typed ratio silently revert with no explanation (`fib.rs:832` rewrites the buffer regardless). Heuristic: help users recognise, diagnose and recover from errors.

**M13 — The object manager has no scroll area and cannot be resized.**
`app.rs:2704-2714`: `.resizable(false)`, default width 320 px, and the row loop at `:2721` sits directly in the window body with no `ScrollArea` (contrast the pinned inspector, which wraps its body at `app.rs:2563`). With 30 objects the window grows past the screen and the "Show all"/"Unlock all" footer becomes unreachable. Each row also packs four small buttons plus two tags into 320 px, so labels crowd immediately.

**M14 — On a common laptop the inspector permanently steals 320–360 px of chart.**
`INSPECTOR_AUTO_PIN_CHART_WIDTH_PX = 1180.0` (`app.rs:89`) and the auto-pin fires whenever the chart pane is narrower (`app.rs:2597-2607`). On a 1366×768 screen, after the 44 px rail and the right dock, the chart is well under 1180 px, so *every* selection docks the inspector and the chart loses another quarter of its width. The rule is defensible in isolation, but its practical effect on the most common laptop resolution should be verified against a real window rather than assumed.

**M15 — Undo history is destroyed by a bar-spec change.**
`clear()` empties `undo` and `redo` (`mod.rs:673-681`). The toast is honest ("the bars were rebuilt under them") and deliberately offers no Undo (`app.rs:1505-1512`), which is the right *data* decision — but the user experience is that flipping a timeframe chip to compare views annihilates the session's markup with no confirmation beforehand. There is no "are you sure, this will discard N drawings" gate on a reversible-looking action. Heuristic: error prevention over error messaging.

### Minor

**m16 — The family caret is a 10 × 10 px target that becomes invisible but stays live.**
`TOOLBOX_CARET_ZONE_PX = 10.0` (`toolrail.rs:32`) inside a 32 px button — well under any pointer-target guidance. Worse, when a draft badge occupies the corner the caret **glyph** is suppressed (`toolrail.rs:941`) but the **click zone** is not (`toolrail.rs:970` does not consult `badge_shown`): clicking the bottom-right of the armed Fib button mid-draft opens the flyout with no affordance having suggested it would.

**m17 — Selectable but not draggable inside the live lane.**
`horizontal_line::hit_test` accepts any x in `chart_rect` (`horizontal_line.rs:62`) and click-selection uses the full chart rect (`pane.rs:1151`), but drag initiation is gated on `drawing_area` = history only (`pane.rs:1178`). Clicking a line over the live lane selects it; pressing and dragging there does nothing at all — no move, no pan, no cursor change.

**m18 — Z-order is one-directional.** `bring_to_front` exists (`mod.rs:758`, manager "Front" button) with no send-to-back and no arbitrary reordering. Recovering from a mis-ordered stack means promoting every other object.

**m19 — Object identity is the z-index and renumbers itself.** Manager rows are labelled `"{tool} {index+1}"` (`app.rs:2728`). Pressing "Front" on "Rectangle 2" renames it and renumbers its neighbours. There is no rename, no stable id, no way to tell two rectangles apart except by selecting each and watching the chart.

**m20 — The manager silently speaks for whichever pane has focus.** It reads `focused_pane()` (`app.rs:2715`, `tab.rs:569`), as does the rail's count badge (`toolrail.rs:790`). On a split time/flow canvas the list contents and the badge number change when focus moves, with no header naming the pane.

**m21 — The locked-delete confirmation is not next to its trigger.** `request_delete_selected`'s doc claims it "raises the confirmation next to the trigger" (`app.rs:1944-1946`), but the prompt only ever renders in the inspector body (`app.rs:2294-2305`). Deleting a locked object from a manager row shows the confirmation in a different window, possibly on the far side of the chart.

**m22 — "Delete" on a custom preset has no confirmation** (`fib.rs:739-744`) while *saving over* one does (`fib.rs:765-775`). The destructive action is the unguarded one, and the deletion is written to disk immediately (`presets.rs:142-152`).

**m23 — Most of the keyboard grammar is undiscoverable.** Hover texts carry `(H)`, `(R)`, `(C)`, `(F)`, `(Shift+F)`, `(1)`, `(2)`, and the action bar shows `Del` (`action_bar.rs:39`). Nothing in the UI mentions `Ctrl+D`, `Alt+L`, `Alt+H`, arrow nudge, `Shift`+arrow, `Backspace`-to-step-back-an-anchor, or `Alt+click` z-cycling. There is no shortcuts help panel anywhere in the app.

**m24 — `Alt+H` (hide) sits beside `H` (horizontal line).** Safe in code (`toolrail.rs:451-453` bails on any command/alt modifier) but a poor mnemonic pair; `Alt+L` (lock) has the same issue against a future "line" tool.

**m25 — The repeat-pin icon is `ARROW_CLOCKWISE`** (`toolrail.rs:702`), which reads as reload/refresh rather than "keep tool active". Only the tooltip disambiguates.

**m26 — 4 px drag-to-place threshold is very low** (`pane.rs:45`). A slightly unsteady click on a rectangle produces a 5-pixel rectangle rather than the intended click-click flow.

**m27 — Duplicate offset of 2 bars is often invisible** (`DUPLICATE_OFFSET_BARS`, `app.rs:110`). Zoomed out, the copy lands on top of the original; the only clue that `Ctrl+D` worked is the manager count.

### Nits

**n28** — The one-point horizontal line commits on mouse-**down** (`pane.rs:846-852`); moving before release does not adjust it. Every other tool's anchors feel drag-adjustable; this one does not.
**n29** — Anchor labels are hardcoded `["A","B","C","D"]` with a `"?"` fallback (`app.rs:2376-2380`); fine for today's 3-point maximum, a latent gap for the next tool.
**n30** — No copy/paste of drawings, between panes, tabs or symbols; `Ctrl+D` in place is the only duplication path.
**n31** — Undo depth 64 (`mod.rs:30`) is per pane and shared with style edits; a long styling session can push out the geometry history.
**n32** — `Ctrl+Z` is bound exclusively to drawings (`app.rs:2038`); pressing it after any other kind of change silently rolls back an unrelated drawing edit instead.


## 4. QUICK WINS vs STRUCTURAL

### Quick wins (localised, low blast radius)

1. **Gate `handle_keys` on rail visibility** — one condition in `toolrail.rs:446` (`if !self.visible { return; }`), plus a test alongside `toolrail.rs:1244` that drives the keyboard rather than `toggle_visible`. Closes M9.
2. **Don't paint the draft while `all_hidden`** — guard `pane.rs:1978` with `!self.drawings.all_hidden()`, or better, have `place_with` refuse (or auto-release hide-all with a toast). Closes M8.
3. **Make Fib validation errors persist** — move `error: Option<&'static str>` from a frame-local (`fib.rs:793`) into `FibPayload` or into `ui.data_mut` temp storage keyed by the row, cleared on the next successful edit. Closes M12.
4. **Wrap the manager body in `ScrollArea::vertical()` and set `.resizable(true)`** — two lines at `app.rs:2704-2714`, mirroring the pinned inspector at `app.rs:2563`. Closes M13.
5. **Add "Delete all drawings" with a count-bearing confirmation** — a trailing-cluster rail button or a manager footer entry beside "Show all"/"Unlock all" (`app.rs:2771`), routed through a new `Drawings::delete_all` that records one undo entry (so the toast's Undo genuinely works). Closes M7.
6. **Confirm the custom-preset delete** — reuse the exact inline "Overwrite? Yes/No" pattern already sitting ten lines above it (`fib.rs:765-775`). Closes m22.
7. **Draw the crosshair whenever a drawing tool is armed** — relax `pane.rs:2015` from `!= Tool::Crosshair` to "crosshair mode *or* a drawing tool is armed". Closes M10 with no new state.
8. **Stop the caret zone from firing when the badge hides its glyph** — add `&& !badge_shown` to the `caret_clicked` test at `toolrail.rs:970`, and widen `TOOLBOX_CARET_ZONE_PX` from 10 to ~14. Closes m16.
9. **Show the armed tool in the status bar** — one segment in `statusbar.rs` reading `toolrail.tool().name()`. Mitigates M9 and gives the rail-hidden case an anchor.
10. **Add a "Shortcuts" entry under Help** (`app.rs:1913-1918`) listing the full grammar from §1.9. Closes m23.
11. **Raise `DRAWING_DRAG_THRESHOLD_PX` to ~8** (`pane.rs:45`) and **make `DUPLICATE_OFFSET_BARS` zoom-aware** (`app.rs:110`). Closes m26, m27.
12. **Confirm before a bar-spec change discards N drawings** — the count is already known at `tab.rs:326`; raising a "Switching the bar type will discard N drawings" gate before `clear_overlay` turns M15 from an apology into a prevention.

### Structural (real design work)

**S1 — Persist drawings, and the UI state around them.** The spec already names the vehicle (`docs/drawing-toolbar-ux.md` §3.5: a `ui-state.toml` sibling of `indicators-state.toml`). Drawings need a versioned, per-(feed, symbol, bar-spec) store keyed so that anchors are only ever restored onto the data that made them — which is the same honesty rule `clear_overlay` enforces today, just applied at load instead of at switch. This requires serialisation for `Drawing`, which means `DrawingPayload` grows `export`/`import` obligations beyond presets (the Fib payload already has both). It also unlocks a much better answer to M15: instead of destroying markup on a bar-type switch, *park* it under its old spec and restore it when the user switches back. Closes B1 and defuses M15.

**S2 — Let navigation and placement coexist.** `handle_drawing_placement` currently claims the whole gesture by returning early (`pane.rs:1058`). It should consume only the primary-button click stream and let scroll-zoom, middle/right-drag pan and the axis handles fall through. This is a rework of the early-return into a "consumed inputs" mask threaded through `handle_navigation`. Closes B2.

**S3 — Promote style into the tool contract.** Today `DrawingStyle` is a fixed struct the preset system deliberately excludes (`mod.rs:51-53`). Making style presetable means: (a) adding `line_style: LineStyle` (solid/dash/dot) with rendering support in each tool's `paint`; (b) letting a preset carry the common envelope alongside the payload; (c) a per-tool "default style for new objects", so `place_with` stops hardcoding `DrawingStyle::default()` (`mod.rs:618`). This one change closes M4 and M5 and makes the existing preset machinery useful to the three tools that currently cannot touch it.

**S4 — A right-click context menu on drawings, and on the empty chart.** A shared menu, built from the same intents the action bar already reports (`action_bar::ActionBarIntent`) plus Clone / Bring to front / Send to back / Settings…, attached to the chart's secondary click and dispatched against `drawing_at`. The action-bar doc comment already anticipates this host ("inspector, object manager, future context menu", `action_bar.rs:3-5`), so the command layer is ready; what is missing is the surface. Closes M6, and gives m18 and m19 a natural home.

**S5 — Fill out the tool set, starting with the trend line.** The registry is a genuinely clean extension port (proven end-to-end by the fake-tool test at `mod.rs:1489`), so each new tool is one file plus one name. Priority order by trader expectation: trend line → ray → vertical line → text/note → measure → long/short position. The trend line is nearly free: it is the parallel channel's first two anchors. Closes M3.

**S6 — A magnet mode.** A rail toggle (the natural neighbour of the repeat pin) that snaps the placement/drag point to the nearest OHLC of the bar under the cursor, with a weak mode that snaps only to the tick grid. `drawing_point_at` (`pane.rs:779`) is the single choke point, and the pane already holds the bars and the tick size. Closes M11, and materially raises the precision of every existing tool.

**S7 — Stable object identity.** Give `Drawing` an id and an optional user name, so the manager stops labelling by z-index (`app.rs:2728`), rename becomes possible, and persistence (S1) has something to key on. Closes m19, supports m18.

## 5. Where the code is already strong

Worth preserving through any redesign: the command funnel (every delete trigger routes through `Drawings::delete_selected`, `mod.rs:714`, so lock rules cannot drift per surface); gesture-coalesced undo (one drag or one slider sweep = one entry, `mod.rs:544-555`, `app.rs:2415-2427`); undo snapshots that shift with prepended history (`mod.rs:704-709`) so undo never re-anchors an object onto bars that moved; the pointer-opacity contract for floating chrome, gated at press time only so a drag survives crossing a panel (`pane.rs:1094-1101`); hide-all as a layer that preserves per-object visibility (`mod.rs:778`, test at `:1143`); the honest refusal to reattach anchors to rebuilt bars; and the registry itself, which is one of the cleaner extension ports in the codebase.

## Priority summary

- **Blockers:** B1 (no persistence across restarts), B2 (navigation dead while a tool is armed).
- **Highest-value quick wins:** #1 (hidden-rail shortcuts), #2 (invisible new drawing under hide-all), #7 (crosshair while placing), #5 (delete-all), #3 (invisible Fib errors).
- **Biggest structural gaps vs TradingView conventions:** no right-click menu (M6), no trend line (M3), no line style / style presets (M4+M5), no magnet (M11).

