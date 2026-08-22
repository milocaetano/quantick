---
name: flake-hidden-layer-paints-nothing
description: a_hidden_layer_paints_nothing falha esporadicamente no CI carregado; rerun resolve — não é o diff
metadata: 
  node_type: memory
  type: project
  originSessionId: 4b8bdcb1-2485-44f4-a7f7-107ad3b19eba
  modified: 2026-08-18T22:22:31.483Z
---

Os testes de app que comparam **contagem total de shapes** entre dois frames
falham esporadicamente **só no runner do CI**, com a contagem *aumentando*.
Família confirmada, não um teste só:

- `a_hidden_layer_paints_nothing` — 349 vs 329, PR #202, 2026-08-18.
- `the_trade_paint_layer_switch_stops_the_marks` — **288 vs 203**, PR #217,
  2026-08-21. Rerun passou sem tocar em código; 15/15 isolado e 3/3 na suíte
  completa no Windows.

Falhas de CI no próprio `main` (runs 32308317414, 32260800751) confirmam que
não depende do diff.

**Why:** a projeção do orderflow chega de um worker em background; num runner
lento a publicação aterrissa entre duas medições e adiciona ~20 shapes de
heatmap. O próprio comentário do teste registra o histórico ("fail on a
loaded CI runner and pass on the next attempt") — o settle de dois frames
reduz mas não elimina a corrida.

**How to apply:** se um desses for o único vermelho num CI cujo diff não toca
camadas, rodar `gh run rerun <id> --failed` antes de investigar. Se falhar
duas vezes seguidas no mesmo commit, aí deixa de ser flake. Sinal de que é o
flake e não regressão: a contagem *sobe* ao desligar a camada — desligar algo
nunca desenha mais. Conserto real (merece missão própria): comparar as
primitivas da camada em questão em vez do total do canvas, ou congelar a
publicação do worker durante a medição.
