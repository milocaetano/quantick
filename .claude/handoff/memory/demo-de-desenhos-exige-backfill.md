---
name: demo-de-desenhos-exige-backfill
description: "QUANTICK_DRAWINGS_DEMO não faz nada com poucas barras — precisa de ~192 slots, ou seja QUANTICK_BACKFILL alto"
metadata: 
  node_type: memory
  type: project
  originSessionId: 137c3ce8-2470-4e79-95a7-ada05441ba16
  modified: 2026-08-18T22:20:04.397Z
---

`QUANTICK_DRAWINGS_DEMO=1` exige `slots >= 8 * DRAWING_TOOLS.len()` — hoje
~192 barras. Abaixo disso ele **não coloca nada e não avisa**: a captura sai
com o chart limpo, e como o inspector só existe com objeto selecionado,
`QUANTICK_DRAWING_INSPECTOR=1` também parece quebrado.

**Why:** num tick(50) ao vivo, esperar 192 barras leva dezenas de minutos —
duas capturas de 45 s saíram vazias antes de eu ler a condição no código, e o
sintoma (nada na tela) não aponta para a causa (poucas barras).

**How to apply:** parear sempre com `QUANTICK_BACKFILL=15000` (15 000 trades ÷
50 por barra = 300 slots) e conferir no rodapé do app que a contagem de barras
passou de 192 antes de acreditar numa captura vazia. Vale para os hooks
irmãos que também esperam barras: `QUANTICK_DRAWING_DRAFT`,
`QUANTICK_TEXT_NOTE`, `QUANTICK_FRVP_DEMO`, `QUANTICK_AVWAP_DEMO` — esses
pedem menos barras, mas nenhum deles reclama quando não roda.

Ver [[rodar-app-nao-e-qa]] para os stores em scratchpad e
[[desktop-ocupado-bloqueia-captura]] para quando o frame sai em branco.
