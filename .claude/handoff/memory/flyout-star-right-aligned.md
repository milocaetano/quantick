---
name: flyout-star-right-aligned
description: "Camilo prefere controles secundários (ex. estrela de favorito) alinhados à direita da linha, nunca sobre o ícone da ferramenta"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: a89e449b-e792-424d-9e27-659ca9a4c457
  modified: 2026-08-14T02:27:04.418Z
---

No flyout de famílias do trilho de desenho (PR #178), Camilo pediu a estrela
de favorito **ao lado da descrição, alinhada à direita da linha**, não sobre o
ícone — mesmo tendo descrito "estrelinha no ícone" no pedido original.

**Why:** sobreposto ao ícone polui o glifo e compete com o clique de armar; à
direita segue o padrão TradingView que ele usa como referência visual.

**How to apply:** em novas linhas de lista/flyout do quantick, ancorar ações
secundárias (favoritar, fixar, remover) na borda direita da linha, deslocando
rótulos de atalho um slot à esquerda; nunca desenhar badges clicáveis sobre o
glifo da ferramenta.
