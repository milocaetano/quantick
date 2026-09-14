# Streamline the chart context menu

Reorganize the chart context menu by placing anchored VWAP first, removing market buy and sell actions, and grouping chart-visible items into an expandable management section.

This reduces the menu's visual noise while preserving quick access to chart display controls and resting paper orders.

**Tier:** medium, raised from small because autonomous expanded-menu coverage pushed the diff beyond the 300-line small-tier ceiling. The product scope remains a bounded rearrangement with no new domain capability or persistent state; medium adds completeness-only delivery review.

## Request ledger

- **R1** — Place the anchored VWAP action at the top of the bare chart context menu. Source: “colcoar vwap ancordara no topo”.
- **R2** — Remove the market buy and market sell actions from the chart context menu. Source: “Remover sell buy market”.
- **R3** — Move the existing chart-layer visibility controls into a section that expands to the right, reducing the primary menu's visual clutter while retaining show/hide control. Source: “AGrupar tudo que é exibido no grafico em uma nova sessao para tirar essa poluição. Ai qndo expandir pra direita com as op~ções de tirar ou remover.”

## Decisions

The mission began at small with no interrogation. No design-driving ambiguity emerged before the required tier raise; the recorded assumptions remain reversible presentation readings.

## Assumptions

- **S1** — “New section” means an egui submenu named `chart layers`, matching the reference image's rightward arrow. This is safe because it is a reversible presentation choice and reuses the existing tape-menu pattern.
- **S2** — “Everything displayed on the chart” means the toggles currently owned by the `chart layers` section; dynamic indicator entries remain in their separate `indicators` section, as shown outside the red box in the reference image. This is safe because the annotated boundary distinguishes the two sections.
- **S3** — Anchored VWAP is first on a bare chart context menu; a clicked drawing may retain its more-specific object actions above general chart actions. This is safe because object actions apply to the item explicitly targeted by the right-click.
- **S4** — Limit and stop entries remain in the trade section; only its two market entries are removed. This follows the request's specific wording and the red deletion annotation.

## Acceptance criteria

- [x] **A1** — On a bare chart right-click, `Anchor VWAP here` is the first actionable entry.
      *Evidence:* focused egui regression test and an open-menu screenshot.
      → PR body and visual-QA evidence path. *(R1)*
- [x] **A2** — The chart context menu contains neither a market buy nor a market sell action, while its existing limit and stop actions remain available under their current validity rules.
      *Evidence:* focused egui regression test and an open-menu screenshot.
      → PR body and visual-QA evidence path. *(R2)*
- [x] **A3** — The bare chart menu exposes one `chart layers` submenu that expands to the right and contains every existing non-tape layer toggle with unchanged state, disabled reasons, and footprint configuration access.
      *Evidence:* focused egui regression tests, existing layer behavior tests, and open/expanded menu screenshots.
      → PR body and visual-QA evidence path. *(R3)*
- [x] **G1** — Every authored artifact is in English.
      *Evidence:* architecture review dimension 8 and the language guard.
      → PR body.
- [ ] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` pass after synchronization with latest `main`.
      *Evidence:* command exit codes at the final implementation head.
      → PR body.
      Local `fmt`, `clippy`, and `build` passed after rebasing onto `1429941f`.
      `cargo test --workspace` passed 2,060 app tests and failed two unrelated
      order-flow projection tests; the first failure reproduces alone in a
      clean detached `origin/main` worktree on Windows, while CI run
      `34900186975` is green for that exact base SHA. Exact-head PR CI remains
      the closing authority.
- [x] **G3** — Performance impact is declared for every touched path: menu composition runs only during the rare context-menu interaction; no per-trade, per-depth, or continuous closed-menu path changes.
      *Evidence:* diff inspection and architecture review performance verdict.
      → PR body.
- [ ] **G4** — Architecture review runs with its low-effort bug pass, with every Blocker and Should-fix resolved or explicitly deferred.
      *Evidence:* durable architecture-review report and current `arch-review-ok` receipt.
      → PR report URL and PR body.
- [x] **G5** — The changed menu is reachable through `QUANTICK_CONTEXT_MENU=chart-layers`, an extension of the existing context-menu hook, and visual QA passes the relevant default, expanded, dense-data, narrow-window, and unavailable-layer states without clipping or misleading controls.
      *Evidence:* hook-registry entry, screenshots, structured scene/diagnostics where available, and visual-QA verdict.
      → PR body and visual-QA evidence path.
- [x] **G6** — Trader UX review finds no unresolved Blocker for Rafa, Marina, or Duda and no unresolved Should-fix unless explicitly deferred in the PR body.
      *Evidence:* persona flow verdict over the captured context-menu states.
      → PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Review repairs

- The low-effort bug pass found and closed two Should-fix items before review
  publication: canonical paper-trading prose now reflects the removal of
  market orders, and A1's regression test compares anchored VWAP against both
  `chart layers` and `trade`. The focused test and repository guards pass at
  the repair head.

## Not applicable

- Hot-path benchmarking is not applicable because the changed menu builders run only while the user has opened the context menu; no ingestion, rendering-with-menu-closed, engine, or depth-update path changes.
- `new-extension` is not applicable because no capability, registry variant, pane, layer, or persistent state is added.
- Engine/determinism gates are not applicable because no engine or bar-building code changes.
- A new second-operator capability is not applicable because this rearranges existing UI doors and preserves the existing control-plane/readback surface.
- MT5 Python and bridge checks are not applicable because neither tree is touched.
- `cargo deny` is not applicable because no dependency or lockfile change is planned.

## Closing steps

- [ ] **C1** — Open the pull request with current-head evidence and durable review links.
- [ ] **C2** — The final mission verifier passes at exact-head green CI and the PR is ready for user evaluation; only the user may merge to `main`.
- [ ] **C3** — Completeness-only `delivery-review` passes against the retained request and ledger at the current review key.

## Verbatim request

Attributed quotation from the trader:

> $mission small Reorganizar botao direito no grafico conforme imagem que esta em C:\temp\features (colcoar vwap ancordara no topo). Remover sell buy market. AGrupar tudo que é exibido no grafico em uma nova sessao para tirar essa poluição. Ai qndo expandir pra direita com as op~ções de tirar ou remover.

Reference image: `C:\temp\features\Screenshot_2.png`.
