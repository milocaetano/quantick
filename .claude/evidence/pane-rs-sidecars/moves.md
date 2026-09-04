# The moves, and every line that is not one (R1–R7, R11 / A1–A5, A9, A10)

## What moved

| Module | Methods | Lines | Cut from `pane.rs` |
| --- | --- | --- | --- |
| `pane/menus.rs` | 5 | 353 | `:2032-2357` |
| `pane/strategy_badges.rs` | 11 | 374 | `:2359-2709` |
| `pane/drawing_gestures.rs` | 17 | 802 | `:3327-3397`, `:3548-4241` |
| `pane/axes_and_chrome.rs` | 15 | 744 | `:6163-6455`, `:7271-7684` |

Each range starts at the moved function's own doc comment, not at its `fn`
line, so no doc comment was orphaned. The four are declared by `mod` lines in
`pane.rs` immediately after its `use` block — where `app.rs:21-25` declares its
own sidecars.

## Bodies unchanged — proved by hash, not by reading

Every moved method was extracted from `origin/main:crates/app/src/pane.rs` and
from its new home, its visibility prefix normalised (`pub(super) fn` → `fn`),
and the two hashed:

```
moved methods: 48   byte-identical to main: 48   differing: 0   not found in main: 0
  pane/axes_and_chrome.rs:  15 methods, all identical
  pane/drawing_gestures.rs: 17 methods, all identical
  pane/menus.rs:             5 methods, all identical
  pane/strategy_badges.rs:  11 methods, all identical
```

`git diff --cached --color-moved=zebra -- crates/app/src`, with each `+`/`-`
line classified by the colour git gave it:

```
classified +/- lines: {'moved': 4283, 'plain +': 161, 'plain -': 40}
```

## Every non-move line, and why it exists

The 201 unclassified lines are exhausted by four groups. There is no fifth.

**1. Two imports pruned from `pane.rs` (3 lines).** Their only users moved out;
`warnings = "deny"` makes an unused import a build failure, so leaving them was
not an option.

```
-use crate::plot_area::{self, PlotAreas, fmt_time_as, plot_split, split_time_strip};
+use crate::plot_area::{self, PlotAreas, plot_split, split_time_strip};
-use quantick_orderflow::{ … five items … };
+use quantick_orderflow::reserved_span_ms;
```

**2. The four `mod` lines (5 lines, one blank).**

```
+mod axes_and_chrome;
+mod drawing_gestures;
+mod menus;
+mod strategy_badges;
```

**3. Twenty-four `pub(super)` prefixes (24 `-`/`+` signature pairs).** This is
the one visibility widening the mission performs, and it is forced rather than
chosen. Brief ledger #9 is right that a child sees its ancestor's private items
— no moved body needed a change to reach `self`. The converse is what costs: a
private method *defined in a child* is private to that child, so any moved
method still called from `pane.rs` or from a sibling module stops resolving.
The compiler named all 24 (`E0624`); none was widened speculatively, and none
went past `pub(super)`.

| Module | Method | Still called from |
| --- | --- | --- |
| `axes_and_chrome` | `draw_axis_marks` | `pane.rs`, `pane/tests/mod.rs` |
| `axes_and_chrome` | `draw_backfill_divider` | `pane.rs` |
| `axes_and_chrome` | `draw_crosshair` | `pane.rs` |
| `axes_and_chrome` | `draw_feed_gaps` | `pane.rs` |
| `axes_and_chrome` | `draw_lane_time_axis` | `pane.rs`, `pane/tests/mod.rs` |
| `axes_and_chrome` | `draw_pointer_compass` | `pane.rs`, `pane/tests/mod.rs` |
| `axes_and_chrome` | `draw_price_axis` | `pane.rs` |
| `axes_and_chrome` | `draw_seam_divider` | `pane.rs` |
| `axes_and_chrome` | `draw_time_strip` | `pane.rs` |
| `axes_and_chrome` | `pointer_compass` | `pane.rs`, `pane/tests/mod.rs` |
| `drawing_gestures` | `anchor_time` | `pane.rs` |
| `drawing_gestures` | `candle_at_slot` | `pane.rs` |
| `drawing_gestures` | `drag_drawing_handle` | `pane.rs` |
| `drawing_gestures` | `drawing_at` | `pane.rs`, `pane/tests/mod.rs` |
| `drawing_gestures` | `drawing_below_selection` | `pane.rs` |
| `drawing_gestures` | `drawing_handle_at` | `pane.rs` |
| `drawing_gestures` | `drawing_handle_in` | `pane.rs` |
| `drawing_gestures` | `drawing_pick_at` | `pane.rs` |
| `drawing_gestures` | `drawing_point_at` | `pane.rs` |
| `drawing_gestures` | `handle_drawing_placement` | `pane.rs` |
| `drawing_gestures` | `place_drawing_point` | `pane/menus.rs` |
| `menus` | `layer_checkbox` | `pane.rs` |
| `strategy_badges` | `draw_strategy_menu_entries` | `pane/menus.rs` |
| `strategy_badges` | `paint_strategy_badge` | `pane.rs` |

