---
name: pine-input-color-nao-chega-ao-plot
description: "input.color no Pine do quantick não chega a plot/plotshape — cai no âmbar default, sem warning; cor de plot é a aba Style"
metadata: 
  node_type: memory
  type: project
  originSessionId: 18049ced-493e-40b2-b6f9-4574c70ed952
  modified: 2026-08-20T02:47:26.389Z
---

No dialeto Pine do quantick, `input.color(...)` passado como `color=` de um
`plot`/`plotshape` **não funciona**: a cor sai âmbar (o `DEFAULT_PLOT_COLOR`)
e **nenhum warning é emitido** — `plotshape` não chama `warn_unfoldable_plot_arg`.

A causa está em `crates/pine/src/compile.rs`: o `fold()` só dobra
`InputInt | InputFloat | InputBool | InputString`. `Builtin::InputColor` está
fora de propósito — dobrar congelaria o *default* no load, e a cor que o trader
escolhesse no diálogo nunca chegaria ao desenho.

**Why:** o diálogo renderiza o color picker mesmo assim, então o trader ganha
um controle que não move nada — pior que controle ausente, pela regra da casa
(`app.rs`: "an unsupported property is *absent* from the inspector, never
present and inert"). `force_bar.pine` escapa porque usa `input.color` em
`barcolor()`, que é canal de eval-time, não de load-time.

**How to apply:** num script novo, cor de plot vem de literal (`color.red`,
`color.teal` — esses dobram) e a customização por plot fica na **aba Style**
do diálogo (`ResolvedPlot { visible, color, width }`, persistida). Nunca
declare `input.color` para um plot. Se precisar provar que não voltou, asserte
no teste que nenhum `InputSpec::Color` existe no descriptor — foi assim que
`exhaustion_reversal.pine` pinou isso.

Detectado por captura de tela, não por teste: os 19 testes de linha passavam
verdes com as duas setas da mesma cor. Ver [[teste-de-plot-precisa-pinar-cor-shape-location]].
