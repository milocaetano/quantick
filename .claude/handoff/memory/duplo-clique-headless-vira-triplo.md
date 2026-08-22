---
name: duplo-clique-headless-vira-triplo
description: double_clicked() não dispara em teste headless logo após outro clique — o egui conta como triplo
metadata: 
  node_type: memory
  type: project
  originSessionId: e13efc7c-b26c-4400-a3bd-24baa860ab2e
  modified: 2026-08-18T07:59:52.407Z
---

Nos testes de frame do `quantick-app`, um par press/release/press/release
entregue pouco depois de qualquer outro clique é lido pelo egui como **triplo**
clique (`count == 3`), e `Response::double_clicked()` fica `false`.

**Why:** o egui compara o release com `last_last_click_time` contra
`2 * max_double_click_delay` (0,6 s por padrão). Como `run_frame_sized` manda
`RawInput` sem `time`, o relógio anda só `predicted_dt` (1/60 s) por frame — os
cliques de setup ficam dentro da janela.

**How to apply:** rodar frames vazios até passar de `2 * max_double_click_delay`
antes do duplo-clique, derivando a contagem de
`ctx.options(|o| o.input_options.max_double_click_delay)` e
`ctx.input(|i| i.predicted_dt)` em vez de um literal. Exemplo em
`a_parked_context_bar_greets_the_next_drawing_too` (`crates/app/src/app.rs`).
Sintoma de que falta isso: `clicked=true` mas `double=false`.
