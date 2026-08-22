---
name: captura-pega-janela-errada-do-app
description: MainWindowHandle do quantick-app às vezes aponta para janela auxiliar — capturar a MAIOR janela visível do PID
metadata: 
  node_type: memory
  type: reference
  originSessionId: 18049ced-493e-40b2-b6f9-4574c70ed952
  modified: 2026-08-20T02:47:54.557Z
---

O `quantick-app.exe` tem **três janelas top-level** por processo: a do chart
(título `quantick`), uma de 16x16 sem título, e uma cujo título é o caminho do
exe. `(Get-Process -Id $pid).MainWindowHandle` às vezes devolve a última —
resultado: PNG preto de 993x519 e a impressão falsa de que o app não renderizou.

**Why:** o `heatmap-design-ref/capture_window.ps1` filtra por nome de processo
e usa `MainWindowHandle`, então herda o mesmo problema. `fps` saudável no log
com captura preta é o sintoma — a janela está renderizando, a errada foi
fotografada.

**How to apply:** enumerar com `EnumWindows`, filtrar por PID + `IsWindowVisible`,
e capturar a de **maior área**. Script pronto em
`scratchpad/vqa/capture_best.ps1` (imprime as três janelas com título e tamanho
antes de escolher).

Armadilha vizinha: o `QUANTICK_UI_STATE` do scratchpad guarda o tamanho da
janela, então uma run anterior pode fazer a seguinte abrir em 160x28 e o
`QUANTICK_WINDOW_SIZE` não corrige. Apagar `ui-state.toml`,
`indicators-state.toml` e `chart-layers.toml` entre runs de validação.

Ver também [[captura-offscreen-sem-roubar-foco]] e [[rodar-app-nao-e-qa]].
