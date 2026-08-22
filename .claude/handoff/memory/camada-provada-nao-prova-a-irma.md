---
name: camada-provada-nao-prova-a-irma
description: Provar bolhas não prova L2 — no quantick as duas nascem em caminhos de produção diferentes
metadata: 
  node_type: memory
  type: feedback
  originSessionId: c043459e-c654-4c58-a35b-d4e6027bf9d3
  modified: 2026-08-16T01:06:33.326Z
---

Em 15/08/2026, ao separar os switches do tape dos candles, provei a
independência com as **bolhas** (teste + captura, nos dois sentidos) e tratei o
**L2** como o mesmo caso. Não era: o Camilo testou no app e o L2 do tape ainda
apagava junto com o do gráfico. O erro estava em
`orderflow/projection.rs`, onde a produção das células do heatmap era gateada
por `config.depth_visible()` — o switch dos candles — enquanto o recorte por
painel acontece só depois, no renderer (`layer_clip`). O dado morria antes de
alguém decidir onde desenhá-lo.

**Why:** as duas camadas parecem simétricas na config e não são no pipeline. As
bolhas passam por `any_layer_enabled()`; as células do mapa têm um portão
próprio em `project_settled` e outro em `project_live`. Corrigir a config e
provar com a camada mais barata deixa a outra quebrada com a suíte verde — e
havia até um teste afirmando o comportamento errado como garantia
(`a_hidden_depth_map_projects_no_depth_primitives`).

**How to apply:** quando mexer num switch que governa mais de uma camada,
rastrear **cada** camada até onde o dado é *produzido*, não só até a config —
`grep` pelo resolvedor da config dentro de `projection.rs` e do render antes de
declarar pronto. E desconfiar de teste existente que afirma exatamente o
comportamento que se está mudando: ele pode estar codificando o bug. Ver
[[operacional-mark-i]] para por que o L2 do tape importa tanto no operacional.
