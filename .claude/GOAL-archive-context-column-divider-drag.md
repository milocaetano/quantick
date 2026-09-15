# Fix the context-column divider drag behavior

Fix the context-column divider so dragging opens, resizes, and magnetically closes the column while clicks do nothing and collapsed scene bounds stay truthful. This restores a reliable pointer gesture for the context column without losing the existing keyboard path or exposing hidden canvases to control clients.

**Tier:** small — the change is confined to an existing app interaction and its scene projection, and is expected to remain within the small-tier changed-line ceiling. Small skips interrogation and delivery review.

## Request ledger

- **R1** — Pressing and dragging the collapsed rail right opens the context column and makes it follow the pointer from the rail to release. Source: “Opening is press-and-drag” and “the column follows the pointer”.
- **R2** — A slow multi-frame right drag on the open divider widens the context column from the width currently drawn and does not re-collapse it because of a stale fraction.
- **R3** — A plain click on either the collapsed rail or open divider never changes the collapsed state. Source: “NEVER close by a click” and “a plain click on the rail must not toggle it”.
- **R4** — Dragging the open divider left past the collapse threshold automatically snaps the context column closed. Source: “Closing is a magnet”.
- **R5** — Reopening an openable context column whose persisted split fraction is below the display floor restores at least the 240 px floor.
- **R6** — While the context column is collapsed, `scene.controls` and `workspace.summary` agree and hidden context canvases do not report overlapping live-pane bounds.
- **R7** — Ctrl+0 remains the keyboard route for reopening the context column.
- **R8** — Regression tests for the requested gestures, click behavior, floor restoration, and scene agreement are written before production implementation.

## Assumptions

- **S1** — The drag accumulator belongs to the tab because the divider interaction ID and split fraction are already tab-local. This is safe because it preserves existing ownership and is reversible without contract changes.
- **S2** — The collapsed rail drag starts from the rail’s drawn edge and uses pointer displacement across frames; it does not treat press-only release as an open request. This directly follows the supplied interaction rules.
- **S3** — The scene projection will use the repository’s existing visibility/disabled vocabulary where available and otherwise omit hidden context canvases. This is safe because the request explicitly permits reporting the rail or marking canvases collapsed, and code inspection will select the established representation.

## Acceptance criteria

- [x] **A1** — A slow multi-frame drag right from the collapsed rail opens the context column and its resulting width follows the pointer through release.
      *Evidence:* named egui regression test and visual interaction evidence in the PR.
      → PR Validation and UI review report. *(R1)*
- [x] **A2** — A slow multi-frame drag right on an open divider widens from the drawn width without collapsing when the stored fraction began below the floor.
      *Evidence:* named egui regression test exercising multiple frames.
      → PR Validation. *(R2)*
- [x] **A3** — Clicks on the rail and divider leave the collapsed state unchanged.
      *Evidence:* named egui regression tests for both hit targets.
      → PR Validation. *(R3)*
- [x] **A4** — Dragging the divider left past `COLLAPSE_AT_PX` snaps the context column closed without requiring release or a click.
      *Evidence:* named egui regression test and visual interaction evidence.
      → PR Validation and UI review report. *(R4)*
- [x] **A5** — Ctrl+0 reopens a context column with a stale sub-floor fraction at no less than `MIN_PANE_WIDTH_PX`.
      *Evidence:* named keyboard/layout regression test.
      → PR Validation. *(R5, R7)*
- [x] **A6** — A collapsed workspace reports `context_collapsed = true` and no visible `pane.1` or `pane.2` canvas with bounds overlapping `pane.0`.
      *Evidence:* named control-plane scene/workspace agreement test.
      → PR Validation. *(R6)*
- [x] **A7** — The requested regression tests exist in an earlier commit than the production fix.
      *Evidence:* branch commit history and the initial tests-only commit.
      → PR Commits and Validation. *(R8)*
