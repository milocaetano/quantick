# GOAL — o trilho nunca esquece um favorito

**Missão**: favoritar uma ferramenta no trilho grava a escolha no workspace na
hora, e nada além dela pode apagá-la — qualquer sessão futura abre com os
favoritos que o trader marcou.

Branch: `feat/favorites-remembered` · worktree
`../quantick-worktrees/feat-favorites-remembered`

## Diagnóstico

- `favorite_tools` vive dentro de `SavedChrome` (`ui_state.rs:226`) — dentro do
  *arranjo*, não como escolha permanente do trader.
- Só chega ao disco quando o workspace inteiro é salvo: saída com autosave
  ligado, Save explícito, autostart. Fechamento sujo ou "Save on exit"
  desligado = favorito perdido.
- Abrir um workspace nomeado chama `set_favorites(&chrome.favorite_tools)`
  (`app.rs:4043`): um bookmark antigo **apaga** os favoritos atuais.

O padrão certo já existe: `write_replay_folder` (`app.rs:3909`) — lê o arquivo,
troca um campo, escreve. "Standing choice, not a description of the screen, so
it must not wait for a clean exit and must not drag the current arrangement
into the file with it." Favorito é exatamente isso.

## Decisões do trader (perguntadas e respondidas)

1. Abrir um bookmark **não** mexe nos favoritos — eles são preferência, não
   arranjo.
2. "Save on exit" desligado **não** impede a gravação de um favorito. Aquela
   chave governa o arranjo, não as preferências.

## Critérios de aceite específicos

1. **Gravação imediata**: marcar ou desmarcar uma estrela escreve o campo de
   favoritos no arquivo de workspace na mesma interação — leitura-troca-escrita
   como `write_replay_folder`, sem arrastar tabs, layout ou dock junto.
2. **Independente do autosave**: com `save_on_exit` desligado, o favorito ainda
   é gravado; o arranjo continua não sendo.
3. **Campo de nível de arquivo**: favoritos passam a viver em `Workspace`, ao
   lado de `save_on_exit` e `replay_folder`, não dentro de `SavedChrome`.
4. **Migração sem perda**: um `ui-state.toml` escrito antes desta mudança abre
   com os favoritos que tinha — a leitura cai para o campo legado do chrome
   quando o campo novo não existe.
5. **Bookmark não apaga**: abrir um workspace nomeado deixa a seção de
   favoritos exatamente como estava.
6. **Reset preserva**: "Reset startup layout" descarta o arranjo e mantém os
   favoritos, como já faz com os bookmarks.
7. **Testes** cobrindo cada um: round-trip do campo novo, toggle grava, toggle
   grava com autosave off, arquivo legado migra, bookmark não sobrescreve,
   reset preserva.

## Portões padrão injetados

- [ ] Quatro checks verdes após rebase no `main` atualizado
      (`fmt --check`, `clippy -D warnings`, `build`, `test`).
- [ ] **Impacto de performance declarado**: a gravação é *rare* — um I/O de
      arquivo por clique de estrela, nunca por frame. A leitura de
      `favorites()` no trilho permanece per-frame e inalterada. Nada per-trade,
      nada per-depth. Sem bench: nenhum caminho quente é tocado.
- [ ] `arch-review` sobre `git diff main...HEAD`, Blocker/Should-fix resolvidos
      ou deferidos no corpo do PR.
- [ ] **Superfície visual**: nenhuma superfície nova e nenhum pixel alterado —
      a estrela e a seção de favoritos já existem e não mudam de forma. O que
      muda é quando o estado chega ao disco, provado por teste. `visual-qa` e
      `trader-ux-review` ficam fora do escopo por isso; se o Camilo autorizar
      abrir o app, rodo uma passada de regressão do trilho.
- [ ] PR aberto com CI verde e evidências no corpo. Merge não faz parte.
