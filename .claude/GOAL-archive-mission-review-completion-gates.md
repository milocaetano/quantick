# Mission: enforce complete review gates

Harden Quantick mission completion and shipping so only the current PR head with green CI, valid applicable reviews, a durable AI-review report, valid AI-review completion, zero unresolved AI-review threads, and literal `What done means` reconciliation can be declared complete.

This matters because PR #306 was presented as ready after architecture review, delivery review, and green CI while AI review was absent, a review marker had been written manually, and five required threads remained unresolved.

**Tier:** `high` — this changes the workflow's completion authority, private review evidence, GitHub evidence, guardrail scripts, tests, and canonical instructions. It therefore needs the full source-first preflight, full architecture and delivery reviews, and current-head AI review.

## Request ledger

- **R1** — Audit the current `mission`, `ship`, `arch-review`, `delivery-review`, `ai-review`, `pr-gate`, and guardrail workflow and map each readiness bypass to one mechanical owner. Source: “Investigar o workflow atual de mission, ship, arch-review, delivery-review, ai-review, pr-gate e guardrails.”
- **R2** — Make every mission tier explicitly declare four AI-review obligations in its goal record: execution, a durable PR report, zero unresolved AI-review threads, and valid completion evidence for the current review. Source: “em qualquer tier” and the four required bullets.
- **R3** — Mechanically reject readiness when AI completion is absent or stale, AI-review threads remain open, or applicable architecture or delivery evidence is invalid, including an already-ready PR that bypassed the draft transition. Source: “Impedir que uma PR seja considerada pronta quando...”
- **R4** — Apply the same current-head validation at mission completion and ship completion, independently of `gh pr ready`, GitHub `MERGEABLE`, or green CI alone. Source: “também aconteça no encerramento de `$mission` e em `$ship`” and “Não declare conclusão apenas porque GitHub informa MERGEABLE ou porque o CI está verde.”
- **R5** — Prevent or detect hand-written review markers by requiring durable, review-specific evidence bound to the current branch, head, base, and review key instead of trusting marker-file existence. Source: “Impedir ou detectar marcadores gravados manualmente...”
- **R6** — Require a literal, clause-by-clause reconciliation against the canonical `What done means` section before mission completion. Source: “Exigir uma reconciliação literal contra ‘What done means’...”
- **R7** — Add exact PR #306 regressions: the PR is already non-draft, CI is green and mergeable, architecture and delivery evidence are present, and either AI completion is absent or five AI-review threads are open; both mission and ship completion must refuse. Source: objective 7.
- **R8** — Preserve valid behavior for every tier, with architecture and AI review required throughout and only the bounded `small` delivery-review exemption retained. Source: “Preservar o fluxo válido para todos os tiers...” and “Não trate arch-review ou delivery-review como substitutos de ai-review.”
- **R9** — Keep all repository artifacts in English and update one canonical workflow plus thin Claude/Codex mappings without duplicated or divergent rules. Source: objective 9 and the final language constraint.

## Decisions

No user decision was required. The request fixes the tier, failure modes, trust boundary, regression fixtures, authority boundary, and final delivery outcome.

## Assumptions

- **S1** — The durable-report requirement applies at minimum to AI review, while architecture and delivery reviews also need machine-verifiable durable receipts because otherwise a manually written marker for either skill remains indistinguishable from valid execution. This is the narrow mechanical reading of R5.
- **S2** — An already-ready PR is not inherently invalid. It may complete only after the same final verifier succeeds without relying on a draft-to-ready transition. The independent source-first preflight confirmed this reading.
- **S3** — Goal files declare the four AI-review obligations and their evidence destinations before archival; final execution evidence lives on the PR and in private projections, because editing the archive after review would stale the reviews it is meant to prove.
- **S4** — A shared command-line verifier is the mechanical owner for mission and ship completion. Canonical skills must invoke it; host wrappers remain thin pointers to that owner.

## Acceptance criteria

- [x] **A1** — The implementation and evidence identify the current behavior of all seven workflow components and map every PR #306 bypass to a single mechanical owner.
      *Evidence:* an investigation table with before/after ownership and code references.
      → `docs/workflow/evidence/mission-review-completion-gates.md`. *(R1)*
- [x] **A2** — The canonical mission workflow makes every tier's goal record explicitly enumerate AI review execution, a durable PR report, zero unresolved AI-review threads, and valid `ai-review-complete` evidence for the current review key.
      *Evidence:* canonical skill text plus a regression that checks all four obligations and all tiers.
      → `.claude/skills/mission/SKILL.md`; `.claude/hooks/guardrails_test.sh`. *(R2)*
- [x] **A3** — One mechanical final-completion verifier used by both mission and ship rejects missing or stale AI completion, open AI-review threads, invalid applicable architecture or delivery evidence, and head/PR identity mismatches even when GitHub reports the already-ready PR green and mergeable.
      *Evidence:* executable verifier and hermetic regression cases for both completion callers.
      → `.claude/hooks/mission_ship_gate.sh`; `.claude/hooks/guardrails_test.sh`. *(R3, R4)*
