---
name: goal-arquivado-nao-e-entregue
description: Um GOAL-archive commitado não prova entrega — conferir se o PR existe antes de dar a branch por fechada
metadata: 
  node_type: memory
  type: feedback
  originSessionId: f5e5d785-ac42-46c3-bfaf-e22e63c89918
  modified: 2026-08-15T23:44:50.686Z
---

Uma branch pode ter os quatro checks verdes, o arch-review gravado e o
`.claude/GOAL-archive-*.md` commitado, e ainda assim **nunca ter sido
entregue**. Aconteceu com `feat/force-bar`: trabalho completo, pushado, goal
arquivado como cumprido — e `gh pr list --head <branch>` não retornava nada. O
Camilo só descobriu ao não achar o indicador no app rodando da `main`.

**Why:** o arquivamento do goal é escrito pelo agente que fez o trabalho, antes
do último passo. Ele registra intenção, não resultado. O único fato que prova
entrega é o PR mergeado — e o merge é sempre de outro agente
([[merge-por-outro-agente]]), então a branch fica parada sem ninguém perceber.

**How to apply:** quando algo "deveria existir" e não está no app, checar nesta
ordem antes de suspeitar de bug ou build velho:

```sh
git branch -a --sort=-committerdate     # a branch existe?
gh pr list --state all --head <branch>  # virou PR?
git log --oneline origin/main..origin/<branch>
```

Se a branch existe e o PR não, o diagnóstico acabou: faltou entregar. Rebase em
`origin/main`, quatro checks, arch-review sobre o diff **pós-rebase** (a marca
antiga fica com o SHA errado), PR.

Vale varrer as branches não mergeadas de tempos em tempos — se uma ficou para
trás, outras podem ter ficado. Ver também
[[worktree-targets-enchem-o-disco]]: worktree vivo de branch mergeada é o sinal
oposto — entregue, mas não limpo.
