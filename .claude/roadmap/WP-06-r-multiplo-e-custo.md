# WP-06 — R-múltiplo e custo por trade: a régua do operacional

**Missão**: o operacional inteiro é medido em **R** (múltiplos do risco) e
descontado de **custo**. O simulador hoje não sabe nenhum dos dois: o stop
planejado é perdido antes do trade fechar, e custo é explicitamente fora de
escopo. Sem estes dois números, todo gate de graduação da §12 do operacional é
calculado à mão em planilha — que é onde a disciplina morre.

Branch: `feat/r-multiple-and-cost` · worktree
`../quantick-worktrees/feat-r-multiple-and-cost`

Depende de: nada. Bloqueia: nada (mas WP-02 fica muito mais útil com ele).

## Decisão registrada que este pacote reabre — leia antes de começar

`docs/ux/paper-trading.md:285` lista, em "Out of scope (recorded decisions)":
**"Fees, margin, multi-account, per-instrument currency P&L."** E `:31-32`
diz "No book depth, no queue position, no slippage model — and the UI never
implies otherwise".

Este pacote **reabre parcialmente** essa decisão, e o PR precisa dizer isso com
todas as letras. A reabertura é estreita e defensável:

- O custo entra **em pontos**, nunca em moeda — a regra de honestidade original
  (o workspace não tem tabela de tick value por instrumento) permanece intacta.
- O custo é um **parâmetro do operador**, não uma tabela de corretagem embutida.
  O simulador não sabe quanto custa negociar; ele sabe descontar o número que o
  operador declarou ter medido.
- O modelo de fill **não muda**: continua sem book, sem fila, sem slippage
  simulado. Custo por trade é um débito contábil, não uma simulação de execução.

O que **não** entra: moeda, margem, tabela de taxas, slippage modelado.
Atualizar `docs/ux/paper-trading.md` faz parte do pacote — a decisão registrada
tem que refletir a decisão vigente.

Efeito colateral a tratar: `docs/ux/paper-trading.md:79` descreve o botão
*Breakeven* como "with no fees simulated, break-even is the entry exactly". Com
custo declarado, ou o botão passa a mirar entrada + custo, ou o texto explica
por que continua na entrada exata. Decidir e documentar.

## Parte A — R-múltiplo

O problema real: **o stop planejado não sobrevive ao trade.** `Bracket`
(`order.rs:47-53`) vive na ordem; no fill, `opened_position`
(`simulator.rs:774-792`) copia só os níveis correntes para `Position`
(`position.rs:11-37`), e `record_close` (`simulator.rs:741-769`) nunca os lê.
Pior: `Command::SetBracket` sobrescreve o stop durante o trade
(`simulator.rs:474-475`) — é assim que o botão Breakeven funciona. Ou seja, no
fechamento não existe mais nenhum vestígio do risco que foi assumido na entrada.

**Ponto de captura**: `Position` ganha o stop **inicial** (imutável depois de
definido), carimbado em `opened_position` (`simulator.rs:774`) e propagado em
`record_close` para o `ClosedTrade`.

Casos de borda que o desenho **precisa** resolver explicitamente, com decisão
escrita no doc do campo:

1. **Bracket descartado** por `admissible_bracket` (`simulator.rs:549-581`,
   emite `SimEvent::BracketDropped`) → sem stop → R é `None`, nunca zero.
2. **Averaging in** (`simulator.rs:614-633`) não recria a `Position`: o stop
   inicial persiste enquanto `entry_price` muda. O crate já resolveu esse
   mesmo dilema para MAE/MFE medindo contra a média final
   (`simulator.rs:39-43`) — seguir o mesmo princípio e dizê-lo no doc.
3. **Reversal** (`simulator.rs:656-657`) cria `Position` nova → stop inicial
   novo, o da entrada oposta.
4. **Close parcial** (`simulator.rs:726-730`) mantém a `Position` → os dois
   `ClosedTrade` carregam o mesmo stop inicial.
5. **Reset** (`simulator.rs:330-348`) fecha no mark → R continua computável se
   o stop inicial estiver na `Position`.
6. **Ordem sem bracket** (entrada manual sem stop) → `None`. Um trade sem
   risco declarado não tem R, e fingir que tem seria inventar dado.

Saída: `ClosedTrade` ganha o risco planejado; o R-múltiplo pode ser campo
derivado ou calculado no relatório — decidir e justificar (calcular no
relatório evita persistir número redundante; persistir o risco em pontos é o
que realmente falta no arquivo).

## Parte B — custo por trade

- **Parâmetro**: campo de configuração no `Simulator` (`simulator.rs:126-136`,
  hoje `#[derive(Debug, Default)]` com `new()` = `default()`), com default
  zero/`None` para que `Simulator::new()` e todos os testes existentes
  continuem compilando e valendo.
- **Cálculo**: em `record_close` (`simulator.rs:741-769`), único produtor.
- **`pnl_points` continua bruto.** Ele já está persistido como bruto em
  arquivos v1/v2 no disco; redefini-lo como líquido reescreveria o significado
  de dado histórico. O custo entra como campo próprio — `None` = "não
  registrado", `Some(0)` = "sem custo". O formato já sabe representar essa
  distinção e a doutrina da casa é explícita: *unknown is not zero*.
- **Decidir e declarar**: se `realized_points()` (`simulator.rs:193`, lido em
  `paper_trading.rs:659`, `:1532`, `:2035`) passa a ser líquido, são três
  superfícies visíveis mudando de significado. Recomendação: manter bruto e
  expor o líquido como número separado no relatório.

