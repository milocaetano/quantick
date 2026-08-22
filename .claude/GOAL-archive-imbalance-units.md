# Missão: revisão das barras de imbalance + unidades (trades/volume/dollar)

Branch: `feat/imbalance-units` — worktree `../quantick-worktrees/feat-imbalance-units`.
(NÃO COMMITAR este arquivo — é o registro da missão, não parte da feature.)

Revisar o comportamento das barras de imbalance no mini-índice (cap de 5000 na UI
e fechamento percebido como rápido demais), corrigir o que estiver errado, e
estender o imbalance para os três alvos do López de Prado — trades, volume e
dollar — selecionáveis na UI.

## Critérios de aceitação

1. [x] Veredito: engine correto e imutável desde #55; "rápido" = tape denso
       equilibrado leva E[b] sob o floor 0,05 e E[T] ao clamp mínimo →
       regra efetiva |θ| ≥ target/80. Números (WINV26): 08-13 (1,48M trades)
       target 5000 → mediana 339 trades/5,1s (mín 63 = exatamente o piso);
       target 2000 → 113 trades/1,7s. 08-11 (118k trades, buy 0,577) →
       target 2000 dá mediana 2033 (~8min) — o "antes mais lento".
2. [x] Cap da UI 5000 → 1.000.000 (`W_IMBALANCE_PARAM`, speed 25); default 100
       intacto.
3. [x] Engine: `ImbalanceUnit{Trades,Volume,Dollar}`, test-first (6 fixtures
       novas), Trades bit-exato (teste dedicado + golden verdes sem edição).
       Ordem de avaliação por unidade documentada (elefante não infla o
       próprio threshold nas unidades pesadas).
4. [x] Specs `imbalance:volume:N` / `imbalance:dollar:N` no app E no backtest
       (parser paralelo), `imbalance:100` intacto, round-trip testado.
5. [~] Chips de unidade inline na toolbar (gated por traded_volume), persistem
       via workspace; alcançável por `QUANTICK_UI_STATE` (hook existente, sem
       hook novo necessário); trader-ux-review: sem Blocker, 1 Should-fix
       corrigido (largura do collapse plan e3b6d7e). visual-qa: BLOCKED —
       exige autorização do Camilo para lançar o app (memória).
6. [x] Bench: trades 545,5→551,3 ns/trade (flat); imb-vol 1316, imb-dlr 1594
       (caminhos novos). Demais builders inalterados.
7. [x] 4 checks verdes no worktree; arch-review completo (passo 0
       code-review high, 10 finders: 2 Blockers corrigidos — saturação do
       threshold, pesos como magnitude + guard de tape sem medida — e 6
       Should-fixes aplicados em 1ced653); PR #191 aberta, CI em
       acompanhamento. Merge fica com outro agente (regra).

Commits: 5b49a67 (feature), e3b6d7e (largura toolbar), 1ced653 (fixes da
review: saturação, magnitude, vocabulário as_str/parse_token, example
multi-unidade, keep_b).

## Dados

- Replay WIN: C:\src\quantick\WINV26\ (2026-07-01, 2026-08-11, 2026-08-13).
- Example de auditoria (commitado): crates/replay/examples/imbalance_audit.rs.
- Bench baseline main: imbalance(100) = 545,5 ns/trade.
