---
name: merge-por-outro-agente
description: Nunca mergear PRs do quantick — outro agente/Camilo faz o merge; aguardar aviso antes da limpeza pós-merge
metadata: 
  node_type: memory
  type: feedback
  originSessionId: a89e449b-e792-424d-9e27-659ca9a4c457
  modified: 2026-08-17T15:00:05.718Z
---

Em 2026-08-14 (PR #178), mesmo com "pode mergear" na conversa, Camilo
interrompeu o `gh pr merge` e disse: "espera outro agente mergear, eu te
aviso quando mergear".

**Why:** o merge é coordenado fora da sessão (outro agente/fluxo próprio);
o papel desta sessão termina em PR aberto com CI verde — como o CLAUDE.md
já diz, merge nunca faz parte da missão, nem com aprovação aparente.

Em 2026-08-15 (PR #190), com "faz pr e mergeia" explícito e CI verde, o
`gh pr merge` foi **negado pelo classificador de permissão** do harness —
não pelo Camilo. Ou seja: mesmo autorizado na conversa, o comando não passa;
oferecer `! gh pr merge <n> --merge` para ele rodar no prompt.

**How to apply:** por padrão não rodar `gh pr merge` — entregar o PR verde
e parar; outro agente costuma mergear. Ao receber um "pode mergear",
PRIMEIRO checar `gh pr view <n> --json state,mergedBy`: no PR #178 o
comando veio com o PR já mergeado pelo outro agente (era só sinal de
limpeza); no PR #181 (2026-08-14) veio com o PR ainda OPEN e CI verde —
autorização direta, mergeei (`gh pr merge --merge`, estilo merge-commit do
repo) sem objeção. Idem no PR #191 (2026-08-16): OPEN + CI verde +
"okay pode mergear" → merge direto passou sem bloqueio do classificador.
Idem no PR #194 (2026-08-17): "pode fazer o merge" com PR OPEN → aguardei
o CI do último commit fechar verde (nunca mergear com check pending) e
mergeei direto. A limpeza pós-merge (git pull na main, worktree remove
+ branch -d local e remoto) vem depois da confirmação do merge; um
`ui-state`/preset toml gerado por demos minhas pode ser descartado com
`--force` na remoção do worktree. Se o app estiver rodando do exe do
worktree (demo ao vivo), a remoção do worktree espera o app fechar — o
Windows trava o delete do binário em uso.

Em 2026-08-19 (PR #206): "ficou bom pode fazer o merge" com o PR OPEN. O
merge passou direto, sem bloqueio do classificador. Um detalhe novo: entre a
abertura do PR e o "pode mergear" a `main` andou quatro PRs, e o PR ficou
`CONFLICTING/DIRTY` — checar `gh pr view <n> --json mergeable,mergeStateStatus`
antes de mergear, rebasear em `origin/main`, rodar os seis checks de novo e
esperar o CI do commit rebaseado. Um `mergeable=MERGEABLE` com
`mergeStateStatus=UNSTABLE` significa apenas que o CI ainda está correndo.
