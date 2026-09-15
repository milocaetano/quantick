# Mission — per-indicator vertical mouse guide

**Objective:** Add a workspace-persisted, per-indicator vertical mouse guide that mirrors the price-chart hover time into only the enabled non-price indicator pane.

**Why:** Let the trader correlate a price swing high or low with the indicator value at the same chart time without visually guessing between stacked panes.

**Tier:** medium — this is a bounded but user-visible per-frame UI change with workspace persistence, a new trader action, operability obligations, measured performance, and the full inline completeness/delivery review path.

Branch: `feat/mouse-vertical-line`

Worktree: `C:\src\quantick-worktrees\feat-mouse-vertical-line`

## Request ledger

- **R1** — A non-price indicator pane, such as CVD, offers a right-click option for a vertical mouse line (source: “opção para janelas não preços como CVD” and “Clicando com botao direito do mouse nessa janela”).
- **R2** — The option applies only to the individual indicator pane where it was enabled (source: “Somente na janela que eu ativei” and “fica apenas para esse indicador”).
- **R3** — While the pointer is over the price chart, the enabled indicator pane shows a subtle dashed vertical line aligned to the same chart time, with no horizontal line (source: “mouse no preço”, “linha vertical tracejada bem sutil”, and “sem a linha horizontal”).
- **R4** — The guide makes it easy to correlate a price top or bottom with the corresponding top or bottom in CVD or another non-price indicator (source: “Assim eu consigo ver que aquele preço fez aquele fundo ou topo no cvd”).
- **R5** — The existing time-axis pointer behavior remains intact; the new guide extends the visual alignment into the selected indicator pane (source: “Hoje ja temos isso no eixo X da hora, mas nao temos no grafico”).
- **R6** — The per-indicator choice is saved and restored with the workspace (source: “Fica salvo tbm no workspace”).

## Decisions

No user decision was required: the request identifies the hover source, target ownership, isolation, appearance, and persistence precisely enough for a medium mission.

## Assumptions

- **S1** — “Non-price windows” means any pane-hosted indicator, not a CVD-only special case. This is safe because CVD is introduced as an example (“como CVD”), and a general indicator-owned option avoids hardcoded indicator vocabulary.
- **S2** — The setting defaults to off. This is safe because it preserves current behavior and old workspace files can deserialize without inventing a visible guide.
- **S3** — The guide is visible only while the pointer is inside the price-chart plot area and disappears over chrome, axes, menus, or outside the price chart. This is safe because the requested source is explicitly “mouse no preço” and matches the existing pointer/crosshair lifecycle.
- **S4** — The English UI label is `Mouse vertical line`. This is safe because the request allows approximate wording (“algo assim”), the choice is reversible in one edit, and repository UI text must be English.
- **S5** — Alignment uses the shared chart-time/bar coordinate, not raw screen X. This is safe because panes may have different rectangles and the requested semantic is the same market instant.
- **S6** — Performance rates are: pointer capture, projection, and conditional painting are per-frame; context-menu toggling, workspace serialization, and control invocation are rare; no per-trade or per-depth path is touched. This is safe because the feature consumes existing frame input and saved layout state only.

## Acceptance criteria

- [x] **A1** — Every non-price indicator pane exposes a checked `Mouse vertical line` item in its right-click context menu; toggling it changes only that indicator instance and the default is off.
      *Evidence:* focused app tests for menu routing, default state, and per-instance ownership plus a harness capture of the menu.
      → `crates/app/src/app/tests/indicators_tests.rs` and `docs/evidence/mouse-vertical-line/ui-harness.md`. *(R1, R2)*
- [x] **A2** — While the pointer is inside the price-chart plot area, each enabled indicator pane paints one subtle dashed vertical guide at the same chart time/bar; it paints no horizontal guide and disappears when the source pointer leaves the price plot.
      *Evidence:* painter-shape/alignment tests and visible enabled/cleared screenshots.
      → `crates/app/src/pane/tests/mod.rs` and `docs/evidence/mouse-vertical-line/visual-qa.md`. *(R3, R4)*
- [x] **A3** — With at least two indicator panes, enabled and disabled instances remain visually independent, including indicators with equal local slot numbers on different chart panes.
      *Evidence:* multi-pane and multi-indicator regression tests plus the isolation screenshot matrix.
      → `crates/app/src/app/tests/indicators_tests.rs` and `docs/evidence/mouse-vertical-line/visual-qa.md`. *(R2, R4)*
- [x] **A4** — Existing time-axis pointer/crosshair behavior is unchanged when the new setting is off and remains present when an indicator guide is enabled.
      *Evidence:* existing crosshair/time-axis regressions plus a focused coexistence test.
      → `crates/app/src/pane/tests/mod.rs`. *(R5)*
- [x] **A5** — Workspace save/import and restart restore the toggle for the correct indicator instance; old workspace documents without the field load with the guide off and preserve all existing content.
      *Evidence:* serialization compatibility, round-trip, and app workspace restore tests.
      → `crates/app/src/ui_state.rs` and `crates/app/src/app/tests/indicators_tests.rs`. *(R6, R2)*
