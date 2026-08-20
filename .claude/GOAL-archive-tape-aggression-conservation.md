# Missão — nenhuma agressão desaparece, e o tape responde por si

**Objetivo em uma frase**: a fita (tape) abre e se configura sozinha,
independente do L2/heatmap do gráfico ao lado, e o projetor de bolhas passa a
**conservar toda agressão**: agregar é fundir (uma bolha maior), nunca apagar —
o conjunto desenhado deixa de mudar com o zoom do gráfico ou com a velocidade
do tape.

Branch: `fix/tape-aggression-conservation` ·
worktree `../quantick-worktrees/fix-tape-aggression-conservation`

## Por que isso importa

O trader lê pressão de compra e venda nessas bolhas para decidir entrada e
saída. Uma agressão que some sem aviso não é um detalhe visual: é informação
faltando na hora da decisão, e prejuízo no dinheiro do cliente. Quem paga o
salário é o cliente — a fita tem que ser honesta com ele.

## Suspeitos levantados antes de cortar o worktree

1. **`cap_aggressions` trunca** (`crates/app/src/orderflow/projection.rs:1067`):
   ordena por quantidade e faz `truncate(max_aggression_primitives)` (700 por
   padrão). Descarte puro — as bolhas mais fracas somem. Quantas primitivas
   existem depende da janela visível, logo **mudar o zoom muda quem sobrevive
   ao corte**. É o candidato número um para "some e aparece do nada".
2. **`EffectiveGrouping::resolve` é adaptativo ao zoom**
   (`crates/app/src/orderflow/grouping.rs:31`): `DisplayGrouping::Adaptive`
   deriva o múltiplo de `visible_span`, então o zoom do gráfico redefine a
   largura da faixa de preço que agrupa os clusters — e essa mesma grouping é
   passada para o tier do tape. Cross-talk entre os dois painéis.
3. **Pisos que fazem `retain`** (`projection.rs:1205` e `:1249`):
   `min_quantity_decimal()` remove clusters abaixo do piso — política
   declarada do usuário, mas precisa ser distinguida de sumiço acidental.
4. **`merge_dust_clusters` com janela zero** (`projection.rs:1218`): confirmar
   que funde e nunca descarta quando `bubble_dust_merge_ms == 0`.
5. **Independência do tape**: `lane_depth_visible()` exige `self.enabled`
   (captura de L2) — `config.rs:1327`; e `ChartLayer::TapeHeatmap` /
   `TapeBubbles` são desabilitadas por `capabilities.book_capture` /
   `traded_volume` (`pane.rs:1511-1513`). Verificar se abrir o Tape depende do
   heatmap do gráfico estar ligado.

## Critérios de aceite

### Específicos da missão

1. **O tape abre sozinho.** Clicar em Tape exibe a fita com o L2/heatmap do
   gráfico desligado, e com um feed sem captura de book. Nenhum switch do
   gráfico da esquerda apaga um pixel da fita, e nenhum switch da fita muda o
   gráfico. Provado por teste que projeta os dois painéis com o outro
   desligado, e por captura visual.
2. **Conservação da agressão (invariante duro).** Para toda combinação de
   zoom, velocidade do tape e grouping, a soma das quantidades das bolhas
   desenhadas é igual à soma das quantidades dos prints retidos na janela
   (descontado apenas o que um piso *explícito* do usuário filtrou, e esse
   piso é declarado na tela). Teste de propriedade sobre uma grade de zooms e
   velocidades.
3. **Agregar é fundir, nunca apagar.** `cap_aggressions` deixa de truncar: o
   excedente é fundido em bolhas vizinhas (mesmo lado, mesma faixa de preço),
   somando quantidade e unindo evidências. O que não puder ser fundido é
   declarado ao usuário (rótulo de dado incompleto), nunca descartado em
   silêncio — regra de honestidade de dado do CLAUDE.md.
4. **Independência de eixo.** Mudar o zoom do gráfico principal não altera
   nenhuma primitiva da lane; mudar a velocidade da fita não altera nenhuma
   primitiva do corpo do gráfico. Teste que projeta dois frames e compara os
   conjuntos elemento a elemento.
5. **Reversibilidade.** Voltar o zoom (ou a velocidade) ao valor anterior
   devolve exatamente o mesmo conjunto de bolhas — o projetor é função pura da
   janela, sem histerese.
6. **Laudo da investigação cirúrgica.** Cada caminho de sumiço rastreado até
   `arquivo:linha`, com veredito (bug / política declarada / falso positivo),
   contestado por agentes críticos adversariais, e anexado ao corpo do PR.

### Portões padrão injetados

7. Quatro checks verdes (`fmt`, `clippy -D warnings`, `build`, `test`) após
   rebase no `main` atualizado.
8. **Impacto de performance declarado**: a projeção é caminho *per-frame* e
   *per-trade* (retenção). Evidência medida — `APP_HEALTH_SUMMARY` fps /
   frame_avg sob tape denso contra um controle no `main`, ou bench sobre
   fixture — números no corpo do PR.
9. **UI**: `ui-harness` (hook de env para cada superfície nova/alterada),
   `visual-qa` com todas as superfícies PASS ou defeito aceito explicitamente,
   `trader-ux-review` sem Blocker em aberto.
