# Mission

Replace quantick's fixed two-pane canvas with a flexible multi-pane layout
system: a preset registry reached from a toolbar picker, panes that reorder,
resize and collapse to a rail with a guaranteed way back, and the heatmap
structurally protagonist in every preset.

Branch: `feat/flexible-canvas-layout`
Worktree: `../quantick-worktrees/feat-flexible-canvas-layout`
Cut from: `origin/main` @ c47de18

## Why

Today `CanvasLayout` has three values and `PaneSide` has two, so two panes is a
ceiling baked into the *types*. `clamp_pane_fraction` holds every pane at 25%
of the canvas, so a pane can never be dismissed. The time pane is hard-wired
left and cannot be moved. Layout is reachable only from `View -> Layout`.

## The model (decided, from four specialist reviews)

A **two-level row**, never a recursive tree: an ordered row of columns, each
column a vertical stack of panes. Depth is fixed at two. This is tmux's
`main-vertical` shape, and it is what makes "two charts on the left beside the
heatmap" expressible at all — a single-axis row of columns cannot say it.

Presets are a **registry** (`LAYOUT_PRESETS`), not enum variants: a named seed
plus a recogniser. Every preset ends with a `Flow` pane, so the heatmap being
protagonist is a property of the data table rather than a runtime check.

Shipping presets: `flow`, `time`, `time+flow`, `time+time+flow`, `flow+time`.
Deferred and named as deferred: three stacked context panes, `flow+flow`,
grids of any kind.

## Acceptance criteria

### The feature

1. **Preset registry.** `LAYOUT_PRESETS` is a table; adding a preset is a table
   entry. Proved by a `#[cfg(test)]` fake preset and a fake third `PaneKind`
   that the row lays out, the DTO names and persistence round-trips **with no
   `match` arm inside the layout engine growing a case**. A grown arm is a
   failed port and a Blocker against this change.
2. **`time+time+flow` renders**: two stacked context charts in the left column,
   heatmap full-height right. This is the CEO's stated ask and the mission is
   not done without it on screen.
3. **The picker.** A `LAYOUT` icon in toolbar zone 2 opens a popover of preset
   thumbnails, the flow block filled to teach which pane is the heatmap.
   `Ctrl+1..5` apply presets. `View -> Layout` is kept as the discoverable path.
4. **Reposition through one path.** `layout.pane.move` is the named call; the
   drag gesture and the header menu entries both call it. Two code paths for
   one action is a Blocker — the drag is sugar over the call, never a twin.
5. **Collapse leaves a handle.** A collapsed pane is an 8 px rail with a >= 24 px
   hit target and restores to its prior width. Never literally zero, never
   handle-less. The shared invariant with the vertical axis
   (`indicators::COLLAPSED_PANE_HEIGHT_PX`, "Never zero") is stated once in
   code; the two axes may differ in pixels because one holds a text row and the
   other a grip, and that difference is documented where both can see it.
6. **Pane identity.** `pane_ids(tab) = (tab*2, tab*2+1)` is deleted and replaced
   by a never-reused monotonic counter. Test: 3 tabs x 3 panes, pairwise-distinct
   ids, and add-after-remove never repeats one. This is a live bug today — a
   third pane on tab 0 takes tab 1's flow-pane id and shares its drag.
7. **Sizing.** `MIN_PANE_FRACTION = 0.25` is arithmetically incompatible with
   three panes and is replaced by a pixel floor plus a collapsed width, in the
   two-pass shape `indicators::split_panes` already proved. One clamp, one
   owner — the drag, the restore path and the control-plane call all call it.
   Default split changes 50/50 -> 35/65 in the heatmap's favour.

### Honesty and trust

8. **The consent string stops being a lie.** `control/gateway.rs:1399` promises
   the trader that agent access "cannot ... change your layout". The seven new
   `layout.*` capabilities land under a **new `cockpit` effect and permission
   that the `Annotator` profile does not inherit**, and the string is rewritten
   to match what the tier can actually do.
9. **Old workspaces still open.** A checked-in v1 `ui-state.toml` carrying
   `layout` + `split_fraction` and no `panes` key opens to the right two-slot
   row (golden migration test). `FORMAT_VERSION` stays 1, per the repo's own
   additive-field policy at `ui_state.rs:317-319`.

### Standard gates

10. **English throughout** — every identifier, comment, UI string, test name and
    doc line this branch authors. `arch-review` dimension 8 and
    `crates/app/tests/language_guard.rs`.
11. **Four checks green** after rebasing on latest `main`:
    `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -D warnings`,
    `cargo build --workspace`, `cargo test --workspace`.
