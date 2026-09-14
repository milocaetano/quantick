# Determine whether drawing copy and paste works on the latest Quantick version

Determine whether Ctrl+C and Ctrl+V copy and paste chart drawings correctly on the latest `origin/main`, so the trader has a current yes-or-no answer backed by automated and live Windows evidence.

**Tier:** small — this is a bounded verification mission with no planned runtime change, no interrogation, and no delivery review.

## Request ledger

- **R1** — Determine whether Ctrl+C captures the selected chart drawing without changing the chart. Source: “Ctrl C”.
- **R2** — Determine whether Ctrl+V creates an independent copy of the captured drawing. Source: “Ctrl V”.
- **R3** — Reconcile the automated shortcut result with the production Windows desktop path. Source: “ta funcionando”.
- **R4** — Run the verification against the latest fetched Quantick `origin/main`. Source: “ultima versao do quantick”.
- **R5** — Launch the verified local build visibly so the trader can exercise the shortcut directly. Source: “roda no lcoal para eu ver se passa”.

## Assumptions

- **S1** — “Ctrl C + Ctrl V” refers to the recently merged chart-drawing clipboard feature, not editable text fields. This is safe because the repository contains a merged feature and explicit UI contract using those exact shortcuts.
- **S2** — “Latest version” means the tip of `origin/main` after `git fetch origin --prune`. This is the repository's conventional current-version reference and requires no product choice.
- **S3** — ~~The focused egui interaction regression is the primary behavioral proof.~~ Withdrawn after the live run: the test bypasses `egui-winit`'s native Ctrl+C/Ctrl+V translation and is not sufficient production-path evidence.
- **S4** — No corrective runtime edit is authorized unless verification fails. A failure would expand the mission beyond bounded verification and require the tier and checklist to be reconsidered first.
- **S5** — The trader's direct gesture in the fresh visible build plus the before/after structured drawing count is decisive for the yes-or-no verdict. This is safe because no synthetic input or pixel inference substitutes for the observed production interaction.

## Acceptance criteria

- [x] **A1** — The evidence gives an observed yes-or-no answer for Ctrl+C on the selected drawing in the real Windows app; the observed answer is FAIL.
      *Evidence:* trader reproduction, unchanged structured drawing count, and native-event root-cause inspection.
      → `docs/quality/copy-paste-latest-verification.md`. *(R1)*
- [x] **A2** — The evidence gives an observed yes-or-no answer for Ctrl+V after Ctrl+C in the real Windows app; the observed answer is FAIL.
      *Evidence:* trader reproduction, `drawings=1` before and after, and native-event root-cause inspection.
      → `docs/quality/copy-paste-latest-verification.md`. *(R2)*
- [x] **A3** — The passing focused tests and failing production Windows run are both recorded, and the false positive is explained by the event-adapter boundary.
      *Evidence:* named test results plus inspected `egui-winit` early returns and Quantick's key-only handler.
      → `docs/quality/copy-paste-latest-verification.md`. *(R3)*
- [x] **A4** — The evidence identifies the fetched `origin/main` commit and proves that the merged copy/paste implementation is reachable from that commit.
      *Evidence:* fetched SHA, merge ancestry, PR merge metadata, and source locations.
      → `docs/quality/copy-paste-latest-verification.md` and the PR body. *(R4)*
- [x] **A5** — A fresh executable from the verified revision is open locally with isolated state, a selected drawing, and healthy live presentation for the trader's direct test.
      *Evidence:* executable timestamp, process identity, launch hooks, health summaries, and trader feedback.
      → `docs/quality/copy-paste-latest-verification.md` and the PR body. *(R5)*
- [x] **G1** — Every authored artifact is in English except the marked, attributed verbatim user quotation allowed by `CLAUDE.md`.
      *Evidence:* Quantick guards and diff inspection.
      → `docs/quality/copy-paste-latest-verification.md` and the PR body.
- [x] **G2** — Prose-only validation is complete: repository guards pass, diff hygiene and changed relative links are checked, and full exact-head CI is green.
      *Evidence:* local command results and registered PR checks.
      → `docs/quality/copy-paste-latest-verification.md` and the PR body.
- [x] **G3** — Performance impact is declared as none: this mission changes only evidence Markdown and does not alter per-trade, per-depth, per-frame, or rare runtime paths.
      *Evidence:* final name/status diff and inspected content.
      → `docs/quality/copy-paste-latest-verification.md` and the PR body.
- [ ] **G4** — `arch-review` runs for the final diff and every Blocker or Should-fix is resolved or explicitly deferred in the PR body.
      *Evidence:* durable current-review report and valid `arch-review-ok` projection.
      → PR discussion and PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

AI review evidence destinations for G-AI1 through G-AI4 were declared before archival; the evidence itself will be published in the PR discussion and summarized in the PR body after review.

## Not applicable

- Runtime code-change gates are not applicable because no runtime, test, fixture, schema, dependency, build configuration, hook, script, or generated input change is planned.
- Hot-path measurement is not applicable because the mission changes no executable path.
- `visual-qa` and `trader-ux-review` are not applicable because no user-visible surface is changed; the existing behavior was exercised in a visible live Windows run and checked against structured health output.
- `new-extension` and second-operator capability gates are not applicable because no capability or trader action is added or changed.
- Engine determinism gates are not applicable because the engine is untouched.
- `delivery-review` is not applicable under the bounded small-tier exemption.

## Closing steps

- [ ] **C1** — A PR is open for the mission branch with current-head evidence and review links in its body.
- [ ] **C2** — `mission_ship_gate.sh mission <pr>` reports PASS at the exact non-draft PR head with green CI.

## Verbatim request

The trader requested:

> $mission small verificar se o Ctrl C + Ctrl V ta funcionando para a ultima versao do quantick?

The trader then requested and reported during the visible local run:

> roda no lcoal para eu ver se passa
>
> acabei de dar contrl c e ctrl v
>
> e nao copiou o desnho que coloquei no grafico
