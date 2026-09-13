# Temporary right-drag volume profile

Implement a temporary right-drag measurement selection that offers contextual volume-profile insertion, dismisses on outside chart clicks, and converts into a regional volume profile when chosen. This matters because the trader should be able to inspect a range and place its fixed-range volume profile with one uninterrupted chart gesture and one explicit action.

**Tier:** medium — this changes a per-frame chart gesture, adds visible transient chrome, and adds a trader action that requires complete UI, operability, review, and delivery evidence.

## Request ledger

- **R1** — Holding the secondary mouse button and dragging on the chart creates a temporary selection painted like the ruler. Source: “botão direito do mouse (segurar) e arrastar” and “mostrando o desenho que a regua faz”.
- **R2** — Releasing a qualifying right drag shows contextual drawing-style chrome with an action for fixed-range volume profile. Source: “ao soltar vai aparecer um barra de ferramante ... com uma opção para inserri volume profile”.
- **R3** — Clicking outside the temporary action surface on the chart dismisses both the temporary ruler and its chrome. Source: “clicar fora no grafico” and “O desnho ... some”.
- **R4** — Invoking the volume-profile action replaces the temporary ruler with a persistent fixed-range volume profile over the selected range. Source: “aparece o voluem profile no lugar e o desneho dessa regua some”.
- **R5** — The ordinary ruler tool keeps its existing persistent drawing behavior. Source: “Diferente de clicar na regua na barra de ferramenta ao lado ... o desnh[o] permace com as medidas”.
- **R6** — The completed flow makes regional fixed-range volume profile insertion available through right-drag plus one explicit click. Source: “Assim eu posso inserir volume profile em uma regiao apenas clicando com botao direito arrastando”.

## Decisions

No user decision was required: the repository already distinguishes the fixed-range profile's bar interval from the ruler's two-axis readout, and the request distinguishes a held drag from a plain secondary click.

## Assumptions

- **S1** — A plain secondary click below the existing drag threshold continues to open the chart context menu. This preserves shipped behavior and follows directly from the request naming a held drag.
- **S2** — The temporary overlay reuses the ruler's two-axis visual readout, but conversion uses the existing fixed-range profile's horizontal bar interval; the vertical extent does not constrain profile prices. The registered fixed-range profile already owns this product definition.
- **S3** — The temporary contextual bar contains the fixed-range profile action and only controls meaningful to a temporary range. Color, width, persistence, lock, hide, duplicate, and delete are not offered because the temporary range is not a drawing object.
- **S4** — The gesture is available while the Pointer tool is active and does not disarm or replace an explicitly armed drawing tool. This prevents a secondary gesture from corrupting an in-progress drawing draft.
- **S5** — Per-frame work is bounded to pointer-state checks and painting one two-anchor preview only while active; conversion and dismissal are rare paths. This is safe to implement before measurement because no trade, depth, or engine path is touched.

## Acceptance criteria

- [x] **A1** — A secondary-button drag that starts on drawable chart canvas and crosses the existing drawing drag threshold paints a ruler-equivalent temporary preview without inserting a drawing into the drawing store.
      *Evidence:* focused gesture/state unit tests and active-state screenshot.
      → `docs/evidence/right-drag-volume-profile/targeted-tests.txt` and `docs/evidence/right-drag-volume-profile/visual-qa.md`. *(R1)*
- [x] **A2** — Releasing a qualifying right drag leaves the temporary ruler visible and opens contextual chrome containing a discoverable “Fixed range volume profile” action; a secondary click without a qualifying drag still opens the existing chart context menu.
      *Evidence:* regression tests, semantic scene assertion, and released-state screenshot.
      → `docs/evidence/right-drag-volume-profile/targeted-tests.txt` and `docs/evidence/right-drag-volume-profile/visual-qa.md`. *(R2)*
- [x] **A3** — A primary click on chart canvas outside the temporary chrome dismisses the temporary ruler and chrome without adding, deleting, or editing persistent drawings.
      *Evidence:* focused dismissal test and before/after structured drawing snapshot.
      → `docs/evidence/right-drag-volume-profile/targeted-tests.txt` and `docs/evidence/right-drag-volume-profile/visual-qa.md`. *(R3)*
- [x] **A4** — Invoking the temporary chrome action creates one persistent fixed-range volume profile with the selected horizontal anchors and clears the temporary ruler and chrome in the same state transition.
      *Evidence:* conversion test, structured drawing readback, and converted-state screenshot.
      → `docs/evidence/right-drag-volume-profile/targeted-tests.txt` and `docs/evidence/right-drag-volume-profile/visual-qa.md`. *(R4)*
- [x] **A5** — A ruler placed through the ordinary drawing toolbar remains a persistent, selectable ruler after placement and after an unrelated chart click.
      *Evidence:* existing-behavior regression test.
      → `docs/evidence/right-drag-volume-profile/targeted-tests.txt`. *(R5)*
- [x] **A6** — The end-to-end chart flow requires one right-drag gesture followed by one action click and works consistently in the affected pane without stealing focus or covering the forming bar/live lane.
      *Evidence:* trader persona walkthrough and visual state matrix.
      → `docs/evidence/right-drag-volume-profile/trader-ux-review.md` and `docs/evidence/right-drag-volume-profile/visual-qa.md`. *(R6)*

## Injected gates

