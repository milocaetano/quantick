# Right-drag Fibonacci actions

Add right-drag Fibonacci retracement and projection actions, and keep Volume Profile creation available even when the selected range extends beyond the latest price.

The current secondary-button range exposes only a conditionally enabled Volume Profile action. This mission makes the temporary range a compact drawing launcher while preserving the exact range the trader selected, including chart space ahead of the latest trade.

**Tier:** medium — the change affects a user-visible, per-frame chart surface and adds operable actions, so it requires the full gate table, inline delivery completeness review, and bounded interrogation.

## Request ledger

- **R1** — After a secondary-button drag on a chart, offer Fibonacci Retracement as a clickable icon. Source: “deve aparecer opção de Fibonacci retrac ... como icon para poder clicar”.
- **R2** — After the same gesture, offer Fibonacci Projection as a clickable icon. Source: “e projection como icon para poder clicar”.
- **R3** — The retracement action creates the normal registered retracement directly from the dragged endpoints. Source: “vai colocar apenas o fib retração normal como se tivesse traçando fibo direto”.
- **R4** — The projection action uses the dragged move and starts its projection at the final endpoint. Source: “o projection segue a projeção a partir do ponto final”.
- **R5** — The Volume Profile action is always enabled, including when the selected endpoint is ahead of the latest market price/time. Source: “O botão deve ficar SEMPRE ATIVO”.
- **R6** — Activating Volume Profile for a future-space range creates the same fixed-range profile behavior as a historical range. Source: “Se passou do preço vai criar volume profile mesmo assim ... o comportamento deve ser o mesmo”.
- **R7** — Deliver the work as a medium Quantick mission. Source: “$mission medium”.

## Assumptions

- **S1** — “Projection” maps to the already registered `fib-extension` drawing because that is the repository's three-anchor Fibonacci projection tool. This is safe because no competing Fibonacci projection tool exists in the registry.
- **S2** — “Starts at the final endpoint” means the projection anchors are `[drag start, drag end, drag end]`: A→B defines the measured move and C=B starts the projected levels. This follows the request literally and is reversible in one localized mapping.
- **S3** — The existing temporary ruler remains the gesture preview and all three actions replace it with one durable registered drawing. This preserves the established interaction and avoids inventing a new preview style.
- **S4** — The three icons appear in the order Volume Profile, Fib Retracement, Fib Projection, preserving the existing action first and adding the requested choices beside it. This is a reversible presentation choice.

## Acceptance criteria

- [x] **A1** — Releasing a qualifying secondary-button drag shows clickable icons for Volume Profile, Fib Retracement, and Fib Projection in the temporary range action bar.
      *Evidence:* focused app regression test and visual-QA screenshot of the ready range.
      → `crates/app/src/app/tests/drawings_tests.rs` and the PR evidence section. *(R1, R2)*
- [x] **A2** — Choosing Fib Retracement creates exactly one registered `fib-retracement` drawing whose two anchors equal the settled drag endpoints.
      *Evidence:* focused app regression test asserting tool ID and anchors.
      → `crates/app/src/app/tests/drawings_tests.rs` and the PR evidence section. *(R1, R3)*
- [x] **A3** — Choosing Fib Projection creates exactly one registered `fib-extension` drawing with anchors `[start, end, end]`.
      *Evidence:* focused app regression test asserting tool ID and all three anchors.
      → `crates/app/src/app/tests/drawings_tests.rs` and the PR evidence section. *(R2, R4)*
- [x] **A4** — The Volume Profile icon remains enabled when either dragged anchor lacks market time because it lies in future chart space.
      *Evidence:* focused surface and app regression tests covering a future-space endpoint.
      → `crates/app/src/surfaces/drawing_chrome/quick_range.rs`, `crates/app/src/app/tests/drawings_tests.rs`, and the PR evidence section. *(R5)*
- [x] **A5** — Choosing Volume Profile for a future-space range creates one registered fixed-range profile from the settled endpoints and consumes the temporary range.
      *Evidence:* focused app regression test asserting the resulting tool, anchors, and dismissal.
      → `crates/app/src/app/tests/drawings_tests.rs` and the PR evidence section. *(R5, R6)*
- [x] **A6** — Each visible quick-range action has a stable semantic control and invokes a named registered action so the behavior is discoverable, readable, and operable without a mouse.
      *Evidence:* control-scene and operability-registry tests plus `arch-review` second-operator verdict.
      → relevant control/operability tests, durable architecture report, and the PR evidence section. *(R1, R2, R5)*
- [x] **A7** — The durable mission record identifies the work as tier `medium` and reconciles every requested outcome with evidence.
      *Evidence:* archived mission ledger and delivery completeness PASS.
      → `.claude/GOAL-archive-right-drag-fibonacci-actions.md` and the durable delivery report. *(R7)*

- [x] **G1** — Every authored artifact is English and the language guard passes.
      *Evidence:* `cargo test -p quantick-guards` and architecture-review dimension 8.
      → PR evidence section and durable architecture report.
- [x] **G2** — The final rebased head passes `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace`.
      *Evidence:* local command logs and exact-head CI checks.
      → PR evidence section and CI check URLs.
