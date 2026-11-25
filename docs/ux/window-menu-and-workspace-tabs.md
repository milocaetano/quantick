# Window menu, Chrome-style tabs and a workspace per tab

Status: design approved by the trader on 2026-10-31. Phase 1 is being built:
the strip, the `⋮` menu and the "this tab" row, without per-tab workspaces. This page
is the brief to implement from; the prototype beside it shows the behaviour.
It extends [`market-shelf-onboarding.md`](market-shelf-onboarding.md) (Home
tab, `+` palette, My markets), which stays the brief for the Home page itself.

- [`window-menu-prototype.html`](window-menu-prototype.html) — clickable
  prototype: the tab strip in the title bar, the `⋮` window menu, the
  Settings window, the "this tab" row with its workspace control, the
  "Open a market" palette with its Bars and Workspace selectors, tab tear-off,
  and a ten-step test script. Prices, trades and venue lists are simulated.
  The page's "Today" image is a crop of the trader's own screenshot.

## The brief, in the trader's words

- "Like Chrome: the tabs go up there." The Windows title bar goes away; the
  first row of the window is the tab strip, and it saves one row of height for
  the chart. A tab can be dragged off into its own window, as in Chrome.
- "File, View, Workspace, Tools and Help need a home that pollutes nothing."
- "Every tab is a market. I can open BTC using workspace X, and I must see
  which workspace each tab is on." The earlier idea of workspace tabs holding
  market tabs was dropped as too complex.
- When opening a market, the bar type and the workspace are chosen in plain,
  labelled selectors. Typed grammar (`@scalp`, `500t`) and a hidden action
  panel were prototyped and refused: "strange for a human to understand".

## Vocabulary

Four words, four things. "Session" was rejected as the window's name because
it collides with the B3 trading session, the `session` crate and the
`sessions:N` history reach; "profile" collides with volume profile and
"template" with the indicator and drawing presets.

| Word | What it is | Scope | Called today |
|---|---|---|---|
| Market tab | One feed + symbol. It does not change after opening; another market is another tab. A contract roll is an explicit "Roll to WINF27" action that keeps workspace and drawings | per market | tab |
| Workspace | A named recipe for a tab: canvas (Flow / Timeframe / both), context panes and their intervals, indicators per pane, layer toggles, footprint style. Any tab can use any workspace | per workspace | the per-pane "Layouts" strip (Fluxo, Force Bar Only…) and View › Layout |
| Market settings | Flow bar size (WIN tick 2000, BTC tick 50), bubble preset and thresholds, history reach, replay link, recording, paper position. They stay with the market when its workspace changes | per market, keyed by asset + venue, stable across rolls | spread over `layouts.toml` and `bubble-assets.toml` |
| Desk | The open market tabs in order, each with its workspace and unsaved changes, plus panels, rail, timezone and window. Restored on launch, exported as a file | per window | Workspace › Save workspace, Save as, Open, Export |

## The window

### Tab strip in the title bar

- The window is created undecorated (`with_decorations(false)`). The tab row
  is the drag region; double-click maximizes; minimize, maximize and close are
  drawn by the app at the right end. Win+← / Win+→ keep working.
- Row, left to right: `⌂ Home` (pinned, always first, never closes), the
  market tabs, `+`, free space, the **Agent on** pill while local agent access
  is on (amber: it is an exposure, not a health state; click opens the access
  panel), `⋮`, window controls.
- A market tab reads `● WINZ26  MetaTrader 5 ×` (`● BTCUSDT  Binance ×`):
  venue dot, symbol, and the broker in muted ink — the venue's name before its
  ` — ` qualifier. No workspace name and no edited mark: the status bar's
  workspace cell is the one place those show. The tooltip carries the full
  title. When tabs get narrow the broker truncates first (to four letters, then
  dropped); the symbol never shortens.
- Dragging a tab below the strip opens it in its own window with the same
  feed, workspace and drawings; closing that window brings the tab back. This
  replaces the "Move to new window" menu item as the primary gesture (the item
  stays for whoever does not drag). Two windows, two tabs: WIN and WDO side by
  side without a split.
- Right-click on a tab: Workspace ▸ (the list, ✓ on the current one),
  Back to live (replay tabs only), Duplicate tab, Move to new window, Close tab.

### The `⋮` window menu

