# GOAL — o Tape chart é um gráfico com interruptor próprio

**Missão**: dar ao Tape chart (a live lane à direita do canvas) um interruptor
próprio no canto superior direito da área de desenho, e cortar de vez a herança
das camadas: o tape abre sempre com bolhas de agressão **e** book de L2 ligados,
o botão direito sobre ele desliga as suas, e os botões de L2/bolhas da toolbar
passam a governar **apenas** os candles.

**Branch**: `feat/tape-chart-switch`
**Worktree**: `C:/src/quantick-worktrees/feat-tape-chart-switch`
**Base**: `origin/main` @ 6d120a9

---

## Onde o código está hoje (levantado antes de começar)

- A live lane é o "Tape chart": `LiveLaneStyle` em
  `crates/app/src/orderflow/config.rs:843`, dentro de `HeatmapConfig`.
- `LiveLaneStyle::show_depth` / `show_aggressions` são `Option<bool>`: `None`
  **herda** o switch do gráfico. É essa herança que a missão remove.
- `HeatmapConfig::lane_depth_visible()` / `lane_aggressions_visible()`
  (`config.rs:1290` e `:1297`) resolvem a herança; `depth_visible_anywhere()` e
  `aggressions_visible_anywhere()` decidem se a projeção ainda produz primitivas.
- Os setters do gráfico (`OrderflowView::set_depth_visible` /
  `set_bubbles_enabled`, `orderflow_view.rs:217` e `:249`) hoje "congelam" o
  valor da lane ao **desligar**, mas ao **ligar** ainda arrastam a lane junto.
- O menu do botão direito sobre o tape já existe:
  `ChartPane::draw_tape_menu_section` (`pane.rs:1492`), roteado por
  `context_menu_on_tape` em `draw_layer_menu` (`pane.rs:1592`).
- Os botões da toolbar ficam em `draw_layers` (`toolbar.rs:661`), no grupo
  `right_to_left` do canto direito, e caem em `ToolbarAction::SetHeatmap` /
  `SetBubbles` (`app.rs:1969`).
- A largura da faixa sai de `LiveLaneStyle::resolved_width_px`, lida em
  `pane.rs:3919`; `lane_width_px > 0.0` já é o sinal que a projeção usa.
- A captura L2 (`HeatmapConfig::enabled`) é ligada sozinha por
  `Tab::ensure_book_capture` (`tab.rs:1095`) sempre que o feed a suporta, então
  "tape com L2 por padrão" não precisa acender captura nenhuma.

## Decisão do usuário

O interruptor do Tape chart é **um botão flutuante desenhado sobre o canvas, no
canto superior direito da área de desenho** (perto do badge de status do book),
não um quarto ícone da toolbar.

---

## Critérios de aceitação

### Específicos da missão

1. **O interruptor existe e fica onde foi pedido**: um controle desenhado no
   canto superior direito do canvas do flow pane liga e desliga o Tape chart
   inteiro. Screenshot mostrando-o nas duas posições.
2. **Desligado, o tape não existe**: a faixa não é reservada (os candles usam a
   largura toda), nada do tape é desenhado e a projeção não constrói primitivas
   só para ele. Teste que prova a largura da faixa = 0 e que a projeção não é
   pedida por causa da lane.
3. **Default do tape: bolhas ON + L2 ON, sem herdar**: instalação limpa abre o
   tape com as duas camadas ligadas, quaisquer que sejam os switches do gráfico.
   Teste sobre o estado inicial.
4. **A toolbar não toca no tape, nas duas direções**: alternar o botão de L2 e o
   de bolhas — ligando *e* desligando — não muda
   `lane_depth_visible()` / `lane_bubbles_enabled()`. Teste que faz os quatro
   movimentos.
5. **O botão direito continua sendo a casa dos switches do tape**, agora com o
   liga/desliga do próprio tape na mesma secção.
6. **Sobrevive ao relaunch**: tape ligado/desligado e as suas duas camadas
   voltam como foram deixados.
7. **Arquivo antigo degrada com honestidade**: um preset gravado antes desta
   mudança (sem os campos, ou com `null`) abre com o tape em ON/ON; um que grave
   `false` explicitamente continua respeitado.

### Gates injetados (mudança de código, superfície visível, caminho por frame)

8. `cargo fmt --all -- --check` — exit 0.
9. `cargo clippy --workspace --all-targets -- -D warnings` — exit 0.
10. `cargo build --workspace` — exit 0.
11. `cargo test --workspace` — exit 0.
12. **Impacto de performance declarado no plano, não na revisão**: cada caminho
    tocado classificado por taxa (por trade / por depth / por frame / raro). O
    botão e o gate da faixa são **por frame**; a mudança de herança é rara
    (clique), mas apaga um `Option` lido por frame.
13. **Evidência de performance, não crença**: `APP_HEALTH_SUMMARY` (fps /
    frame_avg) sob tape denso comparado com uma corrida de controle em `main`,
    ou um bench sobre fixture. Números no corpo do PR.
    ⚠️ **Depende de autorização do Camilo para abrir o app** (memória
    `no-agent-app-launches`); sem ela, o critério fica declarado como BLOCKED no
    PR, não silenciado.
14. **`ui-harness`**: a superfície nova (o botão sobre o canvas) ganha hook de
    env na mesma mudança, e o hook de menu de contexto do tape continua
    alcançando a secção nova.
15. **`visual-qa`**: matriz de estados (tape on/off × camadas do tape × camadas
    do gráfico) com todas as superfícies PASS, ou defeitos explicitamente
    aceites. Mesma dependência de autorização do critério 13.
16. **`trader-ux-review`**: sem Blocker por resolver.
17. **`arch-review`** sobre `git diff main...HEAD`, com todo Blocker e
    Should-fix resolvido ou deferido no corpo do PR.
18. **PR aberto** com a evidência no corpo. Merge não faz parte da missão.

---

## Fora de escopo (recusar)

- Mexer no que a toolbar desenha além de deixar de tocar no tape.
- Redesenhar o painel L2/bolhas do dock.
- Qualquer mudança no engine, na projeção ou no modelo de captura que não seja
  consequência direta do gate da faixa.
