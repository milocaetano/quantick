# GOAL — ferramentas que param de pedir retrabalho

**Missão**: quatro incômodos das ferramentas de desenho, num só passe — o
ícone do Fibonacci lê como o do TradingView, a retração desenha a partir do
**primeiro** clique (hoje as linhas nascem no segundo), a configuração de uma
ferramenta (níveis, cores, rótulos, estilo) pode ser salva como padrão e
restaurada de fábrica com um clique, e a nota de texto abre já digitável com
sua barra de configuração embaixo.

Branch: `feat/fib-defaults-and-inline-text` ·
worktree `../quantick-worktrees/feat-fib-defaults-and-inline-text`

## O que o levantamento em `main` já provou

- `fib.rs:450 level_price` **já** põe 0 no ponto solto e 1 no ponto inicial —
  a numeração da imagem já é a de hoje. O que trai é o span: `Extend::Forward`
  é o default (`fib.rs:148`) e projeta `(âncora mais à direita → borda)`, ou
  seja, as linhas nascem no segundo clique.
- O preview do draft **já** completa a geometria com o ponto sob o cursor
  (`pane.rs:6119-6124`), então os níveis aparecem durante o arrasto — mas do
  cursor para a direita, não do primeiro clique até ele.
- `PresetHost` já guarda estilo padrão por ferramenta e preset **nomeado**
  padrão (`mod.rs:113-134`, aplicados em `pane.rs:3410`). Falta o gesto de um
  clique: salvar a configuração inteira como padrão sem batizar preset.
- `FibPayload::export_preset` (`fib.rs:378`) já serializa níveis, cores,
  banda, rótulos e `extend` — o template pedido cabe nele.
- A nota de texto hoje só é editável pelo inspector (`text.rs:opens_settings_on_place`).

## Critérios de aceite

1. **Ícone**: Fib retracement e Fib extension no trilho, no flyout e no
   favorito desenham as âncoras como o TradingView. O dado novo é do
   registro (port de ícone), nunca um caso especial no chrome; teste de
   contrato cobre todo ícone declarado.
2. **Retração a partir do primeiro clique**: durante o arrasto e no objeto
   pronto, os níveis correm do primeiro clique até o segundo ponto. Teste
   headless fixa o default por `FibKind`; screenshot com
   `QUANTICK_DRAWING_DRAFT` prova o preview em voo.
3. **Padrão salvável**: um clique grava a configuração completa da ferramenta
   (níveis, cor por nível, rótulos, `extend`, banda, estilo) como padrão de
   novos objetos, e outro restaura o de fábrica. Objetos já na tela nunca são
   repintados por isso. Round-trip provado no `PresetStore`, aplicação provada
   na criação, e um host falso segundo implementador testado.
4. **Texto inline**: colocar a nota abre o campo na âncora com o cursor já
   dentro e a barra de configuração embaixo; o que se digita vai para o
   payload; Esc/clique fora encerra sem perder o texto. Hook de env alcança a
   superfície.
5. **Quatro checks verdes** após rebase em `origin/main` atualizado.
6. **Impacto de performance declarado** por caminho tocado (per-frame: paint
   do Fib e da nota; raro: store de presets e chrome do inspector), com
   `APP_HEALTH_SUMMARY` (fps / frame_avg) da branch contra um controle em
   `main` na mesma janela.
7. **arch-review** rodado sobre `git diff main...HEAD`, todo Blocker e
   Should-fix resolvido — ou deferido explicitamente no corpo do PR.
8. **visual-qa** com todas as superfícies PASS (ou defeito aceito por escrito)
   e **trader-ux-review** sem Blocker em aberto.
9. **PR aberto** com as evidências e o blast radius (arquivos adicionados vs.
   editados) no corpo. Merge não faz parte da missão.
