# CP-01 — Control plane, fase 2: o que falta do MVP (§18)

**Missão**: fechar o *definition of done* do plano
`docs/mcp-control-plane-development-plan.md` (§18) entregando, na ordem abaixo
e cada um em worktree próprio, os quatro pedaços que a fase 1 deixou
explicitamente **não iniciados**: os módulos de snapshot restantes, a cena
semântica, a camada *annotate/notify* (PR 5b) e os *evidence bundles* (PR 5c).
A fase 1 (#220, #213, #221, #222, #223) entregou contrato, observer, gateway,
adaptador MCP, eventos/cursor e o mark humano; este pacote é o lado que
**responde** — no chart, por notificação, por script — e o que **prova** uma
investigação.

Leia antes de tudo: `.claude/roadmap/DISPATCH.md` (protocolo de despacho:
um pacote, um worktree, um agente; quatro checks; `arch-review` antes do PR;
marker do `pr-gate`), depois os corpos dos PRs #221/#222/#223 e
`docs/control-plane/{control-contract.md,
adr-0001-local-transport-and-instance-discovery.md, observer-threat-model.md,
capability-inventory.md}` e, até a pilha mergear, os docs de evidência que
vivem nas branches dos PRs (`pr2-performance.md`, `pr3-gateway-evidence.md`,
`pr4-mcp-evidence.md`, `pr5a-events-evidence.md`). O contrato (§§2.6, 5, 8, 11) é a fonte de nomes e
códigos de erro; nenhum pacote inventa vocabulário novo.

## Pré-condição de base

A pilha da fase 1 é `main ← #213 ← #221 ← #222 ← #223`. Dois cenários:

- **Pilha mergeada** (o caso normal): cada pacote nasce de `origin/main`
  atualizado, como manda o `DISPATCH.md`.
- **Pilha ainda aberta**: o pacote nasce do topo (`feat/control-events`) e o PR
  aponta para ela; quando a base for reescrita, re-empilhe com
  `git rebase --onto <nova-base> <head-antigo-da-base>` (um `rebase` simples
  re-aplica os commits da base antiga e conflita).

Em ambos, o corpo do PR diz de onde nasceu.

## Ordem e dependências

```
CP-01.A snapshots (analysis → orderflow → session)   ─┐
CP-01.B scene                                         ├─→ CP-01.D evidence (5c)
CP-01.C annotate / notify / attach_script (5b) ───────┘
```

- **A** e **B** e **C** podem rodar em paralelo (arquivos disjuntos: módulos no
  `registry`, cena num módulo próprio, ações em `actions.rs`), **um agente por
  worktree**. A única ponte: o critério "attach visível no escopo de
  indicadores" de **C** precisa do módulo `indicators` de **A.1** — se **C**
  chegar antes, marca esse critério como gap no corpo e a prova vai no PR de
  A.1.
- **D** nasce por último: consome cena, módulos e eventos.

Cada letra é um PR (A são três PRs pequenos). Nada aqui é "um PR grande":
o raio de impacto de cada um é um arquivo novo + linhas de registro.

---

## CP-01.A — Módulos de snapshot restantes (três PRs)

Branches: `feat/control-snapshots-analysis`, `feat/control-snapshots-orderflow`,
`feat/control-snapshots-session` · worktrees em `../quantick-worktrees/<slug>`.

Depende de: nada. Bloqueia: D (aceite de evidência) e o critério de leitura do
`attach_script` em C.

**Onde docar** (porta já existe, é só registrar):
- `crates/app/src/control/registry.rs`: `ProjectionRegistry::register_module`
  + `register_scope`; a lista canônica é `standard_registry()` no `mod.rs` do
  controle — **uma linha por módulo**. O escopo `system.health`
  (`crates/app/src/control/health.rs`) é o exemplar de DTO: decimais exatos
  como string, sufixo `_unix_ms`, proveniência declarada, nada de tipo egui.
- Cada escopo declara as permissões que exige (hoje `observe.*`); o perfil
  `observer` e o *safe default grant* vivem em `contract.rs`.
- Eventos: cada módulo que muda de estado ganha **uma** comparação em
  `emit_semantic_changes` (`gateway.rs`), no molde do `TabKey` — comparação
  *in place*, alocação só quando algo mudou (a revisão do #223 derrubou a versão
  que alocava por frame; não volte a ela).
- Schemas: `schemas/control/*.schema.json` + catálogo; regenerar com
  `QUANTICK_UPDATE_CONTROL_SCHEMAS=1 cargo test -p quantick-app observer_schemas_are_versioned_valid_and_ui_framework_free`
  e `… observer_capability_catalog_is_registry_derived_and_versioned`;
  o snapshot test falha se esquecer.

**A.1 analysis** — escopos `indicators` (host headless: descritor, inputs
efetivos, leituras atuais das séries por pane, erros de compile pendentes) e
`drawings` (objetos por pane/lado com id estável, ferramenta, banda, escopo
this/all charts, `locked/hidden`, **autor** — ver C — e texto do usuário
**não** vazado, como já faz `observer_resolves_mirrored_drawings_without_leaking_user_text`).

**A.2 orderflow** — `tape`, `footprint`, `bubbles`, `heatmap`, `l2`: só o que o
chart já computa (nunca recomputar no snapshot); limites por
`CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS`-like constantes em
`quantick_control::limits`, nomeadas; lado inferido rotulado
(memória da casa: delta é tick rule; bid/ask da XP são limites de banda).

**A.3 session** — `replay` (sessão, posição/elapsed, playing/finished, speed,
trace presente e completo?) e `paper` (posição, ordens, histórico com
proveniência `paper_trading_session_ledger`).

**Critérios de aceite (cada PR)** — os do plano §11/PR 2, aplicados ao módulo:
1. Teste headless cria `QuantickApp`, muda estado pelo caminho normal e
   verifica o snapshot.
2. Captura de dois panes preserva foco e proveniência.
3. Cada escopo valida contra o próprio schema declarado, por teste
   (`observer_modules_project_headless_state_that_matches_their_schemas` cobre
   o que está registrado — o novo módulo entra nele).
4. Nenhum tipo egui no wire (`observer_schemas_are_versioned_valid_and_ui_framework_free`).
5. Sem request, sem custo por frame: `control_idle_dense_replay_benchmark`
   (pares candidato × controle na mesma janela de condições, números no corpo)
   e os guardas `observer_*_stays_within_the_ui_budget` (mediana; o p99 é
   leitura `#[ignore]`).
6. Um evento por mudança relevante do módulo no journal (`replay.state.changed`
   é o exemplar; para indicadores: attach/detach/compile error; para drawings:
   created/removed/edited com autor).
7. Blast radius no corpo: arquivo(s) novo(s) + linha de registro + schemas.

---

## CP-01.B — Cena semântica

Branch: `feat/control-scene` · worktree `../quantick-worktrees/feat-control-scene`.

Depende de: nada. Bloqueia: D (correlação de screenshot) e a ferramenta
`quantick_get_scene` do adaptador (omitida em #222 *porque* a cena não existia).

**O que é** (plano §6.3): a árvore do que está na tela sem rasterizar —
controles visíveis, rótulo e **id estável entre frames**, estado
enabled/selected, **razão de indisponibilidade** como dado (nunca texto
renderizado), bounds quando úteis, dono (painel/diálogo/aba/pane) e a
capacidade registrada relacionada.

**Onde docar**: um módulo `scene.rs` registrado via `register_module` como os
de A; a fonte dos controles é o mesmo lugar que já alimenta os hooks do
`ui-harness` (rail de ferramentas `DRAWING_TOOLS`, toolbar, painéis) — **um
registro só**, nunca uma lista paralela "para o agente" (arch-review,
dimensão 7: lista à mão ao lado de um registro é finding). O cursor (§6.5) já
devolve o controle sob o ponteiro por id; a cena usa o **mesmo id**.

**Critérios de aceite**:
1. Ids estáveis entre frames, provado por teste (dois frames, mesma árvore).
2. Razões de indisponibilidade explícitas sem parse de texto
   (`AvailabilitySnapshot { available, reason }` é o molde).
3. Um controle citado pela cena é o mesmo que o cursor devolve ao apontar
   para ele (teste cruzado com `observer_cursor_*`).
4. `quantick_get_scene` entra em `crates/mcp/src/tools.rs` (uma entrada `Tool`
   + um braço no `match` de `tools::call`, com schema embutido e o braço
   `ErrorResponse` do `oneOf` como as outras) **e** o teste
   `the_tool_list_is_fixed_and_named_as_the_contract_says` é atualizado.
5. Os mesmos critérios 3–5–7 de A (schema, sem egui, sem custo sem request,
   blast radius).

---

## CP-01.C — PR 5b: annotate, notify e attach_script

Branch: `feat/control-annotate` · worktree `../quantick-worktrees/feat-control-annotate`.

Depende de: #223 (o `ActionRegistry`, o perfil `annotator` declarado, o
control trace). Bloqueia: D.

É o primeiro tier que **escreve**, e o que deliberadamente não pode perder
trabalho do usuário (plano §2.6: *annotate* = o usuário consegue desfazer; nada
de cockpit, nada financeiro). Rate class: ação humana ou de agente — nunca
por trade nem por frame.

**Onde docar (tudo já existe como porta ou declaração):**
- `crates/app/src/control/actions.rs` — `ActionRegistry::register(descriptor, handler)`
  com handler `fn(&mut QuantickApp, &mut ControlAccess, &ActorContext, &Value) -> Result<Value, ControlError>`;
  `attention.mark.create` (`create_mark`) mostra o formato completo (descritor,
  schemas gerados de structs, validação de input/output, evento no journal com
  `actor`, `target_source`). Constantes `ANNOTATE_EFFECT_ID`,
  `ANNOTATE_PERMISSION_ID`, `ANNOTATE_ATTENTION_PERMISSION_ID`,
  `ANNOTATOR_PROFILE_ID` já declaradas.
- **Despacho remoto de ações** (hoje ações só rodam localmente via
  `QuantickApp::control_action`): `ObserverContract::prepare` (`contract.rs`)
  ganha um `PreparedDispatch::Action(...)` quando a capacidade é uma ação
  registrada e `required_permissions ⊆ effective_scopes` (o check já existe
  para leituras); `execute_on_ui` (`gateway.rs`) roteia para
  `ControlAccess::invoke_local_action(app, id, input, origin)` com um
  `ActionOrigin::Remote` que carrega o `ActorContext` **confiável da conexão**
  (actor_kind `agent`, principal/conexão do handshake). Para isso
  `begin_frame(&QuantickApp)` passa a `&mut QuantickApp` — a chamada no
  `app.rs` já faz `take()`/`put back` do `control_access`, então o `&mut` é
  barato. O trace (§11) já é escrito por `invoke_local_action`; ações remotas
  passam **pelo mesmo caminho** — é o critério "toda ação usa o control trace".
- Perfis e escopos: `annotator` existe; acrescentar permissões
  `annotate.chart`, `annotate.notification`, `annotate.sound` (**off por
  padrão**), `annotate.script`; o painel de acesso gera os checkboxes a partir
  de `selectable_permissions` (nada a desenhar à mão); o adaptador aceita
  `--profile annotator` em `crates/mcp/src/main.rs` (`AVAILABLE_PROFILES`),
  com os hints conservadores de `invoke` já implementados em `tools.rs`.
- Limites: `CONTROL_NOTIFICATION_RATE_PER_MINUTE` / `CONTROL_NOTIFICATION_BURST`
  já estão em `quantick_control::limits`; o limitador por cliente é
  `ClientRateLimiter` (`gateway.rs`) — reuse o padrão, não duplique. Dê às
  notificações **uma política de efeito própria** (`notify`) com risk flags
  `user_interrupt` / `audible_output`, para o mark não mentir sobre
  interromper.
- `attach_script`: `quantick_pine::compile` devolve `Vec<PineError>` com
  `code`, `span`, `message`, `notes` → vão como `details` estruturados do
  erro (`ControlError` já tem `context.details`); `IndicatorHost` é headless;
  o anexar/soltar passa pelo **mesmo** caminho que a UI usa para adicionar um
  indicador a um pane (uma função nomeada, o botão chama ela — regra
  "act/read/discover"), e o resultado é lido de volta pelo escopo
  `indicators` (A.1).
- Drawings: anotações do agente são **drawings** com campo de autor
  (`author`: actor kind + client name) **visível no inspector e na context
  bar** — data honesty: objeto do agente indistinguível do do trader é
  finding de Blocker — e removíveis em **uma ação** pelo usuário
  (`annotate.remove` por id + um gesto na UI que remove as anotações daquele
  autor). Memória da casa: "a popup" pode ser a context bar ou o inspector —
  mostrar o autor nas duas.
- MCP: ferramentas nomeadas do contrato §8 — `quantick_annotate`,
  `quantick_notify`, `quantick_attach_script` (+ detach) — em
  `crates/mcp/src/tools.rs`, como `quantick_read_events` foi feito em #223
  (schema embutido + `ErrorResponse` no `oneOf`, hints por perfil).

**Critérios de aceite (plano PR 5b + arch-review dimensão 7):**
1. **O mesmo handler serve UI e agente** — mostrado no PR (teste: a mesma
   função chamada pelo gesto e por um `invoke` remoto com perfil `annotator`
   produz o mesmo evento/objeto, atribuído a atores diferentes).
2. Anotação criada por agente é visivelmente atribuída e o usuário a remove em
   uma ação (teste headless + hook `ui-harness` para a captura; `visual-qa`
   com autorização do dono, senão BLOCKED por escrito).
3. Compile que falha devolve `PineError` (code, span, notes) como dado
   estruturado, nunca string renderizada.
4. Attach bem-sucedido legível no escopo `indicators`; detach restaura o estado
   anterior **exatamente** (snapshot antes == snapshot depois).
5. Nenhuma capacidade do tier descarta estado criado pelo usuário nem toca
   posição — revisado contra a tabela §2.6, dito no corpo.
6. Testes de *flood* de notificação provam rate e burst por cliente; cliente
   sem `annotate.sound` não produz áudio.
7. Toda ação remota aparece no control trace durante replay (teste no molde de
   `a_mark_during_replay_is_traced_and_replayed_at_the_same_logical_time`:
   re-injeção com `target_source: replayed` / ator `automation`).
8. Observer continua sem alcançar nada disso
   (`gateway_observer_cannot_create_a_mark_remotely_…` ganha irmãs para cada
   ação nova).
9. Hooks `ui-harness` para cada superfície nova (autor no inspector, remoção,
   toast/popup) — linha na tabela do skill, como `QUANTICK_CONTROL_MARK`.

**Fora de escopo**: cockpit (PR 6), paper/estratégias (PR 7), export para
disco, qualquer escrita de mercado.

---

## CP-01.D — PR 5c: evidence bundles

Branch: `feat/control-evidence` · worktree `../quantick-worktrees/feat-control-evidence`.

Depende de: A, B, C. Rate class: capturas sob demanda.

**O que é** (plano §8): uma captura coerente — `evidence_id` + hash de
integridade, versão/commit/protocolo, SO/backend gráfico, ids de instância e
sessão, **revisão de captura consistente**, workspace, janela de chart e cena,
dados exatos das projeções, eventos e ações recentes, logs estruturados
relevantes, métricas de frame/feed/book/worker, configuração efetiva **com
redação**, lacunas/dado inferido/campos indisponíveis e **a lista explícita do
que não foi capturado**. Fica **em memória** por retenção limitada e volta
como *resource*; export para disco é ação separada (cockpit, fora daqui).

**Onde docar**: `SnapshotCapture` (captura coerente com revisão) e
`EventPage` já existem; `APP_HEALTH_SUMMARY` / `health.rs` têm as métricas;
`CONTROL_EVIDENCE_*` já estão em `limits`; a permissão `observe.evidence`
(sensível, confirmação *Prompt*) já está declarada em `contract.rs`; a leitura
de *resource* é uma capacidade de leitura paginada por
`CONTROL_MAX_RESPONSE_BYTES` com o cursor `retained_resource`
(`PaginationConsistency::RetainedResource` já existe no contrato). Screenshot
opcional: estampado com a **mesma revisão** da cena, para cada região de pixel
mapear um id de controle/objeto — por isso B vem antes.

**Critérios de aceite (plano PR 5c = aceite do MVP):**
1. Um agente explica a sessão em execução sem screenshot (já vale desde #222;
   o bundle empacota a mesma informação).
2. Mudanças de feed, replay, indicador e erros de conexão aparecem pelo cursor
   (`events.read`/`wait`): os eventos de A alimentam o bundle.
3. O bundle reporta informação omitida e lacunas de cobertura.
4. Bundle com screenshot mapeia **todo** controle nomeado a uma região da imagem.
5. Pelo menos um skill de validação existente (`ui-harness` ou `visual-qa`) lê
   e afirma pelo control plane vivo (o fixture pode continuar vindo de um hook
   determinístico até a ação equivalente existir em PR 6).
6. Redação: nada de token, caminho de usuário, texto de drawing do usuário ou
   chave de config no bundle — teste que procura por eles.
7. Retenção e tamanho limitados por constantes nomeadas; estouro é
   `control.backpressure`/`control.resource_*` do vocabulário existente.

---

## Portões (todos os PRs deste pacote)

Além dos do `DISPATCH.md` (quatro checks, `arch-review` com step 0
`code-review <PR> high`, marker gravado **numa chamada separada** antes de
`gh pr create`, CI acompanhado com `gh pr checks <n> --watch`):

- **Schemas e catálogo regenerados e versionados**; snapshot tests verdes;
  nenhum tipo egui no wire.
- **Performance por classe de taxa no corpo**; hot path com medida
  (`control_idle_dense_replay_benchmark` em pares na mesma janela; guardas de
  orçamento pela mediana). Sem request, zero custo por frame.
- **Segundo operador** (arch-review, dimensão 7): toda ação é chamada nomeada
  com schemas e id registrado, resultado legível, descoberta por `describe`;
  autoria gravada e visível; nenhuma ação de mercado/segurança por caminho
  mais curto que o do trader (Blocker).
- **`workspace_deps`** e `CLAUDE.md` se surgir crate novo (não deve surgir:
  tudo aqui doca em `app`, `mcp` e `control`).
- **`ui-harness`**: toda superfície nova com hook registrado na mesma mudança;
  `visual-qa`/`trader-ux-review` só com autorização do dono para abrir o app —
  senão, BLOCKED por escrito no corpo, nunca pulado em silêncio.
- Testes com nome no idioma da casa (frase declarativa com contraste), e cada
  achado confirmado do review ganha um teste que falha sem a correção.
- Corpo do PR no molde de #221–#223: *Summary · Rate class and tier ·
  Docking/Blast radius · Acceptance (tabela critério → teste) · Deferred ·
  Verification · Architecture review (step 0 + shape)*.

## Lacunas herdadas da fase 1 (carregar, não esquecer)

- Revisões de módulo são derivadas de captura (deep compare); o sucessor de
  #223 deve torná-las contadores dirigidos pelo journal (dá `module_revision`
  aos eventos).
