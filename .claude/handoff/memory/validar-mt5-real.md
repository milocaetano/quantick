---
name: validar-mt5-real
description: A aba MT5 do workspace do Camilo foi corrigida para WINV26 — o launch normal agora abre no MetaTrader
metadata:
  type: project
---

O `ui-state.toml` da casa real
(`C:\Users\Camillo\OneDrive\Documents\Quantick`) guardava uma aba
`metatrader-b3 / WINQ26` enquanto o `quantick-symbols.toml` já registrava
**WINV26**. A aba ficava órfã (`UI_STATE_TAB_DROPPED`) e o app restaurava só a
aba Binance. **Corrigido em 19/08/2026** com o app fechado: agora sobe
`UI_STATE_RESTORED tabs=2 active=1`, a aba 1 é WINV26 e o bridge Python nasce
sozinho (`MT5_BRIDGE_SPAWNED` → `MT5_HELLO_OK` com
`server_utc_offset_s=-10800` → `MT5_BACKFILL_START` de ~940k ticks).

**Why:** o contrato do WIN vira a cada dois meses e o workspace guarda o
antigo. Quando isso voltar a acontecer (WINZ26, e assim por diante), a
validação de MT5 fica olhando BTCUSDT sem perceber e o `bridge_autostart`
parece quebrado quando na verdade nada pediu MT5.

**How to apply:** antes de lançar, comparar a aba `metatrader-b3` do
`ui-state.toml` com o contrato em `quantick-symbols.toml`; se divergirem,
corrigir o `symbol` com o app fechado (não recriar na mão na UI) e fazer
backup da casa primeiro — `save_on_exit = true` apaga a aba órfã de vez, ver
[[aba-orfa-some-no-save-on-exit]]. Para uma validação que precisa ignorar o
workspace sem tocar no arquivo real, `QUANTICK_UI_STATE` apontando para um
caminho inexistente no scratchpad, mais `QUANTICK_DEFAULT_FEED=metatrader-b3`
e `QUANTICK_DEFAULT_SYMBOL=<contrato atual>`. Ver [[rodar-app-nao-e-qa]].
