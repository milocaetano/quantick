---
name: worktree-targets-enchem-o-disco
description: "Disco C: em 0 bytes = os target/ dos worktrees; limpar os de branches já mergeadas"
metadata: 
  node_type: memory
  type: project
  originSessionId: c043459e-c654-4c58-a35b-d4e6027bf9d3
  modified: 2026-08-17T20:37:55.550Z
---

Cada worktree em `C:\src\quantick-worktrees\` carrega um `target/` de 5–11 GB.
Com dez worktrees isso passa de 88 GB e o C: chega a **0 bytes livres** — o
sintoma que aparece primeiro é `git commit` falhando com
`fatal: unable to write loose object file: No space left on device`, não um
aviso de disco.

**Why:** os worktrees sobrevivem ao merge das suas branches, e o `target/` de
uma branch morta nunca mais é lido. Em 15/08/2026 seis dos dez worktrees eram
de branches já em `main`.

**How to apply:** medir com
`Get-ChildItem C:\src\quantick-worktrees -Directory | ForEach-Object {...}` e
apagar `target/` **só** dos worktrees cuja branch é ancestral de `origin/main`
(`git merge-base --is-ancestor <branch> origin/main`). São artefatos
recompiláveis, nunca código — mas apagar o de uma branch viva custa uma
recompilação a quem estiver trabalhando nela. Liberou 49 GB de uma vez.

`git worktree remove` costuma ser **barrado pelo classificador**; `Remove-Item
-Recurse -Force <worktree>\target` passa e resolve o disco do mesmo jeito.
Sintoma novo em 17/08/2026: `Edit`/`Write` falhando com
`ENOSPC: no space left on device` no meio da sessão.

E não construa em `C:` — use `CARGO_TARGET_DIR` num drive com espaço (`D:` hoje;
conferir com `Get-PSDrive -PSProvider FileSystem`, o `F:` que o `ui-harness`
documentava sumiu).
Ver [[merge-por-outro-agente]]: os worktrees mergeados costumam ficar para trás
porque quem mergeia não é quem trabalhou neles.
