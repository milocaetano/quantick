# Mission — prove the bug pass ran at the level the tier bought

**Objective:** make `arch-review`'s step 0 provably run the bundled
`code-review` at the effort level the mission's declared tier buys, instead of
reporting whichever level happened to run.

**Tier:** `medium`. The diff is small and almost entirely markdown, which
argues for `small` — but the request carries one decision that is the trader's
and not the implementer's: what `arch-review` does when it discovers the bug
pass ran at the wrong level. Re-invoking buys a second bug pass, which is the
opposite of the goal; aborting stalls the review; warning is what happens today
and did not prevent anything. `medium` buys the two questions that decision
needs, and the completeness pass that checks the answer actually shipped.

**Why it matters:** the tier exists to let the trader buy less review on
purpose. A step 0 that silently runs at a level nobody selected spends the
budget the tier was meant to cap — measured at 192,701 subagent tokens on a
markdown branch — and reports the overspend as though it were the plan.

## Request ledger

| | Ask |
| --- | --- |
| **R1** | The effort argument does not reach `code-review`; step 0 runs at a level the tier did not buy. The defect goes. |
| **R2** | The fix cannot live in `code-review` — it is the bundled skill and does not live in `.claude/skills/`. It lives in *"como o arch-review a chama, ou em como o arch-review verifica o que voltou"*. |
| **R3** | An `arch-review` running under a declared tier must **prove** the bug pass ran at the level that tier buys — *"prove que a passada de bug rodou no nível que aquele tier compra"* — rather than reporting the level that happened to run. |
| **R4** | Today the skill only asks to name the level in the header, which records the error without preventing it. That is not enough. |
| **R5** | Reproduce before fixing: test the invocation forms and establish whether the argument passes at all — *"Comece reproduzindo antes de consertar"*. |
| **R6** | If it cannot pass in any form, the deliverable is `arch-review` handling that honestly — *"não uma prosa dizendo que passa"*. |
| **R7** | Decide, with the trader, what `arch-review` does on discovering it ran at the wrong level: re-invoke, abort, or warn. |
| **R8** | Establish first whether it reproduces at all. *"se não reproduzir, o achado vira 'não reproduzível' e isso é um resultado legítimo, não um fracasso"* — one sighting, possibly specific to that session. |

## Decisions taken by the trader

- **D1** *(answers R7)* — **Asymmetric re-run.** On divergence, re-invoke once
  at the tier's level **only when the pass ran below** what the tier bought.
  When it ran **above** — today's failure mode, `xhigh` for a `medium` tier —
  accept the result and record the overspend in the header and the PR body: a
  deeper pass has already answered the question, and a second pass is the exact
  cost this mission exists to remove. One retry, never two.
- **D2** *(answers R3, R6)* — **Construction plus a negative check.** Invoke
  effort-first, so the parser structurally takes the level as explicit; treat
  any "No effort level given / reusing X" line in the returned report as proof
  of failure. No such line means the requested level ran, and the header says
  both the level and that no reuse notice came back.

## Assumptions

- **S1** — The fix ships as an edit to `.claude/skills/arch-review/SKILL.md`
  plus the one other tracked file that teaches the same wrong argument order
  (`docs/control-plane/roadmap.md`). Repairing a second copy of the exact
  defect is discharging R1, not scope invented beyond it; historical evidence
  files that *record* the old invocation are left alone, because they are a
  record of what happened rather than an instruction to repeat it.
- **S2** — Reproduction runs against a tiny in-repo path target rather than the
  branch, so the two invocation forms can be compared without paying for two
  full branch reviews. The parser does not care what the target is; only the
  token position matters.
- **S3** — No Rust changes, so the four checks are expected to be a formality;
  they still run, per the injected gate.
- **S4** *(wanted to ask; the two-question budget went to D1 and D2)* — the
  header line's exact wording was decided rather than asked. It extends the
  existing `step 0: code-review at <level>` line instead of replacing it, so
  every archived arch-review stays readable against the new one.

## Acceptance criteria

