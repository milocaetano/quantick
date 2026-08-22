# WP-07 — HUD checklist determinístico v0 (o anti-copilot)

**Missão**: um painel que mostra os gates do setup ativo como linhas
`medido / limiar / ✓✗`, com os **números visíveis**. Zero IA, zero score
composto, zero sugestão. O copilot dizia "pode operar isto?"; este diz
"largura 4,2×ATR ≤ 6,0 ✓" e deixa a decisão inteira com o trader.

Branch: `feat/checklist-hud` · worktree `../quantick-worktrees/feat-checklist-hud`

Depende de: nada (ver abaixo). Bloqueia: WP-14.

## A descoberta que tirou a dependência

O plano original marcava este pacote como bloqueado pela "porta de valores
nomeados" (WP-08). O reconhecimento mostrou que não é verdade **para a v0**:

Tudo que atravessa o canal do worker de indicadores é **posicional** —
`columns[i]` / `row[i]` "one value per declared plot, in descriptor order"
(`indicator_worker.rs:44-46`). Nome existe só no `PlotSpec.title`
(`indicators/output.rs:148-164`), e o par nome→número é reconstruído por índice
na UI (é o que a legenda faz em `indicator_legend.rs:154-168`). Não há método
na trait `Indicator` que devolva escalares nomeados nem limiares — e o engine
também não guarda limiar nenhum, por decisão explícita: *"the engine attaches
no thresholds of its own — 70% is the caller's convention, not this module's"*
(`engine/profile.rs:25-27`).

**Mas os gates da v0 não vêm de indicador.** Vêm de três fontes que o app já
tem em mãos, no mesmo frame:

| Gate da v0 | Fonte | Onde já existe |
| --- | --- | --- |
| dia classificado range / largura do range ≥ L_min | `ChartState` (barras) + a marcação do OB | `state.rs::bars()` |
| preço a ≤ 0,5×ATR do extremo | barras + ATR calculado no app | função livre, testável |
| janela horária operável | timestamp do último trade | `tab.rs::latest_trade_ms` |
| trades do dia < cap · stops seguidos | `PaperTrading` / `sim` | `paper.closed_trades()` |
| fita saudável (sem gap recente) | status do feed | o mesmo dado da status bar |

Logo: **v0 calcula os gates no app, em funções livres puras**, que é o padrão
da casa para lógica testável sem janela (`covered_slots` em `frvp.rs:161`,
`hud_offset_px` em `indicator_legend.rs:67`, `entry_drift` em
`paper_hud.rs:39`). A porta de valores nomeados (WP-08) só é necessária quando
um gate precisar vir de um script `.pine` de usuário — que é a v1, no WP-14.

## Onde o painel vive: decisão a tomar e justificar

Duas portas legítimas, ambas registradas na skill `new-extension`:

1. **Aba do dock** (`dock.rs`) — o lugar reservado; o doc do módulo diz
   "Indicators docks here when it lands" (`dock.rs:4-5`). Custo: variante nova
   + `ALL: [Self; 5]` → `[Self; 6]` (`dock.rs:51-57`) + `icon`/`title`/
   `hover_text` (`:65,77,89`) + array `widths` (`:159`, hoje 5 elementos) +
   braço do `match` em `draw` (`:300-334`) + `SavedDockTab` e as **duas** `From`
   em `ui_state.rs:130-160` + parse do hook `QUANTICK_DOCK_TAB`
   (`app.rs:1276-1296`) + lista do menu (`app.rs:3839-3843`). Os testes
   `every_tab_has_strip_metadata` (`dock.rs:466`) e
   `the_dock_draws_every_tab_against_a_real_context` (`:479`) varrem
   `DockTab::ALL` e cobrem a aba nova de graça.
2. **HUD flutuante** no molde de `paper_hud.rs` (`draw(ctx, chart_rect, …)`,
   `Area::fixed_pos(chart_rect.left_top() + 8)`, `Order::Middle`). Ressalva
   verificada: **o canto superior-esquerdo já está disputado** — o HUD de
   posição e a legenda de indicadores convivem por `hud_offset_px`
   (`indicator_legend.rs:67`, chamado em `app.rs:1954-1957`), e essa aritmética
   é **binária** hoje. Um terceiro morador exige entrar nela ou escolher outro
   canto.

