# Campaign coordinator mission

Build a shared, durable campaign coordinator and deliver a ready PR with green
CI so large objectives can advance across agents and sessions.

**Tier:** high: operational authority, recovery and cross-session coordination
need independent review. Later steering adds executable review/merge guards.

## Request ledger

- R1: Deliver a canonical campaign skill and a thin other-client adapter.
- R2: Support create/bare objective, run, status, resume, human-tasks and close.
- R3: Refresh local/remote state and read project instructions/relevant skills.
- R4: Discover existing issues, PRs, Projects and dependencies; avoid duplicates.
- R5: Run/reuse quantick-score baselines and targets when the objective calls for it.
- R6: Create/reuse a Project with all six requested states.
- R7: Persist parent objective, outcome, scope, criteria, constraints, dependencies, risks and metrics.
- R8: Decompose into reviewable child issues, ordered and independent where possible, linked to parent/Project with explicit dependencies.
- R9: Classify autonomous, human decision, manual validation and external authorization tasks.
- R10: Execute unblocked work autonomously with one implementation per agent and existing task/review skills.
- R11: Human tasks explain reason, steps, copyable prompt where applicable, returned evidence and objective completion.
- R12: Cycle implementation, risk-based validation, issue-linked PRs, CI monitoring/repair, checkpointing and next task.
- R13: Persist context-free handoffs, checkpoints, next actions, decisions, recovery and bounded attempts on GitHub.
- R14: Detect stalled CI/PRs and escalate blockers while continuing independent work.
- R15: Require evidence before completion; stop only for completion, no ready work, indispensable human decision or unsafe continuation.
- R16: Give concise decision-oriented updates and preserve authorization boundaries for merge, publication, deployment, spending and irreversible/external actions.
- R17: Protect credentials and prepare independent work before one concrete instruction for missing GitHub permission.
- R18: Integrate mission, issue, new-task, ship, delivery-review, arch-review, ai-review and applicable quantick-score without duplicating responsibilities.
- R19: Consider #314, #322, #323 and #324; do not execute the future A+ campaign or publish unrelated positioning claims.
- R20: Provide short examples, state/human-task/recovery formats, appropriate validation, implementation issue, ready PR and green CI; simulate a small persisted campaign and independently resume it.
- R21: Keep local validation proportional: docs/skills avoid full local workspace suite; CI may supply it; strengthen architecture/concurrency/persistence/security checks; complexity is not measured by text size alone.
- R22: Use a dedicated integration branch per implementation campaign; each child retains its own branch/worktree and builds on integrated predecessors.
- R23: Persist the exact campaign merge destination, initial/current SHAs, scope and user authorization; intermediate merges require reviews and green CI.
- R24: Main merging is exclusively human, including no auto-merge, merge queue, direct integration push or protection bypass by agents.
- R25: Existing task/review skills and shared hooks use the campaign base explicitly, retaining main as default and invalidating reviews when the campaign base changes.
- R26: Score the campaign SHA honestly, synchronize main with new integration checks, and deliver a fully reviewed consolidated main PR.
- R27: Separate candidate readiness from main integration; stop autonomous integration at ready for user evaluation and observe the user's merge before recording main completion.

## Decisions

- D1: User explicitly authorizes repository/GitHub analysis, issue/Project creation, isolated implementation, commits, testing, PR creation and CI follow-through. No merge request was made.
- D2: User explicitly directs targeted local validation for docs/skills and permits full workspace verification in CI; this overrides the older full-local wording for this mission only.
- D3: User explicitly requests all seven supplied project skills. Delivery review runs in full from fresh context.
- D4: Later user steering requests the integration-branch model and states main merge must be theirs. This authorizes implementing that mode, not performing a merge for this coordinator delivery.

## Assumptions