- Limpeza de descritores obsoletos e detecção de reuso de PID (ADR-0001 §5) —
  housekeeping do adaptador.
- Trace: o sidecar é aberto por ação com dois `sync_data` no thread da UI
  (por gesto); manter o handle por sessão e tirar o sync do resultado do frame
  quando uma medição em disco lento disser que importa.
- `interaction.selection.changed` não dispara mais para mudança de
  propriedade do drawing selecionado (lock/hide/rename) — os eventos do módulo
  `drawings` (A.1) devem carregar isso.
- Ações que resolvem estado na hora da chamada (o mark sem `target` resolve o
  ponteiro): o trace grava o `canonical_input` **antes** do handler, logo um
  chamador remoto que omite o target deixaria um intent não reproduzível
  (hoje o replay o recusa). Em C, dê à porta de ações um passo `resolve`
  (canonicalizar o input antes da linha de intent) para que o trace grave o
  input efetivo — vale para label/arrow/zone também.
- Checagem ao vivo Codex/Claude contra uma instância desktop nunca foi rodada
  (sem autorização de abrir o app): `quantick-mcp setup --client codex|claude`
  imprime os comandos; o primeiro agente com autorização roda e registra.

## Portão final — §18 depois deste pacote

| Critério §18 | Fecha com |
| --- | --- |
| agente responde no chart via annotate, autoria visível, remoção em uma ação | C |
| indicador descrito em prosa, compilado, corrigido por diagnóstico, anexado | C (+ leitura por A.1) |
| evidence bundle reproduz uma investigação | D |
| mudanças de feed/replay/indicador/conexão pelo cursor | A |
| `quantick_get_scene` | B |
| o resto (acesso opt-in, MCP, schemas, explicar sem screenshot, apontar, cursor, sem escrita financeira, orçamento por frame, hot paths, saúde, gates) | já fechado na fase 1 |

Quando as quatro letras estiverem mergeadas, o MVP do plano está completo e o
próximo documento é o de PR 6 (cockpit) — que **não** começa antes de o dono
decidir sobre a camada de autoridade (§9.2).
