# WP-01 — harness de backtest headless

**Missão**: o consumidor que a arquitetura sempre previu e nunca foi
construído. O `CLAUDE.md` diz "**one engine, three consumers: chart, backtest
and bot**" — este pacote entrega o segundo. Carrega sessões gravadas, roda o
mesmo motor de barras da tela, deixa uma estratégia decidir, e devolve
expectância, profit factor e drawdown por sessão.

É o instrumento que responde à única pergunta que importa: **o operacional tem
edge?** Sem ele, tudo no roadmap é construído no escuro.

Branch: `feat/backtest-harness` · worktree
`../quantick-worktrees/feat-backtest-harness`

Depende de: nada. Bloqueia: WP-02, WP-03.

## Leia isto antes de escrever qualquer linha

`crates/indicators/tests/bot_readiness.rs` **já é este harness em miniatura**.
O doc do arquivo (`:1-8`) diz literalmente: *"The bot-readiness proof: a
backtester consumes indicators with zero UI involvement, through exactly the
API a chart uses. This test* is *the future backtest/bot access path."*

Ele já faz: `fixture::parse_trades` → `engine_golden::replay` → `IndicatorHost`
→ ler `plots(id).column(PlotId)` → detectar cruzamento. Falta acrescentar:
(a) `Session::load` no lugar da fixture, (b) o `Simulator` entrelaçado no loop
de trades, (c) `PerformanceReport::from_trades` por sessão.

## Onde vive: `crates/backtest` (crate novo)

Três opções foram avaliadas; esta é a recomendação, e o PR deve confirmá-la ou
justificar a mudança:

| Opção | Custo | Veredito |
| --- | --- | --- |
| `crates/backtest` (novo) | 3 arquivos de contrato (abaixo) | **Recomendado** — `CLAUDE.md` já nomeia "backtest" como consumidor; o crate tem casa própria para crescer com testes |
| `crates/app/examples/backtest.rs` | zero contrato | Arrasta eframe/wgpu/tokio na compilação de um binário que não tem UI; dá ao `app` responsabilidade que não é dele |
| `crates/replay/examples/…` | — | **Inviável**: `replay` só pode depender de `engine`, e o harness precisa de `sim` e `indicators` |

Os **três arquivos de contrato** que um crate novo obriga (idênticos aos do
WP-09, e dois deles são testes que quebram se esquecidos):

1. `Cargo.toml` raiz: uma linha em `members`. Não há
   `[workspace.dependencies]`; cada crate declara as suas com versão literal.
   `version/edition/license.workspace = true`, edition **2024**.
2. `CLAUDE.md`: parágrafo do crate na lista de arquitetura, com o nome entre
   crases, **e** a linha de direção de dependências atualizada. O teste
   `workspace_deps.rs::claude_md_lists_every_crate` varre `crates/*/Cargo.toml`
   e falha se faltar.
3. `crates/pine/tests/workspace_deps.rs`: entrada na whitelist hardcoded de
   `the_domain_crates_never_depend_upwards` (`:62-69`), algo como
   `("backtest", &["engine", "replay", "indicators", "pine", "sim"])`. **O loop
   itera sobre a whitelist, não sobre o diretório** — esquecer não falha o
   teste, apenas deixa o crate sem guarda de dependência.

## O laço, com as assinaturas reais

```
replay::scan(dir) → SessionEntry.path
  → Session::load(&path, ParseOptions::default()) → session.trades: Vec<Trade>
    para cada trade:
      (a) sim.on_trade(&trade) -> Vec<SimEvent>        // brackets, fila, repouso
      (b) builder.push(&trade) -> Option<Bar>          // no máx. 1 barra por trade
            → host.push_closed_bar(&bar)
            → a estratégia lê host.plots(id).value(PlotId, i) e devolve comandos
      (c) sim.apply(command) -> Vec<SimEvent>
  → PerformanceReport::from_trades(sim.closed_trades())
```

Armadilhas verificadas que o agente **precisa** tratar:

- `sim.apply` antes do primeiro print → `SimEvent::Rejected(NoMarketPrice)`.
- Ordem a mercado preenche no **próximo** `on_trade`, nunca neste.
- Bracket é validado contra a marca, mas o fill acontece num print posterior;
  se a fita ultrapassou o nível, ele é **descartado** com
  `SimEvent::BracketDropped` — o harness precisa contar isso, não ignorar.
- `NaN` em coluna de plot **é** `na` (warmup, plot condicional). Testar
  `is_nan()` antes de usar como sinal. `column(id)` **panica** com id inválido;
  `value(id, row)` devolve `NaN` para row fora de range.
- `Session::load` materializa o `Vec<Trade>` inteiro. Uma sessão real de WIN
  tem milhões de prints — processar **uma sessão por vez** e soltá-la antes da
  próxima.
- **`golden::replay` não serve para o laço principal**: ele descarta a posição
  do trade no fluxo. Serve só para um pré-passe de barras.

## Critérios de aceite