- [x] **A1** — The reproduction is recorded as a measurement, not a claim: the
      parser's actual argument order, the persisted `codeReviewLastEffort`
      value, and the two live invocations with what each returned. The record
      says explicitly whether the defect reproduces.
      *Evidence:* a written reproduction record naming each observation and how
      it was obtained. → `.claude/evidence/arch-review-effort-level/reproduction.md`. *(R5, R8)*
- [x] **A2** — `arch-review`'s step 0 documents the invocation form the parser
      actually accepts, with the level first, and says why the order is
      load-bearing rather than stylistic.
      *Evidence:* the revised step 0 block. → `.claude/skills/arch-review/SKILL.md`. *(R1, R2)*
- [x] **A3** — Step 0 carries a verification the reviewer performs on the
      returned report — the negative check of D2 — stated as a step that can
      fail, not as a request to name a level.
      *Evidence:* the verification paragraph. → `.claude/skills/arch-review/SKILL.md`. *(R3, R4)*
- [x] **A4** — Step 0 states what happens on divergence, exactly as D1 decided:
      the re-run is asymmetric, bounded to one, and an accepted overspend is
      recorded in the header and the PR body.
      *Evidence:* the divergence paragraph. → `.claude/skills/arch-review/SKILL.md`. *(R7)*
- [x] **A5** — The skill states honestly what it cannot prove: the level is
      established by construction and by the absence of a notice, never by a
      positive statement from a skill this repository cannot edit.
      *Evidence:* the honesty paragraph. → `.claude/skills/arch-review/SKILL.md`. *(R6)*
- [x] **A6** — No other tracked file still teaches the wrong argument order.
      *Evidence:* a grep over the tree for the invocation form, with its output,
      in the reproduction record.
      → `.claude/evidence/arch-review-effort-level/reproduction.md`. *(R1)*

### Injected gates

- [x] **G1** — Every artifact in this branch is English: the prose, the branch
      name, the commit messages, and the goal file bar its one marked
      quotation.
      *Evidence:* `cargo test -p quantick-guards` green plus the arch-review
      dimension 8 verdict. → the PR body.
- [x] **G2** — The four checks pass on the rebased branch.
      *Evidence:* the four command exit codes. → the PR body.
- [x] **G3** — Performance impact declared. No code path is touched — the diff
      is markdown — so the rate classification is "no touched path", stated
      rather than omitted.
      *Evidence:* the statement. → the PR body.
- [ ] **G4** — `arch-review` run over the final branch with every Blocker and
      Should-fix resolved, or deferred in the PR body.
      *Evidence:* the arch-review verdict and the `arch-review-ok` marker.
      → the PR body.

## Evidence recorded

- **A1** — `.claude/evidence/arch-review-effort-level/reproduction.md`. Verdict:
  reproduces 2/2, deterministically. Run A (documented order) opened with
  "Reusing your last effort level, xhigh"; 111,465 subagent tokens, 405 s, on a
  129-line file with no diff, for a requested `low`. Run B (effort first)
  returned inline with no notice.
- **A2** — `.claude/skills/arch-review/SKILL.md`, step 0 invocation block, now
  `"<effort> <target>"`, followed by the paragraph naming why the order is
  load-bearing and what the wrong order cost.
- **A3** — same file, *Then prove it, because naming it is what failed*: proof
  is construction plus the negative check on the returned report.
- **A4** — same file, *On divergence, the re-run is asymmetric and bounded to
  one*, exactly as D1 decided, including the recorded overspend.
- **A5** — same file, *Say what cannot be proven, rather than implying it was*.
- **A6** — grep in section 4 of the reproduction record; two live instructions
  fixed, two historical records deliberately left.
- **G1** — `cargo test -p quantick-guards`: exit 0, 4 + 5 tests passed.
- **G2** — `cargo fmt --all -- --check` exit 0; `cargo clippy --workspace
  --all-targets -- -D warnings` exit 0 (2m 28s); `cargo build --workspace`
  exit 0 (5m 19s); `cargo test --workspace` exit 0, zero failures.
