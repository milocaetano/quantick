# Show drawings on all charts quick toggle

Add a compact “show drawings on all charts” icon beside the existing “Hide drawing” control so drawings can be made visible across every chart with one click.

This removes a frequently repeated trip through the drawing inspector while keeping the selected drawing's scope explicit and reversible.

**Tier:** small — the change is a focused shortcut to an existing drawing property, with no interrogation and no delivery-review while the diff stays within the small-tier line ceiling.

## Request ledger

- **R1** — Add a small icon beside the existing Hide drawing button for the selected drawing.
- **R2** — One click must switch the selected drawing to “Show on all charts” without opening another popup.
- **R3** — Use UX design guidance to choose a clear, consistent icon for this frequent action.
- **R4** — The shortcut must make drawings visible in every compatible chart/window rather than only the source chart.

## Assumptions

- **S1** — “All charts/windows” means the existing `DrawingScope::AllCharts` behavior, which projects a drawing across compatible panes for the same market. This is safe because the phrase names an existing product option and the implementation can be identified directly in under a minute.
- **S2** — The quick control is a toggle: active means all charts, inactive means this chart. This is safe because it mirrors the existing reversible checkbox and the neighboring Hide drawing toggle.
- **S3** — `GRID_FOUR` from the app's existing Phosphor regular icon family represents multiple charts; active styling and a state-specific tooltip carry the exact meaning. The requested icon choice is reversible in one edit, and the UX skill's icon search returned no verified database match after its required retry.
- **S4** — This is another trigger for an existing drawing-scope action, not a new control-plane capability. The existing structured drawing scope remains the readback, and the existing drawing demo hook reaches the changed context-bar surface. This is safe because behavior and surface already exist; the mission only shortens access to them.

## Acceptance criteria

- [x] **A1** — The selected drawing's context bar shows one compact Phosphor icon immediately beside Hide drawing.
      *Evidence:* focused context-bar unit test and visual capture.
      → `crates/app/src/drawings/context_bar.rs` and the PR body. *(R1, R3)*
- [x] **A2** — Clicking the icon changes a shareable drawing from this chart to all charts in one action, without opening the inspector.
      *Evidence:* focused drawing-chrome integration test.
      → `crates/app/src/app/tests/drawings_tests.rs` and the PR body. *(R2)*
- [x] **A3** — Clicking the active icon returns the drawing to this chart, and the icon's active state and tooltip truthfully describe both states.
      *Evidence:* focused context-bar and drawing-chrome tests plus visual capture.
      → `crates/app/src/drawings/context_bar.rs`, `crates/app/src/app/tests/drawings_tests.rs`, and the PR body. *(R2, R3)*
- [x] **A4** — A drawing switched to all charts through the shortcut uses the existing cross-chart projection and appears on every compatible chart for that market.
      *Evidence:* existing cross-chart projection regression plus a focused shortcut-to-scope test.
      → `crates/app/src/app/tests/drawings_tests.rs` and the PR body. *(R4)*
- [x] **G1** — Every authored artifact is in English under `CLAUDE.md`.
      *Evidence:* language guard and manual diff review.
      → PR body and arch-review report.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` pass after synchronization with the latest `main`.
      *Evidence:* command exit codes and exact verified head.
      → PR body.
- [x] **G3** — Performance impact is declared and verified as rare: scope mutation occurs only on a click, while the added context-bar state read is constant work on frames where a drawing is selected.
      *Evidence:* code inspection and arch-review performance verdict.
      → PR body and arch-review report.
- [x] **G4** — The changed context-bar state is reachable through the existing drawing demo hook and passes visual QA with no accepted defects.
      *Evidence:* hook registry lookup, screenshot paths, and visual-qa verdict.
      → PR body.
- [x] **G5** — Trader UX review has no unresolved Blocker; the icon is consistent, adjacent to Hide drawing, stateful, and explained by a tooltip.
      *Evidence:* trader-ux-review verdict.
      → PR body.
- [ ] **G6** — Architecture review runs at the small-tier scope with its low-effort bug pass, and every Blocker or Should-fix is resolved or explicitly deferred in the PR body.
      *Evidence:* durable arch-review report and current projection.
      → PR body and arch-review report.
- [x] **G7** — The existing action remains operable without relying solely on the new mouse shortcut: its scope is readable as structured drawing data, the surface is hook-reachable, and the new trigger reuses the drawing-chrome response path.
      *Evidence:* focused tests, hook evidence, and arch-review second-operator verdict.
      → PR body and arch-review report.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Non-applicable gates

- Hot-path benchmark — not applicable: the mutation is click-only and the context-bar read is constant work only while a drawing is selected.
- New-extension package gate — not applicable: the change adds a shortcut to the existing drawing-scope property and does not add a feed, bar type, indicator, layer, panel, crate, or new capability.
- Engine determinism fixture — not applicable: no engine or aggregation behavior is touched.
- Delivery review — not applicable while this remains within the bounded small-tier diff ceiling.

## Closing steps

- [ ] **C1** — Open a non-draft PR whose branch, head, and base match the mission, with current evidence in its body.
- [ ] **C2** — Run `mission_ship_gate.sh mission <pr>` successfully at the exact green CI head.

## Verbatim request

> $mission small colcoar opção sho on all charts como um icone pequeno ao lado do botao hide drwaing. Assim com um click eu consigo exibir desenhos em todos a janeplas ao inves de ter que abrir outra popup para fazer isso. Como eu uso bastante isso, um incone ali fica mais pratico. Pode usar Design de UX para deifnir melhor icone para fazer esssa opção
