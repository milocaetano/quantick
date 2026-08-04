# Drawing toolbar & inspector — UX redesign

Status: **specification, not yet implemented.** Supersedes the corner-docked
toolbox geometry described in [`ux/ui-design-model.md`](ux/ui-design-model.md)
§7 and the placement bullets in
[`ux/drawing-tools-ux-spec.html`](ux/drawing-tools-ux-spec.html) §05. Everything
else in both documents survives unchanged and stays authoritative — the
registry contract, the level editor, the escape stack, the textual action bar,
lock/hide/delete semantics, the undo coalescing and the colour rules.

Three user complaints drive this redesign:

1. the toolbar reads as a placeholder rather than a tool;
2. the properties popup opens on top of the drawing it describes and eats
   clicks meant for the chart;
3. the toolbar cannot be docked on a lateral edge — the four "corners" only
   flip the reading direction of the same top/bottom strip.

---

## 1. Design direction

**What "TradingView quality" means here, concretely.** Not more chrome — more
*resolution* in the chrome that already exists. TradingView's left rail is
persuasive for four reasons, and all four are cheap in egui: it is **vertical
and lateral**, so it never eats the chart's vertical budget where price lives;
it is **grouped**, with hairline rules separating cursors from drawing tools
from global protections, so the eye lands on a cluster rather than scanning a
run of glyphs; it **scales through families**, where a whole class of tools
occupies one slot that remembers which member you last used and opens the rest
on a caret; and its **states are legible from the corner of your eye** — the
armed tool carries a tinted backdrop *and* an accent edge marker, so you never
have to look directly at the rail to know what a click will do. The current
strip has none of these: it is a flat, evenly spaced run of eleven buttons in a
44 px band across the top of the window, which reads as unfinished because it
*is* undifferentiated.

**What stays quantick.** We are not cloning the rail. Three things stay ours.
First, the size discipline: `CLAUDE.md` says this is not a trading platform,
and §5 of the design model says the chart buys every pixel. TradingView's rail
is 52 px wide and hosts thirty tools behind eight family flyouts; ours is
**44 px in its cross axis in every orientation** — the same 44 the design
model already budgets for the top/bottom strip, so the shell's pixel accounting
survives docking verbatim — and ships exactly one family flyout, because
exactly one family (Fibonacci) currently has two members. The flyout mechanism
exists so the tenth tool costs zero new chrome, not so we can look busy on day
one. Second, the amber thread: `#F0B90B` is reserved for provenance honesty and
does not appear anywhere in this document. The rail's only colour is
`accent/overlay` `#8AB4F8`, and it means exactly one thing — *this control is
armed*. Third, the honesty rule applied to chrome: a control that cannot act
is dimmed and explains itself on hover, never hidden, and the overflow flyout
lists what it swallowed **by name with its shortcut**, so a collapsed rail is
still a readable inventory rather than a mystery button.

**The one deliberate risk.** The trailing cluster (repeat pin, hide-all,
lock-all, Objects) is **pinned to the far end of the rail**, not packed behind
the tools. That leaves a variable empty gap in the middle of the rail, which
looks wrong in a mockup and right in use: the two clusters answer different
questions (*what am I about to draw?* vs *what have I already drawn?*), and
separating them by whitespace rather than by another hairline means the hand
learns two destinations instead of one list. It also gives the overflow
staging a place to absorb pressure without ever reflowing the top of the rail —
the tools stay exactly where they were on the last window size.

---

## 2. Tool rail

### 2.1 Geometry

One thickness for every orientation. `TOOLBOX_HEIGHT_PX` is renamed
`TOOLBOX_THICKNESS_PX` because it is now a width half the time.

