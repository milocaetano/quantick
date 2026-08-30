# Mission — the delivery review gate

**Objective:** Make the task workflow prove that what shipped is what was
asked — `mission` interrogates the request and writes a traceable acceptance
checklist into `.claude/GOAL.md`, and a new independent `delivery-review`
skill grades the finished branch against that checklist before `pr-gate` will
let a PR open.

Why it matters: today `mission` writes the checklist *and* ticks its own
boxes. `arch-review` grades shape, its step 0 grades bugs, and neither one
ever asks "is this what the trader asked for?". A request carrying eight asks
is paraphrased into six criteria, and the two that fell out of the paraphrase
are invisible from that moment on. The trader then has to be the missing
reviewer, by hand, every time.

## Request ledger

The trader's request is the source of record. Under `CLAUDE.md`'s quotation
exemption the fragments below stay in the trader's own words — translating a
request is how a request gets lost, which is the exact failure this mission
exists to fix. Every line's operative statement is English; the quote is the
evidence it was read correctly.

Each `R` line names one atomic ask and the criteria that discharge it. An `R`
with no criterion is a hole, and `delivery-review` reports it as one.

| ID | The ask | Verbatim fragment (trader, 2026-08-30) | Discharged by |
| --- | --- | --- | --- |
| R1 | A reviewer that grades delivered against requested must exist; there is none today | "não tem um revisor. Eu acho que precisa ter talvez um, mas que revisa se o que foi entregue está [100]% compatível com o que foi pedido" | A4, A5 |
| R2 | The comparison runs at the end, before go-live, item by item | "no final, antes de dar o go-live finalizado, tem que ter uma comparação, um checklist" | A4, A7 |
| R3 | `mission` is the author of that checklist | "Esse papel do checklist, inicialmente, quem tem que montar é o mission" | A1, A3 |
| R4 | The checklist must be well-formed enough for another skill to review it | "A skill mission tem que ter um checklist bonitinho para que, no futuro, outra skill revise o que a Mission montou" | A3, A4 |
| R5 | `mission` must question more than it does today | "a mission Precisa também questionar mais" | A2 |
| R6 | Doubt, double meaning and self-contradiction in the request must generate questions | "se tiver algo duvidoso, gera questão: que eu possa duvidar ou minha palavra gera alguma questão, como se eu falo de duplo sentido" | A2 |
| R7 | Decisions that belong to the trader must be surfaced as questions, not decided quietly | "perguntar algumas coisas que eu possa achar que precisam de mim, que precisariam de uma tomada de decisão minha" | A2 |
| R8 | `GOAL.md` must carry objectives that correctly reflect what was requested | "ela precisa montar, dentro do goal, os objetivos corretos com base no que eu solicitei" | A1, A3, A9, A10 |
| R9 | The named failure mode — many asks in one request, only partly met — must be structurally prevented, not merely discouraged | "quando eu peço bastante coisas relacionadas à mudança, no final do processo essas coisas não são atendidas. Eles atendem parcialmente" | A1, A4, A6, A7, A10 |
| R10 | The point of all of it: more quality with the human out of the confirmation loop | "sem precisar de um humano toda hora ficar confirmando para ver se fez e funcionou tudo mais" | A5, A6, A7 |

## Decisions taken by the trader

Asked and answered before any work started, in this session's interrogation
round. These are settled; re-opening one is a scope change, not a judgement
call.

- **D1 — Enforcement.** A separate `delivery-review` skill with its own
  `delivery-review-ok` marker, and `pr-gate` denying `gh pr create` until it
  matches HEAD. Not a ninth `arch-review` dimension, not a report-only note.
- **D2 — Independence.** The reviewer runs as a fresh-context subagent seeing
  only `GOAL.md`, the diff and the evidence written to files. It never
  receives the implementing session's narrative.
- **D3 — Questioning cadence.** One mandatory interrogation round before work
  starts; afterwards the mission proceeds under assumptions written into
  `GOAL.md`, which the reviewer audits.
- **D4 — Gap handling.** Fix and re-review autonomously, bounded to three
  rounds, then escalate. A gap ships only as a deferral the trader approved.

## Assumptions

Recorded rather than asked, because each has a conventional answer and a cheap
reversal. `delivery-review` audits this list too: an assumption that turned
out to drive the design is a question that should have been asked.

- **S1** — Three rounds is the right bound for D4. A single number in the
  skill makes it one edit to change if it proves wrong.
- **S2** — Prose-only skill files keep `arch-review`'s existing docs waiver for
  shape dimensions 1–7. `guardrails.sh` does not: it is executable logic with
  its own test file, so it gets the full shape pass.
