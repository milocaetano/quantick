# GOAL — carregamento progressivo do histórico de venue + seam discreto

Branch: `feat/progressive-venue-history`
Worktree: `../quantick-worktrees/feat-progressive-venue-history`

## Missão

O histórico de candles do venue hoje chega em **um único** `FeedEvent::OhlcvHistory`
com 90 dias inteiros (Binance ~130 páginas, Hyperliquid ~26): o gráfico fica sem
prefixo nenhum durante toda a busca e depois tudo aparece de uma vez. Trocar isso
por entrega **progressiva do agora para trás**, em fatias, com opção de ligar/desligar
— e deixar o divisor de seam do venue discreto (branco quase transparente) em vez de
âmbar.

## Contexto apurado (não re-descobrir)

- Contrato: `crates/app/src/feed/mod.rs` — `FeedCommand::FetchOhlcv { span_ms }`,
  `FeedEvent::OhlcvHistory { interval_ms, bars, complete }`, hoje "exatamente uma
  resposta por pedido"; `TIME_HISTORY_SPAN_MS` = 90 dias; base 1m.
- Paginação real: `crates/feed-binance/src/klines.rs:349` `fetch_history`
  (páginas de 1000, `PAGE_DELAY`), `crates/feed-hyperliquid/src/candles.rs:293`
  (páginas de 5000). Ambas rodam em task fora do select loop
  (`crates/app/src/feed/binance.rs:115`, `hyperliquid.rs:90`).
- MT5 (`feed/metatrader.rs:655`) e replay (`feed/replay.rs:290`) respondem de um
  bloco já em mãos — não paginam sob demanda.
- Consumo na app: `tab.rs:559` `take_ohlcv_history` → `ohlcv_base` →
  `tab.rs:626` `refold_history_prefix()` (`resample::fold` + `trim_to_seam`) →
  `pane.rs:1644` `install_history_prefix`. O prefixo é display-only, nunca entra
  no engine.
- Spinner: `crates/app/src/loading.rs` — `LoadingTask::VenueHistory`,
  label "loading venue history"; `complete: false` hoje só vira log
  (`tab.rs:566` `OHLCV_INCOMPLETE`).
- Seam: `crates/app/src/pane.rs:5070` `draw_seam_divider` — tracejado em
  `theme::AMBER` (`theme.rs:50`, `rgb(240,185,11)`) + rótulo "venue"; camada
  `ChartLayer::SeamDivider` (`chart_layers.rs`), visível por padrão.
  `theme.rs:8,80` documenta AMBER como reservado a "provenance honesty".

## Critérios de aceite

### Específicos da missão

1. **Entrega progressiva**: o histórico de venue chega em várias fatias, da mais
   recente para a mais antiga, e cada fatia é renderizada assim que chega — o
   gráfico começa a mostrar prefixo em segundos, não só no fim. Contrato de porta
   atualizado e documentado (uma ou mais respostas, a última marcada como final);
   provado por teste com um feed falso que emite N fatias.
2. **Opção do usuário**: existe toggle explícito para carregamento progressivo
   vs. tudo-de-uma-vez, alcançável na UI e persistido; o caminho não-progressivo
   continua se comportando exatamente como hoje (teste cobre os dois).
3. **Sem regressão de custo**: o refold/instalação do prefixo por fatia não é
   O(n²) sobre o histórico acumulado — número de folds limitado e medido; sem
   flicker nem salto de viewport a cada fatia que chega (a posição de câmera do
   trader não pode ser puxada quando barras antigas são prependadas).
4. **MT5 e replay inalterados**: quem serve de um bloco pronto responde uma única
   fatia final; comportamento idêntico ao de hoje, coberto por teste.
5. **Seam discreto**: o divisor "venue" passa a ser branco semi-transparente
   (não âmbar), constante nomeada no `theme.rs` com o comentário de reserva do
   AMBER atualizado para não mentir; rótulo igualmente discreto. Legível em light
   e dark, verificado por screenshot.
6. **Progresso honesto**: enquanto fatias chegam, a UI diz que ainda está
   carregando mais (não some o indicador na primeira fatia), e histórico
   incompleto continua rotulado como tal — nunca remendado em silêncio.

### Portões padrão (mudança de código, UI-visível, adiciona capacidade)

7. Quatro checks verdes após rebase no `main` atual: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`,
   `cargo test --workspace`.
8. **Impacto de performance declarado no plano**: cada caminho tocado classificado
   por taxa (per-trade / per-depth / per-frame / rare). O desenho do seam é
   per-frame → evidência de fps/frame_avg via `APP_HEALTH_SUMMARY` contra controle
   em `main`, números no corpo do PR.
9. **`ui-harness`**: toda superfície nova/alterada alcançável por hook de env,
   com o hook adicionado na mesma mudança.
10. **`visual-qa`**: todas as superfícies PASS ou defeito explicitamente aceito
    (inclui estado intermediário: prefixo parcial no meio do carregamento).
11. **`trader-ux-review`**: nenhum Blocker em aberto.
12. **`new-extension`**: porta nomeada, edições de registro apenas, segunda
    implementação falsa testada, blast radius (arquivos adicionados vs. editados)
    no corpo do PR.
13. **`arch-review`** sobre `git diff main...HEAD` com todo Blocker/Should-fix
    resolvido ou explicitamente adiado no corpo do PR.
14. **PR aberto** com a evidência no corpo e CI verde. Merge não faz parte da
    missão.

## Decisão de default (declarada)

O toggle nasce **ligado** (progressivo), porque o carregamento gradual é o próprio
objetivo da missão; o modo antigo continua disponível pelo toggle. Isso desvia da
regra "defaults preservam o comportamento de hoje" do `new-extension` — desvio
consciente, registrado aqui e a ser repetido no corpo do PR.
