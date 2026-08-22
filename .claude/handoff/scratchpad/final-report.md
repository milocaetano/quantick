# Relatório final — missão "finalizar o control plane MCP (MVP)"

Missão: levar `docs/mcp-control-plane-development-plan.md` ao seu *definition of done* (§18), terminando o que o Codex deixou na árvore e entregando os PRs restantes na ordem do plano — cada um em branch/worktree próprio, empilhado no anterior. O merge não faz parte da missão (é do dono); os PRs empilhados re-apontam a base quando a anterior entra.

## 1. PRs (todos abertos, CI verde)

| # | Entregável | PR | Base | Head | CI |
| --- | --- | --- | --- | --- | --- |
| D1 | hardening do contrato `quantick-control` | https://github.com/milocaetano/quantick/pull/220 | `main` | 87907656 | pass 5m17s |
| D2 | PR 2 — observer (projeções, captura coerente, cursor) — promovido de draft para ready | https://github.com/milocaetano/quantick/pull/213 | `main` | 48cec863 | pass 5m17s |
| D3 | PR 3 — gateway local + crate `quantick-control-local` | https://github.com/milocaetano/quantick/pull/221 | `feat/control-observer` (#213) | 60425039 | pass 5m37s |
| D4 | PR 4 — `quantick-mcp` (adaptador STDIO MCP) | https://github.com/milocaetano/quantick/pull/222 | `feat/control-gateway` (#221) | f2ee943a | pass 6m11s |
| D5 | PR 5a — journal, cursor, `events.read`/`events.wait`, mark Ctrl+M, action registry, control trace | https://github.com/milocaetano/quantick/pull/223 | `feat/mcp-observer` (#222) | d3a5fa5c | pass 5m47s |
| D6 | PR 5b — annotate/notify/attach_script | — | — | — | **não iniciado** |
| D7 | PR 5c — evidence bundles | — | — | — | **não iniciado** |
| D8 | scene + módulos de snapshot restantes (indicators/drawings/orderflow/replay/paper) | — | — | — | **não iniciado** |

Ordem de merge: #220 (independente) e #213 → #221 → #222 → #223.

## 2. Critérios do GOAL.md, um a um, com evidência

### D1 — hardening PR
- Stash aplicado; `deny_unknown_fields` (conflitava com `flatten`) substituído por transform de schema com nomes reservados (contrato §6, leitor tolerante); teste de acordo schema/codec mantido; schemas regenerados. Evidência: commits em `fix/control-contract-hardening` (head 87907656), corpo do #220.
- Quatro checks exit 0; arch-review reportado no corpo (step 0 `code-review high`: achados resolvidos/deferidos com motivo); marker gravado; PR #220 aberto; CI pass.

### D2 — PR #213 ready
- Rebase em `main` (renomes `dropped_aggressions→folded_aggressions`, 5º argumento de `project_visible`, schemas regenerados); quatro checks exit 0; step 0: 9 achados, 8 resolvidos em 48cec863, 1 deferido com motivo; promovido de draft para ready-for-review com evidência no corpo (revisão de saúde = só abas, `CAPABILITY_UNAVAILABLE` retryable para chart antes do primeiro paint, `market_data_provenance` compartilhado, teste de orçamento pela mediana + p99 `#[ignore]`); CI pass.

### D3 — PR 3 gateway
- Trabalho do Codex commitado e rebaseado no PR 2; discovery + cliente loopback extraídos para `crates/control-local`; hooks `QUANTICK_CONTROL_PANEL`/`QUANTICK_CONTROL_ACCESS`; correções de auditoria (read timeout ciente de idle, `request_id` duplicado, timeout anunciado, discovery limitado) + 7 testes; testes exigidos pela ADR-0001 conferidos (presentes ou listados como gap no corpo); evidência de hot path: benchmark idle 8 pares candidato × controle, candidato nunca mais lento (números no corpo); quatro checks exit 0; step 0: 6 achados plausíveis/baixos deferidos com motivo; PR #221 com base `feat/control-observer`; CI pass.
- Após o primeiro CI do #222 expor `budget_exceeded` com `>` vs `>=` do loop (igualdade após truncar `as_micros`), corrigido em 60425039 — quatro checks exit 0, CI pass.