## Parte C — relatório e persistência

1. **`PerformanceReport`** (`report.rs:26-111`) é construído **só** em
   `from_trades` (`:188-353`) — não há literal fora dela, então acrescentar
   campo é puramente aditivo. Seguir o molde
   **média + denominador exposto** já usado por
   `avg_winner_mae_points` / `winners_with_mae` (`report.rs:97-104`), que o app
   renderiza como "over N of M" (`paper_trading.rs:3392-3399`). É exatamente a
   forma certa para "R médio sobre os N trades com stop conhecido" e "custo
   sobre os N trades com custo registrado".
2. **Razão sem denominador é `None`** — usar `fn ratio` (`report.rs:175-177`),
   nunca um número inventado.
3. **Bump do histórico para v3** (`history.rs`), procedimento exato:
   - `FORMAT_VERSION = 3` (`:39`);
   - congelar o header atual como `HEADER_V2` privado ao lado de `HEADER_V1`
     (`:46`), e `pub const HEADER` passa a ser o novo;
   - `expected_header` (`:146-149`) vira `match version`;
   - `parse_row` (`:210`) passa a `match version { 1 => 8, 2 => 12, _ => 12+N }`
     e o ramo de preenchimento (`:230-239`) ganha o terceiro caso, com campos
     novos `None` em v1 **e** v2;
   - `write_trade` (`:98-112`) emite os campos novos sempre, com `opt_decimal`
     (vazio para `None`);
   - a mensagem de erro de `:141-144` diz "version 1 or 2" — com v3 ela passa a
     mentir; corrigir para uma faixa.
   - Testes a atualizar, todos inline no arquivo:
     `unknown_version_is_fatal_not_guessed` (`:387`, usa literalmente
     `# quantick-trades 3`), `a_torn_final_line_costs_one_reported_row_not_the_file`
     (`:375`, asserta "12 fields"),
     `a_v1_row_re_exports_with_empty_fields_not_invented_values` (`:353`),
     `unknown_exit_reason_is_a_problem_row` (`:413`),
     `a_v2_file_with_the_v1_header_is_fatal` (`:404`); mais um par novo
     v2-ainda-carrega / v3-round-trip espelhando `:329` e `:312`.
4. **UI** (`paper_trading.rs`): linha na grade do relatório
   (`draw_report_grid`, `:3279-3417`, com o helper `row(label, value, explain)`
   de `:3291-3296`); coluna no `export_csv` (header literal de 17 colunas em
   `:4380-4382` + placeholder + argumento); rodapé de honestidade
   (`:2511-2518`) declarando que o custo também é em pontos. **Cuidado com os
   tiles**: `draw_report_tiles` (`:2874-2917`) divide a largura por 3
   literalmente em `:2875` — um quarto tile exige mudar esse divisor.
5. **Onde o operador define o custo**: o ticket em `paper_trading.rs`, com
   persistência no sidecar `paper_state.rs` (`PaperStateFile`, `:28-34`,
   `version = 1`; campo novo com `#[serde(default)]` é aditivo e compatível).
   Se isso criar superfície nova de configuração, ela exige hook `QUANTICK_*`
   nesta mesma mudança.
6. **Relatório lê do disco, não da sessão** (`:2373-2409`, `:3569-3575`) — logo
   R e custo só aparecem para trades cujo arquivo já tenha as colunas. Trades
   antigos ficam `None` e caem no padrão "over N of M". Isso é a resposta certa,
   não um bug a esconder.

## Critérios de aceite (resumo verificável)

1. Um trade fechado com stop conhecido reporta R; sem stop conhecido reporta
   `None` — provado por teste para cada um dos 6 casos de borda acima.
2. Custo em pontos, parametrizado, default zero, `pnl_points` inalterado.
3. Relatório expõe R médio e custo com denominador declarado.
4. Histórico v3 grava e relê; v1 e v2 continuam carregando com `None` nos
   campos novos.
5. `docs/ux/paper-trading.md` atualizado: a decisão registrada sobre fees
   reflete o que passou a valer, e o parágrafo do Breakeven diz a verdade.
6. Nomes de teste no idioma da casa. Modelos reais:
   `market_fills_at_the_next_print_not_the_last_one` (`simulator.rs:906`),
   `a_version_1_file_still_loads_with_honest_unknowns` (`history.rs:329`),
   `all_winners_leave_profit_factor_undefined_not_infinite` (`report.rs:657`).
   Testes vivem em `#[cfg(test)] mod tests` no fim do próprio arquivo — o crate
   `sim` **não tem** diretório `tests/`.
7. Fixtures de aritmética calculadas à mão e comentadas, no estilo de
   `report.rs:491-638`.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto de performance declarado: **por trade fechado** (não per-frame,
      não per-tick) — custo desprezível, mas declarado.
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] Reabertura da decisão de escopo documentada no corpo do PR **e** em
      `docs/ux/paper-trading.md`.
- [ ] `ui-harness`: se surgiu superfície de configuração de custo, hook novo
      nesta mudança. Superfícies existentes (relatório, ledger, export) já têm
      `QUANTICK_PAPER_REPORT_AUTOSTART` e `QUANTICK_DOCK_TAB=trades`.
- [ ] `visual-qa` no relatório e no ledger.
- [ ] `trader-ux-review`: o trader entende, sem manual, que R é do stop
      planejado e que custo é o que ele declarou — não uma tabela da corretora.
- [ ] PR aberto com CI verde. Merge não faz parte.