| Constant | Value | Meaning |
|---|---|---|
| `TOOLBOX_THICKNESS_PX` | `44.0` | rail cross axis, all four docks |
| `TOOLBOX_MARGIN_PX` | `6.0` | outer margin, both axes |
| `TOOLBOX_ITEM_GAP_PX` | `4.0` | gap between buttons inside a cluster (was 6.0) |
| `TOOLBOX_GROUP_GAP_PX` | `4.0` | clear space each side of a separator |
| `TOOLBOX_SEPARATOR_THICKNESS_PX` | `1.0` | the hairline itself |
| `TOOLBOX_SEPARATOR_INSET_PX` | `8.0` | inset from each rail edge, so the rule floats |
| `TOOLBOX_GRIP_LENGTH_PX` | `18.0` | grip extent along the rail |
| `TOOLBOX_GRIP_GLYPH_PX` | `14.0` | grip glyph font size |
| `TOOLBOX_MIN_CLUSTER_GAP_PX` | `12.0` | smallest allowed gap between leading and trailing clusters |
| `TOOLBOX_CARET_ZONE_PX` | `10.0` | square corner zone of a family button that opens its flyout |
| `TOOLBOX_FLYOUT_WIDTH_PX` | `208.0` | flyout popup width |
| `TOOLBOX_FLYOUT_ROW_HEIGHT_PX` | `26.0` | one flyout row |
| `TOOLBOX_TOOLTIP_DELAY_MS` | `350` | hover delay before a tooltip appears |

Buttons get their own icon geometry rather than borrowing `RAIL_ICON` (which
the dock tab strip also uses and which must not move). In `widgets.rs`:

```rust
/// Drawing-rail geometry: an 18 px glyph on a 32 px hit target — the 32 px
/// touch target the accessibility contract requires, with enough padding
/// around the glyph that a 44 px rail does not read as cramped.
pub const TOOLRAIL_ICON: IconSize = IconSize { glyph: 18.0, hit: 32.0 };
```

A 32 px button centred in a 44 px rail leaves 6 px each side — exactly
`TOOLBOX_MARGIN_PX`, so the rail is one arithmetic identity:
`44 = 6 + 32 + 6`.

### 2.2 Cluster order

Reading order in every dock: **leading → trailing**. Top-to-bottom in a
vertical rail, left-to-right in a horizontal one. The old `is_left()` layout
reversal disappears with the corners — a top rail and a bottom rail now lay
out identically, and so do a left rail and a right rail.

```
LEADING (packed at the rail's start)          TRAILING (pinned at the rail's end)
┌────────┐                                    ┌────────┐
│  grip  │  DOTS_SIX_VERTICAL / DOTS_SIX      │  ───   │  separator
├────────┤                                    │ repeat │  REPEAT
│pointer │  CURSOR            (1, Esc)        │hide-all│  EYE / EYE_SLASH
│crossh. │  CROSSHAIR         (2)             │lock-all│  LOCK_SIMPLE(_OPEN)
│  ───   │  separator                         │  ───   │  separator
│  horz  │  MINUS             (H)             │objects │  LIST + count badge
│  rect  │  RECTANGLE         (R)             └────────┘
│channel │  PARALLELOGRAM     (C)
│  fib ⌄ │  family slot       (F / Shift+F)
└────────┘
         ⋮  flexible gap, ≥ TOOLBOX_MIN_CLUSTER_GAP_PX
```

Nothing is lost relative to today's rail. Pointer, Crosshair, the five
registry tools, the repeat pin, the Objects toggle, hide-all, lock-all and the
overflow flyout all survive; the repeat pin moves from "after the tools" into
the trailing cluster, next to the other two things that modify *how drawing
behaves* rather than *what gets drawn*, and the two Fibonacci tools share one
slot.

### 2.3 Tool families

Two Fib entries side by side in a five-tool rail is the shape of a rail that
does not scale. A family is declared by the tool, in the registry, so the rail
never grows a central match and no dependency reverses:

```rust
/// A family of related tools sharing one rail slot. Declared by each member,
/// never listed centrally — the rail folds consecutive registry entries with
/// equal `id` into a single split button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolFamily {
    pub id: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
}

// on DrawingToolImpl, defaulted so existing tools need no edit:
fn family(&self) -> Option<ToolFamily> { None }
```

`fib_retracement.rs` and `fib_extension.rs` both return
`ToolFamily { id: "fib", title: "Fibonacci", icon: icons::ROWS }`. Rules:

- The rail folds a **consecutive run** of registry entries with equal
  `family.id` into one slot. Consecutive, not sorted — rail order stays
  registry order, so the layout is deterministic and adding a tool cannot
  silently reorder the rail.
- The slot paints the **last-armed member's** own icon, not the family icon.
  The family icon is the fallback before any member has been used, and the
  title labels the flyout header.