- [x] **G1** — Every authored artifact is English under `CLAUDE.md`.
      *Evidence:* language guard and architecture-review dimension 8.
      → CI and architecture review report.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` pass after rebasing on latest `main`.
      *Evidence:* local command exit codes and exact-head CI checks.
      → PR Validation and CI.
- [x] **G3** — Performance impact is declared by touched path rate, and the per-frame canvas path is flat or better against a dense-tape `main` control.
      *Evidence:* alternating runs of the seeded 8,000-trade control benchmark: median average frame time 1.108804 ms on `dc08545c` and 1.109274 ms on the candidate (+0.04%); median p99 +0.37%. Scene projection is request-time and tests are test-only.
      → PR Performance.
- [ ] **G4** — Architecture review passes with every Blocker and Should-fix resolved or explicitly deferred in the PR body.
      *Evidence:* durable current-review report and `arch-review-ok` projection.
      → PR architecture review report.
- [x] **G5** — Every changed UI state is reachable through the UI harness.
      *Evidence:* environment hook coverage and UI-harness capture.
      → PR UI Validation.
- [x] **G6** — Visual QA passes for collapsed, dragged-open, widened, and magnetically closed states, or records an explicitly accepted defect.
      *Evidence:* screenshot report with state-by-state verdicts.
      → PR visual QA report.
- [x] **G7** — Trader UX review has no unresolved Blocker.
      *Evidence:* durable trader-UX review verdict.
      → PR trader UX report.
- [x] **G8** — The interaction remains operable without a mouse through Ctrl+0 and readable through the workspace/scene control surface.
      *Evidence:* keyboard and control-plane tests plus architecture review’s second-operator dimension.
      → PR Validation and architecture review report.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Non-applicable gates

- **New extension:** not applicable because this changes an existing context-column interaction and existing scene controls; it adds no capability, panel, layer, crate, or registration point.
- **Engine/determinism:** not applicable because no engine or bar-building code changes.
- **Docs/skills only:** not applicable because runtime Rust and tests change.

## Closing steps

- [ ] **C1** — Open a draft PR carrying the concise mission summary and current evidence.
      *Evidence:* PR URL and mission-summary block.
      → GitHub PR.
- [ ] **C2** — The final mission verifier passes at exact-head green CI, after the PR is ready if required.
      *Evidence:* `mission_ship_gate.sh mission <pr>` literal PASS reconciliation.
      → GitHub PR.

## Verbatim request

> User request: “small Fix the context-column divider and make it open by drag, close by magnet. Repro on main dc08545c: a tab on layout time_time_and_flow persisted with split_fraction 0.072 (~106 px on a 1470 px canvas, under COLLAPSE_AT_PX = 120). Ctrl+0 reopens the column drawn at the 240 px floor, but draw_canvas_divider (crates/app/src/tab/canvas.rs:651) computes wanted_px from the stale split_fraction plus one frame's drag_delta, so any slow drag right lands under 120 and re-collapses — the column "snaps back" and cannot be widened. Trader's interaction rules, verbatim intent: (1) NEVER close by a click — no click anywhere on the divider or rail collapses the column. (2) Closing is a magnet: dragging the divider left past the threshold snaps it shut on its own. (3) Opening is press-and-drag: press on the collapsed rail and drag right, the column follows the pointer from the rail to wherever it is released; this is the primary way to open, preferred over a plain click (a plain click on the rail must not toggle it). Ctrl+0 stays as the keyboard path (operability rule). Fix: base every drag on the drawn width (never below MIN_PANE_WIDTH_PX while open) and accumulate the gesture across frames, write the resulting width back to split_fraction, give the collapsed rail Sense::drag with a hit area that reaches COLLAPSED_HIT_PX, and restore at least the floor on reopen. Same change: scene.controls must not report pane.1/pane.2 canvases with 142 pt bounds while context_collapsed is true (they overlap pane.0 at x=72); report the rail or mark them collapsed. Tests first: slow drag right from collapsed opens and follows the pointer; slow drag on the open divider widens it without re-collapsing; a click on rail or divider never changes collapsed state; drag left past the threshold snaps closed; reopen from split_fraction below the floor lands at the floor; scene/workspace agreement while collapsed.”