- **S3** — The archived `GOAL-archive-*.md` files are not retrofitted to the
  new format. They are records of finished work; rewriting them would falsify
  what those missions actually promised.
- **S4** — `delivery-review` grades the branch against the request, never the
  request against good sense. "Did the trader get what they asked for" is its
  whole job; "is this a good idea" stays the trader's.

## Acceptance criteria

Each line is one observable outcome, the kind of evidence that proves it, and
where that evidence gets written. A criterion whose evidence exists only as a
claim in the session transcript is **UNPROVEN**, not met.

### Mission-specific

- [ ] **A1** — `mission` produces a request ledger: every distinct ask in the
      trader's request appears as a numbered `R` line in `.claude/GOAL.md`,
      each mapped to at least one acceptance criterion, with the verbatim
      fragment kept where the wording carries the ambiguity.
      *Evidence:* the ledger section of the skill's written template, and this
      very file carrying `R1`–`R10`. → `.claude/skills/mission/SKILL.md`,
      `.claude/GOAL-archive-delivery-review-gate.md`. *(R3, R8, R9)*
- [ ] **A2** — `mission` runs one mandatory interrogation round before any
      work, covering ambiguity, double meaning, contradiction between asks,
      silent narrowing, and calls that are the trader's — and the skill names
      what does **not** qualify, so the round stays short. Anything not asked
      becomes a written assumption.
      *Evidence:* the step's text, including its negative list.
      → `.claude/skills/mission/SKILL.md`. *(R5, R6, R7)*
- [ ] **A3** — The checklist format is specified, not improvised: stable IDs,
      one observable outcome per line, the evidence kind, the path the
      evidence lands at, and the ledger back-reference. A copyable template
      lives in the skill.
      *Evidence:* the template block. → `.claude/skills/mission/SKILL.md`.
      *(R3, R4, R8)*
- [ ] **A4** — A new `delivery-review` skill grades every ledger line and every
      criterion as DELIVERED / PARTIAL / MISSING / UNPROVEN, and carries an
      explicit anti-rubber-stamp rule: evidence that exists only as a claim in
      chat is UNPROVEN, and "the code looks right" is not evidence.
      *Evidence:* the skill file, its verdict table and its refusal rules.
      → `.claude/skills/delivery-review/SKILL.md`. *(R1, R2, R4, R9)*
- [ ] **A5** — The reviewer runs in a fresh-context subagent: the skill states
      the exact inputs it may receive (`GOAL.md`, the diff, evidence files) and
      forbids handing it the implementing session's narrative.
      *Evidence:* the dispatch section naming inputs and the prohibition.
      → `.claude/skills/delivery-review/SKILL.md`. *(R1, R10)*
- [ ] **A6** — Gap handling is bounded and autonomous: fix, re-review, at most
      three rounds, then escalate to the trader. The only way to ship a gap is
      a deferral the trader approved, recorded in both `GOAL.md` and the PR
      body.
      *Evidence:* the loop section with the round bound and the deferral rule.
      → `.claude/skills/delivery-review/SKILL.md`. *(R9, R10)*
- [ ] **A7** — `pr-gate` denies `gh pr create` until **both** `arch-review-ok`
      and `delivery-review-ok` match the exact HEAD, and the denial names which
      marker is missing or stale.
      *Evidence:* passing cases in the hook's own test file, run and pasted.
      → `.claude/hooks/guardrails.sh`, `.claude/hooks/guardrails_test.sh`.
      *(R2, R9, R10)*
- [ ] **A8** — `CLAUDE.md` and the `ship` skill name the new gate, so the
      requirement does not live only inside `mission`.
      *Evidence:* the workflow bullet and the ship step.
      → `CLAUDE.md`, `.claude/skills/ship/SKILL.md`. *(R2)*
- [ ] **A9** — This mission dogfoods its own mechanism: this file is written in
      the new format that `mission` now documents — request ledger, decisions,
      assumptions, criteria, closing steps, verbatim request.
      *Evidence:* this file, read against the format spec in the skill.
      → `.claude/GOAL-archive-delivery-review-gate.md`. *(R8)*

      The other half of the dogfood — `delivery-review` actually grading this
      branch, and its verdict reaching the PR body — is `C1`/`C4` below, not a
      criterion. It cannot be observed at the moment the reviewer writes its
      verdict, which is precisely the rule this branch adds at
      `mission/SKILL.md`'s *Closing steps are not criteria*. Written as an `A`
      it would have failed every time, including this one.