- Last-armed memory is a `BTreeMap<&'static str, DrawingTool>` on `ToolRail`
  (`BTreeMap` over `HashMap` per the project's ordering rule, even though this
  one is chrome).
- **Left-click** arms the shown member. **Right-click anywhere on the button**,
  or a left-click inside the trailing-corner `TOOLBOX_CARET_ZONE_PX` square,
  opens the flyout.
- The caret is a 5 px filled triangle in the button's trailing-bottom corner,
  inset 3 px: `TEXT_FAINT` idle, `TEXT_PRIMARY` on hover, `ACCENT` when the
  family is armed. It is hidden while a draft badge occupies that corner
  (§2.5) — a tool mid-draft is unambiguously armed, so the caret has nothing
  to add.
- The flyout is an `egui::Area` at `Order::Foreground`, opening on the rail's
  **chart-facing side**, its first row aligned with the button's leading edge,
  clamped into the screen rect. `CONTROL` `#232936` fill, 1 px `BORDER`
  stroke, 6 px corner radius, `POPOVER_SHADOW` (offset `[0, 4]`, blur `12.0`,
  spread `0.0`, colour `#000000` at alpha `96`).
- Each row is `TOOLBOX_FLYOUT_ROW_HEIGHT_PX` tall: 18 px glyph, name in
  `TEXT_PRIMARY`, shortcut right-aligned in `TEXT_FAINT`. The armed member's
  row carries the active fill.
- Escape, a click outside, or arming a member closes the flyout. A closed
  flyout returns focus to the rail button.

### 2.4 Button states

Keep `widgets::icon_paint` and its four resolved states — the function is
already unit-tested and the state table it encodes is the design model's. Two
additions:

| State | Backdrop | Glyph | Extra |
|---|---|---|---|
| Idle | none | `TEXT_MUTED` `#96A0AF` | — |
| Hover | `BORDER` `#2E3648` | `TEXT_PRIMARY` `#D2DAE2` | tooltip after 350 ms |
| **Pressed** (new) | `press_tint(ACCENT)` = `#8AB4F8` @ alpha `84` | `ACCENT` `#8AB4F8` | — |
| Active | `active_tint(ACCENT)` = `#8AB4F8` @ alpha `56` | `ACCENT` `#8AB4F8` | accent edge marker |
| **Focused** (new) | as its other state | as its other state | 1.5 px `ACCENT` ring, inset 1 px, radius 4 |
| Disabled | none | `TEXT_MUTED` @ 40 % | tooltip explains why |

Precedence, resolved before any egui call so it stays testable:
`disabled > pressed > active > hover > idle`. The focus ring composes on top
of whichever of those won — it is a keyboard affordance, not a state.

Two new tokens in `theme.rs`, no parallel palette:

```rust
/// Alpha of a "pressed" tint: a layer accent at 33% over the chrome. One
/// step deeper than ACTIVE_TINT_ALPHA, so a press on an already-active
/// button is still visible.
const PRESS_TINT_ALPHA: u8 = 84;

#[must_use]
pub fn press_tint(accent: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), PRESS_TINT_ALPHA)
}

/// `text/support` — small explanatory lines that carry real information
/// (the inspector's locked/hidden notes). TEXT_FAINT stays reserved for
/// decoration and disabled states, where 4.5:1 is not required.
pub const TEXT_SUPPORT: Color32 = Color32::from_rgb(0x86, 0x92, 0xA4);
```

**Active edge marker.** A 2 px `ACCENT` bar hugging the button's **outer**
edge — the one facing the window edge the rail is docked against — inset
`ACTIVE_MARKER_INSET_PX` from both ends, radius 1 px. In a left rail it is a
2×20 px vertical bar at the button's left; in a top rail a 20×2 px horizontal
bar at the button's top. It rotates with the dock and is the single detail
that makes the armed tool findable in peripheral vision.

```rust
// widgets.rs
pub const ACTIVE_MARKER_WIDTH_PX: f32 = 2.0;
pub const ACTIVE_MARKER_INSET_PX: f32 = 6.0;
pub const FOCUS_RING_WIDTH_PX: f32 = 1.5;
```

Corner radius stays `CORNER_RADIUS = 4.0`.

### 2.5 Badges