10. **Test-first** no núcleo do projetor: fixture + saída esperada escritas
    antes do código; teste golden guarda o determinismo.
11. `arch-review` rodado, todo Blocker/Should-fix resolvido ou deferido no
    corpo do PR.
12. **PR aberto** com CI verde e as evidências no corpo. Merge não faz parte da
    missão.

## Decisões do dono do produto (tomadas com o laudo em mãos)

1. **Orçamento por painel, fusão só no gráfico.** A fita ganha cota própria e
   independente — o zoom do gráfico nunca mais a toca. Dentro da cota da fita,
   se ainda estourar, funde **do print mais antigo** (a borda esquerda, que já
   está saindo) e preserva print a print na **borda direita**, que é onde o
   trader lê. No corpo do gráfico: fusão de vizinhos do mesmo lado e faixa,
   **nunca `truncate`**. Soma das quantidades desenhadas == soma dos prints,
   nos dois painéis.
2. **O aviso vive onde o dado vive.** Uma bolha fundida se distingue de uma
   bolha nativa na própria fita (contorno + contador de prints), em vez de só
   um número dentro do painel de configuração.

## Plano de correção derivado dos 13 achados

| # | Correção | Achados que fecha |
|---|---|---|
| 1 | Âncora do tape passa a ser o último **print**, não o último book; `live_end_ms` perde o portão `depth_visible_anywhere()` | 7 |
| 2 | `layer_blocked` responde por `TapeChart`; chip e pixel deixam de discordar | 8 |
| 3 | `cap_aggressions` vira fusão com cota por painel | 1, 2, 3 |
| 4 | Marca de fusão visível na fita (contorno + contador) | 4 |
| 5 | A fita ganha agrupamento de preço próprio; deixa de herdar a `EffectiveGrouping` do zoom do gráfico | 5, cadeia A |
| 6 | A janela de tempo da fita deixa de ser derivada das barras **visíveis** | 12 |
| 7 | As duas metades passam a concordar sobre `summarizing` | 11 |
| 8 | `slot_prints` deixa de perder prints quando o resumo está off | 10 |
| 9 | Testes reescritos: o que consagra o vazamento vira o espelho correto | 9, falsa segurança |


---

# O que de facto embarcou

## Os treze achados, e o que cada um virou

| # | Achado | Correção |
|---|---|---|
| 1-3 | O teto de bolhas era um `truncate` por quantidade sobre **as duas metades juntas** | Orçamento por painel; excedente fundido, nunca descartado |
| 4 | A perda só aparecia dentro do painel de configuração | Bolha fundida usa anel + contador na própria fita |
| 5 | A fita herdava a `EffectiveGrouping` adaptativa ao zoom do gráfico | A fita clusteriza em resolução de captura; `Multiple(n)` explícito continua sendo obedecido |
| 7 | `live_end_ms` vinha de `latest_book_ms` atrás de um portão de profundidade | Relógio de prints no histórico; portão passa a ser "alguma camada de fluxo" |
| 8 | O botão do Tape acendia e nada aparecia | Consequência do 7; resolvido junto |
| 9 | Um teste consagrava o comportamento defeituoso | Reescrito |
| 10 | Alargar a fita retirava prints do corpo do gráfico | A metade settled deixou de perguntar onde a fita começa |
| 11 | As duas metades discordavam sobre "resumir" | Unificadas |
| 12 | A janela da fita vinha das barras **visíveis** | Vem das barras mais novas da série |
| 13 | Evidência de consumo alocada na fronteira móvel | Coberto pela mudança do 10 |

## Achados que só apareceram escrevendo o código

- **O cache da metade settled não invalidava quando a costura se movia.** Chavear na borda da fita teria reconstruído o histórico comprimido a cada frame; a saída foi tirar a pergunta da metade settled.
- **O primeiro fold era guloso e quadrático** — 2794 ms contra 111 ms do `main`. O bench pegou.
- **`project_live` devolve marcas dos dois painéis.** Fundi-las juntas desenhava volume de um painel dentro do outro.
- **A âncora comparava contra o total corrente**, não contra o membro mais pesado: um grupo [2,2,2,5] ancorava no 2.
- **A régua lateral contava o mesmo contrato duas vezes** num frame resumido, e misturava duas grades de preço.

## Regras que passaram a valer

1. **Conservação por painel.** A soma das quantidades desenhadas é a soma dos prints da janela, em qualquer zoom ou velocidade.
2. **Fundir, nunca apagar.** O excedente do teto é fundido; o teto deixa de ser absoluto quando respeitá-lo exigiria cruzar um lado, um painel ou uma barra.
3. **Uma fusão nunca cruza lado, painel ou barra.** Correção supera o alvo de performance.
4. **Uma fusão ancora onde o volume está** — no membro mais pesado, nunca na média.
5. **Uma bolha fundida se declara** — anel e contador, na fita.

## Ficou de fora, declarado

- **Tremulação do fold da fita entre frames.** Confinada à borda esquerda pelo `OldestFirst`. Corrigir de vez exige ancorar os grupos em janelas de tempo fixas.
- **O fold no seu próprio módulo.** `projection.rs` passou de 4300 linhas.
- **Os outros caminhos de descarte** (piso `min_quantity`, recorte pela faixa de preço visível, retenção) continuam sem declaração em contratos na tela.
