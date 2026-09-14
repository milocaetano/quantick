# Pane-local layout strips

Give every visible Quantick chart pane its own layout-tab strip in its footer instead of showing one focus-dependent shared strip for the whole canvas.

This keeps the selector visually attached to the chart it controls, so changing focus no longer makes one shared control appear to change meaning.

**Tier:** high — this is a user-visible, per-frame workspace interaction whose pane addressing, narrow-layout behavior, persistence, operability, and performance require full UI and delivery review.

## Request ledger

- **R1** — Render the shared layout-tab catalog in the footer of every visible chart pane and remove the single canvas-wide strip. Source: “exibir no rodape de cada janela” and “replicar visual para cada janela”.
- **R2** — Each pane-local strip shows that pane's active layout independently; changing focus alone changes no strip's meaning or active marker. Source: “ao inves de dividirem o mesma visualizçaão que muda ao clicar na janela”.
- **R3** — Change presentation and direct mouse addressing only; preserve the shared catalog, saved layout contents, per-pane assignments, persistence, keyboard and menu behavior, and the control-plane contract. Source: “isso apenas exibir”.
- **R4** — Keep each selector visually inside and attached to the pane it controls, eliminating the focus-dependent ambiguity. Source: “Isso fica confuso” and “Seria melhor ter para cada janela a lista de abas”.

The source-first completeness pass reviewed this map after the clarification. It found no remaining user ambiguity, promoted the display-only constraint into R3, and refined the action and narrow-width criteria below without adding product scope.

## Decisions

- **D1** — The trader explicitly chose one shared layout catalog rendered in each pane footer, not an isolated catalog per pane.

## Assumptions

- **S1** — “Window” means each visible chart pane (`ChartPane`) rather than a separate native OS window. This is safe because the reported behavior is the existing focused-pane strip, and the clarification asks to replicate that visual across the charts already sharing the canvas.
- **S2** — The transient rename editor and delete confirmation originate from the pane-local strip that invoked them, while the resulting shared rename or delete is reflected by every strip. This is safe because it preserves the current shared mutation model and can be adjusted locally without changing saved data.

## Performance classification

- The strip composition and pane geometry paths are **per-frame**.
- Switch, create, rename, and delete actions are **rare interaction** paths.
- No **per-trade** or **per-depth** path is intentionally changed.
- Because a per-frame path is touched, dense-tape health must be measured against a same-environment `main` control before delivery.

## Acceptance criteria

- [x] **A1** — Every visible chart pane renders its own layout-tab strip inside its footer, and the canvas-wide shared strip no longer renders.
      *Evidence:* focused unit/integration geometry tests plus default and split-pane screenshots.
      → `crates/app/src/app/tests/panes_layout_tests.rs` and the PR body's **Visual evidence** section. *(R1, R4)*
- [x] **A2** — Every pane-local strip independently marks that pane's assigned layout, and changing focus alone changes neither another strip's meaning nor its active marker.
      *Evidence:* a split-pane state with distinct pane layouts asserted in tests and captured through `QUANTICK_PANE_LAYOUTS`.
      → `crates/app/src/app/tests/panes_layout_tests.rs` and the PR body's **Visual evidence** section. *(R2, R4)*
- [x] **A3** — Switching a tab or creating a layout from a pane-local strip targets that pane; rename and delete retain their shared-catalog behavior and existing delete confirmation.
      *Evidence:* action-addressing regression tests covering pane switch/create and shared rename/delete semantics.
      → `crates/app/src/app/tests/panes_layout_tests.rs`. *(R1, R3)*
- [x] **A4** — The shared catalog, layout contents, per-pane assignments, save/restore, keyboard shortcuts, View menu behavior, and control-plane read/write contract remain compatible.
      *Evidence:* existing and focused app/control tests, inspected contract diff, and full workspace validation.
      → the PR body's **Compatibility evidence** and **Validation** sections. *(R3)*
- [x] **A5** — At supported narrow pane widths and the existing 12-layout maximum, every pane-local strip provides an operable way to reach its tabs and actions without overlapping neighboring panes or chart controls.
      *Evidence:* narrow-window/max-layout visual capture and geometry/interaction assertions.
      → the PR body's **Visual evidence** section and focused tests. *(R1, R4)*
- [x] **G1** — Every authored repository artifact is English under `CLAUDE.md`.
      *Evidence:* language guard and architecture-review dimension 8 PASS.
      → the PR body's **Validation** section and architecture-review report URL.