One button at the right end of the strip, opened by click or `F10`. Not `≡`
(already used on the tool rail and the dock icons), not a gear (promises
settings and hides Desk, Replay and Help), not `Alt` (the sell modifier in CMD
trading; `Alt+1…9` switch layouts). Submenus open to the left, two levels at
most.

```
Save to Scalp              Ctrl+S        (disabled: no unsaved changes on this tab)
Save desk                  Ctrl+Shift+S
Hide panels                Ctrl+Shift+B
Market Replay…             Ctrl+R
─
Workspace ▸   Use for this tab: Scalp ✓ · Tape · Swing · Default (🗑 asks once more)
              ─ Save as new workspace… · Rename "Scalp"…
Panels ▸      L2 · Bubbles ✓ · Session · Paper trading ✓ · Trades
              ─ Drawing toolbar: Left ● Top Bottom Hidden
Desk ▸        Export desk to file… · Import desk from file…
              ─ Reset startup desk · ☑ Reopen last tabs on launch
─
Command palette…           Ctrl+Shift+P
Settings…                  Ctrl+,
Help ▸        Keyboard shortcuts… · Replay file format… · About quantick
```

- Exit is not in the menu: one row under Replay, with no confirmation, it
  closed the app with a position open. `app.exit` stays a named command,
  outside the agent profile.
- Shortcuts are rendered from the constants in `menu_bar.rs`, never typed by
  hand. `Ctrl+S` was free on purpose and becomes "Save to <workspace>".
- Every tooltip and disabled state carries over from `menus.rs` word for word
  ("Nothing saved yet…", the Save as vs Export distinction).

### Where today's entries go

| Today | Entry | Goes to | Command |
|---|---|---|---|
| File | New Tab… | `+` · Ctrl+T | `market.open {feed, symbol, bar, workspace, background}` |
| File | Close Tab | × on the tab · Ctrl+W; with autosave off and edits held it asks first (below) | `tab.close {tab_id, save}` |
| File | Market Replay… / Close Replay | `⋮` root · "Back to live" on the replay tab (keeps layout, indicators, drawings) | `replay.browser.open` · `replay.close` |
| File | Exit | the window's ✕ | `app.exit` (command only) |
| View | Layouts, Layout, Move chart, Hide context, legend | workspace content; the controls move to the chart header once chart-identity ships. Until then View stays | `layout.*` · `pane.legend.*` |
| View | Show/Hide panels, dock tabs, Drawing toolbar | `⋮` root and `⋮ Panels`. Panels belong to the window | `dock.toggle` · `dock.open` · `toolbox.*` |
| View, Tools | Timezone, Perf, history, Appearance, Record deals, Agent access | `⋮ Settings…` in sections (Display, Data, Recording, Agent access); the status-bar clock and the Agent pill are clickable | `settings.*` · `control.access.open` |
| Workspace | Save workspace (Ctrl+Shift+S) | `⋮` "Save desk", same shortcut, same meaning | `desk.save`; a bare `workspace.save` is refused with a code naming both successors |
| Workspace | Save as…, Open, Delete (bookmarks) | retired: the trader's `ui-state.toml` holds zero bookmarks. If missed, they return as "Saved desks" | — |
| Workspace | Export, Import, Reset startup, Save on exit | `⋮ Desk` | `desk.export` · `desk.import` · `desk.forget` · `desk.reopen_on_launch.set` |
| new | This tab's workspace | WORKSPACE field on the "this tab" row · right-click on the tab · `⋮ Workspace` | `tab.workspace.set` · `workspace.save_to {tab}` · `workspace.save_as` · `workspace.autosave.set` · `workspace.rename` · `workspace.delete` · `workspace.list` |
| Help | Replay file format… | `⋮ Help` with Keyboard shortcuts and About | `help.*` |

### Settings window

`Settings…` (Ctrl+,) opens one window with sections, not a pile of checkboxes
in a hover menu: Display (Appearance, Timezone, Perf readings), Data
(Progressive venue history, Venue candles on charts cut by trades), Recording
(Record deals), Agent access (the switch and Manage clients…). Every toggle
stays a palette command.

## Workspace per tab

### The "this tab" row

Under the strip, the row that today holds feed and symbol dropdowns becomes:

```
THIS TAB  WINZ26 · MetaTrader 5 — B3 │ REC · History ▾ │ BUY 1  SELL 1   [dock icons]
```

- Feed and symbol read as a label, not dropdowns, and the words stay clickable:
  the trader asked on 2026-11-02 to keep switching a tab's market in place.
  A contract roll is "Roll to WINF27".
