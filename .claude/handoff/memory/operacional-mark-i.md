---
name: operacional-mark-i
description: "O operacional de trading do Camilo para WIN (célula micro-range, módulos range + tendência rotacional, só fita) vive num artifact; decisões canônicas de síntese multi-agente"
metadata: 
  node_type: memory
  type: project
  originSessionId: 8179880a-326f-42a2-b2ce-cc3d67090832
  modified: 2026-08-14T04:40:58.378Z
---

Operacional de fluxo do Camilo para o WIN (B3), sintetizado em 2026-08-14 e publicado em https://claude.ai/code/artifact/65214f4d-716b-4aa4-b8b7-7864a4880046 (arquivo `operacional-mark-i.html` no scratchpad da sessão 8179880a; de outra sessão, atualizar passando essa URL como `url` no Artifact). A §04 do documento é a fonte única de números.

Arquitetura (v3): **a célula operável é o micro-range de 80–150 pts em posição relevante** — como *gatilho* (entrada + stop nele, lucro na estrutura maior, direção do contexto), nunca como *container* (fade das duas bordas exige ~60% de acerto, morre nos custos). Módulo R = fade de extremo em dia de range; Módulo T = recarga no micro-range do pullback (fib 0,382–0,50, zigzag len=3 fixo) em dia de tendência **rotacional** (3 de 4 critérios, decisão 10:30–11:00); drive/OTF = no-go. Módulo T fica em **observação até o Módulo R passar os gates de replay**. Camilo quer operar manualmente todos os dias — a resposta foi módulos por tipo de dia (2–6 trades/dia), não afrouxar critério.

Decisões canônicas:
- **Barra elefante não existe no código** (grep zero) — a memória do Camilo estava errada. Sinal canônico: **BEI** (trade_count ≤ P25 + volume ≥ P90 + |delta|/volume ≥ 0,6 + corpo ≥ 0,65 + atravessando estrutura).
- **Delta do WIN é 100% tick rule** (flags do broker B3 vêm 100% BUY) → delta com preço parado é contexto, nunca gatilho; gatilhos lado-agnósticos; convenção `~` para lado inferido. **Preço impresso decide, construção (banda/canal/fib) só localiza** — stop nunca em banda σ ou rail.
- **Corretagem do Camilo é zero** → C = 12,5 pts/trade (estresse 22,5); acerto p/ +0,25R ≈ 45%; R$ 5k fecha no limite com stop ≤ 120; R$ 10k confortável (2 contratos).
- Plataforma já tem: parallel-channel (tecla C, midline, extend_right), AVWAP com 3 pares σ, fib retracement/extension, zigzag.pine; desenhos NÃO persistem entre sessões (ritual matinal). Não existe: ta.linreg, perfil de sessão automático, times&sales, R-múltiplos no report.
- Roadmap: proveniência `side_inferred` → 4 .pine do dia 1 (opening_balance, cvd_divergencia, bei, canal-t) → R+custo no sim → HUD checklist ("anti-copilot", ver [[copilot-nao-agradou]]) antes do paper ao vivo → crate `tape` para sinais por print.
