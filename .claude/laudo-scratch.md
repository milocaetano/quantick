# Laudo — por que uma agressão some da fita

Investigação cirúrgica da missão `fix/tape-aggression-conservation`.
Cada achado é rastreado até `arquivo:linha` na árvore em `origin/main` (2e8122e3).

## Achado 1 — o teto de primitivas é COMPARTILHADO pelos dois painéis

`crates/app/src/orderflow/projection.rs:344-361`, em
`SettledProjection::with_live`. O comentário do próprio código diz:

> "The primitive cap is applied here, **over both halves at once**, so the
> budget a frame is allowed to spend on bubbles is the whole chart's and not
> one per half."

As duas metades — a metade *settled* (corpo do gráfico) e a metade *live*
(a fita) — são concatenadas e cortadas juntas em
`config.max_aggression_primitives` (default **700**, `config.rs:1284`).

**Consequência direta**: quanto mais clusters o corpo do gráfico produz, menos
sobra para a fita. Dar zoom out no gráfico principal traz mais barras para a
janela, portanto mais clusters *settled*, portanto a fita perde bolhas — sem
que nada na fita tenha mudado. É exatamente o sintoma relatado
("mexe no zoom do gráfico e muda a agressão no gráfico do extremo direito").

## Achado 2 — o corte é `truncate`, e ordena por quantidade

`crates/app/src/orderflow/projection.rs:1067-1079`:

```rust
fn cap_aggressions(marks: &mut Vec<AggressionPrimitive>, limit: usize) {
    if marks.len() <= limit { return; }
    marks.sort_by(|a, b| b.quantity.cmp(&a.quantity)
        .then_with(|| a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
        .then_with(|| a.agg_id.cmp(&b.agg_id)));
    marks.truncate(limit);
}
```

Descarte puro: as bolhas mais fracas somem, e a quantidade que elas carregavam
some com elas. Nada é fundido. É a violação literal da regra do produto —
"o agregador deve criar uma nova bolha juntando elas, e não apagar a bolha do
nada".

E o critério de corte é a **quantidade**: a fita desenha prints um a um
(quantidades pequenas), o corpo do gráfico desenha clusters já somados
(quantidades grandes). Sob pressão de teto, **a fita perde primeiro, sempre**.

## Achado 3 — o corte acontece duas vezes

Além do corte no join (`:361`), cada metade já se cortou ao ser construída
(`:784-785`). Uma bolha pode morrer antes mesmo de chegar ao orçamento comum.

## Achado 4 — a perda é declarada, mas onde o trader não olha

`dropped_aggressions` existe e sobe até a UI, mas o único lugar onde é lido é
um `ui.small()` dentro do painel de configuração de bolhas
(`crates/app/src/orderflow_view.rs:1970-1975`):

> "{n} above the primitive cap were not drawn"

Quem está lendo a fita não tem esse painel aberto. Na prática o descarte é
**silencioso na superfície onde a decisão é tomada** — o gráfico. Isso
contraria a regra "Data honesty" do CLAUDE.md: dado incompleto tem que ser
rotulado onde é lido.

## Achado 5 — o agrupamento de preço é derivado do zoom do gráfico

`crates/app/src/orderflow/grouping.rs:31-45` — `DisplayGrouping::Adaptive`
resolve o múltiplo de agrupamento a partir de `visible_span`, o span de preço
**visível**. A mesma `EffectiveGrouping` é passada aos dois tiers, então o zoom
do gráfico redefine a largura da faixa que funde clusters *também na fita*.

Fundir por zoom é legítimo (é o que mantém um gráfico legível); o que não é
legítimo é o zoom de um painel decidir o agrupamento do outro.

## Achado 6 — pisos que removem por `retain`

`crates/app/src/orderflow/projection.rs:1205-1210` e `:1249-1251`:
`min_quantity_decimal()` remove clusters abaixo do piso. É política explícita
do usuário (ele escolheu o piso), mas o piso não é declarado na fita — o
trader não vê que existe um filtro ligado.

## Achado 7 — a existência do tape está ancorada no relógio do LIVRO

Dois defeitos empilhados no mesmo instante publicado.

**(a) O portão.** `crates/app/src/orderflow_view.rs:420-430`:

```rust
pub fn live_end_ms(&mut self) -> Option<i64> {
    if !self.config.depth_visible_anywhere() { return None; }
    ...
}
```

