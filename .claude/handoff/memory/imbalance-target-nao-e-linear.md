---
name: imbalance-target-nao-e-linear
description: Barras de imbalance nunca entregam exatamente o target; medir com o audit sob tick rule antes de suspeitar de bug
metadata: 
  node_type: memory
  type: project
  originSessionId: 5d069c30-448a-43ca-804c-4c4ed5650a96
  modified: 2026-08-17T16:07:46.583Z
---

O parâmetro "target trades" das barras de imbalance é um **dial calibrado**,
não uma contagem exata. Depois do fix de 2026-08-17 (PR #195), em fluxo
perfeitamente equilibrado a barra sai a **1,3-1,7× o alvo**; num tape real com
direção (WIN sob tick rule) sai a **0,6-0,65×**. As duas coisas são corretas —
fluxo direcional fecha barra cedo, que é a razão de existir do tipo de barra.

**Why:** antes do fix o `E[T]` era EWMA das barras que ele próprio produzia,
com ponto fixo repulsor em 400 independente do alvo; o valor fugia para um dos
clamps e o botão não era nem monotônico nem reproduzível (1500 → 626
trades/barra, 1600 → 284, 1700 → 749 no mesmo tape). Isso acabou. O que sobra
é o viés estrutural acima, que é esperado e está documentado no módulo.

**How to apply:** antes de tratar "a barra não tem o tamanho que pedi" como
bug, medir com
`cargo run -p quantick-replay --release --example imbalance_audit -- <sessao>.csv --tick-rule --hourly <targets>`.
O `--tick-rule` é obrigatório para reproduzir o MT5 ao vivo: as gravações
trazem `side_source=venue_flags`, mas o app roda `Mt5SideSource::TickRule` por
padrão, e são tapes diferentes. Sintomas de defeito real: não-monotonicidade,
faixa horária maior que ~2×, ou `cap_closes` acima de uns 25%.

Ver [[goal-arquivado-nao-e-entregue]] — o diagnóstico deste bug já existia numa
branch órfã sem PR.
