---
name: aba-orfa-some-no-save-on-exit
description: Aba salva com feed/símbolo que não existe mais é descartada na subida e apagada no exit
metadata: 
  node_type: memory
  type: project
  originSessionId: a1bee7a4-f8db-4a92-8b0f-1d497aa9b0f3
  modified: 2026-08-18T13:10:45.240Z
---

O `ui-state.toml` do Camilo guardava duas abas, uma delas
`feed = "binance", symbol = "WINQ26"` — par que não existe: WINQ26 é contrato
B3, servido pelo feed `metatrader-b3`, e o contrato já rolou para **WINV26**
(é o que `quantick-symbols.toml` registra).

Na subida o app faz a coisa honesta e diz por quê:
`UI_STATE_TAB_DROPPED feed=binance symbol=WINQ26` +
`UI_STATE_RESTORED_PARTIAL saved=2 restored=1`.

**Why:** com `save_on_exit = true` (o padrão dele), fechar o app regrava o
arquivo com as abas que sobreviveram — a aba órfã some do disco de vez. O
descarte é visível só no log; na tela é uma aba que "sumiu sozinha".

**How to apply:** ao ver `UI_STATE_RESTORED_PARTIAL` no log, avisar antes de o
app ser fechado, e copiar a casa para `Documents\Quantick-backup-<data>`
primeiro. Consertar é editar a aba no `ui-state.toml` com o app fechado
(`feed = "metatrader-b3"`, `symbol = "WINV26"`), não recriar na mão na UI.
Ver [[rodar-app-nao-e-qa]].