1. **Porta de estratégia** — um trait pequeno que recebe (barra fechada,
   índice, leitura dos indicadores, estado do simulador) e devolve zero ou mais
   `Command`. Não existe trait `Strategy`/`Rule`/`Signal` no workspace hoje;
   este pacote o cria. Manter mínimo: a v1 precisa de uma implementação real
   (WP-03) e uma fake de teste — exigência do `new-extension` §5.
2. **As regras são Rust, não Pine.** `strategy.*` é rejeitado pelo dialeto com
   código de erro estável (`builtins.rs:375`). Scripts podem *fornecer sinais*
   (o harness compila `.pine` via `quantick_pine::compile` +
   `ScriptIndicator::new` e lê as colunas de plot), mas a decisão de ordem é do
   código Rust.
3. **CLI à mão, sem clap.** Não existe dependência de CLI no workspace nem no
   `Cargo.lock`, e o minimalismo de supply chain é cultivado
   explicitamente. Seguir o padrão de
   `crates/replay/examples/import_mt5_ndjson.rs`: `std::env::args().skip(1)`,
   `match arg.as_str()`, `fn usage()`, e `main() -> ExitCode` com códigos
   distintos (2 = argumentos ruins, 3 = rodou e não produziu saída,
   SUCCESS/FAILURE nos demais).
4. **Diagnóstico em JSON por linha no stderr**, com `event_code` — o padrão do
   importador (`:40-42`): *"a person with `jq` — or an AI reading the log — can
   follow the run without scraping prose"*. O relatório humano vai no stdout;
   o diagnóstico, no stderr.
5. **Relatório próprio** — não existe renderizador de `PerformanceReport` fora
   do `app` (a formatação vive em `paper_trading.rs`). O harness escreve o seu.
   Razão com denominador ausente imprime `—`/`n/a`, **nunca** zero: a regra de
   honestidade do `report.rs` é que `None` não vira número inventado.
6. **Saída por sessão e agregada.** Por sessão: expectância, profit factor,
   win rate, drawdown, nº de trades, e a contagem de eventos anômalos
   (rejeições, brackets descartados). Agregado: o mesmo mais a distribuição.
   Se o harness gravar os trades, usa o formato `quantick-trades` de
   `sim::history` — não inventa CSV novo.
7. **Determinismo, provado por teste** — rodar a mesma sessão duas vezes e
   exigir relatórios idênticos. É o formato de guarda que todo o workspace usa
   (`assert_golden` roda o builder duas vezes; o `sim` tem
   `same_tape_and_commands_produce_identical_output`). Regras operacionais:
   sem `Instant::now`/`SystemTime` na lógica (todo tempo vem de
   `Trade::timestamp_ms`); sem `HashMap`/`HashSet` onde a ordem possa vazar
   (usar `BTreeMap`/`Vec`); nunca iterar `read_dir` cru — `library::scan` já
   ordena por (symbol, date, path); sem RNG, sem paralelismo que altere a
   ordem de agregação (`from_trades` depende explicitamente da ordem).
8. **Transcendentais via `quantick_indicators::fmath`** se a estratégia
   precisar de `powf`/`exp`/`ln`. O `fmath_guard` varre só os fontes do crate
   `indicators` e **não** cobrirá o crate novo — replicar a regra é o
   esperado; considerar um guard próprio no mesmo molde.
9. **Smoke test com a fixture existente.** `crates/replay/tests/fixtures/
   WINQ26-2026-08-12.csv` tem **34 prints em 75 ms** (o leilão de abertura,
   com book cruzado de propósito). Ela prova que o pipe compila e roda — e
   **não** produz nenhum trade fechado. O PR deve dizer isso com todas as
   letras: é smoke test, não validação.

## O que este pacote NÃO entrega

- Nenhuma regra de trading (WP-03) e nenhum classificador de dia (WP-02).
- Nenhuma conclusão sobre edge. Ele é o instrumento; a medição é o WP-03.
- Nenhuma UI. Zero dependência de `app`.

## Nota sobre a biblioteca de sessões

O harness é inútil sem sessões reais. `QUANTICK_REPLAY_DIR` existe mas é
**unset por default** e é variável do `app`, não do crate `replay` — o harness
implementa a leitura por conta própria e aceita `--dir` com precedência.
Sessões reais são exportadas com
`python tools/mt5/export_session.py --symbol WINQ26 --day <YYYY-MM-DD>
--context-sessions 3 --out <folder>`, e gravações NDJSON antigas convertem com
`crates/replay/examples/import_mt5_ndjson.rs`.

## Portões

- [ ] Quatro checks verdes. Atenção: `--all-targets` linta examples e benches,
      então o harness é lintado com `-D warnings`.
- [ ] Impacto de performance declarado: **offline**, com o tempo de uma sessão
      real medido e reportado (é o número que decide se uma varredura de
      parâmetros é viável no WP-03).
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] `new-extension`: porta de estratégia nomeada + segunda implementação fake
      testada; os três arquivos de contrato do crate novo tocados.
- [ ] Teste de determinismo (mesma sessão duas vezes → relatórios idênticos).
- [ ] PR aberto com CI verde. Merge não faz parte.
