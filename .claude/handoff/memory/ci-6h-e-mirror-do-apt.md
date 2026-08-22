---
name: ci-6h-e-mirror-do-apt
description: "CI falhando com ~6h00m de duração é o apt travado, não o código — re-run resolve"
metadata: 
  node_type: memory
  type: project
  originSessionId: 989eb39d-c77a-45d3-9753-5b10be138818
  modified: 2026-08-19T11:22:27.500Z
---

Um job de CI do quantick que falha com duração de ~`6h0m` não falhou: bateu no
teto de execução do GitHub Actions. Em 2026-08-19 (PR #205) a etapa `Install
desktop GUI dependencies` recebeu `Ign` de todos os repositórios do
`azure.archive.ubuntu.com`, começou o fallback para `archive.ubuntu.com` e
parou de emitir linhas por seis horas. `Format`, `Lint`, `Build` e `Test`
nunca iniciaram.

**Why:** a anotação que aparece é "The operation was canceled", que se lê como
falha de teste. Investigar o diff antes de olhar a duração custa tempo à toa.

**How to apply:** ao ver `ci fail` com duração perto de 6h, rodar
`gh run view --job=<id>` primeiro — se a etapa que morreu é anterior ao Rust,
é infra: `gh run rerun <run-id> --failed` e pronto (o re-run do #205 passou em
4m17s). O job não tem `timeout-minutes`, então cada travada dessas queima seis
horas de minutos de Actions e segura a branch; pôr um teto no job continua
pendente. Relacionado: [[cargo-test-concorrente-quebra-mt5]].