- [x] **A6** — The new trader action and its current per-indicator state are discoverable, readable, and invokable without a mouse through the existing operability/control registration boundary, with invalid targets producing no partial mutation.
      *Evidence:* registry/scene/action tests and the operability section of the harness report.
      → `crates/app/src/operability/`, `crates/app/src/control/`, and `docs/evidence/mouse-vertical-line/ui-harness.md`. *(R1, R2, R6)*
- [x] **G1** — Every authored repository artifact is English and passes the language guard and `arch-review` dimension 8.
      *Evidence:* guard report and architecture review verdict.
      → `docs/evidence/mouse-vertical-line/four-checks.txt` and the PR architecture review report.
- [x] **G2** — After rebasing on latest `main`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` all exit successfully.
      *Evidence:* final-head command transcript with exit codes.
      → `docs/evidence/mouse-vertical-line/four-checks.txt`.
- [x] **G3** — Performance impact is declared for every touched path at the rates in S6, and release `APP_HEALTH_SUMMARY` fps/frame_avg under the same dense-tape hooks is flat or better than an `origin/main` control run, with no new per-frame allocation attributable to the guide.
      *Evidence:* interleaved control/candidate measurements, method, raw samples, and comparison.
      → `docs/evidence/mouse-vertical-line/performance.md` and the PR body.
- [ ] **G4** — `arch-review` runs on the current PR diff; every Blocker and Should-fix is resolved or explicitly deferred in the PR body under the delivery contract.
      *Evidence:* durable current-key PASS report and zero unresolved required findings.
      → PR review report and PR body.
- [x] **G5** — The changed menu, enabled guide, disabled isolation, cleared-hover state, restored-workspace state, and operability state are reachable from a fresh launch by documented environment hooks added in the same change.
      *Evidence:* hook documentation, commands, screenshots, and semantic observations.
      → `docs/evidence/mouse-vertical-line/ui-harness.md` and the canonical UI-harness hook table.
- [x] **G6** — `visual-qa` covers normal and narrow windows, normal and dense data, menu off/on, alignment, isolation, hover exit, workspace restore, and visual subtlety; every cell is PASS or an explicitly user-accepted defect.
      *Evidence:* screenshot matrix and visual verdict.
      → `docs/evidence/mouse-vertical-line/visual-qa.md`.
- [x] **G7** — `trader-ux-review` reports no unresolved Blocker for discovery, terminology, target clarity, temporal alignment, visual obstruction, or persistence feedback.
      *Evidence:* durable trader persona review.
      → `docs/evidence/mouse-vertical-line/trader-ux-review.md`.
- [x] **G8** — The feature follows `new-extension`: it docks through an explicit indicator-guide/action boundary and registration point, unchanged defaults preserve current behavior, a fake or independent second target proves the abstraction, and the PR body states the blast radius.
      *Evidence:* architecture-focused tests, source inspection, and PR-body blast-radius declaration.
      → relevant app port/registration tests and the PR body.
- [x] **G9** — The inline medium-tier completeness pass reconciles every request-ledger item with observable criteria and evidence before delivery review.
      *Evidence:* source-first reconciliation recorded in the archived goal and delivery report.
      → `.claude/GOAL-archive-mouse-vertical-line.md` and the PR delivery review report.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Non-applicable gates

- Engine/determinism is not applicable: no bar aggregation, indicator kernel, fixture output, clock, or engine dependency changes are intended.
- Feed, order-book, replay-source, trading, paper-account, and money/safety gates are not applicable: the feature reads existing UI hover state and cannot place, cancel, or influence an order.
- A new crate or third-party dependency is not applicable: the extension belongs at existing app UI, workspace, and operability boundaries.
- Per-trade and per-depth performance evidence is not applicable: S6 classifies the touched runtime path as per-frame and the remaining paths as rare.

## Closing steps

- [ ] **C1** — `delivery-review` publishes a durable completeness PASS for the current PR review key after architecture and AI review are current.
      *Evidence:* current delivery report and `delivery-review-ok` receipt.
      → PR review report and private projection.
- [ ] **C2** — An open, non-draft PR matches this branch, current head, and `main` base, with current-head evidence and report URLs in its body.
      *Evidence:* PR metadata and body inspection.
      → GitHub PR.
- [ ] **C3** — `sh .claude/hooks/mission_ship_gate.sh mission <pr>` passes at exact head after all registered CI checks are green and publishes the literal final reconciliation; only the user may merge to `main`.
      *Evidence:* final verifier comment/receipt and CI check list.
      → GitHub PR.

## Verbatim request

> User, 2026-09-14:
>
> `$mission medium colocar opção para janelas não preços  como CVD. Clicando com botao direito do mouse nessa janela, deve oferecer opção para Mouse Vertical line algo assim. O que isso signfica? que qnod eu to com o mouse no preço, eu vejo uma linha vertical tracejada exibindo no grafico debaixo. Assim eu consigo ver que aquele preço fez aquele fundo ou topo no cvd. Isso fica mais faicl de viaualizar se tiver uma linha vertical tracejada bem sutil exibindo no grafico acompanahdno o meu mouse. Hoje ja temos isso no eixo X da hora, mas nao temos no grafico. Isso seria igual ativar o corsshair apenas naquela janela, mas sem a linha horizontal soh a vertical, ja que o mouse esta na janela de cima e ta vendo al inha vertical ja janela debaixo. Somente na janela que eu ativei. Eu ativie na CVD entao fica apenas para esse indicador. Fica salvo tbm no workspace.`
