# Precise Volume Profile selection

Restrict Volume Profile pointer capture to its visible histogram, lines and borders so empty chart areas remain available for panning.

**Tier:** high. Pointer ownership affects everyday chart navigation and requires geometry, gesture, visual and performance evidence.

## Request ledger (source revision 1, map revision 1)
- **R1** Limit hover/cursor/selection and drag capture to the rendered profile geometry, visible lines and borders. Source: “somente na parte que o volume profile é desenhado”; “bem onde ocupa espaço ou nas suas linhas e bordas”.
- **R2** Empty and nearby chart space must allow clicking and panning the chart behind the profile without accidentally selecting or moving it. Source: “quero apenas clicar no grafico atras para mover”; “toda hora clica no volume profile”.
- **R3** Preserve intentional profile movement through precise grabs. Source: “opção de mover desenho de volume profile (mudando o mouse com opção para cliar no desenho)”.

Independent source-first preflight: request_preflight reviewed source revision 1 and map revision 1 before implementation; PASS, no missing outcomes. Its wording refinements are incorporated into A2/A3. The original verdict is preserved in `.claude/evidence/volume-profile-precise-hit/request-preflight.md`.

## Decisions
- **D1** User authorized continued work with permission: “pode continuar com permissao”.

## Assumptions
- **S1** Correct the default hit area without adding a preference. Safe: the request specifies where capture should occur; no alternate behavior or toggle is requested.
- **S2** Use a narrow profile-specific line tolerance capped at 3 logical pixels; the general drawing selector currently allows 10 pixels, which is too broad for the requested precision. Filled row interiors use actual volume-scaled widths; transparent silhouette interiors and missing rows pass through. Safe: standard pointer tolerance is reversible and does not restore the unwanted broad rectangular target.
- **S3** Labels remain informational; this correction targets histogram geometry and drawn lines/borders. Existing profile move/resize and control-plane commands remain the operator paths.

## Acceptance criteria
- [x] **A1** Hover and selection hit the painted histogram rows and visible profile lines/borders; empty row/range space, hidden lines, transparent silhouette interiors and off-chart geometry do not hit outside line tolerance.
  *Evidence:* targeted geometry regression assertions and recorded output → docs/workflow/evidence/volume-profile-precise-hit.md and PR evidence. *(R1)*
- [x] **A2** Press-drag-release through empty profile space pans the chart without profile hover, selection or movement; clicking empty space clears selection.
  *Evidence:* application pointer-event regression and output → docs/workflow/evidence/volume-profile-precise-hit.md and PR evidence. *(R2)*
- [x] **A3** Painted histogram-row bodies and visible lines still select/move the profile, and selected handles resize it; hover cursor matches the target.
  *Evidence:* geometry and gesture tests, visual captures → docs/workflow/evidence/volume-profile-precise-hit.md and PR evidence. *(R1, R3)*
- [ ] **G1** Repository artifacts are English except attributed source quotations.
  *Source:* CLAUDE.md language rule. *Evidence:* guards and architecture dimension 8 → PR reports and evidence document.
- [x] **G2** The ordered fmt, clippy, workspace build and workspace test checks pass against the current main base.
  *Source:* CLAUDE.md verification loop. *Evidence:* command logs and tree identity → PR evidence.
- [x] **G3** Per-frame projection/paint/hit paths have measured flat or better frame performance versus the same main fixture; no per-trade or per-depth path changes.
  *Source:* mission code/hot-path gates. *Evidence:* fixture benchmark or dense-tape health comparison → evidence document and PR.
- [x] **G4** Changed interaction states are reachable with existing FRVP demo/pointer hooks and visual QA passes the applicable surface/state matrix.
  *Source:* ui-harness and visual-qa. *Evidence:* screenshots, scene/diagnostics readings and state verdicts → evidence document and PR.
- [x] **G5** Trader UX review has no unresolved Blocker or Should-fix.
  *Source:* trader-ux-review. *Evidence:* persona flow review → evidence document.
- [ ] **G6** Architecture review includes the medium bug pass and full shape review, with required findings resolved.
  *Source:* mission high tier and arch-review. *Evidence:* durable current-review PASS report → PR.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

AI gate evidence destinations: current PR AI review report, thread listing and final verifier receipt. These are pending until executed.

## Plan and performance classification
- Geometry projection, painting and pointer hit testing: per frame; share rendered geometry, bound row lookup to the pointer neighborhood, avoid additional session scans.
- Pointer selection/move/resize: user gesture, evaluated per frame while active.
- Necessary detour for R1/R2: the generic invisible-handle shortcut bypassed body precision, so a tool-owned target-radius policy is used by both picking and handle gestures. The profile requires selection; other tools preserve their defaults.
- Independent architecture finding VP-HIT-LOCKED-01 also identified hidden targets on a selected, locked profile. Repair batch 1 aligns local and mirrored hit contexts with locked handle visibility and adds an application pointer regression; actual locked-body blocking stays intact. Evidence and cumulative attempt history are on PR #461 and in the evidence document.
- Regression tests and evidence: validation only.
- Use QUANTICK_FRVP_DEMO and QUANTICK_POINTER with isolated stores; extend an existing hook only if a changed state cannot already be reached.

## Not applicable
- New-extension/fake second implementation: no new capability, port, tool, feed or layer; repair the existing DrawingToolImpl hit-test implementation.
- New action act/read/discover: no new action. Existing registered drawing commands and profile move/resize remain available.
- Engine golden/test-first gate: no engine behavior changes.
- MT5/Python/hook-shell checks and cargo-deny: not applicable unless those inputs change.
- Popup/disabled-state redesign: no changed popup or disabled control; inspect existing empty-profile behavior and selected context bar for regressions.

## Closing steps
- [ ] **C1** Full independent delivery-review PASS published. Source: mission high tier → PR report.
- [ ] **C2** Matching open non-draft PR with all final-head CI checks green and current evidence/review URLs. Source: mission completion → PR.
- [ ] **C3** Shared mission_ship_gate final verifier PASS with literal D1-D8 reconciliation. Source: mission completion → PR receipt.
- Only the user merges to main.

## Original request (verbatim, attributed)
User:
> $mission high deixar opção de mover desenho de volume profile (mudando o mouse com opção para cliar no desenho) somente na parte que o volume profile é desenhado. Hoje clicando em area perto do voluem profile faz com que o mouse clique no volume profile. E as vezes eu quero apenas clicar no grafico atras para mover. Isso dificulta as vezes clicar fora ou manejar o grafico atras pq toda hora clica no volume profile. Para clicar no volume profile precisa ser bem cirurigco... bem onde ocupa espaço ou nas suas linhas e bordas.