- S1: Use the existing canonical Claude entrypoint/thin Codex adapter convention; reversible and already established by #323.
- S2: A skill coordinates an executing client, not an always-on daemon. Checkpoint and expose resume when no host continuation facility exists.
- S3: #324 retains the general mission/hook redesign; this coordinator records applicable overrides without changing those gates.
- S4: A dedicated simulation parent and two read-only child issues are within the requested small simulation. Missing Project scope is recorded honestly; it does not block their independent verification.

## Acceptance criteria

- [ ] **A1** — Both clients expose the operations through one workflow and short examples. *Evidence:* structural validators, relative link checks and workflow text → docs/campaign/validation.md. *(R1, R2, R20)*
- [ ] **A2** — Discovery, scoring, reuse, parent/child/Project state and dependency procedures are specified with portable records. *Evidence:* workflow/state/GitHub operation contract and simulation links → docs/campaign/validation.md. *(R3, R4, R5, R6, R7, R8, R19)*
- [ ] **A3** — Scheduling, task classes, human instructions and authorized task-skill integration are executable instructions. *Evidence:* shared workflow and state contract → docs/campaign/validation.md. *(R9, R10, R11, R12, R18)*
- [ ] **A4** — Resume/recovery, retries, ownership, next actions, stalled work, evidence and stop rules survive a context-free simulation. *Evidence:* GitHub simulation and independent read-only report → docs/campaign/validation.md. *(R13, R14, R15, R20)*
- [ ] **A5** — Concise updates, permission recovery and credential/authority boundaries are explicit; no A+ work or merge performed. *Evidence:* contracts, simulation checkpoint and diff → docs/campaign/validation.md. *(R16, R17, R19)*
- [ ] **A6** — Risk-based checks follow explicit user instruction, with local structural/guards/link/hygiene proof and required full CI at delivery. *Evidence:* recorded local results → docs/campaign/validation.md; CI is a closing step. *(R12, R20, R21)*
- [ ] **A7** — Campaign base, authority and final lifecycle are persisted and integrated with existing skills; main remains exclusively human. *Evidence:* integration contract and helper/boundary tests → docs/campaign/validation.md. *(R22, R23, R24, R25, R26, R27)*
- [ ] **G1** — Authored artifacts are English except this attributed verbatim request. *Evidence:* guards and prose inspection → docs/campaign/validation.md.
- [ ] **G2** — Architecture bug, relevant shape and language review have no unresolved findings. *Evidence:* final-head review → PR review record.

## Not applicable

No Rust, runtime config, UI or public product contract changes. UI/visual/trader
reviews, runtime measurements and engine fixtures are not applicable. Runtime
performance is unchanged; workflow runs rarely at explicit agent invocation.
The added shell authority/base helpers require hermetic positive/negative tests
and the full shared guardrail suite locally. Full Rust integration remains in
CI under D2; no Rust dependency changed. The small-tier fix retains thread and
merge-authority checks after its delivery-review exemption; #324 retains the
general risk-tier redesign.

## Closing steps

C1: Final-head architecture and full independent delivery review pass; AI review
has no open findings. C2: PR references #325, is ready and all required CI is
green. C3: Report Project permission blocker with its one concrete recovery
instruction; do not claim successful live Project writes or merge the PR.

## Verbatim request

The following is an attributed quotation of the user's implementation request.