12. **Performance declared and measured.** `split_canvas` is **per-frame** and
    must be allocation-free (SmallVec, caller-owned areas). N `ChartState`s off
    one tape is **per-trade** and linear in pane count. Evidence required before
    the PR: `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape at 1, 2 and 3
    panes against a `main` control run, numbers in the PR body. A regression is
    reported, never absorbed.
13. **Docks as a module** (`new-extension`): the port is `PaneKind` +
    `LAYOUT_PRESETS`; edits are registration-only outside `tab.rs`/`app.rs`,
    whose two-ness *is* the thing being generalised; defaults preserve today's
    behaviour exactly; blast radius (added vs edited files) stated in the PR body.
14. **Drivable without a mouse** (`arch-review` *The second operator*): every
    layout action is a named capability with a readable result and a registry
    entry, addressed by `pane_id` — never by `pane_side`, which stops being
    unique the moment a row holds two time panes.
15. **UI harness + visual QA.** `QUANTICK_LAYOUT` extended to the new preset ids
    and a new `QUANTICK_PANE_COLLAPSED` hook, both registered in the
    `ui-harness` skill. `visual-qa` pass across the preset matrix with every
    surface PASS or a defect explicitly accepted. `trader-ux-review` with no
    unresolved Blocker.
16. **Accessibility defects fixed, not inherited.** The divider is painted in
    `border` at **1.48:1** against `bg/canvas` today, below the 3:1 floor for a
    draggable control. Raised to a `bg/inset` trough plus a `text/faint`
    hairline (4.01:1), grab area >= 24 px. The `Ctrl+Shift+Arrow` collision with
    `nudge_bars` (`app.rs:6493`, no `!command` guard) is fixed in the same change.
17. **arch-review run** over `git diff main...HEAD`, every Blocker and
    Should-fix resolved or deferred in the PR body. **PR opened.** Merging is
    the CEO's call and is not part of this mission.

## Staging (commits on this one branch, one PR)

- **A — identity and the flat model, invisible.** `PaneId` counter replaces
  `pane_ids()`; `Tab::panes: BTreeMap<PaneId, ChartPane>` with `flow_pane` /
  `time_pane` as accessors; the four inlined `panes_mut()` duplicates
  (`tab.rs:548, 1116, 2147, 2223`) routed through the accessor; new pure
  `canvas_layout.rs` builds today's exact two-pane row. Behaviour
  byte-identical. Ships alone on purpose — it is the surgery.
- **B — N panes on screen.** `split_canvas` over columns, per-pane divider ids,
  the picker, `time+time+flow`, collapse-to-rail, reorder. This is what the CEO
  sees.
- **C — control plane and persistence.** The `cockpit` effect, seven
  capabilities, `workspace.layout.changed`, snapshot v2, `SavedPane` and the v1
  migration. Purely additive.
- **D — docs and polish.** `docs/ux/ui-design-model.md` §11 rewrite, §14 bullet,
  harness hooks, visual-qa and trader-ux-review passes.

## Deliberately out of scope

Free-form dockable workspaces, floating/tear-off panes, OS-window-per-chart,
2x2 and larger grids, named user-saved workspaces, per-pane symbols (that is
what workspace tabs are for), and TradingView-style sync groups (quantick has
one tape per tab — there is nothing to sync).

## Status — end of the first session

Pushed: `origin/feat/flexible-canvas-layout`, nine commits, four checks green
(1746 app tests; the only red in `cargo test --workspace` is
`the_bridge_paging_tests_pass`, which fails here because `python3` resolves to
the Microsoft Store alias — the bridge's own 21 tests pass under `python`, and
CI runs a real `python3`).

**Done: 7 of 17 criteria.**

- (1) Preset registry with the `#[cfg(test)] PaneKind::Fake` port proof.
- (3) Picker icon + popover, `View -> Layout` kept, `Ctrl+1..9` from the
  registry, all three doors calling `apply_layout_preset`.
- (6) `pane_ids(tab) = (tab*2, tab*2+1)` deleted for a monotonic never-reused
  counter, with the 3x3 distinct-id test. This was a live bug: a third pane on
  tab 0 took tab 1's flow-pane id and would have shared its drag.
- (7) `MIN_PANE_FRACTION` replaced by a pixel floor plus a collapsed width,
  one clamp owner; default split 35/65.
- (10) fmt / clippy / build / test green (rebase on `main` still owed).
- (11) `APP_HEALTH_SUMMARY` captured at 60 fps for the two-pane canvas; the
  1/2/3-pane comparison against a `main` control run is still owed.
- (15, partial) `QUANTICK_LAYOUT_PICKER=1` registered in `ui-harness`.

**The one thing blocking criterion 2**, written down so it is not rediscovered:

`Tab` now holds `time_panes: SmallVec<[ChartPane; MAX_CONTEXT_PANES]>`, and
`canvas_layout::split_column` carves the stack. Laying the panes out is done.
Drawing on them is not. `shared_picks`, `apply_shared_interactions` and
`paint_shared_drawings` are **pairwise by construction** — each asks about
"the other pane", singular:

- `SharedPicks { time, flow }` is a two-named-field struct (`tab.rs:275`).
- `apply_shared_interactions` routes an edit with `side.other()`
  (`tab.rs:2808`), which stops naming a unique owner at three panes.
- `paint_shared_drawings` calls `paint_shared_from` once per direction
  (`tab.rs:2833`).

A mark shared across three panes has **two** owners. Generalising this decides
which chart a trader's edit lands on and which charts their drawings appear on,
so it is a design change rather than a rename, and half-wiring it would make
two surfaces disagree about the same object — the bug class this repo treats as
its own. Do this first; `time+time+flow` falls out of it.

Two `#[allow(dead_code, reason = ...)]` markers are the outstanding-work
flags — on `PaneWidth::Collapsed` and on `split_column`/`Axis`. Both come off
in the commit that draws the stack. Grep for them before opening the PR.