e `:448` — `let end_ms = self.live_end_ms()?;` em `live_lane()`. Sem esse
instante, `lane_width_px = 0.0`: some a banda, o divisor
(`orderflow_render.rs:976-979`), as bolhas, o eixo de tempo do tape e o menu
próprio do tape (`pane.rs:1618-1620`, `:1925-1927`). Desligar os **dois**
heatmaps apaga a fita inteira.

**(b) A âncora.** `crates/app/src/orderflow_engine.rs:1186-1190` alimenta esse
instante com `self.history.latest_book_ms()`. Esse campo só é escrito nos
caminhos de snapshot e delta de L2 (`orderflow/history.rs:517, 552, 652`; o
`:618` o zera). O caminho de trade apenas o *lê* (`:590`), nunca o escreve.

**Consequência**: um feed que só entrega negócios — MT5 sem profundidade
(`feed/metatrader.rs:135-155`), replay (`feed/replay.rs:289-290`,
`book_capture: false`) — **nunca tem tape, em nenhuma configuração**. Mesmo
removendo o portão de (a), o número continuaria `None`.

A correção precisa de uma âncora derivada do último **print**, não do último
book.

## Achado 8 — o botão do Tape mente

`layer_blocked` (`pane.rs:1510-1537`) não tem braço para `ChartLayer::TapeChart`
— cai em `_ => None`. Então o chip fica habilitado, `layer_visible(TapeChart)`
responde `true` (`pane.rs:1339`), o estado é gravado como "ligado" — e a tela
não desenha nada. O trader clica, o botão acende, e ele conclui que o produto
está quebrado.

A intenção correta já está escrita num teste, em `crates/app/src/app.rs:11777`:

> "the band itself is still the trader's to show: it carries the marks and the
> time axis whatever the source can produce"

A implementação contradiz a própria especificação declarada.

## Achado 9 — um teste consagra o comportamento defeituoso

`crates/app/src/orderflow_view.rs:2229-2248`,
`clearing_the_candles_does_not_delete_the_tape_itself`, termina com:

```rust
view.set_lane_depth_visible(false);
assert!(!view.config.depth_visible_anywhere());
assert_eq!(view.live_end_ms(), None);
```

Esse teste vai falhar quando a independência for consertada — e deve falhar.
Ele é a especificação errada, não uma rede de segurança.

## Lacunas de teste (independência do tape)

1. **Ninguém testa que a banda existe.** Os testes do switch usam
   `lane_width_px(width)` (`config.rs:1022-1029`), que não passa pelo portão.
   O caminho real de desenho é `live_lane()`, que passa. Os testes provam que a
   *config* mudou, nunca que uma banda foi desenhada.
2. `bubbles_project_with_l2_capture_off` (`projection.rs:2517`) e
   `turning_l2_capture_off_hides_the_map_...` (`:2547`) cobrem só o corpo do
   gráfico — nenhum toca `live_lane()`, `live_end_ms()` ou a fatia da lane.
3. Falta o caso exato do relato: **os dois mapas desligados, bolhas ligadas**.
4. Nenhum teste arma um feed sem `book_capture` e prova que o tape desenha.
5. Nenhum teste de coerência entre o chip e o pixel: hoje `layer_visible`,
   `layer_blocked` e a banda desenhada podem discordar em silêncio.

## Achado 10 — o sentido inverso: alargar o tape RETIRA prints do corpo do gráfico

`crates/app/src/orderflow/projection.rs:1145-1165`:

```rust
let lane_start_ms = timeline.lane_start_ms();
...
let on_tape = lane_start_ms.is_some_and(|start| trade.timestamp_ms >= start);
if on_tape { tape_prints.push(trade); }
if summarizing || !on_tape { slot_prints.push(trade); }
```

Com `bubble_candle_summary == false` — que é o **default de código**
(`config.rs:1274`) — um print que está na fita é **excluído do slot da barra**.

Cadeia: o gesto sobre a faixa (`pane.rs:4506`, `:4511`) → `zoom_live_lane`
(`orderflow_view.rs:478-486`) → `config.live_lane.window` →
`orderflow_engine.rs:988` → `timeline.rs:173` (`start_ms = now_ms - window_ms`)
→ `timeline.lane_start_ms()` → a linha acima.

**Alargar a fita tira os últimos minutos de agressão do corpo do gráfico** e os
empilha na faixa da direita. Com o resumo de vela ligado (o que todos os
presets de `crates/app/config/bubbles.toml` fazem) essa via fecha; com o
default de código, não.

### Correção de uma hipótese minha anterior

