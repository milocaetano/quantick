---
name: pilha-de-prs-rebase-onto
description: "Numa pilha de branches (PR2←PR3←PR4), depois de reescrever a base, reempilhar o filho com `git rebase --onto <nova-base> <head-antiga-da-base>`; um `git rebase <base>` simples replica os commits antigos da base e conflita"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: cd0a7260-1eb8-4d03-a71d-16178fa2b47c
  modified: 2026-08-22T02:05:15.294Z
---

Quando um branch-base de uma pilha é rebaseado (ou recebe fixups), o branch filho ainda contém os commits antigos da base; `git rebase feat/base` tenta reaplicar esses commits antigos sobre a nova base e, se os patch-ids mudaram (contexto alterado), conflita em arquivos que o filho nem tocou. O que funciona: `git rebase --onto feat/base <sha-da-head-antiga-da-base> feat/filho`, que replica só os commits próprios do filho.

**Why:** o git detecta "já aplicado" por patch-id; basta uma linha de contexto diferente para o commit antigo deixar de casar. Aconteceu com `feat/mcp-observer` sobre `feat/control-gateway` em 2026-08-21.

**How to apply:** antes de rebasear a base, anotar `git rev-parse feat/base` (head antiga); depois reempilhar cada filho com `--onto`. Para pilhas de PRs no GitHub, abrir cada PR com `--base <branch anterior>` e merjar em ordem; o GitHub reaponta a base quando a anterior entra. Merge nunca é meu ([[merge-por-outro-agente]]).
