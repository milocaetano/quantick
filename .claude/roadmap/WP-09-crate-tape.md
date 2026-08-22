# WP-09 — crate `quantick-tape`: a fita como domínio puro

**Missão**: criar o crate `crates/tape`, puro e determinístico como `engine`,
que recebe trades e devolve estatísticas de fita que o trait `Indicator` nunca
poderá ver — porque o indicador enxerga barras, e prints elefante e velocidade
de fita vivem *entre* as barras. Duas famílias na v1: **percentil móvel de
tamanho de print** (detector de elefante) e **velocidade da fita** (trades/s e
contratos/s em janela deslizante).

Branch: `feat/tape-crate` · worktree `../quantick-worktrees/feat-tape-crate`

Depende de: nada. Bloqueia: WP-10.

## Por que um crate novo, e não um módulo no app

O que existe hoje de percentil vive em `crates/app/src/orderflow/scale.rs`
(struct `Distribution`, `p99` por rank em `BTreeMap`) e é um **ratchet de
sessão** — o doc do módulo diz literalmente "the scale never forgets". Percentil
móvel não existe em lugar nenhum, e métrica de `trades/s` de fita também não
(a única ocorrência da string no repo é o throughput do bench do engine).

Pôr isso no app o tornaria intestável sem UI e invisível para o harness de
backtest (WP-01), que precisa das mesmas estatísticas rodando sobre sessão
gravada. Crate puro resolve os dois: `app → tape → engine` é direção permitida,
e o backtest consome o mesmo código que a tela.

## Critérios de aceite

1. **Manifesto no idioma da casa** — `crates/tape/Cargo.toml` com
   `version/edition/license.workspace = true`, `description` de uma frase, e
   deps `quantick-engine = { path = "../engine" }` + `rust_decimal = "1"`.
   Sem features (o workspace não tem nenhuma).
2. **Três portões de registro do workspace** (todos obrigatórios — os dois
   últimos são testes que quebram se esquecidos):
   - `Cargo.toml` raiz: uma linha nova em `members`.
   - `CLAUDE.md`: parágrafo do crate na lista de arquitetura **e** menção na
     linha de direção de dependências. O teste
     `crates/pine/tests/workspace_deps.rs::claude_md_lists_every_crate` varre
     `crates/*/Cargo.toml` e falha se o diretório não aparecer como
     `` `nome` `` no CLAUDE.md.
   - `crates/pine/tests/workspace_deps.rs`: adicionar `("tape", &["engine"])`
     à whitelist hardcoded de `the_domain_crates_never_depend_upwards`. **O
     loop itera sobre a whitelist, não sobre o diretório** — um crate ausente
     dela não falha o teste, apenas fica sem guarda. Omitir isso é entregar
     uma guarda cega.
3. **Doc de `lib.rs` no padrão dos crates puros** — primeira linha declarando
   o contrato de I/O ("raw trades in, tape statistics out"), lista explícita
   de ausências (no UI, no network, no async, **no wall clock**: a velocidade
   sai de `Trade::timestamp_ms`, jamais de relógio do host), seção
   `# Determinism` nomeando as estruturas usadas, e seção `# Honesty` dizendo
   o que o crate **não** reporta.
4. **Percentil móvel** — janela de N eventos (ring buffer), percentil exato
   sobre `Decimal` por seleção/rank, sem amostragem. `N` e o percentil são
   parâmetros do chamador; o crate não fixa corte, seguindo a política do
   `FootprintBuilder` (todos os cortes vêm de quem chama).
5. **Agrupamento de evento** — na B3 uma varredura vira vários negócios; o
   detector agrupa prints com mesmo `timestamp_ms` e mesmo lado dentro de uma
   tolerância `group_ms` parametrizada (0 = sem agrupamento). Documentar que
   o lado do grupo herda tick rule e portanto é dado inferido.
6. **Velocidade** — trades/s e contratos/s em janela deslizante de duração
   parametrizada, mais desvio em z-score sobre janela longa. Toda janela
   derivada de timestamps de trade.
7. **Honestidade sobre indefinição** — janela que ainda não encheu e percentil
   com amostras insuficientes retornam `None`, nunca um número inventado.
   Silêncio da fita retorna `None`, não zero: "não medido" ≠ "zero".
8. **Aritmética saturante** em toda entrada vinda de feed (a regra do engine:
   entrada não confiável nunca deve panicar).
9. **Golden test próprio** — `fixture::parse_trades` do engine é reusado para
   ler os trades (é `pub mod` incondicional, sem feature flag); a saída de
   estatísticas tem formato CSV próprio e um `assert_golden` próprio que
   **roda duas vezes e exige runs idênticos**, no molde de
   `crates/indicators/src/golden.rs` (que já faz exatamente isso em cima do
   harness do engine). Fixtures em `crates/tape/tests/fixtures/`, com as
   linhas `#` de comentário contando a aritmética esperada à mão — o idioma
   dos fixtures do engine.
10. **Nomes de teste no idioma da casa**: frase declarativa afirmando a regra,
    com contraste quando couber. Modelos reais:
    `market_fills_at_the_next_print_not_the_last_one`,
    `delta_quantities_are_absolute_not_additive`,
    `playback_is_deterministic_for_the_same_deltas`.
11. **Bench de hot path** — `crates/tape/benches/hot_path.rs`, `harness = false`,
    **sem criterion**, `fn main()` puro, carga sintética determinística gerada
    fora da região cronometrada, saída em uma linha com `M trades/s` e
    `ns/trade`. Cópia fiel do idioma de `crates/engine/benches/hot_path.rs`.
12. **Orçamento per-trade declarado**: zero alocação por trade, sem locks,
    trabalho limitado por evento. O skill `new-extension` é explícito — "if
    the design needs an allocation per tick, the design is wrong". O bench é a
    prova, e o número vai no corpo do PR.

## Fora de escopo (deliberadamente)

- Acoplamento no app: é o WP-10. Este pacote entrega o domínio e seus testes.
- Renderização, ícones, hooks de UI: idem.
- Qualquer decisão de threshold operacional (p99 vs p99,5): o crate expõe o
  parâmetro; a calibração é trabalho de replay, não de código.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto de performance declarado: **per-trade**, com número do bench.
- [ ] `arch-review` sobre `git diff main...HEAD`, Blocker/Should-fix resolvidos
      ou deferidos no corpo do PR.
- [ ] `new-extension`: o crate doca por porta nomeada; sem cirurgia em código
      existente além das três linhas de registro do item 2.
- [ ] Golden test presente e `assert_golden` rodando duas vezes.
- [ ] PR aberto com CI verde. Merge não faz parte.