Eu havia atribuído o sentido inverso ao **encaixe da fronteira em slot de
barra** (`timeline.rs:218-225`). Isso está errado, e o encaixe é justamente o
que **impede** aquele vazamento: como nenhuma barra fica partida,
`summarize_clusters` (`interaction.rs:641-655`) agrupa por
`(bar_index, generation, price_bucket)` e emite as mesmas pizzas com a mesma
quantidade, esteja a barra em qual metade estiver. O doc comment em
`timeline.rs:213-216` diz exatamente por quê. O mecanismo real é o
`slot_prints` acima.

## Achado 11 — as duas metades não concordam sobre o que é "resumir"

`projection.rs:695` (settled): `let summarizing = config.bubble_candle_summary;`
`projection.rs:905-906` (live): `config.bubble_candle_summary && config.show_buy_aggressions && config.show_sell_aggressions`

Com um dos lados de agressão escondido, as duas metades divergem — e mover a
fronteira converte pizzas do corpo do gráfico em prints crus. Vazamento
condicional, e uma inconsistência que não tem justificativa declarada.

## Achado 12 — a janela de TEMPO do tape vem das barras visíveis do gráfico

`orderflow_engine.rs:985` calcula `reference_ms = reserved_span_ms(&request.closed)`
sobre a fatia **visível** (`orderflow_view.rs:663-670`, `pane.rs:4903-4908`) —
mediana das 8 últimas barras fechadas visíveis (`timeline.rs:54-67`).

Consequência: dar pan para o passado, ou zoom in a ponto de restarem menos de 8
barras fechadas visíveis, muda **quanto tempo de mercado a fita mostra**. E
mesmo com a fita pinada em `Fixed`, `effective_cluster_ms` divide por
`cluster_scale(reference_ms)` (`config.rs:1062-1071`) — a janela de
clusterização da fita continua seguindo a fatia visível do gráfico.

## Achado 13 — a evidência de consumo é alocada na fronteira móvel

`projection.rs:729-744` corta os `LiquidityEvent` na mesma fronteira, e
`correlate_tier` no ramo não-summarizing (`:970-978`) concatena tape+slot numa
lista só antes de `correlate_liquidity`. Quem compete por uma redução perto da
fronteira muda com a janela do tape, então `matched_fraction` das marcas sobre
as velas muda com a velocidade da fita.

## Quadro final dos dois sentidos

| Sentido | Veredito | Mecanismo dominante |
|---|---|---|
| zoom do gráfico → bolhas do tape | **PROVADO**, 3 cadeias | (A) `EffectiveGrouping` adaptativa ao `visible_span` do gráfico vira **chave de fusão** dos clusters da fita (`projection.rs:474` → `:918` → `interaction.rs:256`); a `LiveLaneStyle` não tem override de agrupamento de preço. (B) teto único de 700 com ranking por quantidade (`:344-361`, `:1067-1080`) — pizzas de barra esmagam prints unitários. (C) a janela de tempo da fita vem das barras visíveis |
| velocidade do tape → bolhas do gráfico | **PROVADO** | `slot_prints` exclui o que está na fita quando o resumo está off (`:1158-1163`); mais o teto compartilhado no sentido inverso, a divergência de `summarizing` e a alocação de evidência |

## Testes que dão falsa segurança

- `a_cluster_merged_by_zooming_out_reads_bigger_not_smaller`
  (`projection.rs:2778`) — **o pior**. Consagra o vazamento como desejado, e
  nenhum dos seus prints está sequer na lane (a lane começa em 15.000 ms e os
  prints estão em 6.000-7.000 ms). Diz só "no corpo do gráfico o zoom pode
  fundir". Falta o espelho: "no tape, o zoom do gráfico NÃO pode fundir".
- `the_lane_clusters_on_its_own_window` (`:3086`) — prova só a dimensão
  **tempo**. Roda com `DisplayGrouping::Native` (`:2665`) e `PriceWindow`
  constante: é estruturalmente incapaz de ver a cadeia (A).
- `a_prints_bubble_keeps_its_size_when_the_window_zooms_out` (`:2681`) —
  correto e valioso, mas prova só que a escala de **tamanho** não é do
  viewport. Não diz nada sobre contagem, fusão ou tape.
- `no_cluster_spans_the_lane_boundary` (`:3142`) — correto, mas é sobre a
  costura: um cluster pode não atravessar a fronteira e ainda assim ter sido
  fundido por decisão do outro painel.
- `the_primitive_cap_is_one_budget_for_both_halves` (`:1503`) e
  `the_reduction_cap_is_one_budget_for_both_halves` (`:1443`) — **não** são
  falsa segurança: são a especificação escrita do sintoma. Se o teto ganhar
  quota por painel, são estes dois que mudam.
