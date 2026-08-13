# Mission — Bolhas de agressão por região (WIN$N)

Agrupar as bolhas de agressão por região de preço/tempo (em vez de uma bolha
por tick), escalando o tamanho conforme o volume agregado, com design
configurado especificamente para o mini-índice (WIN$N), validado ao vivo no
feed MT5 e revisado por personas de traders de fluxo (estilo Bookmap).

Origem: hoje as bolhas acumulam tick a tick e viram uma pilha ilegível; o
trader quer ler agressão por região, não por print. Mudanças estruturais são
permitidas se necessárias.

Branch: `feat/aggression-bubble-clusters` · worktree
`../quantick-worktrees/feat-aggression-bubble-clusters`

## Acceptance criteria

1. [ ] Bolhas agregadas por região (cluster preço×tempo), tamanho
   proporcional ao volume agregado — fim do acúmulo tick a tick.
2. [ ] Perfil de design específico para WIN$N via config, sem alterar o
   default de outros símbolos.
3. [ ] `trader-ux-review` com personas de fluxo/Bookmap, nenhum Blocker não
   resolvido.
4. [ ] Teste ao vivo WIN$N (MT5) com screenshot via `ui-harness`;
   `visual-qa` com todas as superfícies PASS.
5. [ ] Performance: fps/frame_avg (APP_HEALTH_SUMMARY) sob tape denso vs.
   controle em `main` — números no corpo do PR; caminhos classificados por
   taxa.
6. [ ] Quatro checks verdes após rebase em `main`; `arch-review` com todo
   Blocker/Should-fix resolvido ou deferido no corpo do PR.
7. [ ] PR aberto (merge fora do escopo).