### D4 — PR 4 `quantick-mcp`
- Crate folha + binário, STDIO apenas; ferramentas `quantick_describe`, `quantick_get_snapshot`, `quantick_get_chart_window`, `quantick_get_diagnostics`, `quantick_search_capabilities`, `quantick_invoke` (`get_scene` omitido: o módulo scene não existe); seleção de instância (1 → escolhe, 0 → `control.instance_gone`, N → `control.instance_ambiguous` ordenado, `--instance` pina); instructions ≤ 512 chars; annotations read-only por perfil (contrato §8, testadas); smoke test de pureza do stdout; assistente `setup --client codex|claude`; `workspace_deps` ALLOWED `mcp → control, control-local`; entrada no `CLAUDE.md`; fake second host (`FakeLink`) nos testes; blast radius no corpo (12 arquivos novos + linhas de registro).
- Step 0: 6 achados (1 confirmado: `structuredContent` de erro vs `outputSchema`; 5 plausíveis) — todos corrigidos; 22 unit + 3 fake-gateway + 3 stdio smoke verdes; quatro checks exit 0; PR #222 (base `feat/control-gateway`); re-empilhado sobre o fix do PR 3; CI pass.

### D5 — PR 5a eventos + apontar
- Journal em anel (entradas e bytes) no thread da aplicação, posição publicada por dois atômicos; cursor; `events.read` pela fila de UI; `events.wait` estacionado no lado do gateway (slot de waiter, nunca slot de UI) com waiter manager; eventos de seleção/aba/foco/feed/replay; hotkey Ctrl+M → `attention.mark.create` pela porta `ActionRegistry` (perfil `annotator` e permissões declarados, observer recusado por teste); porta `ControlTrace` (`ReplayTraceFile` + `NoTrace`), sidecar `<sessão>.control-trace.jsonl`, re-injeção no tempo lógico do replay; hook `QUANTICK_CONTROL_MARK` (linha no `ui-harness`); MCP `quantick_read_events`/`quantick_wait_for_change`; 5 schemas novos + catálogo regenerado.
- Step 0: 10 achados (4 confirmados) — 9 corrigidos, 1 deferido e declarado (dois `fsync` por gesto humano em replay): re-injeção por aba e por posição (troca de aba não duplica; restart/seek re-injeta), wait estacionado mantém `request_id` (duplicata recusada), falha ao criar a thread do wait responde backpressure, `dropped_before` visto no park chega na página, `timed_out` honesto, `CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION = 4` + liberação ao desconectar, emissor sem alocação por frame (`SelectionIdentity`), pareamento intent/result pelo último da sequência + sequência contínua, `target_source: pointer | supplied | replayed` e resultado do mark sem wall-clock. Testes novos: `gateway_rejects_a_duplicate_request_id_while_a_wait_is_parked`, `a_wait_page_keeps_the_gap_the_gateway_saw_and_is_no_timeout_once_events_landed`, `a_result_pairs_with_the_latest_intent_of_its_sequence`; os existentes estendidos (troca de aba/restart; cap por conexão/global/desconexão).
- Quatro checks exit 0; PR #223 (base `feat/mcp-observer`); primeiro CI caiu por corrida no teste novo (ordem entre conexões) — teste corrigido (d3a5fa5c), CI pass.