- [ ] **G1** — Every authored artifact is in English and both the language guard and manual branch/commit/PR prose inspection pass.
      *Evidence:* guard output and arch-review language verdict.
      → `docs/evidence/right-drag-volume-profile/four-checks.txt` and `docs/evidence/right-drag-volume-profile/arch-review.md`.
- [x] **G2** — After final synchronization with `origin/main`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` all exit zero.
      *Evidence:* captured final command output with head and base revisions.
      → `docs/evidence/right-drag-volume-profile/four-checks.txt`.
- [x] **G3** — Performance is measured at its declared rates: pointer/state/preview work is per-frame while the gesture or chrome is active, conversion/dismissal is rare, and dense-tape `APP_HEALTH_SUMMARY` is flat or better than a same-hooks `main` control.
      *Evidence:* paired main/branch health summaries and interpretation.
      → `docs/evidence/right-drag-volume-profile/performance.md`.
- [x] **G4** — Every new temporary UI state is reachable from a fresh launch through registered `QUANTICK_*` hooks that reuse the manual action path and default off.
      *Evidence:* hook registry test plus hook-driven captures.
      → `docs/evidence/right-drag-volume-profile/ui-harness.md`.
- [x] **G5** — Visual QA covers active drag, released chrome, dismissal, conversion, empty/dense data, narrow/normal windows, scene honesty, motion, and health; every applicable cell is PASS or an explicit accepted defect.
      *Evidence:* screenshot paths, control-plane readings, evidence IDs, and verdicts.
      → `docs/evidence/right-drag-volume-profile/visual-qa.md`.
- [x] **G6** — Trader UX review walks the flow as Rafa, Marina, and Duda and leaves no unresolved Blocker or Should-fix.
      *Evidence:* persona findings and final verdicts.
      → `docs/evidence/right-drag-volume-profile/trader-ux-review.md`.
- [ ] **G7** — The feature names its docking port and registration point; defaults preserve current behavior; a fake second implementation exercises the port; and the PR reports files added/edited plus lines added to new/existing files.
      *Evidence:* port test, diff statistics, and architecture review.
      → `docs/evidence/right-drag-volume-profile/targeted-tests.txt`, `docs/evidence/right-drag-volume-profile/arch-review.md`, and the PR body.
- [x] **G8** — A non-mouse operator can act through a named call carrying range data and actor, read the stable resulting drawing ID/state as structured data, and discover the stable action ID and parameters from the same registered source that feeds the UI.
      *Evidence:* capability registration/dispatch/readback tests and generated catalog consistency.
      → `docs/evidence/right-drag-volume-profile/targeted-tests.txt` and `docs/evidence/right-drag-volume-profile/arch-review.md`.
- [ ] **G9** — `arch-review` runs on the final branch with the medium-tier low-effort bug pass, all nine dimensions, and no unresolved Blocker or Should-fix.
      *Evidence:* exact-head review verdict and marker.
      → `docs/evidence/right-drag-volume-profile/arch-review.md` and the branch git dir `arch-review-ok` marker.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

The durable evidence destinations for G-AI1 through G-AI4 are the pull request's
AI-review report, review threads, and the branch git directory's
`ai-review-complete` marker.

## Not applicable

- Engine/determinism fixture and golden gates do not apply because the feature does not change aggregation, order flow, indicators, simulation, or replay outputs.
- Feed and MT5-specific checks do not apply because no feed, bridge, or Python path is touched.
- Dependency and license checks do not apply unless implementation changes `Cargo.toml` or `Cargo.lock`; the intended design uses existing app/control dependencies.
- Docs/skills-only waivers do not apply because this is a runtime UI and control capability change.
- Trading authority gates do not apply because placing a chart analysis drawing neither places, changes, nor cancels an order; actor attribution still applies to non-human-created drawings.

## Closing steps

- [ ] **C1** — `delivery-review` returns PASS against the final archived mission and exact branch head, and records `delivery-review-ok`.
      *Evidence:* delivery-review report and marker.
      → `docs/evidence/right-drag-volume-profile/delivery-review.md` and the branch git dir `delivery-review-ok` marker.
- [ ] **C2** — A pull request is open and ready with green final-head CI; its English body names the medium tier, local/reused/CI evidence, performance result, blast radius, reviews, and any explicit deferrals.
      *Evidence:* PR URL, body, readiness state, and `gh pr checks --watch` result.
      → pull request.

## Request as received

The following is a marked, attributed quotation preserved under the repository language-rule exemption:

> $mission medium Ao clicar com botão direito do mouse (segurar) e arrastar. Vai criaruma opção cmo se tivesse usando a regua. Porem ao soltar vai aparecer um barra de ferramante (igual ao que clicamos em desenho no grafico para trocar de cor ou tamanho), mas com uma opção para inserri volume profile. Assim eu posso inserir volume profile em uma regiao apenas clicando com botao direito arrastando (mostrando o desenho que a regua faz) e depois clicando no icone de volume profile. Se eu fizer o desnho e clicar fora no grafico. O desnho feito com botao direito some se eu clicar no grafico. Se eu clicar no icone de volume profile, aparece o voluem profile no lugar e o desneho dessa regua some. Diferente de clicar na regua na barra de ferramenta ao lado, que ao desnhar o desneho permace com as medidas. Com o botao direito do mouse, o desenho é temporario até o usuario escolher uma opção: clicar fora ou clicar no icone de voluem profile
>
> cointinue com permissao
