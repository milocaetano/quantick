# Missão: por que 1500 e 2000 trades de imbalance dão gráficos absurdamente diferentes

Branch: `fix/imbalance-target` — worktree `../quantick-worktrees/fix-imbalance-target`.
(NÃO COMMITAR este arquivo — é o registro da missão, não parte da feature.)

Relato do Camilo: MT5 ao vivo, barras de imbalance. Target 1500, depois 2000.
A mudança no gráfico é desproporcional ao delta de 500. Investigar o
comportamento, dar veredito com números, e corrigir se for defeito.

## Pista de partida (achada antes de começar)

`crates/engine/src/imbalance.rs` no `main` fecha uma barra em
`|θ| >= E[T] * max(|E[s]|, floor)`. `E[T]` é uma EWMA dos comprimentos de
barra que esse mesmo threshold produz — um laço linear sem amortecimento —
com clamps em `[target/4, 3*target]`. Existe uma branch órfã
`fix/imbalance-threshold` (commit 819cffb8, **sem PR**, base anterior à #191)
que diagnosticou isso e propôs fixar `E[T] = target` com floor `sqrt(target)`.
A hipótese a provar ou refutar: o atrator em que `E[T]` estaciona muda entre
1500 e 2000, e com ele o comprimento da barra, por mais de uma ordem de
grandeza.

## Critérios de aceitação

1. [x] **Veredito medido**, não teorizado: rodar 1500 e 2000 sobre a sessão
       WIN gravada (`WINV26/`, ticks reais do MT5 com a mesma tick rule que o
       app usa ao vivo) e reportar, por hora, barras/hora, trades por barra e
       % de barras fechando no cap. O veredito diz explicitamente se 500 de
       delta justifica a diferença vista.
2. [x] Se for defeito: **engine test-first** — fixture + saída esperada
       escritas antes do código. Testes que amarram as três promessas do
       botão: comprimento ≈ target em fluxo equilibrado, monotonicidade
       (target maior → barra maior) e independência do regime anterior.
3. [x] Correção portada sobre o `main` atual, que já tem as **três unidades**
       (`trades` / `volume` / `dollar`) — o floor tem de escalar certo em cada
       uma, não só em trades. Specs `imbalance:volume:N` / `:dollar:N` seguem
       válidas no app e no backtest.
4. [x] Goldens: verdes sem edição, ou regeneradas com a razão escrita no
       cabeçalho da fixture (nunca "atualizei porque quebrou").
5. [x] **Hot path declarado e medido**: `push` é per-trade. Bench
       imbalance(trades/volume/dollar) contra o `main` como controle, números
       no corpo da PR. Flat ou melhor.
6. [x] Se a UI mudar (tooltip/faixa do botão): hook `ui-harness` existente
       cobre; `visual-qa` só com autorização do Camilo para lançar o app.
7. [x] 4 checks verdes após rebase no `main`; `arch-review` rodado com todo
       Blocker/Should-fix resolvido ou deferido no corpo da PR; **PR aberta**
       (merge é de outro agente).
8. [x] Dizer ao Camilo o que fazer com a branch órfã `fix/imbalance-threshold`.

## Dados

- Replay WIN: `C:\src\quantick\WINV26\` (2026-07-01, 2026-08-11, 2026-08-13).
- Example de auditoria já commitado: `crates/replay/examples/imbalance_audit.rs`.
- Referência do diagnóstico anterior: `git show 819cffb8`.


## Resultado

PR #195 aberta, CI verde (a primeira execução falhou baixando
`Swatinem/rust-cache` — 429 do GitHub, não o código; re-executada).
Commits: 350a5575 (fix), c106b9da (arredondamento do floor), b8f01076
(14 dos 15 achados da review).

Veredito: o defeito era o laço de realimentação de `E[T]`. Medido em
WINV26/2026-08-13 sob tick rule — 1500 → 626 trades/barra, 1600 → 284,
1700 → 749; e 3720 vs 226 entre horas do mesmo dia no mesmo ajuste.
Depois: 973 / 1026 / 1098 / 1120 / 1190 / 1254, monotônico, faixa horária
de 18%, zero fechamentos no cap.

Bench: imbalance 531,8 → 442,0 ns/trade; imb-vol 1297,1 → 968,0;
imb-dlr 1585,2 → 1275,5.

Critério 6 (visual-qa): a única mudança visível é o texto de um tooltip
existente, sem alteração de layout. Não lancei o app — exige autorização
do Camilo.

Critério 8: a branch órfã `fix/imbalance-threshold` (819cffb8) está
superada por esta PR e pode ser apagada.
