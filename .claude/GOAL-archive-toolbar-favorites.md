# GOAL — favoritos com estrelinha no trilho + ícones de canal e Fibo

**Missão**: favoritar ferramentas de desenho direto do flyout — uma estrelinha
minúscula sobre o ícone de cada ferramenta; clicar nela cria um botão em
destaque na parte de baixo do trilho principal, armável com um clique, sem ter
que reabrir o grupo. Desfavoritar acontece **só** pela estrelinha no ícone
original. Junto: o ícone do canal paralelo deixa de ser um losango e passa a
ler como paralelas, o ícone do Fibonacci aproxima-se do padrão TradingView, e
o trabalho perdido deste worktree (dock direito removido + settings sempre
popup) é commitado e reaproveitado.

Branch: `feat/toolbar-usability` · worktree `../quantick-worktrees/feat-toolbar-usability`

Contexto de resgate: a branch já foi merged em `main` (Shift horizontal na
linha de tendência **já está lá** — `line_core.rs::levelled_far_end`,
`Constrain::Level`), mas o worktree ficou com ~214 linhas não commitadas
(remoção do `ToolboxDock::Right` + inspector sempre flutuante, com testes).
Esse diff virou o commit "feat: the rail gives up the right edge and settings
always float", rebaseado em `main` atualizado.

## Critérios de aceite específicos

1. **Resgate**: o diff pendente (app.rs, toolrail.rs, ui_state.rs, widgets.rs)
   commitado e rebaseado em `origin/main`, testes verdes. ✔ (commit feito,
   rebase limpo — falta rodar os quatro checks)
2. **Favoritar pela estrelinha**: cada linha do flyout da família mostra uma
   estrela minúscula; clicá-la marca a ferramenta como favorita sem armar a
   ferramenta nem fechar o flyout. Estrela preenchida = favorito.
3. **Seção de favoritos no trilho**: favoritos aparecem numa seção própria em
   destaque na parte de baixo do trilho principal, um clique arma a
   ferramenta. Sem favoritos, a seção não existe.
4. **Desfavoritar só na origem**: o botão de favorito no trilho NÃO
   desfavorita; apenas a estrelinha na linha original do flyout remove.
5. **Persistência**: favoritos sobrevivem a fechar/reabrir via
   `ui-state.toml`. Testes: marcar, desmarcar, ordem estável, round-trip.
6. **Ícone do canal paralelo**: lê como canal (paralelas inclinadas), não
   losango (`PARALLELOGRAM` atual, `parallel_channel.rs:397`).
7. **Ícone do Fibonacci**: padrão TradingView (níveis horizontais entre
   âncoras diagonais), não `ROWS` (`fib.rs:27`).
8. **Shift horizontal**: evidência de teste verde já existente em `main`.

## Portões padrão injetados

- [ ] Quatro checks verdes após rebase no `main` atualizado.
- [ ] Impacto de performance declarado: trilho, flyout e ícones custom são
      **per-frame** — ícones pintados não podem tesselar caro a cada frame
      sem medida; nada per-trade, nada per-depth.
- [ ] `arch-review` sobre `git diff main...HEAD`, Blocker/Should-fix
      resolvidos ou deferidos no corpo do PR.
- [ ] `ui-harness`: superfícies novas/alteradas alcançáveis por env hook,
      hook adicionado nesta mudança (flyout com estrelas, trilho com seção
      de favoritos).
- [ ] `visual-qa` com todas as superfícies PASS ou defeito aceito por escrito.
- [ ] `trader-ux-review` sem Blocker em aberto.
- [ ] Favoritos seguem `new-extension`: porta nomeada, edições de registro
      apenas, defaults preservam o comportamento de hoje, raio de impacto no
      corpo do PR.
- [ ] PR aberto com CI verde e evidências no corpo. Merge não faz parte.
