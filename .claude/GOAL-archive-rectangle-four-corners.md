# Resize rectangles from all four corners

Allow chart rectangles to be resized inward or outward from all four corners so traders can adjust either pair of edges directly.

**Tier:** small — this is a bounded correction inside one existing drawing tool, with no interrogation and no delivery review while the diff remains within the small-tier line limit.

## Request ledger

- **R1** — A rectangle can be resized from every corner. Source: “redimecionar o retangulo em todos os cantos”.
- **R2** — Every corner supports both increasing and decreasing the rectangle size. Source: “aumentar e diminuir nos 4 cantos”.

## Decisions

None. The small tier skips interrogation, and the code identifies the existing chart rectangle drawing unambiguously.

## Assumptions

- **S1** — “Rectangle” means the existing chart drawing tool registered as `rectangle`. This is safe because it is the only user-created rectangle whose current two diagonal anchor handles exactly match the reported behavior.
- **S2** — The request changes selection handles and resize behavior only; creation, whole-body translation, `extend right`, and strategy semantics remain unchanged. This is safe because those behaviors are independent of which corner handles the selected drawing exposes.
- **S3** — Crossing a dragged corner past the opposite corner remains allowed and simply swaps the screen-side ordering. This is safe because the current rectangle is normalized with `Rect::from_two_pos`, and preserving that behavior avoids an invented minimum-size policy.

## Acceptance criteria

- [ ] **A1** — A selected rectangle exposes exactly four handles, one at each visible corner.
      *Evidence:* `a_rectangle_exposes_all_four_corners_as_handles` passed locally. The live screenshot was inconclusive because the all-drawings demo overlapped and partly clipped the selected rectangle, so the visual half remains open.
      → `crates/app/src/drawings/rectangle.rs` and the PR evidence section. *(R1)*
- [x] **A2** — Dragging any one of the four handles moves only that corner's horizontal and vertical edges while preserving the opposite corner, including after the dragged corner crosses either axis.
      *Evidence:* `every_corner_expands_and_contracts_around_its_opposite_corner` covers every handle index, and `a_corner_may_cross_its_opposite_without_losing_resize_control` covers crossing; both passed locally on 2026-09-14.
      → `crates/app/src/drawings/rectangle.rs` and the PR evidence section. *(R1, R2)*
- [x] **A3** — Interaction coverage demonstrates that all four corners can both expand and contract the rectangle.
      *Evidence:* all three focused rectangle tests, the drawing-tool handle contract, and the existing non-modal rectangle-resize integration test passed locally on 2026-09-14. Visual QA reached the live surface but is recorded separately as blocked by the dense demo framing.
      → PR evidence and review reports. *(R2)*
- [ ] **G1** — Every authored artifact is in English.
      *Evidence:* `cargo test -p quantick-guards`, language guard, and arch-review dimension 8.
      → PR evidence and arch-review report.
- [ ] **G2** — The final rebased head passes `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace`.
      *Evidence:* format, clippy, and build passed locally. Workspace tests reached 2,134 passes with two unrelated order-flow projection failures; both failures reproduce from clean `origin/main` at `d3d4b23d`. Exact-head CI remains the required final authority.
      → PR evidence section and CI check run.
- [x] **G3** — Performance impact is declared and remains flat in practical terms: `rectangle.rs` is per-frame only for a selected drawing, and its fixed handle list grows from two to four points without trade/depth work or an unbounded scan.
      *Evidence:* the inspected diff adds four fixed coordinate constructions only while a rectangle is selected; the live harness reported 59–60 fps, about 16.67 ms average frames, and zero worker backlog under the live BTC tape.
      → PR evidence section.
- [ ] **G4** — Arch-review reports PASS with every Blocker and Should-fix resolved or explicitly deferred in the PR body.
      *Evidence:* durable current-head arch-review report and `arch-review-ok` projection.
      → PR review report and body.
- [ ] **G5** — The changed rectangle surface is reachable through the UI harness and visual QA passes all changed states or records an explicitly accepted defect.
      *Evidence:* `QUANTICK_DRAWINGS_DEMO=1` with `QUANTICK_DRAWINGS_DEMO_SELECT=rectangle` reached the live surface on a scratch `tick:1` configuration; `APP_HEALTH_SUMMARY` reported 22 drawings and 59–60 fps. Captures under `C:\src\quantick-qa-rectangle` are BLOCKED evidence because the dense demo overlapped and partly clipped the selected rectangle; no product defect was observed or accepted.
      → PR evidence section and visual QA artifact.
- [x] **G6** — Trader UX review has no unresolved Blocker.
      *Evidence:* the 2026-09-14 persona walk found no Blocker or Should-fix: the gesture count stays at one, no focus or layout changes occur, the opposite corner remains fixed, and persistence continues to use the same two anchors. The generic diagonal resize cursor on the newly exposed off-diagonal corners is recorded as Consider-level polish for the PR.
      → PR evidence section or linked review report.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Not applicable

- Hot-path measurement is not applicable: the change does not run per trade or per depth update, and the per-frame work is a fixed two-point increase only while a rectangle is selected.
- `new-extension` is not applicable: this corrects an existing registered drawing tool and adds no capability, panel, layer, crate, or registration point.
- Second-operator capability work is not applicable: rectangle creation and editing are already represented by the existing drawing control surface; this change adds no new action class.
- Engine/determinism gates are not applicable: only app-owned drawing interaction and rendering geometry are touched.
- MT5 and dependency gates are not applicable: no Python, bridge, Cargo manifest, or lockfile change is planned.

## Closing steps

- [ ] **C1** — A pull request is open for `fix/rectangle-four-corners`, with current-head evidence in its body. Source: mission workflow step 8.
- [ ] **C2** — `mission_ship_gate.sh mission <pr>` reports PASS on the exact final head and publishes the required reconciliation. Source: mission workflow step 8.

## Verbatim request

> User: “$mission small permitir redimecionar o retangulo em todos os cantos. Atualmente somente no cnaod superior esquerdo e no canto inferior direito. Quero poder aumentar e diminuir nos 4 cantos.”