- [x] **A4** — A local review marker is insufficient by itself: readiness and completion require a review-specific durable PR receipt bound to the current branch, head, base, base tip, and review key, and only each review skill's producer command records its projection after publication succeeds.
      *Evidence:* report producer/validator implementation, negative fabricated-marker tests, and canonical review-skill producer instructions.
      → `.claude/hooks/review_report.sh`; `.claude/hooks/guardrails.sh`; `.claude/hooks/guardrails_test.sh`; `.claude/skills/{arch-review,delivery-review,ai-review}/SKILL.md`. *(R5)*
- [x] **A5** — Completion reads the canonical `What done means` clauses literally, reports a clause-by-clause reconciliation for the current head, and refuses an unknown, missing, or unmet clause.
      *Evidence:* canonical clause IDs, verifier output/receipt, and mutation regression coverage.
      → `.claude/skills/mission/SKILL.md`; `.claude/hooks/mission_ship_gate.sh`; `.claude/hooks/guardrails_test.sh`. *(R6)*
- [x] **A6** — Regression fixtures reproduce both PR #306 variants exactly: an already non-draft, green, mergeable PR with current architecture and delivery evidence is rejected when AI completion is absent and when five AI-review threads remain open, for both mission and ship completion.
      *Evidence:* named shell-suite cases that fail when the completion checks are removed.
      → `.claude/hooks/guardrails_test.sh`. *(R7)*
- [x] **A7** — All four tiers keep current valid flow: every tier requires architecture and AI evidence; `medium`, `high`, and `max` require delivery evidence; `small` retains only its existing bounded delivery-review exemption.
      *Evidence:* positive and negative tier matrix tests through both readiness and final completion.
      → `.claude/hooks/guardrails_test.sh`. *(R8)*
- [x] **A8** — Canonical workflow documentation and thin Claude/Codex mappings point to the shared mechanism, remain English, and do not duplicate completion rules or marker-writing recipes.
      *Evidence:* link/drift assertions and architecture review language verdict.
      → `.claude/hooks/README.md`; `.claude/skills/{mission,ship,arch-review,delivery-review,ai-review}/SKILL.md`; `.agents/references/codex-compatibility.md`; `.agents/skills/{mission,ship}/SKILL.md`. *(R9)*
- [x] **A9** — Draft and already-ready delivery paths are both covered: only the draft path calls `gh pr ready`, while both paths must pass the same final verifier before mission or ship completion can be reported.
      *Evidence:* positive/negative path tests and canonical ship sequencing.
      → `.claude/hooks/guardrails_test.sh`; `.claude/skills/ship/SKILL.md`. *(R3, R4)*

## Injected gates

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

- [x] **G1** — Every authored repository artifact is English except the attributed verbatim request below.
      *Evidence:* language guard plus direct architecture-review inspection.
      → final `cargo test --workspace` log and architecture review report.
- [ ] **G2** — Final-head Rust verification passes in order: format check, workspace Clippy, workspace build, and workspace tests.
      *Evidence:* exact command outputs bound to the final commit and final CI run.
      → external validation dossier indexed by `docs/workflow/evidence/mission-review-completion-gates.md` and PR checks.
- [x] **G3** — The touched paths are classified as rare workflow operations; no per-trade, per-depth, or per-frame path changes, and no hot-path measurement is required.
      *Evidence:* path/rate table reviewed by architecture review.
      → `docs/workflow/evidence/mission-review-completion-gates.md` and PR body.
- [x] **G4** — The guardrail shell suite passes and its mutation/regression coverage detects removal of the new completion behavior.
      *Evidence:* `sh .claude/hooks/guardrails_test.sh` exits zero and named mutation checks pass.
      → external validation dossier indexed by `docs/workflow/evidence/mission-review-completion-gates.md`.
- [ ] **G5** — Architecture review, including its medium-effort bug pass for tier `high`, publishes a durable current-review report and resolves or validly defers every Blocker and Should-fix.
      *Evidence:* PR report URL, zero unresolved required findings, and producer-recorded current marker.
      → PR body completion evidence.
- [ ] **G6** — AI review publishes its durable current-review report, records valid current `ai-review-complete`, and leaves zero unresolved AI-review threads.
      *Evidence:* PR report URL, producer-recorded current marker, and `ai_review_threads.sh list` output.
      → PR body completion evidence.
- [ ] **G7** — Delivery review runs last and returns PASS for the final review key; the tier `high` mission is not exempt.
      *Evidence:* PR report URL, PASS verdict, and producer-recorded current marker.
      → PR body completion evidence.
- [ ] **G8** — The work remains in the isolated clean task worktree, preserves all pre-existing checkout/worktree changes, requests no routine approval, and leaves main merging exclusively to the user.
      *Evidence:* worktree/status records and final handoff without a merge action.
      → external validation dossier and PR body.

## Not applicable