- The workspace is not on this row nor on the toolbar: no "Save to" and no
  "Revert" there. It shows only in the status bar's cell, `⊞ Scalp ▴`, whose
  menu switches this tab only. The toast names the new workspace. A switch that would drop held edits asks first, as a close
  does: "Switch WINZ26 to Tape" — what changed since Scalp was saved — Cancel /
  Switch without saving / Save to Scalp; Cancel changes nothing.
  `tab.workspace.set` refuses such a switch with `workspace.unsaved_edits`
  unless its `edits` says `discard`; it never writes the workspace, which
  `workspace.save_to` does first to keep them. The market palette's
  workspace pick refuses it too. Any other switch never asks.
- The tab chip and the status bar cell read one state; a test proves they
  agree.

### Save model: configuration and drawings

Workspace Save keeps the tab's configuration: indicators and their settings,
pane timeframes and bar rules, positions, sizes, pane count, selected layouts
and layout names. These changes are held until Save by default. The existing
`auto_save` and `save_on_exit` choices remain opt-ins; explicitly stored choices
are preserved. `workspace.autosave.set` controls configuration only.

A held configuration marks the workspace cell. Ctrl+S and `workspace.save_to`
commit it. Tabs following the same workspace receive the saved recipe. A failed
write reports failure and keeps the pending configuration. Closing or switching
with held configuration retains the existing Save / discard / cancel flow.
Drawing edits do not mark configuration as modified or add a close prompt.

The capsule's Revert arrow (`workspace.revert`) restores the saved configuration
without writing or changing drawings. A bar rule absent from the saved workspace
returns to the rule the tab opened with. The slot then becomes Restore
(`workspace.revert.undo`), which brings the discarded configuration back until a
Save or workspace switch. Neither action takes over Ctrl+Z or Esc.

"Save as new" creates a workspace for this tab alone; other tabs stay where they
are. Manage workspaces retains Rename and Delete, with Default protected.
Deleting an in-use workspace asks first and moves its tabs to Default.

Drawings and anchored studies have their own automatic persistence. Creating,
editing, undoing or deleting a drawing schedules a write without Workspace Save.
Ctrl+Z undoes successive drawing actions; the chart context menu can clear the
chart's objects. Both persist their resulting drawing state. Discarding workspace
configuration cannot restore a deleted drawing or discard a newly saved one.

The chart context menu's **Save drawing** checkbox is checked by default and
remembered for the market and workspace/layout scope. Turning it off suspends
drawing writes while preserving the last saved drawings. Turning it back on saves
the current drawings. Workspace Save does not override this checkbox. The same
state and transition are available through `chart.drawings.autosave.set`.

### Drawing ownership

A drawing belongs to **venue + asset + workspace identity + named layout
identity**, and optionally to a specific timeframe. A drawing visible on all
timeframes remains inside that workspace and layout; switching to another
workspace or layout selects a different set. Private layout copies resolve to
their named layout. Renaming a workspace or layout preserves drawing ownership,
and a contract roll preserves the configured asset identity.
Deleting a named layout does not transfer its drawings to the neighbouring layout.
Its saved drawings retain the retired identity; a new layout with the same name
receives a different identity.

Drawing persistence reserves the layout ID allocator independently of
configuration. New layout names, count and selection reach disk only with
configuration Save, together with their indicators and arrangement. If a new
layout was never saved, its drawings remain stored under the reserved ID; a
later layout cannot reuse it. Closing and reopening restores saved drawings
when their saved scope is opened; undo history itself is session-local.

Existing drawings migrate only when their workspace/layout ownership is
unambiguous. Unresolved entries remain stored without being shown across unrelated
workspaces. Existing durable drawing identities used by alerts are preserved.
Failed drawing writes remain pending and retry; they never report a successful
save before the write succeeds.

Other market settings, such as bubble choices and history reach, retain their
existing market ownership. Panels, rail, timezone and window belong to the desk.
A workspace never opens or closes the paper panel.

### "Open a market" palette

The approved palette from the Market Shelf brief, with these changes:

- Under the list, one row of two labelled selectors drawn in the app's theme
  (not native controls): **Bars** (tick · volume · dollar · imbalance · time,
  with the size beside it) and **Workspace**. Bars follows the selected row's
  market: moving to WINZ26 shows tick 2000, back to BTC shows tick 50.
  Workspace is Default when the palette opens; nothing guesses the last used.
  A value the trader changed is drawn amber. A feed may name another default
  workspace in `feeds.toml`.
