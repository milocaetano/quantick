# Mission: Inverted chart — ARQUIVADA (PR #202 aberto)

Um jeito de inverter o gráfico verticalmente, com dois caminhos: contínuo
(arrasto do gutter até achatar no limiar e virar no puxão seguinte; sentido
espelha invertido; roda nunca flipa) e discreto (menu de contexto do eixo,
"Inverted chart"). Branch `feat/inverted-chart`, worktree
`../quantick-worktrees/feat-inverted-chart`, PR
https://github.com/milocaetano/quantick/pull/202.

## Critérios — estado final

1. [x] Flip contínuo pelo arrasto — `PriceView::drag_zoom` com
       `FLIP_SPAN_FACTOR` (40 spans) + `FLIP_REARM_FRACTION` (histerese);
       testes de unidade (achatamento, flip no segundo puxão, volta, tremor)
       e teste harness do gesto real no gutter.
2. [x] Menu no eixo — checkbox "Inverted chart" no gutter;
       `QUANTICK_CONTEXT_MENU=axis` + `QUANTICK_INVERTED=1` registrados no
       `ui-harness`.
3. [x] Inversão sem fork — `PriceScale` (+ `band`) e `ProjectedLayout` são as
       duas fronteiras; 15 findings do code-review auditaram os consumidores
       e 13 foram corrigidos (pavios, footprint, zonas, profile, strip,
       marcadores Pine, arrow marks, trade paint, handles de bracket, nudge,
       bolhas).
4. [x] Segundo operador — act `PriceView::set_inverted`; read
       `is_inverted()`; discover: tabela ui-harness + hooks.
5. [x] Performance — MEDIDA (autorização dada em 2026-08-18): replay
       WINQ26 8×, 42 amostras/célula, release. frame_cpu mediano 1,90ms
       (main) / 1,93ms (branch upright) / 1,95ms (branch invertido) com
       carga idêntica (249 bolhas) — flat; números no PR #202.
6. [x] `visual-qa` — RODADO off-screen: 6 células PASS, 1 defeito real
       achado pela captura (time pane não herdava a inversão no boot —
       nasce um frame depois dos hooks) e corrigido em cd867e9b com teste
       harness. Recaptura visual do split invertido pendente de desktop
       livre (sessão bloqueou no meio; comportamento provado por teste).
7. [x] Quatro checks verdes (fmt, clippy -D warnings, build, test — 1480
       testes, 0 falhas) na base `origin/main` d7d8fd58.
8. [x] arch-review: passo 0 code-review em high, 15 findings, 13 corrigidos,
       2 deferidos nomeados no PR (persistência da orientação; porta
       scriptada do menu do eixo do time pane). Marcador `arch-review-ok`
       gravado para o HEAD 503ef707.
9. [x] PR #202 aberto com evidências. CI em observação. Merge é do Camilo.

## Encerramento

Autorização dada; perf e visual-qa rodados e anexados ao PR #202. Demo
visível rodado para o Camilo ("fechou ficou bom"). Shots em
`scratchpad/tape-qa/shots/` da sessão 4b8bdcb1. Merge aguarda o fluxo de
sempre (outro agente, aviso do Camilo).
