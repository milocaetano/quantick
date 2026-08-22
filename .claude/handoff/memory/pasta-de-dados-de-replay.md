---
name: pasta-de-dados-de-replay
description: "As gravações de replay do Camilo ficam em C:\\Users\\Camillo\\Quantick\\replay, por símbolo"
metadata: 
  node_type: memory
  type: project
  originSessionId: 1ea747c9-bf70-4265-b321-95ef13d4b8f3
  modified: 2026-08-18T06:56:02.936Z
---

A pasta de replay do Camilo é `C:\Users\Camillo\Quantick\replay\` (verificado
em 2026-08-18; `C:\Users\Camillo\quantick-data` não existe mais), com uma
subpasta por símbolo (`WINQ26/`, `WINV26/`) e o tape de cada dia em
`<YYYY-MM-DD>.csv` (~60–70 MB por pregão do mini índice; WINV26 2026-08-12
tem 1.398.935 prints).

**Why:** o app resolve essa pasta por `QUANTICK_REPLAY_DIR` ou pelo picker, e
nada no repo aponta para ela — sem isso, cada sessão sai procurando no disco.

**How to apply:** usar como `--dir` do `quantick-backtest` (ver
[[bench-headless-via-backtest]]) e como `--out` do
`tools/mt5/export_session.py` só quando o Camilo pedir o dado de verdade;
para validar código, exportar para o scratchpad e não misturar com o acervo.
Arquivos com nome `WINQ26-2026-08-04.csv` (prefixo do símbolo) são de um
exportador anterior: carregam, mas o dia da sessão fica desconhecido no
browser.
