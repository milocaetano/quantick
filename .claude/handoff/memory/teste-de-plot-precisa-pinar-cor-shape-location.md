---
name: teste-de-plot-precisa-pinar-cor-shape-location
description: "cor, shape e location de plotshape caem em default silencioso — teste que só lê linhas não-NaN não pega troca de abovebar/belowbar"
metadata: 
  node_type: memory
  type: project
  originSessionId: 18049ced-493e-40b2-b6f9-4574c70ed952
  modified: 2026-08-20T02:47:41.243Z
---

Num teste de semântica de script Pine, ler só **quais linhas têm valor
não-NaN** não prova quase nada sobre o desenho. Cor, `shape.*` e `location.*`
dobram no load com **fallback silencioso** (`compile.rs`:
`.unwrap_or(MarkerShape::TriangleUp)`, `.unwrap_or(MarkerLocation::AboveBar)`,
`.unwrap_or(DEFAULT_PLOT_COLOR)`).

**Why:** trocar `location.abovebar` por `belowbar` entre os dois `plotshape`,
ou trocar as duas cores, deixa a suíte inteira verde enquanto a seta de venda
é desenhada embaixo da mínima. `force_bar_semantics.rs` já dizia isso no
próprio docstring ("each assertion also proves that *that particular*
input.color reached the paint channel"), e o teste novo tinha ignorado.

**How to apply:** todo script com `plotshape` ganha um teste que lê o
`descriptor().plots` e asserta, por plot: `marker.shape`, `marker.location` e
`base_color` (valores da paleta em `compile.rs:238` — `color.red` =
`0xF23645FF`, `color.teal` = `0x00897BFF`). Uma asserção por lado, para que as
duas não possam ser trocadas entre si.

Relacionado: [[pine-input-color-nao-chega-ao-plot]].
