---
name: xp-bid-ask-sao-limites-de-banda
description: "Nos ticks de trade do MT5 da XP, bid/ask trazem os limites de oscilação do dia, não o book"
metadata: 
  node_type: memory
  type: project
  originSessionId: 1ea747c9-bf70-4265-b321-95ef13d4b8f3
  modified: 2026-08-14T13:50:45.945Z
---

Nos ticks `COPY_TICKS_TRADE` que o terminal da XP serve para WIN, os campos
`bid`/`ask` não são o topo do book: são os **limites de banda do dia**. Num
tape de WINV26 em 2026-08-13, com preço 170665, veio `bid=187950` e
`ask=153780` — bid acima de ask, ambos muito fora da faixa negociada.

**Why:** `spread_side` (tools/mt5/export_session.py) decide o lado por
`price >= ask` → "B", `price <= bid` → "S". Com o ask abaixo de todo preço
negociado, o primeiro teste é sempre verdadeiro e qualquer print sem flag do
venue receberia "B" — uma inferência errada que se apresenta como evidência.
Hoje não custa nada porque os dias checados vieram com `inferred=0`, mas é um
fallback que não sabe falhar alto.

**How to apply:** ao mexer em inferência de lado no exportador ou no bridge,
tratar book invertido (`bid >= ask`) como ausência de cotação, não como
evidência. Deferido no PR #182; merece issue própria. Ver
[[operacional-mark-i]] — o delta do WIN já é tick rule por esse mesmo motivo.
