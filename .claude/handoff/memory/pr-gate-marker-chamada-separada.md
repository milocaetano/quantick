---
name: pr-gate-marker-chamada-separada
description: O hook pr-gate avalia antes do comando rodar — gravar o marker arch-review-ok e chamar gh pr create no mesmo Bash é negado; são duas chamadas
metadata: 
  node_type: memory
  type: feedback
  originSessionId: cd0a7260-1eb8-4d03-a71d-16178fa2b47c
  modified: 2026-08-22T13:30:24.928Z
---

`git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/arch-review-ok" && gh pr create ...` numa única chamada Bash é negado pelo hook `pr-gate` ("arch-review has not been recorded for this branch"), mesmo com o marker sendo escrito no início do comando.

**Why:** o hook é um prompt hook que inspeciona o estado do repositório *antes* de o comando executar; nesse instante o marker ainda não existe (ou aponta para o HEAD antigo após um novo commit).

**How to apply:** duas chamadas Bash separadas — primeiro gravar o marker (e conferir com `cat`), depois `gh pr create`. Após cada commit novo na branch o marker precisa ser regravado antes de abrir o PR. Relacionado: [[merge-por-outro-agente]], [[pilha-de-prs-rebase-onto]].
