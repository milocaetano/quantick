# Resize stacked chart panes vertically

Allow traders to resize stacked chart panes vertically in either direction when more than two panes are open. This removes the fixed equal-height constraint from the `2 Timeframes + Flow` layout so each context chart can be given the vertical room the trader needs.

**Tier:** small — the behavior is narrow, local to the existing app canvas, and is expected to remain within the small-tier changed-line ceiling. Small missions skip interrogation and delivery review.

## Request ledger

- **R1** — In a layout with more than two chart panes, let the trader resize the stacked panes vertically.
- **R2** — Accept divider movement in both vertical directions: upward and downward.

## Assumptions

- **S1** — “windows” means the context chart panes in the existing `2 Timeframes + Flow` canvas layout. This is safe because that is the repository surface with more than two chart panes whose vertical bands are currently fixed; indicator panes already have vertical resize handles.
- **S2** — A divider changes the heights of its two adjacent context panes while leaving the rest of the canvas allocation unchanged. This is the conventional local splitter rule and is reversible by dragging in the opposite direction.
- **S3** — The vertical sizing is session state, matching the current `CanvasLayout` comment that per-tab chrome persistence remains an open question. Persistence was not requested and adding a storage contract would expand this small mission.

## Acceptance criteria

- [x] **A1** — In `2 Timeframes + Flow`, every seam between stacked context charts exposes a horizontal resize handle whose drag changes the heights of its two adjacent panes.
      *Evidence:* focused app regression tests for a three-pane canvas and visual evidence from the UI harness.
      → `crates/app/src/app/tests/panes_layout_tests.rs` and the PR body. *(R1)*
- [x] **A2** — Dragging a stacked-pane divider upward and downward produces opposite, observable height changes without moving the flow/context column boundary.
      *Evidence:* regression tests exercise both drag directions and assert pane geometry.
      → `crates/app/src/app/tests/panes_layout_tests.rs`. *(R2)*
- [x] **A3** — Divider drags preserve the readable pane-height floor and affect only the adjacent pair.
      *Evidence:* focused regression tests assert the floor and unchanged non-adjacent geometry.
      → `crates/app/src/app/tests/panes_layout_tests.rs`. *(R1, R2)*
- [ ] **G1** — Every authored artifact is English and the language guard passes.
      *Evidence:* `quantick-guards` results and architecture-review dimension 8.
      → the PR body and durable architecture-review report.
- [ ] **G2** — The final rebased head passes `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace`.
      *Evidence:* successful local command output and exact-head CI.
      → the PR body and CI checks.
- [x] **G3** — Performance impact is declared for every touched path: canvas layout and divider interaction run per frame; tests and mission records are rare/development-only. The per-frame work remains bounded by `MAX_CONTEXT_PANES`, allocation-free through existing `SmallVec` storage, and is supported by an `APP_HEALTH_SUMMARY` comparison if implementation changes add measurable work beyond the existing bounded divider pass.
      *Evidence:* inspected diff, guard metrics, and any required main-versus-branch health measurement.
      → the PR body.
- [ ] **G4** — Architecture review runs at the small-tier bug-pass level and resolves or explicitly defers every Blocker and Should-fix finding.
      *Evidence:* current review marker and durable PASS report.
      → the PR architecture-review report and body.
- [x] **G5** — The changed user-visible surface is reachable through the UI harness and visual QA reports PASS for the three-pane layout in both divider directions.
      *Evidence:* harness hook/test, screenshots, and visual-QA verdict.
      → the PR body and durable visual-QA evidence.
- [x] **G6** — Trader UX review reports no unresolved Blocker for discovering and operating the vertical resize handles.
      *Evidence:* current trader-UX review verdict.
      → the PR body and durable trader-UX report.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Non-applicable gates

- Engine and determinism gates do not apply: this changes app-only canvas geometry and input handling.
- New-extension gates do not apply: this repairs the existing multi-pane layout and resize interaction without adding a pane kind, feed, crate, or registration point.
- Trading safety and order-action gates do not apply: the change cannot place, modify, or cancel an order.
- Hot-path measurement is conditional under G3: the surface runs per frame, but the intended implementation reuses the already bounded layout pass and does not touch per-trade or per-depth processing.

## Closing steps

## Validation evidence

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, and
  `cargo build --workspace` passed.
- `cargo test -p quantick-app three_chart_canvas_resizes -- --nocapture`
  passed after implementation and again after the bug-pass repair. The test
  also proves that an undersized canvas refuses an extreme drag instead of
  erasing either chart.
- `cargo test --workspace -- --skip bubbles_project_while_book_capture_stays_off
  --skip the_live_strip_alone_keeps_the_aggression_pipeline_running` passed,
  including 2,066 app tests; the two excluded order-flow tests fail unchanged
  on a clean detached `origin/main` worktree and the base commit's GitHub CI is
  green.
- `cargo test -p quantick-guards` and
  `cargo run -p quantick-guards -- --report` passed.
- Candidate and `origin/main` both held 59–60 fps and approximately 16.67 ms
  frame averages in the three-pane layout. Normal and narrow screenshots pass
  visual QA; the trader UX report has no Blocker.

- [ ] **C1** — Open a PR from `fix/resize-stacked-panes` to `main` with current-head evidence and report URLs.
- [ ] **C2** — Run `mission_ship_gate.sh mission <pr>` successfully at the exact final head and publish its literal reconciliation.

## Verbatim request

> User: “$mission small permitir redimencionamento pra cima e pra baixo, das janelas quando tem mais de 2 janelas abertas. Eu quero poder redimencionar pra cima e pra baixo a janela”