- [ ] **A10** — `GOAL.md` carries the trader's request quoted in full and
      verbatim, as its last section, so `delivery-review` can re-derive the asks
      itself and report any the ledger failed to carry. Without it the reviewer
      grades the mission's own summary and the gate is self-referential.
      *Evidence:* the step-5 rule in the skill, the completeness pass in the
      reviewer, and the section at the foot of this file.
      → `.claude/skills/mission/SKILL.md`,
      `.claude/skills/delivery-review/SKILL.md`,
      `.claude/GOAL-archive-delivery-review-gate.md`. *(R8, R9)*

### Standard gates

- [ ] **G1** — English throughout, per `CLAUDE.md`. `language_guard` scans
      `.claude/skills` and `.claude/hooks`, but only the extensions in its
      `SCANNED_EXTS` — `rs`, `pine`, `md`, `html`, `toml`. **`.sh` is not among
      them**, so this branch's only executable file, `guardrails.sh`, has no
      mechanical cover at all and its prose is read by eye. Say both halves
      rather than one: a guard that runs is not a guard that looked. The
      verbatim fragments in the request ledger are marked, attributed
      quotations, and `GOAL-archive-*.md` is outside the guard's scope by
      design.
      *Evidence:* `language_guard` output at the shipped HEAD, plus a stated
      reading of the shell script and the branch/commit prose the guard cannot
      see. → `scratchpad/delivery-review/cargo-test.log`, `scratchpad/delivery-review/checks.md`.
- [ ] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test
      --workspace`, each run separately, all green on a branch rebased on
      latest `main`.
      *Evidence:* the four exit codes, each naming the HEAD they were run
      against. → `scratchpad/delivery-review/checks.md`, `scratchpad/delivery-review/cargo-test.log`.
- [ ] **G3** — `sh .claude/hooks/guardrails_test.sh` green. Cargo cannot see
      it; CI runs it as its own step, so it is run locally too.
      *Evidence:* the test script's own summary line, plus the by-hand gate
      probe. → `scratchpad/delivery-review/guardrails_test.log`, `scratchpad/delivery-review/gate-probe.log`.
- [ ] **G4** — Performance impact declared. The honest class is **per shell
      tool call**, not "rare": `pr-gate` and `commit-reminder` both run on every
      `Bash` and `PowerShell` call, so whatever they do before deciding "not my
      business" is paid on every command a session issues. That is why the
      command parser was replaced by a substring test behind a `case` on the
      program's first word — the parser it replaced measured about 34 ms per
      call, twice per command. Past that early exit the work is rare: two `git
      rev-parse` calls and two small file reads, only when the command actually
      names the gated program. Nothing in this branch executes in the product.
      *Evidence:* the rate class stated with the reason no numbers are
      required. → `scratchpad/delivery-review/checks.md`.
- [ ] **G5** — `arch-review` run over `git diff origin/main...HEAD` (the remote
      ref, per the rule this branch adds to CLAUDE.md), every Blocker and
      Should-fix resolved or deferred in the PR body. Shape dimensions 1–7 are
      waived for the prose-only skill files and **not** for `guardrails.sh`
      (S2). Step 0 and dimension 8 are never waived.
      *Evidence:* the review's six-line verdict and its step 0 header line,
      naming the sha it graded and post-dating that commit.
      → `scratchpad/delivery-review/arch-review-verdict.md`.

### Closing steps — not graded, and deliberately so

These happen after `delivery-review` writes its verdict, and two of them are
unblocked by it. Written as criteria they would fail every mission on the day
the gate shipped, so they live here instead.

- [ ] **C1** — `delivery-review` returns PASS over this checklist.
- [ ] **C2** — `.claude/GOAL.md` archived to
      `.claude/GOAL-archive-delivery-review-gate.md` and committed, *before*
      either review runs, so both markers record the branch the reviews saw.
- [ ] **C3** — PR opened with the evidence in its body. Merging is the trader's
      call and is not part of this mission.
- [ ] **C4** — `delivery-review`'s verdict pasted into the PR body, so the next
      reader sees what it graded and what it refused.

### Not applicable, and why

- `ui-harness`, `visual-qa`, `trader-ux-review` — the change adds no
  user-visible application surface. Nothing launches, nothing paints.
- Engine / determinism — untouched; no crate under `crates/` changes
  behaviour.
- `new-extension` — the new capability is a skill and a hook mode, not a port
  in the product, so the skill genuinely does not apply. Note what this
  exclusion does *not* claim: the change to `guardrails.sh` is a rewrite of the
  gate's command parsing across five functions, not a registration-only edit to
  its `case` statement. Adding a *mode* would be registration-only; this branch
  did more than that, and saying otherwise would be the kind of tidy
  justification `delivery-review` exists to catch.

## Known limitation, stated up front

`.claude/settings.json` invokes the hook from `${CLAUDE_PROJECT_DIR}` — the
main checkout — so this branch's edit to `guardrails.sh` does not arm the new
gate for this branch's own PR. The gate arms for every session started after
the merge. This branch therefore proves the behaviour with
`guardrails_test.sh` and runs `delivery-review` voluntarily, which is A9.

## Deferral requested — NOT granted

One gap, recorded here rather than quietly counted as met. `delivery-review`
found it and this session cannot grant it: only the trader can.

The heading deliberately does not read `## Deferred`. That token is reserved,
in `delivery-review`'s own text, for a deferral the trader **has** approved —
so using it here would let a later reader, or a later reviewer keying on the
heading, read an approval that was never given.

