---
name: visual-qa
description: Autonomous visual QA of the quantick desktop app — drive every affected surface via ui-harness hooks, capture screenshots across a state matrix, read the images against an objective defect checklist, and report PASS/FAIL with evidence. Use when a change touches UI, when the user asks "how does it look", or as a goal acceptance criterion for any visual work.
---

# Visual QA — the agent looks so the human doesn't have to

Purpose: catch visual defects (clipped buttons, overlapping popups,
unreadable text, lying states) without the user ever opening the app.
Everything here runs on the `ui-harness` skill — load it first; launch,
capture and capture-trust rules live there and are not repeated here.

## 1. Scope the pass

From the diff, list every surface the change can affect — not just the one
it targets. A dock tab edit can move a neighbouring splitter; a new popup
can cover the tape. When in doubt a surface is in scope.

## 2. State matrix

Capture each in-scope surface in the states that actually break layouts,
not just the happy path:

| State | Why it breaks things |
| --- | --- |
| Default open (BTC dense tape preset) | the baseline the user sees first |
| Feature active via its hook | the change itself |
| Popup / menu open **over live data** | occlusion of price, tape, or the working bar |
| Empty data (no session, no fills, no depth) | placeholder honesty, collapsed layouts |
| Dense data (fast tape replay, deep book) | overflow, truncation, overlap |
| Narrow window (~1000 px) and the user's normal size | clipping, wrapped labels, lost buttons |
| Disabled state (capability absent, e.g. replay = no depth) | is the *why* visible, or just a dead control? |

Prefer replay (`QUANTICK_REPLAY_*`, WINJ26 sessions) for anything that must
be reproducible: deterministic tape → the same screen twice, so a defect
found once can be re-captured after the fix. Presets come from
`config/bubbles.toml` — never bare defaults.

## 3. Read the captures — defect checklist

Look at each image and answer explicitly. "It renders" is not a verdict.

- **Integrity**: any control clipped, overlapping, or pushed off-window?
  Splitters and neighbouring panes intact?
- **Readability**: text ≥ the size the app itself calls readable
  (`readable_min_radius` is the reference for marks); contrast holds on the
  dark canvas; numbers not truncated (`1234…` on a price is a fail).
- **Occlusion**: does any popup, tooltip or inspector cover the live price,
  the tape, or the forming bar? (The repo precedent: the drawing inspector
  is opaque to the pointer and placed to not cover the action.)
- **State honesty**: disabled controls explain themselves; inferred or
  incomplete data is visibly labelled (data-honesty rule); an empty panel
  says why it is empty.
- **Motion sanity**: for live surfaces, two captures ~1.2 s apart in the
  same PowerShell call — did the flow advance without layout jumps? The
  live region must keep moving (never frozen — see the rejected
  live-tail-compact precedent).
- **Consistency**: same chip/button language as the P0 surfaces — one
  visual system, no new one-off widget style.
- **Performance is a visual property**: every capture session already logs
  `APP_HEALTH_SUMMARY` — read it, don't just screenshot past it. Under the
  dense-data state: fps ≥ ~59 and no `APP_SLOW_FRAMES` bursts attributable
  to the change. fps ~50s with the feature on and ~59 on a `main` control
  run (same hooks, same tape) is a **FAIL** of this pass, not an
  environment note — the checklist for occluded-window false alarms is in
  `ui-harness`; rule those out first, then blame the change.

Verify by pixel where the eye is unreliable (counting marks, dash
signatures, colour checks) — the technique is in `ui-harness`.

## 4. Report

One verdict per surface × state, most severe first:

- **FAIL** — defect, with the screenshot path, what is wrong in one
  sentence, and the crop/coordinates that show it.
- **PASS** — with the screenshot path that proves it. A PASS without
  evidence is an unproven claim, treat it as not run.
- **BLOCKED** — could not observe (desktop idle, no live feed); say what is
  missing and what was validated by other means (headless frame, pixel
  test). Never report BLOCKED as PASS.

Fix FAILs, then re-run only the failed cells of the matrix and attach the
before/after pair. The pass is done when every cell is PASS or has an
explicitly accepted defect noted for the PR body.
