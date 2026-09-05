# Mission: prepare the architecture before adding future capabilities

Deliver the first foundation milestone for extensible content, external agents,
and window-independent workspaces: a source-backed architecture audit, ownership
decisions and executable migration tasks. This starts the preparation program;
it does not claim that a documentation PR completes the runtime refactoring.

**Tier:** high. Cross-cutting design judgment and delegation require independent
architecture and full delivery reviews even though this first diff is docs-only.

Issue: https://github.com/milocaetano/quantick/issues/314

## Request ledger

- R1: Prepare architecture first, before implementing the future features.
- R2: Plan for community themes, including appearance defaults and typography.
- R3: Plan for an external official assistant through the existing control plane.
- R4: Prepare workspace ownership for future Chrome-like detachable/rejoinable tabs.
- R5: Preserve performance as the application grows, with measured evidence.
- R6: Build on the existing refactoring and produce ordered, agent-executable tasks.
- R7: Coordinate independent agents and integrate their findings into one plan.
- R8: Make architecture quality verifiable by reviewers, rather than a subjective label.
- R9: Prepare portable, reusable workspace/content templates.
- R10: Define the extension boundary for community plugins.
- R11: Prepare runtime indicator authoring and explicit saving/adoption of source.
- R12: Prepare eventual strategy and voice actions without widening current authority.
- R13: Preserve deterministic processing and one engine across consumers.
- R14: Check access to the supplied workspace reference and report the result.

## Decisions taken by the trader

- D1: Architecture preparation takes precedence over feature implementation.
- D2: The assistant coordinates agents and execution.
- D3: Chrome-like workspaces are a future behavior reference, not this milestone's UI.

## Assumptions

- S1: The first PR records the baseline and implementation contracts, as announced
  before work; follow-up tasks implement them. This keeps reviewable changes small.
- S2: Preserve current feed-per-tab and paper-session semantics during mechanical
  extraction; sharing and changed shutdown behavior require separate design work.
- S3: The linked Claude artifact could not be accessed. Only the user's textual
  description informs this milestone; do not infer unseen interaction details.
- S4: No new product taste, trading autonomy, provider selection or paid service
  decision is necessary to deliver architecture preparation.

## Acceptance criteria

- [x] **A1**: Current-state facts, remaining coupling and recent refactors cite
  source evidence at the inspected revision.
  *Evidence:* audit tables -> `docs/architecture/foundation.md`. (R1, R6, R8)
- [x] **A2**: Target boundaries cover content, operations/authority, window,
  workspace, market session and persistence, distinguishing proposals from code.
  *Evidence:* responsibility contracts -> `docs/architecture/foundation.md`. (R2, R3, R4, R9, R10, R11, R12)
- [x] **A3**: Tasks carry dependencies, scope, acceptance proof, rate/risks and
  coordination rules; the first runtime task is ready to execute.
  *Evidence:* task specifications -> `docs/architecture/preparation-tasks.md`. (R1, R6)
- [x] **A4**: Structural baseline and reproducible performance protocol distinguish
  measured results from unmeasured GUI/memory behavior.
  *Evidence:* commands and limitations -> `docs/architecture/baseline.md`. (R5, R8)
- [x] **A5**: Independent workspace, extension and control audits are integrated;
  open-work overlap and the unavailable reference are explicitly recorded.
  *Evidence:* audit provenance -> `docs/architecture/foundation.md`. (R4, R6, R7, R14)
- [x] **A6**: Runtime files, schemas and dependency graph remain unchanged in this
  first milestone; future implementations are explicitly not marked delivered.
  *Evidence:* diff boundary and retained fixtures -> `docs/architecture/baseline.md`. (R1, R5, R13)
- [x] **G1**: All authored repository prose is English except attributed quotations.
  *Evidence:* guards and manual review -> `docs/architecture/baseline.md`.
- [x] **G2**: fmt, clippy, build and workspace tests pass before commit.
  *Evidence:* recorded command results -> `docs/architecture/baseline.md`.
- [ ] **G3**: Architecture review, including the medium bug pass, has no unresolved
  Blocker/Should-fix findings.
  *Evidence:* final diff verdict -> PR body (scratch review dossier before PR).

## Non-applicable gates

No runtime, UI, engine, capability, dependency or hot-path changes in this PR:
visual QA, trader UX review, new UI hooks, extension implementation tests and a
before/after GUI performance claim do not apply. Four checks and the bug/language
review still apply. Future task specifications name their applicable runtime gates.