### D6 / D7 / D8
- **Não iniciados** (explícito). Handoff em `scratchpad/handoff-5b-5c-scene.md`: 5b docka em `ActionRegistry` (+ `annotate.label/arrow/zone`, `notify.*`, `indicator.script.attach/detach`), despacho remoto de ações via `PreparedDispatch::Action` no `ObserverContract::prepare`, perfil `annotator` + permissões `annotate.chart/notification/sound/script`, limites de notificação já em `limits`, `--profile annotator` no adaptador; 5c docka em `SnapshotCapture`/`EventPage`/`APP_HEALTH_SUMMARY` com `CONTROL_EVIDENCE_*`; scene/módulos via `ProjectionRegistry::register_module` (um arquivo por módulo + uma linha em `standard_registry`).

### Gates padrão (cada entregável)
- Quatro checks exit 0 após rebase na base mais recente: sim, em D1–D5, inclusive após cada re-stack.
- Impacto de performance declarado por classe de taxa nos corpos; hot path com evidência medida (D3 benchmark idle; D2/D3 testes de orçamento de captura; D5 emissor sem alocação por frame, declarado e revisado).
- `arch-review` com step 0 em cada PR, Blockers/Should-fix resolvidos ou deferidos no corpo; marker gravado antes de cada `gh pr create`.
- Capacidade nova: porta nomeada, edições só de registro, defaults preservam o comportamento atual, segunda implementação fake testada, blast radius no corpo (D3 `control-local`; D4 `ControlLink`/`FakeLink`; D5 `ActionRegistry`, `ControlTrace`/`NoTrace`, `PreparedUiRead`).
- Ação do trader drivável sem mouse: o mark é uma chamada nomeada com schemas e ID registrado, resultado estruturado, descoberta por `describe` (act/read/discover).
- Superfície visível: hooks registrados (`QUANTICK_CONTROL_PANEL`, `QUANTICK_CONTROL_ACCESS`, `QUANTICK_CONTROL_MARK`); trader-ux-review sem Blocker (corpo do #223); **visual-qa BLOCKED** — sem autorização para abrir o app nesta sessão (reportado, não omitido).
- PR aberto com CI verde e evidência no corpo: sim, os cinco. Merge: do dono.

## 3. O que resta do §18 (MVP definition of done)

| Critério §18 | Estado |
| --- | --- |
| instância anuncia acesso local opt-in | ✅ PR 3 (#221) |
| Codex e Claude conectam via MCP | ✅ adaptador + `setup` (PR 4, #222); checagem ao vivo contra instância desktop **BLOCKED** (sem autorização para abrir o app) |
| ferramentas de leitura retornam schemas documentados | ✅ PR 4 (oneOf resultado/ErrorResponse, validado por teste) |
| agente explica a sessão sem screenshot | ✅ PR 2+3+4 (describe / snapshot / chart window / diagnostics) |
| usuário aponta barra/célula/objeto e o agente nomeia exatamente | ✅ PR 5a (#223: mark + `wait_for_change`) |
| agente responde no chart pela camada annotate, autoria visível, remoção em uma ação | ❌ 5b — não iniciado |
| indicador descrito em prosa, compilado, corrigido por diagnóstico estruturado e anexado | ❌ 5b — não iniciado |
| mudanças seguidas com cursor | ✅ PR 5a |
| evidence bundle reproduz uma investigação | ❌ 5c — não iniciado |
| nenhuma escrita de cockpit/financeira disponível | ✅ observer apenas; mark só local; testado |
| orçamento de controle por frame imposto e medido | ✅ PR 3 (drain limitado por contagem e tempo; testes) |
| hot paths ociosos sem lock/alocação nova | ✅ PR 2/3 benchmark; PR 5a emissor em comparação in-place |
| métricas de saúde sem regressão material vs `origin/main` | ✅ 8 pares na mesma janela (corpo do #221) |
| testes, quatro checks, arch-review e CI passam | ✅ em cada PR |

## 4. Bloqueios e pendências fora do código
- QA visual (painel de acesso, Ctrl+M) e teste ao vivo Codex/Claude: BLOCKED por falta de autorização para abrir o app; hooks e `quantick-mcp setup` cobrem a reprodução.
- `GOAL.md` arquivado em `.claude/GOAL-archive-mcp-control-plane.md`.