> Quero criar no Quantick um Coordenador de Campanha para conduzir trabalhos grandes e de vários dias com pouca supervisão humana.
>
>   Trate esta solicitação como autorização para analisar o repositório, consultar o GitHub, criar a issue necessária, implementar a solução em branch/worktree próprio, testar, abrir o
>   PR e acompanhar o CI. Não pare apenas no planejamento.
>
>   Objetivo
>
>   Criar uma skill compartilhada entre Claude e Codex chamada `campaign`, capaz de transformar um objetivo amplo em uma campanha executável e rastreável.
>
>   Uma campanha deve poder durar horas ou dias, envolver várias issues e PRs e continuar avançando sem que eu precise enviar constantemente novos prompts ou decidir cada próximo passo.
>
>   Comportamento esperado
>
>   Ao receber algo como:
>
>   `$campaign preparar o Quantick para alcançar A+ na avaliação $quantick-score antes da melhoria arquitetural da issue #314`
>
>   o coordenador deve:
>
>   1. Atualizar o repositório local e consultar o estado real do GitHub.
>   2. Ler as instruções do projeto e as skills relevantes.
>   3. Identificar issues, PRs, projetos e dependências já existentes.
>   4. Executar ou consultar `$quantick-score` para obter a linha de base.
>   5. Criar um GitHub Project para a campanha, se ainda não existir.
>   6. Criar uma issue principal com:
>      - objetivo;
>      - resultado esperado;
>      - escopo;
>      - critérios de conclusão;
>      - restrições;
>      - dependências;
>      - riscos;
>      - métricas iniciais e metas.
>   7. Decompor a campanha em issues pequenas, ordenadas e independentes sempre que possível.
>   8. Relacionar todas as issues à issue principal e ao GitHub Project.
>   9. Definir dependências explícitas entre as etapas.
>   10. Classificar cada atividade como:
>       - executável autonomamente pelo agente;
>       - decisão que realmente exige minha opinião;
>       - validação manual que somente eu consigo realizar;
>       - ação externa que exige minha autorização.
>   11. Executar autonomamente tudo o que não depende de mim.
>   12. Para cada tarefa humana, criar uma issue ou item claro contendo:
>       - o que preciso fazer;
>       - por que minha participação é necessária;
>       - instruções passo a passo;
>       - prompt pronto para copiar e executar, quando aplicável;
>       - evidência ou resultado que devo devolver;
>       - critério objetivo para considerar a tarefa concluída.
>   13. Trabalhar em ciclos:
>       - selecionar a próxima issue desbloqueada;
>       - implementar;
>       - validar proporcionalmente ao risco;
>       - abrir o PR;
>       - acompanhar o CI;
>       - corrigir falhas;
>       - atualizar o estado da campanha;
>       - avançar para a próxima etapa.
>   14. Persistir o estado no GitHub para que Claude, Codex ou Astra possam retomar a campanha sem depender do histórico da conversa.
>   15. Evitar recriar trabalho já existente.
>   16. Não marcar uma etapa como concluída sem evidência.
>   17. Quando encontrar um bloqueio humano, continuar executando outras atividades independentes.
>   18. Interromper a campanha somente quando:
>       - o objetivo tiver sido atingido;
>       - não existir mais trabalho desbloqueado;
>       - houver uma decisão humana indispensável;
>       - ocorrer um risco que torne inseguro continuar.
>
>   Experiência para mim
>
>   Quero receber atualizações curtas e orientadas à decisão.
>
>   Não quero acompanhar detalhes operacionais de cada comando, teste ou commit.
>
>   Quando minha participação for necessária, apresente uma lista semelhante a:
>
>   Tarefa humana: avaliar o fluxo de criação de tema
>
>   Motivo:
>   A qualidade percebida exige julgamento humano.
>
>   Passos:
>   1. Abra a aplicação.
>   2. Crie um tema usando o cenário indicado.
>   3. Observe os três pontos descritos.
>   4. Registre o resultado na issue.
>
>   Prompt para executar:
>   `[prompt completo e pronto para copiar]`
>
>   Resultado esperado:
>   - aprovado; ou
>   - reprovado com observações e evidências.
>
>   A skill também deve oferecer comandos ou operações equivalentes a:
>
>   - `$campaign create <objetivo>` — cria uma campanha.
>   - `$campaign run` — executa o próximo trabalho disponível.
>   - `$campaign status` — apresenta progresso, bloqueios e próximas etapas.
>   - `$campaign resume` — retoma usando apenas o estado persistido.
>   - `$campaign human-tasks` — mostra somente o que depende de mim.
>   - `$campaign close` — verifica os critérios finais e encerra a campanha.
>
>   Regras de execução
>
>   - Use o GitHub como fonte persistente do estado.
>   - O GitHub Project deve mostrar backlog, pronto, em andamento, bloqueado, aguardando humano e concluído.
>   - Mantenha no máximo uma implementação por agente ou o limite seguro identificado no repositório.
>   - Uma issue deve produzir um resultado revisável.
>   - Cada PR deve referenciar sua issue.
>   - Validações devem ser proporcionais ao risco da mudança.
>   - Alterações pequenas de documentação ou skills não devem executar localmente toda a suíte do workspace.
>   - A suíte completa pode ficar no CI quando isso for suficiente.
>   - Mudanças de arquitetura, concorrência, persistência ou segurança exigem verificações mais fortes.
>   - Não use quantidade de caracteres ou linhas como único indicador de complexidade.
>   - Não crie processos burocráticos que não produzam evidência ou reduzam risco.
>   - Não escreva em nenhum artefato que o projeto busca agradar recrutadores.
>   - O objetivo público é melhorar sustentabilidade, escalabilidade, qualidade e capacidade de operação por agentes.
>   - Nunca exponha tokens ou credenciais.
>   - Se faltar permissão no GitHub, prepare todo o restante primeiro e então me entregue uma única instrução concreta para remover o bloqueio.
>
>   Compatibilidade
>
>   A implementação deve funcionar tanto no Claude quanto no Codex.
>
>   Escolha uma fonte canônica e crie um adaptador fino para o outro cliente, seguindo as convenções existentes do Quantick. Evite duplicar as instruções completas.
>
>   Integre a skill às ferramentas existentes, incluindo quando aplicável:
>
>   - `$mission`
>   - `$issue`
>   - `$new-task`
>   - `$ship`
>   - `$delivery-review`
>   - `$arch-review`
>   - `$ai-review`
>   - `$quantick-score`
>
>   A skill `campaign` coordena essas ferramentas; ela não deve copiar integralmente suas responsabilidades.
>
>   Preparação para campanhas longas
>
>   O coordenador deve conseguir administrar projetos maiores sem depender de uma única sessão ou de um contexto enorme.
>
>   Projete:
>
>   - checkpoints persistentes;
>   - critérios para retomada;
>   - identificação da próxima ação;
>   - prevenção de trabalho duplicado;
>   - recuperação após falhas;
>   - limite de tentativas;
>   - registro de decisões;
>   - controle de dependências;
>   - detecção de PRs ou CI parados;
>   - escalonamento de bloqueios;
>   - resumo compacto para transferência entre agentes.
>
>   Segurança operacional
>
>   O coordenador pode criar branches, worktrees, commits, issues, projetos, itens de projeto e PRs quando autorizado por esta campanha.
>
>   Merge, publicação, deploy, gastos, mudanças irreversíveis e ações externas devem respeitar as regras de autorização existentes no repositório e no ambiente.
>
>   Primeira campanha futura
>
>   Não execute ainda a campanha completa de melhoria do Quantick dentro deste trabalho. Crie e valide o coordenador para que, após o merge, possamos iniciar esta campanha:
>
>   “Elevar o Quantick até os critérios de A+ da skill `$quantick-score`, corrigindo primeiro as lacunas de maior impacto e preparando o terreno para a issue #314.”
>
>   Considere as issues e PRs existentes, especialmente:
>
>   - issue #314;
>   - issue #322;
>   - PR #323;
>   - issue #324.
>
>   Entregáveis desta missão
>
>   - skill canônica `campaign`;
>   - adaptador para o outro cliente;
>   - documentação curta com exemplos;
>   - modelo persistente de campanha;
>   - formato das tarefas humanas;
>   - regras de retomada e recuperação;
>   - validações adequadas;
>   - issue de implementação;
>   - PR pronto e CI verde.
>
>   Antes de concluir, simule uma campanha pequena e confirme que outro agente conseguiria retomá-la apenas lendo o estado persistido.
>
>   Comece agora e prossiga autonomamente até o PR estar pronto.

