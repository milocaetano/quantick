# Mission — a leaner agentic flow, without lifting the modularity floor

**Tier:** `high`. The deliverable *is* a decision about how much review to give
up, on every future branch. No money and no order flow is at stake, but code
quality on all subsequent work is, and a wrong guess throws the whole branch
away rather than one edit. A `high` mission that buys cheaper missions forever
is the right trade; the irony of paying full ceremony to reduce ceremony is
noted and accepted.

## Objective

Cut the token cost and the review-chain length of a `/mission` in this
repository, and replace the part of the modularity discipline that a shorter
review would have carried with a mechanical guard that costs a second.

## Why it matters

`crates/app` is 192,800 of the repository's 266,968 lines — **72% of the code
in one crate**, with `app.rs` alone at 33,954 lines. The thing `CLAUDE.md`
forbids has already happened. So the flow cannot simply be made cheaper: the
discipline meant to stop that number growing has to move somewhere cheaper *at
the same time*, or the saving is paid for out of the one property the trader
named as non-negotiable.

The predecessor branch `refactor/mission-review-throughput` (merged as #272)
measured the flow with five agents and refuted the obvious diagnosis: ordinary
code branches average **~1 review-fix commit**, while the branches that burn
six rounds are the ones with **zero production lines** — meta-work on the
workflow itself. It armed the edit-time guard hook and named the fast build
loop. It left one criterion explicitly undischarged and carried forward:
`delivery-review` still starts at its most expensive shape, and fixing that
"is its own branch, not a paragraph bolted onto this one". This is that branch.

It also, in the same commit, raised `app.rs` from 9,775 to 9,890 production
lines with a signed comment and extracted nothing in return. That is the
ratchet working exactly as designed and still losing ground, which is the
trader's question made concrete.

## Request ledger

- **R1** — analyse the AI development flow this repository runs.
  *"analisar o fluxo de desenvolvimento de IA que temos no Quatick"*
- **R2** — improve delivery agility; ship faster.
  *"agilidade de entrega"*, *"mais velocidae de entrega"*
- **R3** — spend fewer tokens. *"economia de tokens"*
- **R4** — do not degrade the main thing, which is keeping the code modular.
  *"Sem degradar o principal que é manter o codigo modular"*
- **R5** — specifically, no single file such as `app` holding almost every line
  of the project. *"sem encher um unico arquivo como app com quase todas as
  linhas de codigo do projeto"*
- **R6** — shorten the review chain. *"menos cadiea de review"*
- **R7** — and keep this discipline standing while doing all of the above.
  *"manter essa disciplina"* — the statement of purpose, and the one that
  judges every other line.
- **R8** — decide how new development proceeds against files that are *already*
  big, while the refactor happens gradually, so the rule does not block it.
  *"existem arquivos que ja sao grande como app. Como que vamos lidar a partir
  de agora? pq eu vou refratorar aos poucos"*
- **R9** — route models by task size; smaller models for smaller tasks.
  *"Daria para definir modelos para tarefas? Ja vi pessoal entregando modelos
  moenores para tarefas menores"*
- ~~**R10** — evaluate Codex as an external reviewer.~~ **WITHDRAWN** by the
  trader in the same answer that raised it: *"se vc nao sabe melhor deixa
  quieto"*. No measurement exists that it helps in this repository, and a rule
  written from anecdote is the kind this mission is trying to remove. Recorded,
  not delivered, and deliberately not carried forward.

## Decisions taken by the trader

- **D1** — the branch ships analysis **and** the applied changes, as one PR.
- **D2** — every lever is open: trimming the skills, capping review rounds,
  rebalancing tiers and gates, and mechanising review into `crates/guards`.
- **D3** — modularity is protected by **mechanising**: a stronger ratchet, so
  the discipline is a one-second test rather than an expensive reviewer.
- **D4** — apply the decisions in this session without returning for approval;
  the trader reviews the diff on the PR.
- **D5** — Codex is out of scope entirely (see R10).
- **D6** — on the debt files, growth is **pay-as-you-go**: raising a ceiling is
  accepted only when the same branch moves comparable code out. The debt stops
  growing without any branch being blocked, and each feature pays down a slice
  instead of the refactor needing missions of its own.

## Assumptions

- **S1** — "fluxo de desenvolvimento de IA" means this repository's agentic
  workflow — the skills, the hooks and `CLAUDE.md` — and not the product's own
  assistant or the control plane. The sentence pairs it with review chains and
  token spend, which only the workflow has. *Safe to assume:* the alternative
  reading has no review chain to shorten.
- **S2** — *wanted to ask.* Whether "economia de tokens" means per-mission cost
  or total monthly spend. Went with per-mission cost, since that is the number
  a change to the flow can move and the one the trader observes as waiting.
- **S3** — *wanted to ask.* Whether the round cap should discard findings or
  defer them. Went with **defer to a PR follow-up**, never discard: a discarded
  finding is a silent quality loss, which R4 forbids, while a deferred one is
  visible in the PR body and can be argued with.
- **S4** — the pay-as-you-go rule applies to the files already recorded in
  `size-baseline.txt`, not to every file in the repository. A file under the
  threshold has no debt to pay down. *Safe to assume:* the baseline is already
  the repository's own list of debt files.

## Acceptance criteria

- [x] **A1** — the flow is measured, not asserted: where the tokens go and
      where review rounds are actually born, with numbers taken from this
      repository rather than from intuition, and the predecessor's finding
      either confirmed or corrected against fresh data.
      *Evidence:* a written diagnosis carrying per-artifact line counts, the
      review-round distribution across merged PRs, and the crate/file size
      table. → `.claude/evidence/leaner-agentic-flow/diagnosis.md` *(R1)*
- [x] **A2** — subagent work is routed by model, so retrieval and
      checklist-application stop being billed at open-judgement rates, with the
      bug pass left on the strong model and that exception stated.
      *Evidence:* the routing rule quoted from the skills that spawn agents,
      naming which model each kind of subagent takes and why.
      → `.claude/skills/*/SKILL.md` and the diagnosis *(R3, R9)*
- [x] **A3** — `delivery-review` starts at its cheapest shape without losing
      the fresh-context stranger, discharging the criterion #272 withdrew and
      carried forward — including the false-pass hole that reverted it, where
      grading from the files as they stand lets a reviewer quote a sentence
      that was already on `origin/main`.
      *Evidence:* the changed steps, plus a statement of how the false pass is
      prevented in the cheap shape. → `.claude/skills/delivery-review/SKILL.md`
      *(R3, R6)*
- [x] **A4** — the review chain has a stated bound: a branch knows how many
      rounds of findings it owes before the remainder becomes a recorded PR
      follow-up, and the bound is written where the reviewing skill will read
      it rather than remembered.
      *Evidence:* the bound quoted from the skill, and the deferral shape it
      requires. → `.claude/skills/arch-review/SKILL.md` *(R2, R6)*
- [x] **A5** — growth in the debt files is pay-as-you-go and **mechanically**
      enforced, not reviewed: raising a recorded ceiling without moving
      comparable code out fails a check that runs in about a second.
      *Evidence:* a failing case and a passing case, both as tests in the
      guards crate, plus the guard's own message telling an author the two
      honest ways past it. → `crates/guards/` *(R4, R5, R8, D3, D6)*
- [x] **A6** — no new development is blocked by A5: the rule has a stated,
      affordable path for a change that genuinely must add lines to a debt file
      and has nothing to extract, and that path is visible rather than silent.
      *Evidence:* the escape hatch quoted from the guard's documentation, and a
      test proving it works. → `crates/guards/` *(R8)*
- [x] **A7** — the skills that are read in full on every invocation are shorter
      in tokens without losing a rule: every deletion is prose or rationale,
      and the count is reported before and after.
      *Evidence:* per-file line and byte counts before/after, and a statement
      of what class of text was removed. → the diagnosis *(R3, R2)*
- [x] **A8** — the discipline is stated as still standing: a written account of
      what each cut gave up and what now carries it, so R7 can be graded rather
      than believed. *Evidence:* a section mapping every reduction to the
      mechanism that absorbed it. → the diagnosis *(R7, R4)*

### Injected gates

- [ ] **G1** — every artifact in English, per `CLAUDE.md`; graded by
      `arch-review` dimension 8. *Evidence:* the review verdict.
      → `.claude/evidence/leaner-agentic-flow/arch-review.md`
- [x] **G2** — the four checks green after rebasing on latest `main`.
      *Evidence:* the four commands' output, each run on its own.
      → `.claude/evidence/leaner-agentic-flow/checks.log`
- [x] **G3** — performance impact declared. The guards crate is the only
      compiled code changed; classify the added check by rate.
      *Evidence:* the classification, with the guard's measured runtime.
      → the diagnosis
- [ ] **G4** — `arch-review` run with every Blocker and Should-fix resolved or
      deferred in the PR body, at this tier's effort. *Evidence:* the verdict.
      → `.claude/evidence/leaner-agentic-flow/arch-review.md`
- [x] **G5** — the repository guards pass on this branch, including the new
      one against the branch's own diff. *Evidence:* `cargo test -p
      quantick-guards` output. → `.claude/evidence/leaner-agentic-flow/checks.log`

### Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — the PR is open, naming the tier and carrying the evidence.

## Not applicable, and why

- **`ui-harness`, `visual-qa`, `trader-ux-review`** — no user-visible surface
  changes. The only compiled code is `crates/guards`, which has no UI.
- **`new-extension`** — the mission adds a check to an existing guards crate
  rather than a capability a trader reaches. No port is being carved.
- **Engine / determinism** — the engine is not touched.
- **The second operator** — the guard is a build-time check, not an action a
  trader performs; there is nothing for an agent to drive.
- **Hot-path performance evidence** — no per-trade, per-depth or per-frame path
  is touched. G3 still declares the rate, which is the cheap half of the row.

## The request as received

Quoted verbatim and untranslated, per `CLAUDE.md`'s exemption for a marked,
attributed quotation. The words are the trader's own, in Portuguese, on
2026-09-01; translating them would put an interpretation between the reviewer
and the source, which is the one thing this section exists to prevent.
`delivery-review` reads this file and never this conversation, so it re-derives
the asks from here.

> analisar o fluxo de desenvolvimento de IA que temos no Quatick e ver se dá
> para melhorar a agilidade de entrega e economia de tokens. Sem degradar o
> principal que é manter o codigo modular, sem encher um unico arquivo como app
> com quase todas as linhas de codigo do projeto. A meta é ter menos cadiea de
> review, mais velocidae de entrega e manter essa disciplina.

Mid-session, on the same day, raising R8:

> o ponto é existem arquivos que ja sao grande como app. Como que vamos lidar a
> partir de agora?  pq eu vou refratorar aos poucos. Mas enquanto a gnt nao
> refatora isso pode ser um problema para noovs desenvolvimentos que vão ser
> bloqueados na regra

And in the interrogation, raising R9 and R10:

> Daria para definir modelos para tarefas? Ja vi pessoal entregando modelos
> moenores para tarefas menores. Talvez até usar o codex como reviewer caso
> tenha token no codex, o que vc pensa de um workflow dessE?

> nao faço a menor idiea eu sei que tem pessoal que usa review no codex pelo
> proprio claude se vc nao sabe melhor deixa quieto
