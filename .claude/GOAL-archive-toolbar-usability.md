# Goal — barras de ferramenta: criar arrastando e revisão de usabilidade

Criar um canal (e qualquer ferramenta de mais de dois pontos) arrastando passa
a produzir **o objeto que a ferramenta promete**, com preview ao vivo do que o
próximo clique vai fazer — não uma linha reta parada esperando um terceiro
clique que ninguém sabe que precisa dar. Em cima disso, as quatro barras de
ferramenta do chart passam por revisão de usabilidade com trader-ux-review e
visual-qa, e todo Blocker e Should-fix é corrigido.

Branch: `fix/toolbar-usability` — worktree
`../quantick-worktrees/fix-toolbar-usability`.

## O defeito, nomeado

`ParallelChannel::required_points() == 3` (`drawings/parallel_channel.rs:376`).
O gesto de arrastar em `pane.rs:2254` coloca **uma** âncora no release, então
press+drag+release deixa um rascunho de dois pontos — que é literalmente uma
linha reta. Mesmo buraco em `triangle` (3) e `fib_extension` (3). O chip de
`placement_hint` avisa em texto, mas a forma sob o cursor não é a forma que
está sendo criada, e é a forma que o trader lê.

## Acceptance criteria

1. **Arrastar fixa a linha, mover define a largura.** Press+drag+release em um
   canal fixa a linha de tendência (âncoras 1 e 2); o movimento seguinte
   arrasta a largura; o clique confirma. Nenhuma ferramenta de dois pontos
   muda de comportamento.
2. **Preview ao vivo é a forma final.** Entre a última âncora colocada e o
   clique que falta, o chart pinta a ferramenta inteira sob o cursor — canal
   com os dois trilhos e a midline, triângulo com os três lados, fib extension
   com os níveis projetados — e não o esqueleto de linhas.
3. **A porta é do trait, não um `match` por id.** O preview e o gesto entram
   por método de `DrawingToolImpl` com implementação padrão igual ao
   comportamento de hoje; ferramenta nova ganha o preview sem editar `pane.rs`.
4. **Escape/Backspace continuam honestos** durante o gesto novo: Backspace
   desfaz a última âncora, Escape cancela o rascunho inteiro, e nenhum dos dois
   deixa objeto meio-feito na lista.
5. **Revisão das quatro superfícies**: toolrail esquerda (`toolrail.rs`),
   toolbar superior (`toolbar.rs`), action bar do objeto selecionado
   (`drawings/action_bar.rs`) e context bar (`drawings/context_bar.rs`)
   passam por `trader-ux-review` e `visual-qa`.
6. **Todo Blocker e Should-fix da revisão corrigido**; o que sobra (nice-to-have)
   é listado no corpo do PR, não corrigido em silêncio nem esquecido.
7. **Testes headless** cobrem o gesto: arrastar em ferramenta de 3 pontos
   deixa 2 âncoras e um rascunho vivo; o clique seguinte fecha o objeto com
   largura não-nula; ferramenta de 2 pontos fecha no release como sempre.

## Injected gates

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo build --workspace`
- [ ] `cargo test --workspace`
- [ ] impacto de performance declarado por caminho — per-frame: preview do
      rascunho e paint das barras; nada per-trade, nada per-depth
- [ ] `ui-harness`: hook de env que abre cada superfície revisada, incluindo o
      rascunho de canal em meio de gesto
- [ ] `visual-qa` PASS ou defeitos aceitos explicitamente
- [ ] `trader-ux-review` sem Blocker em aberto
- [ ] `arch-review` com Blocker/Should-fix resolvidos ou deferidos no corpo do PR
- [ ] PR aberto (merge não faz parte do goal)
