---
name: pane-de-fluxo-fica-abaixo-dos-limiares
description: O layout do Camilo (split 1901px) dá ~1080px ao pane de fluxo — abaixo do limiar de 1180px do auto-pin
metadata: 
  node_type: memory
  type: project
  originSessionId: cebd95b1-1dde-4fba-b069-d4bbc5e9b0ac
  modified: 2026-08-17T20:38:09.182Z
---

A workspace real do Camilo (`ui-state.toml`) abre em janela de **1901×1006**
com `layout = "time+flow"` e `split_fraction = 0.368`. O `split_fraction` é a
fatia do pane de *tempo*, então o pane de fluxo fica com ~63% da tela: descontando
rail, faixa direita e eixo de preço, o **chart do pane de fluxo tem ~1080 px**.

**Why:** `INSPECTOR_AUTO_PIN_CHART_WIDTH_PX = 1180` — o pane onde os desenhos
vivem fica *abaixo* do limiar. Qualquer feature de janela flutuante sobre o
chart parece quebrada para ele por padrão: em 17/08/2026 a memória de posição
do popup de propriedades funcionava em todos os testes e em janela única, e
teria chegado inerte na máquina dele, porque toda seleção acoplava o painel.

**How to apply:** ao mexer em qualquer coisa que dependa de largura de chart,
medir contra ~1080 px e não contra a largura da janela. Reproduzir com
`QUANTICK_LAYOUT=time+flow QUANTICK_WINDOW_SIZE=1901x1006` — e capturar nesse
layout, não só em `flow`, senão o defeito não aparece. Ver
[[camada-provada-nao-prova-a-irma]]: painel único provado não prova o split.
