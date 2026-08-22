---
name: duas-popups-do-desenho
description: "Pedido sobre 'a popup do desenho' pode ser a context bar ou o inspector — checar as duas antes de mexer"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e13efc7c-b26c-4400-a3bd-24baa860ab2e
  modified: 2026-08-18T07:59:42.587Z
---

Ao clicar num desenho no chart, quem aparece é a **context bar** (fila de
ícones, arrastável pelo grip). O **popup de propriedades** (`drawing_inspector`)
só abre pela engrenagem dessa barra. O Camilo chama as duas de "popup".

**Why:** em 2026-08-18 ele pediu "a popup deve abrir onde deixei da última vez"
— o inspector já fazia isso desde o PR #197, e o que ele via voltando para junto
do objeto era a context bar. Um pedido que parece já implementado costuma ser a
outra superfície.

**How to apply:** antes de suspeitar de bug ou de dizer "já funciona", identificar
qual das duas: `drawing_inspector` em `app.rs` vs `drawings/context_bar.rs`.
Provar as duas com teste headless. Ver [[goal-arquivado-nao-e-entregue]].