- **G2, the fourth check.** `cargo fmt`, `cargo clippy` and `cargo build` exit
  0 on a branch now rebased on `origin/main`. `cargo test --workspace` exits
  101 on one test, `quantick-feed-mt5 :: the_bridge_paging_tests_pass`, which
  shells out to `python3` — on this machine the Microsoft Store alias, not an
  interpreter. The suite it wraps passes under the real one (*all checks passed
  across 31 tests*, exit 0), this branch changes no Python and no Rust, and CI
  has a real `python3`. So the criterion's *intent* is met and its *letter* is
  not, on this machine, for a reason outside the branch.

  Stated rather than smoothed over, because "all four green" was the promise
  and 101 is not 0. The confirming evidence is the PR's own CI run, which does
  not exist while this is being written — the same ordering problem that put
  `delivery-review` and the PR itself among the closing steps.

  **The trader's call**: accept this as environmental and let CI be the
  authority, or treat the criterion as unmet until a machine with a real
  `python3` runs it.

## The request as received

Quoted in full and verbatim, in the trader's own words, as the section
`delivery-review`'s completeness pass reads. Under `CLAUDE.md`'s quotation
exemption this is the one place the mission record is not translated:
paraphrasing the request here would defeat the only pass that can catch an ask
the ledger above failed to carry.

> É preciso melhorar o nosso fluxo de criação de tarefinhas. Hoje quando eu
> quero criar alguma coisa, eu crio uma missão e, teoricamente, essa missão é
> feita do começo ao fim. O processo cria um objetivo, mapeia algumas coisas e
> tudo mais. Então eu vejo que esse processo precisa dar uma melhorada porque,
> quando eu peço bastante coisas relacionadas à mudança, no final do processo
> essas coisas não são atendidas. Eles atendem parcialmente. Hoje a gente,
> teoricamente, tem um fluxo: tem um arc reviewer que vai verificar a questão
> de arquitetura e tudo mais e temos aí o code reviewer, que é meio revisão de
> código padrão. Mas não tem um revisor. Eu acho que precisa ter talvez um, mas
> que revisa se o que foi entregue está 5% compatível com o que foi pedido. A
> minha ideia seria que, no final, antes de dar o go-live finalizado, tem que
> ter uma comparação, um checklist assim: "Isso foi entregue, isso foi
> entregue, isso foi entregue." Esse papel do checklist, inicialmente, quem tem
> que montar é o mission. A skill mission tem que ter um checklist bonitinho
> para que, no futuro, outra skill revise o que a Mission montou. Eu acho que
> também a mission Precisa também questionar mais porque ela dá um
> questionamento às vezes. Mas eu acho que, se tiver algo duvidoso, gera
> questão: que eu possa duvidar ou minha palavra gera alguma questão, como se
> eu falo de duplo sentido, em palavras que não se contradizem. Então eu acho
> que a missão tem que questionar algumas coisas e perguntar algumas coisas que
> eu possa achar que precisam de mim, que precisariam de uma tomada de decisão
> minha. E ela precisa montar, dentro do goal, os objetivos corretos com base
> no que eu solicitei. O objetivo dessa missão aqui é melhorar nosso processo
> de criação de tarefas para que a gente consiga produzir com mais qualidade,
> sem precisar de um humano toda hora ficar confirmando para ver se fez e
> funcionou tudo mais.

Two readings the ledger fixed rather than inherited, recorded here so the
completeness pass does not re-report them as drift:

- "está 5% compatível" is read as **100%** — a transcription artefact. The
  sentence asks for a reviewer of compatibility with the request, and a 5%
  bar would ask for the opposite of everything around it. `R1` states it as
  100%, and the bracket in the fragment marks the correction rather than
  hiding it.
- "em palavras que não se contradizem" is read as its opposite — the trader is
  naming words that *do* contradict, alongside double meaning, as the thing
  that should raise a question. `R6` carries both readings' operative
  content, and `mission`'s interrogation step lists contradiction and double
  meaning as separate triggers, so nothing depends on which reading is right.
