# GOAL — toolbar rail overflow scrolling

## Mission

When the tool rail does not fit the available extent — which starts happening
after a handful of favorites are starred — it must **scroll with arrow
affordances** instead of collapsing the whole tool run, and the trader must be
able to get back to the previous layout.

## Root cause

`crates/app/src/toolrail.rs`:

- `full_length()` (:269) adds one slot per favorite.
- `stage_for()` (:321) drops from `RailStage::Full` straight to
  `RailStage::Compact` the moment `available < full_length(..)`.
- `Compact` (:715) replaces the entire tool run with "armed tool + More".

So starring a few tools silently swallows the rail. It is a step degradation
with no visible way back, not a state bug.

## Design decisions (confirmed with the trader)

- **Scroll to a floor, then Compact.** While the band can still show
  ~4 tools, it scrolls with arrows. Below that floor the rail falls back to
  today's `Compact` stage — a two-icon scrollable rail is worse than a menu.
- **Favorites are anchored, outside the scroll.** They sit right under
  Pointer/Crosshair and never scroll: the trader starred exactly what they
  want permanently at hand. Only the full tool run scrolls. Favorites that
  would eat the band's floor spill into the top of the scrolling band (still
  star-badged) rather than becoming unreachable.
- `docs/drawing-toolbar-ux.md:300` ("The rail never wraps and never scrolls")
  is now false and must be updated in this same change.

## Acceptance criteria

### Mission-specific

1. With favorites overflowing the extent, **no tool is unreachable**: the
   arrows scroll the band to the last item.
2. Arrows appear **only when content is off-view in that direction**; they
   disappear/disable at the end of travel. No permanent chrome.
3. Anchor clusters never scroll: grip + Pointer/Crosshair at the leading end,
   and the trailing cluster (magnet, repeat, hide-all, lock-all, Objects) at
   the far end. Only the middle band (tools + favorites) scrolls.
4. The mouse wheel over the rail scrolls the same band.
5. Removing favorites restores the previous layout with no residue (offset
   back to zero, arrows gone).
6. Holds for all three docks: vertical (Left) uses up/down, horizontal
   (Top/Bottom) uses left/right.
7. Deterministic unit tests for the overflow/scroll math — offset clamp and
   per-arrow visibility — not just render tests.

### Standard gates

8. Four checks green on top of updated `main`.
9. **Performance impact declared**: `draw_contents` is a **per-frame** path.
   `APP_HEALTH_SUMMARY` fps/frame_avg against a `main` control run, numbers in
   the PR body.
10. `ui-harness`: an env hook reaching the rail in an overflowed state, added
    in this same change.
11. `visual-qa` all surfaces PASS (or defects explicitly accepted);
    `trader-ux-review` with no unresolved Blocker.
12. `arch-review` run, every Blocker/Should-fix resolved or deferred in the PR
    body.
13. PR opened with green CI. Merging is not part of the mission.

## Ground

- Branch: `fix/toolbar-overflow-scroll`
- Worktree: `../quantick-worktrees/fix-toolbar-overflow-scroll`
