---
name: rodar-app-nao-e-qa
description: "\"Rodar o app\" é uso real — nunca apontar os stores QUANTICK_* pro scratchpad"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: a1bee7a4-f8db-4a92-8b0f-1d497aa9b0f3
  modified: 2026-08-18T13:10:35.356Z
---

Quando o Camilo pede para **rodar/abrir o quantick**, lançar o exe *sem nenhuma*
variável `QUANTICK_*` de store. O app tem que subir na casa real dele:
`C:\Users\Camillo\OneDrive\Documents\Quantick` (Documents está no OneDrive).

O protocolo de apontar `QUANTICK_TRADES_DIR`, `QUANTICK_UI_STATE`,
`QUANTICK_PAPER_STATE`, `QUANTICK_INDICATORS_STATE` e irmãos pro scratchpad
é do [[skill-ui-harness-so-em-validacao]] — vale **só** em run de validação/QA
(visual-qa, trader-ux-review, captura de tela para revisar um diff).

**Why:** aplicado num pedido de uso normal, o app sobe com uma casa vazia:
sem histórico de trades, sem indicadores, sem workspace, sem símbolos. Para
ele isso é indistinguível de "tudo que eu configurei se perdeu" — e o susto é
real, ainda que nenhum arquivo tenha sido tocado. Aconteceu em 18/08/2026.

**How to apply:** pedido de uso → `Start-Process` do exe, só `RUST_LOG`.
Pedido de QA → stores no scratchpad. Na dúvida, é uso. E antes de lançar
qualquer instância que possa gravar, copiar a casa real para
`Documents\Quantick-backup-<data>` — `save_on_exit = true` no `ui-state.toml`
faz o app regravar o workspace ao fechar, então uma aba que ele descartou na
subida some do arquivo de vez. Ver [[aba-orfa-some-no-save-on-exit]].
