---
name: zoom-nunca-altera-a-vela-do-operacional
description: Uma barra = uma vela em todo zoom — agrupamento visual foi removido; o Camilo entra na barra de elefante e confiança é binária
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 7b237357-cbdf-4e59-837e-f855ae23fea3
  modified: 2026-08-16T02:02:51.467Z
---

Em 2026-08-15 (PRs #184 → #189), o agrupamento de barras no zoom-out foi
testado ao vivo pelo Camilo e rejeitado em dois tempos: primeiro "os candles se
agrupam e isso prejudica meu operacional pois eu uso a barra de elefante para
entrar na operação"; depois, com o agrupamento confinado abaixo do range
antigo, "como eu vou confiar nos trades?". O PR #189 removeu o agrupamento por
inteiro.

**Why:** ele entra na operação lendo **uma** barra — a de elefante (ver
[[operacional-mark-i]]). Confiança é binária: se *algum* zoom pode mostrar uma
vela que não é exatamente uma barra da regra, todas ficam suspeitas, e nota de
rodapé no canto não devolve a confiança no meio de um trade. Confinar a "zoom
novo" não resolveu — um gesto normal chega lá e o gráfico muda de significado
embaixo dele.

**How to apply:** uma barra = uma vela, em todo zoom, é lei
(`Viewport::candle_width`, piso 1 px/barra — o zoom mais fundo honesto, cada
barra com sua coluna de pixel). Nunca propor LOD/agregação/decimação que mude o
que uma vela significa; densidade se ganha com pisos menores honestos, nunca
com fusão. Lição geral da sessão: regras "espertas" no eixo X (piso derivado
dos dados, agrupamento) quebraram o operacional em minutos — o eixo tem que ser
burro e previsível.
