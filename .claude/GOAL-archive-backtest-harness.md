# GOAL — WP-01: harness de backtest headless

**Missão**: entregar `crates/backtest`, o segundo dos "three consumers" que o
`CLAUDE.md` promete — carrega sessões gravadas, roda o mesmo motor de barras da
tela, deixa uma estratégia em Rust decidir sobre leituras de indicadores, e
devolve expectância, profit factor e drawdown por sessão e agregado.

Branch: `feat/backtest-harness` · worktree
`../quantick-worktrees/feat-backtest-harness` · base `origin/main` @ ac6d6e0

Fonte: `.claude/roadmap/WP-01-harness-backtest.md`, despacho por
`.claude/roadmap/DISPATCH.md`.

## Critérios de aceite

Específicos do pacote:

1. **Crate novo `crates/backtest`** com os três arquivos de contrato tocados:
   membro no `Cargo.toml` raiz (edition 2024, `*.workspace = true`), parágrafo
   no `CLAUDE.md` com o nome entre crases **e** a linha de direção de
   dependências atualizada, entrada na whitelist de
   `crates/pine/tests/workspace_deps.rs::the_domain_crates_never_depend_upwards`.
2. **Porta de estratégia** — trait mínimo: (barra fechada, índice, leitura dos
   indicadores, estado do simulador) → zero ou mais `sim::Command`. Duas
   implementações: uma real mínima e uma fake de teste (`new-extension` §5).
3. **Laço real, não `golden::replay`** — `Session::load` → por trade:
   `sim.on_trade` → `builder.push` → `host.push_closed_bar` → estratégia →
   `sim.apply`. Uma sessão por vez, solta antes da próxima.
4. **Anomalias contadas, nunca ignoradas** — rejeições (`SimEvent::Rejected`) e
   `SimEvent::BracketDropped` entram no relatório como contagem própria.
5. **CLI à mão, sem clap** — padrão de `crates/replay/examples/import_mt5_ndjson.rs`:
   `std::env::args().skip(1)`, `fn usage()`, `main() -> ExitCode` com 2
   (argumentos ruins), 3 (rodou e não produziu saída), SUCCESS/FAILURE.
   `--dir` tem precedência sobre `QUANTICK_REPLAY_DIR`.
6. **Diagnóstico JSON por linha no stderr** com `event_code`; relatório humano
   no stdout.
7. **Relatório próprio** — razão com denominador ausente imprime `—`/`n/a`,
   nunca zero. Por sessão: expectância, profit factor, win rate, drawdown, nº
   de trades, anomalias. Agregado: o mesmo mais a distribuição. Trades
   gravados, se gravados, no formato `quantick-trades` de `sim::history`.
8. **As regras são Rust, não Pine** — scripts podem fornecer sinais via
   `quantick_pine::compile` + `ScriptIndicator`, mas a decisão de ordem é do
   código Rust.
9. **Determinismo provado por teste** — mesma sessão duas vezes → relatórios
   idênticos. Sem `Instant::now`/`SystemTime` na lógica, sem `HashMap`/`HashSet`
   onde a ordem vaze, sem `read_dir` cru (usar `library::scan`), sem RNG, sem
   paralelismo que altere a ordem de agregação.
10. **Guard de transcendentais** no molde do `fmath_guard` do `indicators`, já
    que aquele guard não cobre o crate novo.
11. **Smoke test com a fixture existente** — `crates/replay/tests/fixtures/
    WINQ26-2026-08-12.csv` (34 prints em 75 ms). O PR diz com todas as letras
    que é smoke test, **não** validação: ela não produz nenhum trade fechado.

Portões padrão (DISPATCH.md):

12. **Quatro checks verdes** — `cargo fmt --all -- --check`,
    `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo build --workspace`, `cargo test --workspace`.
13. **Impacto de performance declarado** — **offline**, com o tempo de
    processamento de uma sessão medido e reportado (o número que decide se uma
    varredura de parâmetros é viável no WP-03). Sessão real se houver; sintética
    rotulada como tal se não houver.
14. **`arch-review`** sobre `git diff main...HEAD`, com todo Blocker e
    Should-fix resolvido ou deferido por escrito no corpo do PR.
15. **PR aberto com CI verde.** Merge **não** faz parte — é do Camilo.

## Fora de escopo (o pacote diz explicitamente)

- Nenhuma regra de trading (WP-03) e nenhum classificador de dia (WP-02).
- Nenhuma conclusão sobre edge — este pacote é o instrumento, não a medição.
- Nenhuma UI, zero dependência de `app`.