## Closing steps

- C1: Archive this mission and run independent full delivery review on the final diff.
- C2: Open the PR with review/verification evidence and observe green CI. Do not merge.

## Request as received

Attributed quotation from the trader (Portuguese; clarification and authorization
for this architecture-first milestone, verbatim):

> o plano eh primeiro preparar a arquitetura
>
> Pode começar a preparação da arquitetura e coordenar os agentes.
>
> https://claude.ai/code/artifact/17195533-7538-456c-8808-759c5289d451
>
> consegue acessar esse link?
>
> o objetivo do workspace como chrome seria comoe sse aqui
>
> igual ao chorme Sangria
>
> mas primeiro a gnt focar em pareparar a  arquitetarua

The earlier product request, quoted verbatim for traceability:

> Bom, eu vou falar o meu plano e aí a gente precisa planejar e preparar a arquitetura do Quantic para que o código tenha um kernel, um core pronto para atingir esses meus objetivos.
> 1. Eu quero que o Antique seja extensivo, ou seja, que permita a criação de plugins e de templates. Se eu quero temas diferentes nesses templates, vai alterar a cor dos principais indicadores, das barras de ferramenta e dos acórdãos que são default do sistema. Hoje temos:
> - um acórdão de uma linha horizontal default
> - o acordo da meta mobile
> - as cores dos desenhos do volume profile
> - a cor dos candles
> - a cor do future print
> - chart
> - a cor da janela
> - talvez a fonte da janela, a fonte que está exibida na janela
> - o tamanho
> Tudo isso é meio que hard-coded como default. Quero que seja extensível o suficiente para eu instalar templates, para a própria comunidade criar seus próprios templates, para que tenham temas diferentes, parecidos com o Chrome hoje.
> 2. Eu pretendo colocar dentro desse conceito de plugin, como a gente é um projeto open source. Quero criar um plugin oficial em que eu vou vender serviço, talvez utilizando o Hermes ou alguma coisa desse tipo, para que esse agente de IA conecte com o MCP da plataforma e consiga fazer tudo que o usuário faz. Ele pode analisar o mercado, colocar uma linha, colocar um indicador, criar um indicador no momento em que o cara pedir. A gente usa meio que um conceito de pineScript então a gente poderia criar um indicador on the fly, durante a execução, para que ele veja já o resultado daquele indicador e salve esse indicador. Também, caso ele queira, deixar o Jassal. A gente não tem estratégias ainda mas possivelmente criar estratégias bem parecidas, utilizando o comando de voz. Aí ele consegue ver a estratégia acontecendo, consegue comprar com comando de voz, vender com comando de voz, colocar ordem de stop, pedir para a gente tração, volume profile, enfim tudo via comando de voz. Ele pode simplesmente pedir para colocar um alarme, setar uma estratégia, enfim praticamente tudo que o usuário pode fazer com o mouse e com o teclado, essa a gente pode fazer e eu quero que o quantity seja escalável.
> 3. A gente vai adicionar funcionalidades novas. No caso eu estou pensando em fazer abas de workspace, como do Chrome, colocar um sistema mais amplo onde eu posso destacar a aba e jogar numa outra tela, como se fosse subdividir o contíguo em dois, igual o Chrome faz quando eu pego uma aba e tiro, ela destaco essa aba do Chrome. Eu consigo ver dois Chrome's. Eu vou chamar isso de workspace.
> 4. A gente tem que preparar a arquitetura que a gente tem hoje para que ele possa crescer de uma maneira escalável e manter a sua alta performance que a gente tem hoje.
> 5. Eu quero que a gente traça, dado esse objetivo do que eu quero, onde eu quero chegar, com Camilo. Eu quero que você traça um plano para a gente preparar as tarefas ou não sei, até quando você pode fazer multi-tarefas. Eu não entendo muito bem mas eu quero que você traça um plano para a gente chegar a esse objetivo.
> 6. Ja comecei uma refatoração pesada no quanti. Você pode olhar aí: a gente está refatorando, está separando o código. Eu quero que o chique seja muito bem avaliado por agentes de IA que olham o quantico e falam: “Essa arquitetura do quantico é feita por um engenheiro sênior de IA.” Esse é o objetivo então quero que você traça, para a gente chegar até esse objetivo, o passo a passo: define que é prioridade, a gente vai atacar essas etapas.