- The primary label says exactly what Enter does: `Open`, `Focus tab 2`
  (same market, same workspace), `Open second tab` (same market, another
  workspace or bar; tab 2 is never changed under the trader), `Open replay`,
  `Run` in command mode. Ctrl+Enter opens in the background.
- Recent first; "Start here" only while Recent is empty. Commands only behind
  the `>` prefix, never mixed into market results. Tab moves focus to the
  selectors; it does not cycle anything.
- Typography: proportional UI face for chrome and symbols, monospace with
  tabular figures only for prices and rates; a clearly filled selected row; a
  venue mark on each row; no second lines (the MT5 waiting copy goes in the
  tooltip).

### Named commands

`market.open {feed, symbol, bar, workspace, background}`,
`tab.workspace.set {tab, name}`, `workspace.save_to {tab}` (returns the tabs
it changed), `workspace.save_as {tab, name}`, `workspace.autosave.set {on}`,
`tab.close {tab_id, save}`,
`workspace.rename {name, to}`, `workspace.delete {name}`, `workspace.list`
(each name with the tabs using it), `desk.save`, `desk.export`, `desk.import`,
`desk.forget`, `desk.reopen_on_launch.set`, `replay.close {tab}`,
`tab.move_to_window {tab}`, `settings.open`, `control.access.open`,
`help.*`, `app.exit`. One registry feeds the `⋮` menu, the right-click menu
and the `>` palette; the snapshot gives every tab `{symbol, venue, workspace,
modified}` so a bot answers "which workspace?" from data, not pixels.

## Build order

Each phase is one PR that is usable on its own. Phases 1 and 2 are additive
on today's model; phase 3 is the model change.

| Phase | What ships | Cost |
|---|---|---|
| 1 | Undecorated window; tab strip in the title bar with drawn window controls and drag region; `⋮` with today's entries regrouped (Workspace, Panels, Desk, Help); Agent pill; clickable clock; View stays until chart-identity ships | S |
| 2 | Settings window with sections; one command registry behind menu, right-click and `>` palette | M |
| 3 | Drawings and anchored studies use `market-settings.toml`, scoped by venue/asset, stable workspace/layout and optional timeframe; automatic drawing writes are independent of configuration Save, with lossless migration of legacy entries | L |
| 4 | Workspace per tab: `SavedTab` gains a workspace id and its own unsaved state; the "this tab" row control; tab title suffix; save bar; palette selectors; first-launch migration seeds one workspace per open tab, named after the tab's layout ("Fluxo") or its bare symbol and deduplicated with a number, so nothing changes on screen on day one | L |
| 5 | Tab tear-off by drag into its own window; closing it returns the tab | M |

## Open decisions recorded here

- Save model: copy on the tab with explicit "Save to" (chosen); shared-live
  with an Undo toast (rejected). Indicator sets are the exception (the
  trader's rule, 2026-11-14): tabs on one workspace show one set per strip
  layout, live; tabs on two workspaces never share one.
- Window-level name: Desk (chosen); Session (rejected for collisions).
- The per-pane layout strip (Fluxo / Force Bar Only) becomes workspace
  content; whether to rename it ("Indicator sets") is a separate decision.
- Bookmarks are retired; "Saved desks" is the name if they ever come back.

## How it was decided

The window menu went through a visual designer and a UX researcher using the
project's trader personas (24 findings, 4 blockers: `Alt` collides with the
sell modifier, submenus opened off-window, two shortcuts were wrong, `≡`
collided with dock icons). The workspace-per-tab model went through an
information architect, a UX researcher and an interaction designer with three
visual options. The palette went through three designers (visual, trading
platforms, command palettes); their consensus to hide bars and workspace
behind typed tokens was prototyped and refused by the trader, and the parts
that do not depend on it were kept. The prototype's last section lists each
agreement, disagreement and decision.

## Rules this design must keep

- Data honesty: a replay tab always says REPLAY; a market that has not
  streamed shows no price.
- No capability reachable by mouse alone: every row above has a named command.
- The tab chip and the status bar's workspace cell read one state, proven by
  a test.
- Nothing in `app` holds the workspace list, the market settings store or the
  command registry; they are headless (`workspace`, `drawings`,
  `control-schema`) with their tests.
- English UI strings; the trader's own workspace names are data.