Recomendação: **aba do dock** para a v0. O checklist é lido antes de armar o
trade, não durante — não precisa flutuar sobre o preço, e o dock não disputa
pixel com a barra em formação (heurística de oclusão do `trader-ux-review`).

## Critérios de aceite

1. **Cada linha mostra o número.** Formato `rótulo · medido · limiar · ✓/✗`.
   Um gate que só mostra ✓ falhou o propósito: o trader precisa ver *quanto*
   passou, para saber se passou raspando.
2. **Sem score composto.** Nada de "3/4 condições" como veredito único, nada
   de semáforo agregado, nada de sugestão de entrada. As linhas são
   independentes e o trader compõe. Esta é a diferença de projeto em relação
   ao copilot e ela é inegociável.
3. **Gate sem dado mostra "sem dado", nunca ✓.** Feed sem fita, janela ainda
   não formada, ATR sem barras suficientes → estado explícito de ausência.
   Um gate que passa por falta de informação é a mentira mais cara possível.
4. **Os limiares vêm da §04 do operacional** e são visíveis na tela; nenhum
   número mágico enterrado no código. Se um limiar for configurável, ele
   persiste no sidecar de estado (padrão `paper_state.rs`, `#[serde(default)]`
   é aditivo e compatível).
5. **Lógica em funções livres puras**, testadas sem abrir janela — cada gate é
   uma função `(entradas) -> GateResult`. A renderização é burra.
6. **Hook de harness no mesmo commit** — regra literal do `ui-harness`:
   "New surface → new `QUANTICK_*` env hook in the same commit: read the var
   next to the existing autostart block in `crates/app/src/app.rs`, call the
   same function the manual toggle calls, default off. Then add one row to the
   registry table. That row is part of the feature's definition of done."
   Se a v0 for aba do dock, `QUANTICK_DOCK_TAB=checklist` já cobre — mas a
   linha na tabela do skill continua obrigatória. Nome desconhecido deve
   emitir `tracing::warn!` com `event_code`, nunca ser adivinhado
   (`app.rs:1287-1294`).
7. **Testes no padrão headless da casa**: `egui::Context::default()` +
   `ctx.run(...)` em **duas passadas**, extraindo texto de
   `Shape::Text(galley) => galley.galley.text()`. Modelos:
   `the_dock_draws_every_tab_against_a_real_context` (`dock.rs:479`),
   `the_hud_paints_the_position_and_only_the_position` (`paper_hud.rs:236`).
   Nomes de teste em frase declarativa minúscula, com doc `///` acima
   explicando o bug que o teste previne — costume da casa.
8. **Um teste prova que gate sem dado não pinta ✓.** É o teste que protege a
   propriedade central do pacote.

## Fora de escopo

- Gates vindos de script `.pine` → WP-08 + WP-14.
- Bloquear os botões BUY/SELL (session guard) → WP-14. A v0 **informa**; ela
  não impede. Impedir é mudança de contrato de execução e merece pacote
  próprio, com o trader ciente.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto de performance declarado: **per-frame**, com os gates calculados
      uma vez por frame e nada no caminho per-trade.
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] `new-extension`: porta nomeada (aba do dock ou HUD), registro como única
      edição ao existente, defaults preservam o hoje.
- [ ] `ui-harness`: hook no mesmo commit + linha na tabela do skill.
- [ ] `visual-qa` com a matriz de estados: aberto por default, ligado por hook,
      dado vazio, dado denso (replay rápido), janela estreita ~1000 px e normal,
      estado desabilitado. `APP_HEALTH_SUMMARY` com fps ≥ ~59 sob fita densa.
- [ ] `trader-ux-review`: custo de olhada (o número-chave legível sem se
      inclinar), orçamento de interrupção zero (não cobre preço/tape/barra em
      formação), dado inferido rotulado no ponto de leitura.
- [ ] PR aberto com CI verde. Merge não faz parte.