- UI harness, visual QA, and trader UX review do not apply because no trader-visible application surface changes.
- Extension docking does not apply because no feed, bar type, indicator, layer, panel, capability, or crate is added.
- Engine determinism fixtures do not apply because no engine or market-data path changes.
- Hot-path measurement does not apply because every touched path runs only during rare repository delivery operations.
- MetaTrader Python lint and bridge/exporter tests do not apply unless the implementation unexpectedly touches `tools/mt5/` or `bridge/mt5/`.
- `cargo deny check bans licenses` does not apply unless `Cargo.lock` changes.

## Closing steps

- [ ] **C1** — Archive this goal as the branch's last repository commit before final reviews.
- [ ] **C2** — Push the branch and open or reuse a linked draft PR for issue #389 with the tier and verification provenance in its English body.
- [ ] **C3** — Obtain current architecture, AI, and delivery verdicts through their producer commands; never write review markers manually.
- [ ] **C4** — Observe all required CI green at the exact PR head with no missing or pending required check.
- [ ] **C5** — If the PR is draft, run `gh pr ready <pr>` only after readiness gates pass; if it is already ready, do not use that transition as evidence.
- [ ] **C6** — Run the shared mission completion verifier, publish its literal `What done means` reconciliation for the current head, and verify the receipt before reporting completion.
- [ ] **C7** — Report the human-review-ready PR URL; do not merge it to `main`.

## Request as received

The following is an attributed verbatim quotation from the trader and is retained under the repository language-rule exemption:

> `$mission high Corrigir o workflow de missões do Quantick para impedir que uma PR seja declarada pronta para merge sem`
> `  completar todos os gates obrigatórios.`
>
> `  Contexto do defeito observado:`
>
> `  Na PR #306, uma missão foi encerrada após arch-review, delivery-review e CI verde, mas o ai-review não foi executado.`
> `  O agente também gravou manualmente o marcador arch-review-ok e declarou a PR pronta, embora não existisse ai-review-`
> `  complete e posteriormente tenham sido encontrados cinco threads não resolvidos.`
>
> `  Objetivos obrigatórios:`
>
> `  1. Investigar o workflow atual de mission, ship, arch-review, delivery-review, ai-review, pr-gate e guardrails.`
> `  2. Fazer com que toda mission, em qualquer tier, registre explicitamente no GOAL.md:`
> `     - ai-review executado;`
> `     - relatório durável publicado na PR;`
> `     - zero threads de AI review não resolvidos;`
> `     - ai-review-complete válido para a revisão atual.`
> `  3. Impedir que uma PR seja considerada pronta quando:`
> `     - ai-review-complete estiver ausente ou stale;`
> `     - houver threads abertas retornadas por ai_review_threads.sh list;`
> `     - arch-review-ok ou delivery-review-ok forem inválidos quando aplicáveis;`
> `     - a PR já estiver fora de draft e, portanto, não passar por gh pr ready.`
> `  4. Garantir que a validação também aconteça no encerramento de $mission e em $ship, não apenas na transição de draft`
> `     para ready.`
> `  5. Impedir ou detectar marcadores gravados manualmente sem a execução válida da skill correspondente. Não confiar`
> `     somente na existência do arquivo.`
> `  6. Exigir uma reconciliação literal contra “What done means” antes de uma missão poder ser concluída.`
> `  7. Adicionar testes de regressão que reproduzam exatamente o caso da PR #306:`
> `     - PR já não é draft;`
> `     - CI verde e mergeable;`
> `     - arch-review e delivery-review presentes;`
> `     - ai-review ausente ou com threads abertas;`
> `     - resultado obrigatório: missão/ship recusam prontidão.`
> `  8. Preservar o fluxo válido para todos os tiers, incluindo as exceções legítimas de delivery-review no tier small.`
> `  9. Atualizar a documentação canônica e os wrappers Codex/Claude sem criar regras duplicadas ou divergentes.`
> `  10. Executar os testes dos guardrails e todos os gates aplicáveis, fazer as revisões obrigatórias — incluindo ai-`
> `  review — e abrir uma PR pronta para revisão humana.`
>
> `  Restrições:`
>
> `  - Não trate arch-review ou delivery-review como substitutos de ai-review.`
> `  - Não grave marcadores manualmente para satisfazer gates.`
> `  - Não declare conclusão apenas porque GitHub informa MERGEABLE ou porque o CI está verde.`
> `  - Não peça permissão para etapas rotineiras.`
> `  - Use worktree limpa e preserve qualquer worktree existente com alterações.`
> `  - A solução deve ser mecanicamente aplicável; não pode depender apenas de instruções para o agente “lembrar”.`
> `  - Todo texto e código no repositório deve permanecer em inglês.`
>
> `  Critério final de sucesso:`
>
> `  Uma missão só pode ser apresentada como concluída quando o workflow comprovar, para a cabeça atual da PR, CI verde,`
> `  reviews aplicáveis válidos, ai-review-complete válido, relatório publicado e zero threads de AI review abertas. O`
> `  cenário regressivo baseado na PR #306 deve falhar antes da correção e passar depois dela.`
