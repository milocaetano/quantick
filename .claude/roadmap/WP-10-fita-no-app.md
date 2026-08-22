# WP-10 — prints elefante e velocímetro na tela

**Missão**: acoplar o crate `quantick-tape` ao app e mostrar o que ele mede —
prints elefante realçados na camada de bolhas que já existe, e velocidade da
fita numa célula da status bar.

Branch: `feat/tape-in-app` · worktree `../quantick-worktrees/feat-tape-in-app`

Depende de: WP-09. Bloqueia: nada.

## Onde acoplar (ponto único, verificado)

`crates/app/src/tab.rs::ingest_live_trade_at` (`:1474-1487`) é **o** ponto por
trade. Hoje ele faz `self.paper.on_trade(trade)` (`:1483`) e
`pane.ingest_live_trade(trade)` (`:1485`), com o comentário que fixa a doutrina:
*"The simulator taps the same per-trade point the bar engine does, so paper
trading works identically on a live feed and a replay."* A fita entra
exatamente ali, pelo mesmo motivo: funciona idêntico em live e replay por
construção.

O `paper` é campo público do `Tab` (`:237`) com escopo por aba justificado no
doc — o estado de fita segue o mesmo padrão.

Três decisões obrigatórias, cada uma com sua linha:

1. **Reset de timeline** — `reset_market_state()` (`:1494-1516`) já reseta
   `tape_mut().reset_for_symbol()` (`:1512`) e `paper.on_timeline_reset()`
   (`:1515`). O acumulador de percentil **precisa** entrar aí: sem isso, um
   seek de replay contamina o percentil com prints de uma timeline descartada.
2. **Backfill** — `FeedEvent::Backfilled` (`:1378-1390`) hoje só semeia o mark
   do paper com o **último** trade (`:1383-1385`), porque preencher contra o
   passado seria look-ahead. Para percentil de tamanho a decisão é
   provavelmente **oposta**: sem o histórico, a primeira meia hora não tem
   amostras. Isso é uma divergência consciente do padrão do paper e tem de
   estar documentada no código, não implícita.
3. **Uma leitura por drain** — o padrão do arquivo é fazer trabalho de UI uma
   vez por drain, não por trade (`publish_partial` em `:1434-1438`). A célula
   da status bar lê o estado agregado, nunca recalcula por trade.

## Critérios de aceite

1. **Prints elefante realçam a camada de bolhas que já existe** — realce/anel
   sobre as bolhas de agressão, não camada nova. Menos superfície, menos
   registro, menos risco.
2. **Velocímetro como célula da status bar**, no padrão das células existentes:
   texto curto (a seção do meio divide linha com o painel de máquina, por isso
   os labels são curtos de propósito) e `on_hover_text` com o detalhe. Célula
   nova exige campo em `StatusModel` (`statusbar.rs:90-155`), bloco em
   `draw_content()` (`:285-355`), preenchimento em `QuantickApp::status_model()`
   (`app.rs:3578-3627`) e atualização dos **dois** struct-literals de teste
   (`statusbar.rs:485-510` e `:536-561`).
3. **Honestidade**: tamanho de print é fato (volume negociado real, verificado
   no protocolo do bridge) e vai sem marca; **lado** do print e do burst é
   inferido e leva a marca de lado inferido (WP-05). Janela sem trades mostra
   `—`, nunca `0` — silêncio não é zero medido.
4. **Fita furada é declarada**: janela que contenha `MT5_SEQ_GAP` mede taxa
   sobre fita incompleta. Exibir "fita com perda" em vez de um número que
   parece bom.
5. **Orçamento per-trade provado**: zero alocação por trade no caminho de
   ingestão. O bench do WP-09 é a prova; o `APP_HEALTH_SUMMARY` sob fita densa
   é a confirmação de que a UI não regrediu.
6. Hook de harness para a célula/realce, no mesmo commit, reusando o caminho do
   toggle manual e default off.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto declarado: **per-trade** (ingestão) + **per-frame** (leitura),
      com número do bench e do health summary.
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] `ui-harness`: hook no mesmo commit + linha na tabela.
- [ ] `visual-qa` com a matriz de estados, incluindo fita densa (replay
      acelerado) e fita silenciosa.
- [ ] `trader-ux-review`: o realce não polui a leitura das bolhas; o número da
      status bar é legível sem inclinar.
- [ ] PR aberto com CI verde. Merge não faz parte.
