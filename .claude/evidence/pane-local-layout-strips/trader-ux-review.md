# Trader UX review: pane-local layout strips

Verdict: **PASS**. No Blocker or Should-fix remains.

## Identify and switch a pane's layout

- Rafa sees the target and its active layout in the same footer without first
  moving focus. One click still switches; keyboard cycling remains available,
  and dense-tape measurements show no frame regression.
- Marina can compare two charts with distinct layouts without a shared strip
  changing meaning when focus moves. The shared catalogue and saved per-pane
  assignments remain unchanged across persistence.
- Duda sees the same complete layout list beneath each chart it controls. The
  physical attachment removes the previous hidden prerequisite of noticing
  which pane had focus.

Evidence: `wide-distinct-layouts.png`, `narrow-max-layouts.png`,
`pane_local_strip_actions_target_the_footer_that_raised_them`, and
`per_pane_layouts_are_recorded_and_restored`.

## Create, rename, and delete

- Creating from `+` targets the pane whose footer contains it.
- Rename is edited only in the originating footer, then updates the shared
  catalogue everywhere.
- Delete remains shared and destructive, so it keeps the existing explicit
  confirmation and consequences instead of becoming a one-click action.

Evidence: `rename-origin-pane.png`, `delete-confirmation.png`, and
`pane_local_strip_actions_target_the_footer_that_raised_them`.

## Findings

No Blocker, Should-fix, or deferred finding. During QA, the twelve-layout
narrow state initially opened one pane with its active tab off-screen. The
implementation now reveals an active tab only when that pane's assignment
changes, preserving free horizontal scrolling afterwards; the final evidence
is `narrow-max-layouts.png`.

- Rafa can trade through it: yes; no added critical gesture, focus theft, or
  measurable frame cost.
- Marina can keep her workspace: yes; shared layouts, pane assignments,
  drawings, persistence, menu, keyboard, and control-plane semantics remain.
- Duda can figure it out alone: yes; each selector is attached to its target,
  and destructive deletion still explains itself before acting.
