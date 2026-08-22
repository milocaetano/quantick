---
name: code-review-concorrente-perde-finders
description: "Duas invocações simultâneas da skill code-review colidem no nome \"code-review\" — os finders da primeira reportam para a segunda e a revisão fica só com a passada própria do agente; serializar as revisões"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: cd0a7260-1eb8-4d03-a71d-16178fa2b47c
  modified: 2026-08-22T02:05:01.266Z
---

Invocar `Skill(code-review)` duas vezes em paralelo (ex.: uma por branch de uma pilha de PRs) faz os finder angles da primeira revisão entregarem seus relatórios ao agente mais novo registrado sob o mesmo nome `code-review`. O agente original para com "Finders are running… I'll wait" e nunca recebe os achados; ao ser reativado por mensagem, entrega apenas a passada própria (em xhigh, nível reaproveitado).

**Why:** o nome do subagente é o endereço das mensagens; o mais recente vence. Na sessão de 2026-08-21 as revisões de `fix/control-contract-hardening` e do PR #213 perderam os dez finders cada uma.

**How to apply:** uma revisão por vez — esperar a notificação final (ou o relatório via SendMessage para "main") antes de lançar a próxima; se um agente parar em "waiting on finders", reativá-lo com `SendMessage(to: "code-review")` pedindo que feche com o que tem e diga quais angles não reportaram. O cabeçalho do arch-review deve registrar isso ("finders perdidos por colisão; passada própria").