- [ ] **G2** — Targeted regressions and the ordered `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` loop pass after rebasing on latest `main`.
      *Evidence:* commands and exit codes at the final verified tree.
      → the PR body's **Validation** section.
- [x] **G3** — Dense-tape UI performance is flat or better than a same-environment `main` control, with healthy `APP_HEALTH_SUMMARY` fps/frame averages and no attributable slow-frame burst.
      *Evidence:* paired main/branch measurements using identical hooks and replay/configuration.
      → the PR body's **Performance** section.
- [x] **G4** — The changed pane-local strip states are reachable from a fresh launch through `ui-harness`; `QUANTICK_PANE_LAYOUTS` documentation and any necessary hook behavior describe the pane-local result.
      *Evidence:* hook declaration/registry guard plus successful hook-driven launches.
      → the hook source/registry and the PR body's **UI harness** section.
- [x] **G5** — `visual-qa` passes every applicable changed state, including default, split with distinct layouts, dense data, narrow window, maximum layouts, and rename/delete surfaces, with no unaccepted defect.
      *Evidence:* screenshot paths, structured scene/diagnostic readings, and the visual-QA verdict.
      → the PR body's **Visual evidence** section and visual-QA report URL.
- [x] **G6** — `trader-ux-review` finds no unresolved Blocker or Should-fix for Rafa, Marina, or Duda across layout identification and switching flows.
      *Evidence:* persona review tied to screenshots and interaction code.
      → the PR body's **Trader UX review** section.
- [ ] **G7** — `arch-review` runs at medium bug-pass depth and resolves or explicitly defers every Blocker and Should-fix in the PR body.
      *Evidence:* current-key durable architecture-review PASS report and `arch-review-ok` projection.
      → the architecture-review PR report and PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Not applicable

- Engine/determinism gates do not apply: the change is confined to application chrome and does not alter bars, market data, replay, or deterministic domain output.
- `new-extension` does not apply: no feed, bar type, indicator, layer, panel, crate, port, or registration point is added.
- The new-action capability gate does not apply: switch/create/rename/delete already have keyboard, menu, and control-plane reach; this mission only relocates their existing mouse surface while preserving that contract.
- Docs/skills-only waivers do not apply because Rust runtime UI code changes.

## Verification evidence before review

- A1-A5 and G1/G3-G6 are evidenced in
  `.claude/evidence/pane-local-layout-strips/`, including four durable
  screenshots, the control-plane evidence manifest and health summaries, the
  visual-QA verdict, the trader-persona review, and the dense-frame comparison.
- The ordered fmt, clippy, and build commands pass. `cargo test --workspace`
  passed 2,068 app tests and reproduced two unrelated order-flow worker-test
  failures identically on clean `main` at `eb7bb039`; G2 remains unchecked
  until exact-head CI supplies an authoritative green result or the upstream
  baseline is repaired.
- G7, G-AI1 through G-AI4, and C1 through C3 intentionally remain unchecked:
  the mission contract archives this file before those PR-bound reviews and
  delivery steps run.

## Closing steps

- [ ] **C1** — Full `delivery-review` publishes a current-head PASS after every criterion and gate is evidenced.
      *Evidence:* durable delivery-review report and valid `delivery-review-ok` projection.
      → the delivery-review PR report and PR body.
- [ ] **C2** — The mission branch has an open PR with the objective as its first body line and current-head evidence/report URLs.
      *Evidence:* GitHub PR state, branch, base, head, and body.
      → the open PR.
- [ ] **C3** — The open PR is non-draft, exact-head CI is green, and `mission_ship_gate.sh mission <pr>` publishes PASS without merging to `main`.
      *Evidence:* final verifier output and literal reconciliation comment.
      → the PR and final session handoff.

## Verbatim request

> User: “$mission high colocar tab de layout em cada janela ao inves de compartilhar o layout em todos. Hoje a gnt clica ja janela e visualiza o layout no canto inferior equerdo. Isso fica confuso. Seria melhor ter para cada janela a lista de abas de cada janela.”
>
> User clarification: “isso apenas exibir no rodape de cada janela ao inves de dividirem o mesma visualizçaão que muda ao clicar na janela. Vai entao agora replicar visual para cada janela ao inves de todos comaprilharem o mesmo visual”