- **G3** — no code path touched; the diff is markdown only. Rate class: none.
- **G4** — see the arch-review verdict recorded for this branch.

### Not applicable, and why

- **Hot path** — nothing per-trade, per-depth or per-frame is touched; the diff
  is markdown. No measurement is owed, and G3 says so rather than staying
  quiet.
- **User-visible surface** — no UI, so `ui-harness`, `visual-qa` and
  `trader-ux-review` do not apply.
- **New capability** — `new-extension` does not apply: no port, no registry, no
  new crate. The change repairs an existing instruction.
- **Engine / determinism** — no engine code, so the test-first rule has nothing
  to bind to. The reproduction record is this branch's equivalent evidence.
- **Second operator** — the change adds nothing a trader *does*, so the
  drivable-without-a-mouse rule has no surface to grade.

### Closing steps

- **C1** — `delivery-review` returns PASS (completeness pass, `medium` tier).
- **C2** — the PR is open, naming the tier beside the verification boxes.

## The request as received

Quoted verbatim, in the trader's own words and language, as `mission` step 5
requires and `CLAUDE.md`'s exemption for a marked, attributed quotation allows.
It is not translated because a translation would be a paraphrase, and the
paraphrase is exactly what `delivery-review` exists to check the ledger
against. Received 2026-09-01 from the trader, as the argument to
`/mission medium`.

> o step 0 do arch-review roda no nível errado: o argumento de
> effort não chega na skill code-review e ela reusa o nível da sessão anterior.
>
> Evidência, do PR #274 (branch refactor/leaner-agentic-flow):
> - arch-review/SKILL.md manda invocar `Skill(code-review), args: "<target> <effort>"`
>   e diz "Never omit it: with no level the skill reuses whatever was typed last,
>   in some other session".
> - Invoquei exatamente assim, com args "refactor/leaner-agentic-flow medium",
>   numa missão de tier `high` (que mapeia para `medium`).
> - A skill respondeu: "No effort level was typed, so I reused xhigh — the level
>   from last time" e rodou o protocolo xhigh completo: 10 ângulos inline, dedup,
>   varredura de lacunas.
> - Custo: 192.701 tokens de subagente numa branch quase toda markdown. O tier
>   tinha comprado `medium`.
>
> Note que `code-review` é a skill embutida, não vive em .claude/skills/, então o
> conserto não pode ser nela — tem que ser em como o arch-review a chama, ou em
> como o arch-review verifica o que voltou.
>
> O que eu quero: que um arch-review rodando sob um tier declarado prove que a
> passada de bug rodou no nível que aquele tier compra, em vez de reportar o nível
> que calhou de rodar. Hoje o skill só pede para "nomear o nível no cabeçalho", o
> que registra o erro sem impedir.
>
> Comece reproduzindo antes de consertar — teste as formas de invocação e descubra
> se o argumento passa de alguma maneira. Se não passar de jeito nenhum, o
> entregável é o arch-review lidando com isso honestamente, não uma prosa dizendo
> que passa.
>
> Duas coisas que eu deixei de propósito dentro do prompt em vez de decidir por você:
>
> Por que medium e não small. O diff é pequeno, mas tem uma decisão que não é minha: o que o arch-review faz quando descobre que rodou no nível errado. Re-invocar custa uma segunda passada de bug — que é o oposto do objetivo. Abortar trava a review. Só avisar é o que já acontece hoje e não impediu nada. Essa é escolha sua, e medium dá duas perguntas para ela ser feita.
>
> Por que "comece reproduzindo". Eu não sei qual é a forma correta de invocação — sei só que a documentada não funcionou uma vez. Se o próximo agente partir de "a forma X é a certa", ele escreve prosa afirmando algo que ninguém mediu, que é exatamente o defeito que esta branch levou três rodadas para tirar de mim.
>
> Uma ressalva honesta: eu vi isso uma vez. Pode ser específico da minha sessão. A primeira coisa que a missão deve estabelecer é se reproduz — se não reproduzir, o achado vira "não reproduzível" e isso é um resultado legítimo, não um fracasso.