The other 24 moved methods kept exactly the visibility they had: 4 `pub`, 8
`pub(crate)`, and 12 still private. No item anywhere gained `pub(super)` for a
reason other than a call site the compiler pointed at.

Those 12 are worth naming, because the move made them *more* encapsulated, not
less — each was private inside a 7,771-line file and is now private inside a
few hundred lines: `draw_chart_layer_entries`, `draw_tape_menu_section`,
`draw_drawing_menu_section`, `rewarm_strategy_trigger`, `bar_extreme`,
`magnet_value`, `candle_nearest_ohlc`, `placement_target`, `shaped_placement`,
`draw_last_price`, `draw_drawing_axis_tags`, `gap_slot`. The net visibility
change of the branch is 24 methods widened by one module level and 12 narrowed
by roughly an order of magnitude in reachable lines.

**4. The four module headers (~120 lines).** A module doc comment, the imports
each file actually uses, `use super::{…}` for the parent items it names, and
the `impl ChartPane {` / `}` wrapper. The import lists were produced by
restoring the parent's whole `use` block into each child and letting
`cargo fix` delete what the compiler found unused, so no file carries an import
it does not need and none was pruned by guesswork.

### One line that looks like an edit and is not

`git diff` shows `+        area: egui::Rect,` inside `pane.rs`. It is a hunk
alignment artifact: `handle_drawing_placement`'s departing signature shares
three parameter lines with `handle_navigation`'s, which immediately follows it,
so git pairs those as context and reports the one differing parameter as added.
`handle_navigation` itself is untouched — see `numbers.md`, where it hashes
identical to `origin/main`.

## Free functions do not travel (R11 / A9)

`paint_placement_hint`, `snap_bar_to_tape` and `magnet_price_of` sit at module
scope outside the `impl ChartPane` block this mission cut from, and they are
called from both sides of the cut — `snap_bar_to_tape` and `magnet_price_of`
from `drawing_gestures`, `paint_placement_hint` from code that stays. Leaving
all three in `pane.rs` costs no re-export and no widened visibility, and leaves
`pane/tests/mod.rs`, which calls two of them by bare name through `use super::*`,
completely untouched. `draw_dashed_vertical` (`pane.rs:150`) stays for the same
reason and is additionally called from `orderflow_render.rs`.

So: **no `pub(super) use` re-export was added anywhere**, and the only
`pub(super)` tokens on the branch are the 24 method prefixes above.

## `interact_shared` stays in `pane.rs` (R12 / A10)

Brief ledger #4 asks the mission to read it and say which side it belongs to.
It sits between the drawing group's two ranges and reads like a gesture, but it
is not one: it works a mark that lives on the *other* pane, and every answer it
returns leaves in market time and price precisely so neither pane learns the
other's bar space. Its only caller is `handle_navigation`, which does not move.
Filing it under drawing gestures would put cross-pane coordination in a module
whose stated subject is this pane's own pointer arithmetic, and would separate
it from the function that calls it. It stays, byte-identical, now at `:2660`.