A badge is a pill in the button's trailing-bottom corner, inset 2 px:
`BADGE_HEIGHT_PX = 12.0`, `BADGE_RADIUS_PX = 3.0`, `BADGE_TEXT_PX = 9.0`
monospace, horizontal padding 3 px, `INSET` `#10141D` fill.

- **Draft progress** on the armed tool while a multi-point object is being
  placed: `1/3`, text in `ACCENT`. Answers "how many more clicks?" without a
  status-bar trip. Absent for one-point tools.
- **Object count** on the Objects button when the store is non-empty: the
  item count, text in `TEXT_MUTED` — it is a reading, not a state. Counts
  from ten upward render as-is; the badge grows, the button does not.

### 2.6 Rail chrome

Fill `CHROME` `#171B26`. **No four-sided stroke** — instead a single 1 px
`BORDER` `#2E3648` line along the rail's chart-facing edge. The current
`Frame::stroke` paints a hairline against the window edge too, which reads as
a seam that is not there.

Separators are 1 px `BORDER` lines spanning the rail's cross axis inset
`TOOLBOX_SEPARATOR_INSET_PX` from both edges (so 28 px of line in a 44 px
rail), with `TOOLBOX_GROUP_GAP_PX` of clear space on each side.

Grip: `DOTS_SIX_VERTICAL` in a vertical rail, `DOTS_SIX` in a horizontal one —
the dots always run across the rail, reading as something to take hold of.
`TOOLBOX_GRIP_GLYPH_PX` = 14, `TEXT_FAINT` idle, `TEXT_MUTED` on hover, hit
area `32 × TOOLBOX_GRIP_LENGTH_PX` (rotated for horizontal docks).

### 2.7 Panel construction and declaration order

```rust
match dock {
    Left   => SidePanel::left("drawing_toolbox_left"),
    Right  => SidePanel::right("drawing_toolbox_right"),
    Top    => TopBottomPanel::top("drawing_toolbox_top"),
    Bottom => TopBottomPanel::bottom("drawing_toolbox_bottom"),
}
.exact_width(TOOLBOX_THICKNESS_PX)   // exact_height for the top/bottom pair
.resizable(false)
```

Keep the current declaration order in `App::update`: menu → toolbar → status
bar → **rail** → dock → pinned inspector → `CentralPanel`. Two consequences
worth stating, because they are both correct and both free:

- A vertical rail automatically spans only the space between the toolbar and
  the status bar, because those panels are declared first.
- When the rail and the dock are both on the right, the rail sits **outermost**
  (against the window edge) and the dock sits inboard of it, matching
  TradingView and keeping the rail's distance from the window edge constant
  across docks.

The rail remains outside `CentralPanel`, so the chart never renders behind it —
the existing test `every_dock_position_reserves_space_outside_the_central_chart`
extends to the two new docks unchanged in spirit.

### 2.8 Length budget and overflow

The rail never wraps and never scrolls. It has three stages, chosen from the
available **long-axis** extent — the same rule in both orientations, so one
function serves all four docks.

Slot arithmetic: one button slot is `32 + 4 = 36` px; a separator block is
`4 + 1 + 4 = 9` px; the grip block is `18 + 4 = 22` px.

```
full_length(n_tool_slots) =
      MARGIN(6) + grip(22)
    + modes(32 + 4 + 32 = 68) + sep(9)
    + tools(n*32 + (n-1)*4)
    + MIN_CLUSTER_GAP(12)
    + sep(9) + globals(32 + 4 + 32 + 4 + 32 = 104) + sep(9) + objects(32)
    + MARGIN(6)
```

For today's registry (`n = 4`: horizontal, rectangle, channel, fib family):

| Stage | Trigger | Rail shows | Length |
|---|---|---|---|
| **Full** | available ≥ `417` | everything above | 417 px |
| **Compact** | `345` ≤ available < `417` | Pointer, Crosshair, armed tool, More; full trailing cluster | 345 px |
| **Minimal** | available < `345` | Pointer, armed tool, More, Objects | 191 px |

