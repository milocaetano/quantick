---
name: gap-da-fita-sao-dois-relogios
description: "Bolha longe da borda da fita = latest_book_ms − latest_print_ms, não bug de render"
metadata: 
  node_type: memory
  type: project
  originSessionId: 9892839e-28c2-457f-90a1-25a752119b6f
  modified: 2026-08-21T14:26:34.559Z
---

Quando as bolhas de agressão param antes da borda direita da fita enquanto o
heatmap L2 chega até ela, a distância **é exatamente** `latest_book_ms −
latest_print_ms`. Não é atraso de frame nem de projeção.

Por quê: `history.rs::latest_ms()` = `max(relógio do book, relógio dos
negócios)` e a borda da lane é esse máximo (`orderflow_engine.rs::live_edge`);
o mapa L2 é desenhado até `latest_book_ms` (`projection.rs`, `open_run_end_ms`)
e as bolhas são posicionadas pelo relógio dos prints. Passando a janela da
fita, os prints voltam para o slot da barra — **fita vazia nunca quer dizer que
nada negociou**.

Diagnóstico: ler `tape_newest_print_age_ms` no `APP_HEALTH_SUMMARY` (existe
desde `fix/tape-keeps-up-with-the-tape`) e `BRIDGE_TAPE_STATS.tick_lag_ms` no
Experts do MT5. Número grande com mercado ativo = entrega; acompanhando os
períodos mortos = mercado parado mesmo.

Frame atrasado do worker **não** produz esse desenho: o frame antigo é esticado
na lane inteira, as bolhas encostariam na borda. Ver
[[win-nao-tem-tick-de-cotacao]] antes de acusar a ponte de backlog.