## Subsequent user steering (attributed quotation)

> › entao a main tem um trava somente eu posso mergear. Mas e se a gnt crair um branch por camapnha? ai os merges ficam em cima da branh da campanha. Assim nao temos risco de causar
>   dano a main. E teriamos temos para avaliar como ficou apos o final dela. Ai eu daria full autorziacao para margear na branch. Eu coloco essa regra  na skill da campanha ou nao?
>
> • Sim — eu colocaria esse modo na skill campaign. Uma branch de integração por campanha combina bem com o que você quer: autonomia para integrar as etapas e uma decisão sua antes de
>   levar o conjunto à main.
>
>   O fluxo seria:
>
>   main
>    └─ campaign/architecture-a
>        ├─ missão 1 → PR para campaign/architecture-a
>        ├─ missão 2 → PR para campaign/architecture-a
>        └─ missão 3 → PR para campaign/architecture-a
>
>   campaign/architecture-a → PR final para main → você decide o merge
>
>   Cada missão continua em sua própria branch e worktree. Depois de integrar uma missão, a seguinte parte da branch atualizada da campanha, incorporando o trabalho anterior.
>
>   Isso protege a main de alterações diretas durante a campanha. Ainda precisamos testar a integração final: quanto mais tempo a campanha durar, maior a possibilidade de divergência
>   com mudanças feitas na main.
>
>   Eu separaria a regra geral da autorização específica:
>
>   - Na skill: como criar, atualizar, testar e concluir uma campanha com branch de integração.
>   - No registro de cada campanha: branch exata de destino, base inicial, escopo e sua autorização para merges.
>   - No GitHub: manter a proteção da main. A skill orienta o agente; a proteção técnica garante o limite.
>
>   Há um ajuste importante no Quantick: hoje as instruções presumem branches saindo de origin/main e reviews comparando com origin/main. Só adicionar a frase na campanha não basta.
>   new-task, mission, reviews, ship e os hooks precisam aceitar a base da campanha explicitamente, mantendo main como padrão.
>
>   Eu colocaria estas condições no modo de campanha:
>
>   1. PRs de missão só podem ser integrados na branch exata autorizada, com revisões e CI verde.
>   2. A próxima missão usa o resultado já integrado nessa branch.
>   3. A nota é medida no SHA da campanha; ela não deve ser apresentada como nota da main.
>   4. Atualizações da main são incorporadas com novos checks, preservando o trabalho da campanha.
>   5. O encerramento entrega um PR consolidado para main, com revisão do conjunto, evidências e CI. O merge final é exclusivamente seu.
>
>   Também vale cuidar do significado de “concluído”: uma tarefa pode estar integrada na campanha, enquanto a campanha está pronta para sua avaliação, sem estar entregue na main.
>
>   Minha recomendação é adotar esse modo como padrão para campanhas de refatoração do Quantick. Você concede autonomia dentro de uma branch delimitada e mantém a aprovação do resultado
>   completo. A autorização deve citar o nome dessa branch, evitando um “pode mergear tudo” genérico.
>
> › Quero que o merge para main seja meu.
>
> • O merge para main fica exclusivamente com você. O Astra pode integrar os PRs intermediários na branch da campanha e preparar o PR final, mas deve parar antes de integrá-lo à main.
>
>   A regra na skill deve ser explícita:
>
>   > O coordenador só pode fazer merges na branch de integração autorizada para a campanha. O merge final para main é reservado ao usuário. Não habilitar auto-merge, inserir em fila de
>   > merge nem contornar proteções para esse destino.
>
>   A campanha termina a execução autônoma como “pronta para avaliação do usuário”, com PR final, revisões, CI e scorecard. Só registra “integrada à main” depois de verificar o merge
>   feito por você.
