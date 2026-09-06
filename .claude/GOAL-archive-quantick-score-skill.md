# Create an evidence-based Quantick scorecard skill

Create a cross-client Quantick assessment skill with explicit, evidence-based scoring rules and measurable targets. This matters because a project-wide quality score must measure sustainable growth, scalability and AI readiness instead of treating a strict diff review as the grade for the whole project.

**Tier:** `small` — this is a bounded docs-and-skills-only change with no runtime, user-interface or product behavior.

## Request ledger

- **R1** — Create a skill that assesses Quantick.
- **R2** — The assessment must produce a numeric grade.
- **R3** — Define clear rules that make the target measurable and attainable.
- **R4** — Keep one canonical workflow usable by both Claude and Codex.

## Assumptions

- **S1** — Use three separately reported dimensions and a weighted 0–10 overall score. This is reversible prose and prevents one aggregate from hiding a weak dimension.
- **S2** — Name the skill `quantick-score`. It is short, project-specific and follows the repository's skill naming convention.
- **S3** — Assessments are read-only and score current evidence rather than plans. This keeps a measurement skill separate from implementation workflows.

## Decisions taken by the trader

- **D1** — Optimize the rubric for Quantick's own long-term quality, sustainable growth, scalability and AI readiness.

## Acceptance criteria

- [x] **A1** — A canonical English `quantick-score` skill assesses sustainable engineering, agentic development and AI-operated product behavior.
      *Evidence:* canonical skill and rubric inspection.
      → `.claude/skills/quantick-score/SKILL.md`, `docs/quality/quantick-score-rubric.md`. *(R1)*
- [x] **A2** — The rubric calculates separate dimension scores and a reproducible weighted overall score from 0.0 to 10.0.
      *Evidence:* formulas, fixed point budgets and grade bands in the rubric.
      → `docs/quality/quantick-score-rubric.md`. *(R2)*
- [x] **A3** — Explicit maturity anchors, evidence rules, A+ gates and target conditions make every deduction and next step explainable.
      *Evidence:* scoring and report contracts in the canonical workflow and rubric.
      → `.claude/skills/quantick-score/SKILL.md`, `docs/quality/quantick-score-rubric.md`. *(R3)*
- [x] **A4** — Codex discovers a thin adapter that delegates to the canonical Claude workflow and the shared compatibility mapping.
      *Evidence:* adapter link validation.
      → `.agents/skills/quantick-score/SKILL.md`. *(R4)*
- [x] **G1** — Every changed artifact is in English.
      *Evidence:* guards language check in the workspace test run.
      → `.claude/evidence/quantick-score-skill/verification.md`.
- [x] **G2** — The four workspace checks pass on the final branch.
      *Evidence:* command transcript with four zero exit codes.
      → `.claude/evidence/quantick-score-skill/four-checks.log`.
- [x] **G3** — Performance impact is declared as rare, assessment-time Markdown loading with no runtime path change.
      *Evidence:* PR body performance note.
      → `.claude/evidence/quantick-score-skill/verification.md`.
- [ ] **G4** — The small-tier bug pass and English review close without unresolved findings.
      *Evidence:* `arch-review` verdict and exact-diff marker.
      → PR body and worktree git metadata.

## Gates that do not apply

- UI harness, visual QA and trader UX review do not apply because no user-visible application surface changes.
- Hot-path measurements do not apply because the diff contains Markdown instructions only and runs at explicit assessment time.
- The extension protocol does not apply because this adds a development skill, not a product capability.
- The second-operator product gate does not apply because the assessment performs no product action.
- Engine test-first and determinism gates do not apply because no engine behavior changes.
- Delivery review is omitted by the `small` tier.

## Closing steps

- **C1** — Archive this mission, pass `arch-review`, open a PR that closes issue #322, and watch CI until green.

## Request as received

The following is an attributed user request and is retained verbatim under the repository's language exemption:

> "$mission small criar uma skill que vai medir e avaliar o quantick e dar nota. Prcisa ter regras mais claras para que a gnte consiga atingir a meta. Isso deve funcionar tanto para claude quanto para codex"
