# Mission

Redraw the two Fibonacci tool icons to the trader's sketch: each names itself
with one letter (`R` retracement, `P` projection) and hangs its levels where
they say what the tool does — between the anchors for the retracement, beyond
them for the projection — so the two are told apart at rail size without
hovering for a tooltip.

Source of the design: a sketch the trader drew and handed over in session —
a leg with a marked anchor at each end, the level lines in the quadrant the
leg leaves empty, and a large letter in the corner opposite them.

## Acceptance criteria

Mission-specific:

1. Both Fib icons follow the sketch: marked anchors on the leg, level lines
   placed where they distinguish the two tools, one large letter each.
2. The letters differ (`R` / `P`) and are the same size in both icons, so
   they read as a choice between two rows rather than two unrelated marks.
3. The letter's corner is provably empty: a test holds every stroke and every
   anchor of both icons clear of the region the letter is painted in.
4. The icon stays registry data — a tool declares its letter, the chrome
   paints what it is handed and never learns which tool it is drawing.
5. Both icons are legible at both sizes they are painted: the 18 px rail
   button and the 14 px flyout row.

Injected by the kind of mission (code, user-visible, per-frame path):

6. English throughout every artifact.
7. Four checks green on the branch rebased on latest `main`
   (`cargo fmt --all -- --check`, `clippy -D warnings`, `build`, `test`).
8. Performance impact declared: the rail's icons are a **per-frame** path.
   Evidence that frame cost is flat against a `main` control run
   (`APP_HEALTH_SUMMARY` fps / frame_avg), numbers in the PR body.
9. `ui-harness`: every surface the change touches is reachable by env hook
   from a fresh launch — `QUANTICK_TOOL_FAVORITES` (rail buttons) and
   `QUANTICK_TOOLBOX_FLYOUT=fib` (flyout rows). No new surface, so no new
   hook; if that turns out false, the hook lands in this change.
10. `visual-qa` pass over both surfaces, or defects explicitly accepted.
11. `trader-ux-review` with no unresolved Blocker.
12. `arch-review` run over `git diff main...HEAD`, every Blocker and
    Should-fix resolved or deferred in the PR body.
13. PR opened. Merging is not part of this mission.