- The armed tool always keeps a real slot; it is never buried in the flyout.
- The More flyout (`DOTS_THREE`, replacing today's misleading `PLUS`) lists
  everything the stage swallowed, by name and shortcut, in registry order:
  Crosshair and the unarmed tools at Compact, plus repeat / hide-all /
  lock-all at Minimal. The existing narrow-strip rule — *Pointer, the armed
  tool and Objects always survive* — is preserved exactly; this spec only adds
  the intermediate stage and makes the budget axis-aware.
- Stage changes are hysteresis-free: pure functions of available extent, so a
  window resize gives the same rail whichever direction it was dragged from.
- **Minimum:** 191 px. The app opens at `1100 × 650` and sets no minimum
  window size; add `with_min_inner_size([900.0, 560.0])` in `main.rs` so a
  horizontal rail (191 px of a 900 px window) and a vertical rail (191 px of
  560 px minus the 100 px of menu + toolbar + status bar = 460 px) are both
  unreachable failure modes rather than clipped chrome.

---

## 3. Docking & drag

### 3.1 From corners to edges

`ToolboxDock` becomes four **edges**, not four corners:

```rust
pub enum ToolboxDock { #[default] Left, Right, Top, Bottom }
```

This is the behavioural change the third complaint asks for. `TopLeft`,
`TopRight`, `BottomLeft` and `BottomRight` are gone; the layout-direction flip
they controlled disappears with them (§2.2).

**Default dock: `Left`.** TradingView-style, and the right choice here for a
reason beyond familiarity — the right edge is already claimed by the price
axis, the dock and the pinned inspector, while the left edge is empty in every
shipped configuration.

### 3.2 Drop rule — nearest edge, normalised

Pixel distance would bias every drop toward the top or bottom, because the
window is wider than it is tall. Normalise first, which partitions the screen
into four triangles meeting at the centre:

```rust
fn nearest(pointer: Pos2, screen: Rect) -> ToolboxDock {
    let left   = (pointer.x - screen.left())   / screen.width();
    let right  = (screen.right() - pointer.x)  / screen.width();
    let top    = (pointer.y - screen.top())    / screen.height();
    let bottom = (screen.bottom() - pointer.y) / screen.height();
    // ties resolve Left > Right > Top > Bottom, so the result is deterministic
}
```

A release outside the screen rect clamps into it first; there is no "cancel by
dropping outside".

### 3.3 Grip and drag feedback

- **Affordance.** The grip (§2.6) sets `CursorIcon::Grab` on hover and
  `CursorIcon::Grabbing` while dragging. Hover text:
  `"Drag to dock the toolbar on another edge"`.
- **Feedback: an edge preview band, and nothing else.** While the drag is
  live, an `egui::Area` at `Order::Foreground` covering the screen rect paints,
  along the currently-nearest edge, a `TOOLBOX_THICKNESS_PX`-wide band filled
  with `active_tint(ACCENT)` (`#8AB4F8` at alpha 56) and edged on its
  chart-facing side with a `TOOLBOX_DROP_BAND_EDGE_PX = 3.0` line in solid
  `ACCENT`. The band is the exact rectangle the rail will occupy, so the
  preview is the answer, not an approximation of it.
- **No cursor ghost.** A translucent copy of the rail trailing the pointer was
  considered and cut: it duplicates information the band already carries, and
  in a 44 px rail it is mostly empty space chasing the mouse. One accessory
  removed.
- The rail itself keeps painting in place during the drag — it does not go
  translucent and does not vanish. Nothing moves until release.
- The band updates every frame as the nearest edge changes, so crossing a
  diagonal snaps the preview from one edge to the next; that snap *is* the
  feedback that the partition exists.
- **Escape cancels.** While a rail drag is live, `Esc` aborts it and keeps the
  current dock. This becomes the topmost level of the app's existing escape
  stack (rail drag → text input → draft → selection → Pointer); every level
  below it is untouched.

### 3.4 Keyboard and menu path

Dragging is not the only path. The **View** menu's existing drawing-toolbox
entry gains a sibling submenu:

```
View → Drawing toolbar → Left · Right · Top · Bottom     (radio, current checked)
       Hide drawing toolbar                              (existing entry, renamed)
```

"Toolbox" becomes "toolbar" in user-facing strings throughout — it is a
toolbar, and the code already calls the module `toolrail`.

### 3.5 Persistence

The dock is a `dock: ToolboxDock` field on `ToolRail`, session state today.
It persists through the UI-state file named as the open question in
`ux/ui-design-model.md` §13 — a `ui-state.toml` sibling of
`indicators-state.toml` — alongside dock width, active dock tab and rail
visibility, whenever that file lands. Until then a restart returns to `Left`.
No new persistence mechanism is invented for this feature alone.

---

## 4. Properties inspector

### 4.1 The two failures

The popup lands on the drawing because `inspector_target_position` tries four
candidates, and when none fits it falls back to `candidates[0]` — *right of the
bounding box* — and then clamps that into the chart. On a wide object, or near
the right edge, the clamp walks the panel straight back over the geometry. A
blind first candidate is the worst possible fallback: it is the one position we
already know does not fit.

The popup eats clicks because `handle_navigation` reads the raw
`primary_pressed` and hit-tests drawings **regardless of what the pointer is
over**, deliberately, "so an already-selected drawing remains draggable even
when its stroke or handle is underneath that inspector". With a horizontal line
— whose stroke spans the whole chart width — that exception fires for a press
anywhere on the inspector at that price row, and the press lands twice: once on
the slider, once as the start of a drag.

### 4.2 Placement algorithm

Inputs: `bbox` (projected anchors expanded by `DRAWING_ANCHOR_RADIUS_PX` =
12 px), `size` (the measured area rect, else `320 × 280`, or `360 × 280` for a
tool with an extra tab), `chart` (`last_chart_area`, already excluding both
axes and the live lane), `gap` = `INSPECTOR_OBJECT_GAP_PX` = 12 px.

**Candidates, in preference order** — four beside the object, four in the
chart's corners:

| # | Position |
|---|---|
| C1 | `(bbox.right + gap, bbox.top)` — right of the object |
| C2 | `(bbox.left - gap - w, bbox.top)` — left of it |
| C3 | `(bbox.left, bbox.bottom + gap)` — below it |
| C4 | `(bbox.left, bbox.top - gap - h)` — above it |
| C5 | `(chart.left + gap, chart.top + gap)` — chart top-left |
| C6 | `(chart.right - gap - w, chart.top + gap)` — chart top-right |
| C7 | `(chart.left + gap, chart.bottom - gap - h)` — chart bottom-left |
| C8 | `(chart.right - gap - w, chart.bottom - gap - h)` — chart bottom-right |

**Selection:**

1. Clamp **every** candidate into `chart` first, so the score is computed on
   the rectangle that will actually be used — not on one that the clamp is
   about to move.
2. Score each clamped candidate by `intersect(rect, bbox).area()`, `0.0` when
   disjoint.
3. Pick the minimum. Break ties by the **greater** distance between
   `rect.center()` and `bbox.center()`; break remaining ties by candidate
   index. Deterministic, and at zero overlap the index tie-break reproduces
   today's C1 → C2 → C3 → C4 preference exactly.

The four corner candidates are what make "least overlap" meaningful: an object
in the middle of the chart always has a clear corner unless the inspector is
larger than a chart quadrant, and when it *is* larger, the corner that clips
the object least wins instead of the corner the loop happened to reach first.

**When even the best candidate is bad.** Rather than an overlap ratio
threshold, reuse the trigger the previous spec already chose: if
`chart.width() < INSPECTOR_AUTO_PIN_CHART_WIDTH_PX` (`1180.0`), the inspector
**opens pinned** to the side panel, where it cannot overlap anything at all and
the canvas pays for it honestly. Auto-pin applies only until the user expresses
a preference: once the pin button is toggled in either direction, that choice
holds for the session and the width rule stops firing.

### 4.3 Manual position always wins

- `inspector_moved` is set the first time the user drags the window and is
  **never cleared automatically**. Selection changes, tool changes, new objects
  and closing/reopening the inspector all leave the manual position alone.
- One exception, and it is a repair rather than an override: if the stored
  rectangle no longer fits inside `chart` (the window shrank, the dock opened,
  a sub-pane appeared), clamp it back inside using the same clamp as §4.2 and
  **keep** `inspector_moved` set. The user's intent survives; the off-screen
  panel does not.
- **Reset path:** double-clicking the title bar clears `inspector_moved` and
  re-runs the placement algorithm on the current selection. The title bar's
  hover text states this: `"Drag to move · double-click to reposition
  automatically"`.

### 4.4 Title bar

A 28 px title bar (`INSPECTOR_TITLE_HEIGHT_PX`), leading to trailing:

| Element | Icon | Behaviour |
|---|---|---|
| Grip + title | `DOTS_SIX_VERTICAL` 14 px `TEXT_FAINT`, then `tool.settings_title()` in `TEXT_PRIMARY` | the **whole title bar** is the drag area; the body never is, so a slider drag can never move the window |
| Hide / Show | `EYE` / `EYE_SLASH` | per-object visibility; when hidden, the body's banner keeps the way back |
| Pin | `PUSH_PIN` | active-tinted while pinned; docks to the side panel |
| Close | `X` | deselects, mutates nothing |

All three use `TOOLBAR_ICON` (16 px glyph / 28 px hit) so the bar stays 28 px,
and all three carry tooltips naming the action and its resulting state. This
replaces today's `small_button("Close")` / `("Pin")` / `("Hide")` text row.

The **`DrawingActionBar` stays exactly as it is**: two full-width textual
buttons, "Lock drawing" / "Unlock drawing" and "Delete drawing · Del", always
visible, never behind a scroll, never glyph-only. The rule that destructive
and protective actions are named in words applies to the body, not to standard
window chrome. Likewise keep the locked/hidden explanatory lines, the inline
delete confirmation for locked objects, the tab strip, and the Undo toast —
all of it already matches the previous spec. The explanatory lines move from
default text to the new `TEXT_SUPPORT` `#8692A4` token, which clears 4.5:1 on
`CHROME` where `TEXT_FAINT` does not.

### 4.5 Pointer routing

**The inspector is opaque to the pointer.** No chart interaction reads a press
that lands on it.

Concretely, in `handle_navigation`, the entire drawing-interaction block —
hover cursors, the selection click, and drag *initiation* — is gated on the
pointer not being over a floating surface:

```
let over_chrome = ctx.is_pointer_over_area();   // any Area/Window/Panel above the canvas
```

When `over_chrome` is true, the chart does not hit-test drawings, does not set
a cursor, does not select and does not start a drag. This covers the inspector,
the object manager, the toast, the rail flyouts and every future popup with one
rule instead of a list.

**The current exception is removed.** Deciding the case the brief asks about —
*a click on a drawing that lies under the inspector's edge* — the drawing does
**not** receive it; the inspector does. The exception bought one rare
convenience (dragging geometry hidden beneath the panel) at the cost of a
common bug (every press near a horizontal line firing twice), and §4.2 removes
most of its reason to exist: the inspector no longer sits on the selected
object. The escape hatches are all cheap and discoverable — move the inspector
by its title bar, pin it to the side, close it with `Esc`, or grab the object
anywhere else along its geometry.

**One carve-out, and it is about continuity, not priority:** the gate applies
at *press* time only. A drag that started on the canvas keeps running while the
pointer travels across the inspector, so moving an object under the panel never
breaks mid-gesture. `drawing_drag.is_active()` already distinguishes these two
moments.

The inspector also stops going non-interactive during a drag: today it is
built with `.interactable(!self.drawing_drag.is_active())`, which the previous
spec explicitly forbids ("o inspetor nunca fica cinza/desabilitado durante
drag"). With the press gate above, that workaround is no longer load-bearing
and should go.

### 4.6 Object manager placement

The manager's fixed `(70, 140)` opening position assumed a top-left toolbox.
It now opens `DRAWING_MANAGER_GAP_PX = 12.0` inboard of the rail's inner edge,
aligned with the rail's leading end, clamped into `chart` — so it appears next
to the button that opened it in all four docks.

---

## 5. Acceptance details

Measurable checks an engineer can turn into unit tests. Each is a pure
function of state that egui can drive headlessly, in the style of the existing
`toolrail.rs` and `app.rs` test modules.

**Rail geometry and layout**

1. `TOOLBOX_THICKNESS_PX == 44.0`, and for each of the four docks the
   `CentralPanel` rect is inset by exactly 44 px on that edge and by 0 px on
   the other three (extends the existing
   `every_dock_position_reserves_space_outside_the_central_chart`).
2. `44.0 == 2.0 * TOOLBOX_MARGIN_PX + TOOLRAIL_ICON.hit`.
3. In every dock, the Objects button's rect is the last one along the rail's
   long axis, and its distance to the rail's trailing inner edge equals
   `TOOLBOX_MARGIN_PX` — the trailing cluster is pinned, not packed.
4. In every dock, the grip's rect precedes every button along the long axis.
5. Rendering left, right, top and bottom at the same available extent yields
   the same set of visible buttons — orientation changes positions, never
   inventory.

**Overflow staging**

6. At Full extent (≥ 417 px for the shipped registry) every registry tool has
   a rect except the folded family members, and the fib family occupies
   exactly one.
7. At Compact extent (345–416 px) the armed tool and Pointer and Crosshair and
   Objects all have rects; an unarmed non-family tool has none.
8. At Minimal extent (< 345 px) Pointer, the armed tool, More and Objects have
   rects; Crosshair, hide-all and lock-all have none.
9. Rendering at extent *E* after shrinking from a larger extent produces the
   same stage as rendering at *E* after growing from a smaller one.
10. The More flyout at each stage lists exactly the controls that lost their
    slot — no duplicates, no omissions — comparing against `DRAWING_TOOLS`.

**Docking**

11. `ToolboxDock::nearest` on an 800 × 600 screen returns `Left` for
    `(10, 300)`, `Right` for `(790, 300)`, `Top` for `(400, 10)` and `Bottom`
    for `(400, 590)`.
12. Normalisation holds on a wide screen: on 1920 × 600, `(300, 300)` returns
    `Left`, not `Top` — the raw pixel distances are 300 vs 300, the normalised
    ones 0.156 vs 0.5.
13. Dragging the real grip and releasing at each of the four edge midpoints
    docks there, driven through `egui::Context::run` exactly like today's
    `dragging_the_real_grip_docks_at_each_screen_corner`.
14. `Esc` during a live grip drag leaves `dock` unchanged.
15. Default `ToolRail::new().dock == ToolboxDock::Left`.

**Inspector placement**

16. With a 40 × 40 bbox at the chart's centre and a 1400 × 800 chart, the
    chosen rect does not intersect the bbox.
17. With a bbox spanning 90 % of the chart, the chosen rect's overlap with the
    bbox is less than or equal to that of all seven other clamped candidates.
18. For every candidate the algorithm can return, `chart.contains_rect(rect)`
    holds — the inspector never leaves the chart pane, so it can never cover
    the price axis, the time axis or the live lane.
19. With a bbox hugging the chart's right edge, the result is not C1 — the
    old blind fallback is gone.
20. Two calls with identical inputs return identical positions.

**Inspector behaviour**

21. After `inspector_moved` is set, changing the selection to a different
    object leaves the window position unchanged.
22. Shrinking the chart so the manual position falls outside it moves the
    window back inside and leaves `inspector_moved` set.
23. Double-clicking the title bar clears `inspector_moved`, and the next frame
    places the window at `inspector_target_position`.
24. With `chart.width() < 1180.0` and no user pin preference recorded, the
    inspector opens pinned; after the user toggles the pin, a further
    selection at the same width respects the user's choice.

**Pointer routing**

25. A press inside the inspector's rect that also lies on a selected
    horizontal line's stroke leaves `drawing_drag == DrawingDrag::None` and
    does not change the selection.
26. A press on the chart that starts a drag, followed by pointer motion across
    the inspector's rect, keeps the drag active and keeps moving the object.
27. Hovering the inspector sets no chart cursor icon (no `Move`, no
    `ResizeNwSe`, no `NotAllowed`).
28. The inspector's `Window` is built with `interactable(true)` on every frame,
    including frames where `drawing_drag.is_active()`.

**Tokens**

29. `press_tint(ACCENT) == Color32::from_rgba_unmultiplied(0x8A, 0xB4, 0xF8, 84)`
    and its alpha exceeds `active_tint`'s, so a press on an armed button is
    distinguishable.
30. `icon_paint` precedence: disabled beats pressed beats active beats hover
    beats idle, checked across the full boolean cross-product.
31. `TEXT_SUPPORT == Color32::from_rgb(0x86, 0x92, 0xA4)`, and no rail or
    inspector surface uses `AMBER` — grep-guarded the way the `libm` rule is
    in the indicators crate.
