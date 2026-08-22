---
name: rail-horizontal-trailing-cluster-clipped
description: "Bug pré-existente — no dock horizontal do rail, o cluster trailing renderiza fora dos 44 px e fica inacessível"
metadata: 
  node_type: memory
  type: project
  originSessionId: db556f4a-66ba-4ea2-acb3-6792e9608329
  modified: 2026-08-14T04:04:57.370Z
---

Com o rail de desenho docado em `top` ou `bottom`, o cluster trailing
(magnet, repeat, hide-all, lock-all, Objects) é desenhado **abaixo** da faixa
de 44 px do painel e some por clipping — os cinco controles ficam
inalcançáveis. Medido em 2026-08-14 com tela 560×900, dock Top: o rail é
`[0,0]-[560,44]` e `objects_rect` cai em `[522,41]-[554,73]`. Reproduz igual
nos estágios Full (largura 900), Scroll (560) e Compact (450).

**Why:** é anterior ao PR #179 (banda rolável) — os estágios Full e Compact
não passam por nenhuma linha daquele branch, o que isola a causa no layout
`bottom_up` / `right_to_left` do cluster trailing em `draw_contents`
(`crates/app/src/toolrail.rs`), não na banda.

**How to apply:** não é regressão de [[toolbar-overflow-scroll]]; ainda não
tem issue aberta. Ao mexer no rail horizontal, começar por aqui. O teste
`the_band_scrolls_along_the_long_axis_in_every_dock` afere só o eixo longo
justamente por causa disto, e diz o porquê em comentário.