- [x] **G3** — Performance impact is declared by touched path: secondary-drag input and drawing creation are rare; the ready action bar is per-frame while visible. Its dense-tape frame cost is flat or better than `main` within measurement noise.
      *Evidence:* comparable `APP_HEALTH_SUMMARY` measurements on `origin/main` and the mission head.
      → PR performance section.
- [x] **G4** — Every changed UI state is reachable through an environment hook and passes `ui-harness` and `visual-qa`, with defects fixed or explicitly accepted by the trader.
      *Evidence:* hook test, screenshots, and durable visual-QA result.
      → PR visual evidence section.
- [x] **G5** — `trader-ux-review` reports no unresolved Blocker for the right-drag choice and future-space profile flow.
      *Evidence:* durable trader-UX review verdict.
      → PR review evidence section.
- [ ] **G6** — `arch-review` runs at low bug-pass effort and full shape depth, with every Blocker and Should-fix resolved or explicitly deferred in the PR body.
      *Evidence:* durable current-head PASS report and valid `arch-review-ok` projection.
      → PR architecture report and evidence section.
- [ ] **G7** — The action surface satisfies the second-operator act/read/discover requirements without adding an unregistered mouse-only path.
      *Evidence:* named action/scene tests and architecture-review verdict.
      → relevant registry tests, durable architecture report, and PR evidence section.
- [ ] **G8** — The Fibonacci annotation capabilities dock through the existing annotation action registry, preserve existing defaults, expose both tools through discovery, and prove the registration path with both implementations.
      *Evidence:* annotation registry/capability tests, generated catalog checks, blast-radius declaration, and architecture-review docking verdict.
      → control-plane tests, generated capability artifacts, PR architecture section, and durable architecture report.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Non-applicable gates

- **Engine/determinism:** no engine, aggregation, replay, or headless-domain behavior changes; the work is confined to registered app drawings and UI/control glue.
- **New drawing or surface type:** no drawing kind, panel, layer, feed, bar type, indicator, or crate is added. Two annotation capabilities are added through the existing action registry because the newly reachable Fibonacci actions may not remain mouse-only.
- **Sensitive trading authority:** the actions create annotations only; they do not place, cancel, size, or otherwise affect orders.
- **Python and dependency gates:** no MT5 Python path or dependency/lockfile change is planned.

## Implementation evidence

- Focused behavior: `quick_range_fibonacci_actions_use_the_dragged_move`, `a_future_space_range_still_creates_a_volume_profile`, `the_profile_capability_reports_an_honest_future_anchor`, and `both_fibonacci_tools_dock_through_the_annotation_registry` pass.
- Semantic operability: the control-scene regression names all three stable quick-range controls, each mapped to its registered annotation capability; generated capability, retry, and behavior artifacts agree with the registry.
- Visual QA: `candidate-flow-ready.png`, `candidate-narrow-ready.png`, and `candidate-future-final.png` under `C:\src\quantick-evidence\right-drag-fibonacci-actions`; no clipping, overlap, inactive affordance, or layout jump was found. The trader explicitly accepted the resulting screen on 2026-09-14.
- Motion sanity: captures 1.25 seconds apart had distinct SHA-256 digests and 276 changed sampled pixels while the action bar remained stable.
- Performance: with `tick(1)`, a 1000 x 650 flow canvas, 8 px bars, and the ready-range hook, `main` measured 59–60 FPS and 1.320 ms average CPU/frame; the mission head measured 59–60 FPS and 1.294 ms average CPU/frame. The difference is within noise and flat in the required direction.
- Trader UX: PASS with no Blocker or Should-fix. The flow adds no modal or focus theft for Rafa, preserves registered drawing defaults and persistence for Marina, and presents standard icons with tooltips and no unexplained disabled state for Duda.
- Mandatory local gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` pass. The test run explicitly removed the ambient `QUANTICK_BUBBLES` override from its process so repository fixtures, not a user preset, governed the order-flow tests.
- Blast radius: 22 pre-existing files touched; the largest production deposits are 87 lines in `quick_range.rs` and 36 lines in `drawing_input.rs`. No dependency or `Cargo.lock` change.

## Closing steps

- [ ] **C1** — The medium-tier delivery completeness pass publishes a durable PASS for the current PR head and records `delivery-review-ok`.
- [ ] **C2** — A draft PR is opened from `feat/right-drag-fibonacci-actions` to the current `main`, then made ready only after all required reviews and exact-head CI are green.
- [ ] **C3** — `mission_ship_gate.sh mission <pr>` publishes PASS and verifies the literal mission reconciliation at the final head.
- [ ] **C4** — The user alone decides whether to merge the ready PR into `main`.

## Verbatim request

> User, 2026-09-14: “$mission medium adicionar novos campos para botao direito e arrasta.  Ao clicar botao direito e arrastar no grafico, deve aparecer opção de Fibonacci retrac e projection como icon para poder clicar. Se for retracao vai colcoar apenas o fib retracao normal como se tivesse tracando fibo direto e o projetion segue a projecao a patir do findo final. Corrigir o volume profile icon para ativar quando utlrapassar preço final. Atualmente se eu projeto o desenho com botao direito pra frente do preço atual, o botao fica inativo. O botao deve ficar SEMPRE ATIVO. Se passou do preço vai criar volume profile mesmo assim, pois a gnt consegue criar volume pfile pra frente do preço, o comportamneto deve ser o mesmo”
