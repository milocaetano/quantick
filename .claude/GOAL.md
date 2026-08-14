# GOAL — navegação e densidade do gráfico

**Missão**: dar ao gráfico o espaço e a densidade que faltam para operar —
espaço vazio à direita suficiente para projetar canal e Fibo, espaçamento
entre velas configurável, zoom-out que continua além do limite atual (mais
passado na tela, principalmente no gráfico de imbalance) e footprint que
começa a mostrar números com muito menos zoom, tudo com defaults escolhidos
por critério de design e salvos.

Branch: `feat/chart-navigation-density` ·
worktree `../quantick-worktrees/feat-chart-navigation-density`

## O que o usuário pediu, desambiguado

Três perguntas foram respondidas antes de começar:

1. **"Arrastar pra esquerda infinitamente"** = empurrar as velas para a
   esquerda e ganhar **espaço vazio à direita**, para traçar canal prolongado
   e projeção de Fibonacci. **Não** é carregar mais histórico. Hoje o teto é
   `FUTURE_MARGIN_BARS = 40` (`viewport.rs:23`) — a 8 px por vela, 320 px de
   espaço, que é curto para projetar.
2. **Eixo do tempo**: a direção atual está certa (arrastar para a direita
   expande). O que falta é o outro lado — arrastar para a esquerda tem que
   **espremer muito mais** do que o limite de hoje.
3. **Fonte de dados onde os limites aparecem**: MT5 / XP ao vivo.

## Estado de partida (fatos, não suposições)

- `MIN_CANDLE_WIDTH = 2.0` px por slot (`viewport.rs:14`) é o teto do
  "espremer": ~800 velas numa tela de 1600 px, e não desce mais.
- `body_width_frac = 0.72` existe em `CandleStyle` (`style.rs:222`), mas está
  enterrado no diálogo LOOK → candles e some no zoom-out
  (`pane.rs:3792`, `half = (cw * frac / 2).max(0.5)`).
- Footprint: `Marks ≥ 8 px`, `Profile ≥ 18 px`, `Compact ≥ 40 px` (o primeiro
  número), `Detailed ≥ 72 px` — constantes fixas em
  `footprint_render.rs:49-52`.

## Critérios de aceite específicos

1. **Espaço de projeção à direita**: empurrando o gráfico para a esquerda, o
   espaço vazio à direita chega a aproximadamente uma janela inteira (a vela
   mais recente pode ser levada até perto da borda esquerda), em qualquer
   tipo de barra e também no pane de timeframe. Teste unitário no `viewport`
   provando que a margem escala com a largura da janela em vez do teto fixo
   de 40 barras, e que voltar ao live (duplo clique + chip `jump_to_live`)
   continua funcionando a partir do extremo.
2. **Espaçamento entre velas configurável**: controle explícito de
   espaçamento, com default definido por critério de design e justificado no
   corpo do PR (referência: TradingView Lightweight Charts — corpo ≈ 80 % do
   slot, com pelo menos 1 px de vão sempre que houver pixel para isso).
   Persiste entre execuções. Teste cobrindo o vão em larguras de vela
   pequena, média e grande.
3. **Espremer muito mais**: o zoom-out deixa de parar em 2 px por barra.
   Abaixo do limite de legibilidade o gráfico **agrupa** barras por slot
   (fusão OHLC exata: open da primeira, high/low extremos, close da última,
   volume somado — a solução que Sierra Chart chama de *bars to merge*),
   mantendo slot legível com vão, e **diz** o fator de agrupamento em vez de
   fingir que cada traço é uma barra. Resultado medido: pelo menos 5× mais
   barras na tela do que hoje no gráfico de imbalance. Desenhos, indicadores
   e bandas continuam pousando na barra certa (teste do mapeamento
   `x_at_bar_position` sob agrupamento).
4. **Footprint aparece com menos zoom**: os pisos de nível viram configuração
   com defaults revisados para baixo e justificados, e no zoom default o
   footprint já mostra mais que `Marks`. Persiste em `footprint-settings.toml`
   junto do resto da configuração do footprint. Testes: novos defaults e a
   histerese continuando a impedir piscada entre níveis.
5. **Engine e crates puras intocadas**: o agrupamento é projeção de
   visualização, nunca um tipo de barra novo.
   `git diff main...HEAD --stat -- crates/engine crates/indicators
   crates/orderbook crates/replay crates/sim` sai vazio.

## Portões padrão injetados

- [ ] Quatro checks verdes após rebase no `main` atualizado:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo build --workspace`, `cargo test --workspace`.
- [ ] **Impacto de performance declarado** (tabela do `arch-review`), decidido
      no plano e não na revisão: tudo aqui é **per-frame** — matemática do
      viewport, pintura das velas, LOD do footprint. Nada per-trade, nada
      per-depth. Risco nomeado: com agrupamento alto o range visível cresce em
      barras (dezenas de milhares), então a fusão OHLC precisa ser
      memoizada/incremental em vez de varrer tudo a cada frame.
- [ ] **Evidência de performance**: `APP_HEALTH_SUMMARY` (fps / frame_avg) sob
      tape densa, comparado com uma corrida de controle em `main`. Números no
      corpo do PR.
- [ ] `ui-harness`: cada superfície nova ou alterada alcançável por env hook,
      com o hook adicionado nesta mesma mudança.
- [ ] `visual-qa`: matriz de estados (zoom default / espremido / agrupado /
      footprint em cada nível / espaço de projeção à direita) toda PASS, ou
      defeito aceito explicitamente. **Abrir o app exige autorização do
      Camilo** — sem ela, o critério fica BLOCKED e é reportado como tal.
- [ ] `trader-ux-review` sem Blocker em aberto.
- [ ] `arch-review` sobre `git diff main...HEAD`, com Blocker/Should-fix
      resolvidos ou deferidos no corpo do PR; registro do gate gravado em
      `arch-review-ok` antes de `gh pr create`.
- [ ] **PR aberto** com a evidência no corpo. Merge não faz parte da missão.
