---
name: desktop-ocupado-bloqueia-captura
description: "fps=19 tem três causas — lock screen, Camilo trabalhando, ou desktop dormindo por ociosidade longa (volta sozinho)"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 26a7a39d-026b-40a9-bd71-ea5b07cfce9d
  modified: 2026-08-18T22:19:52.083Z
---

Captura em branco (`colors=12`) com `fps=19` / `frame_avg≈51ms` / `frame_cpu≈2ms`
tem **três** causas distintas, e o diagnóstico as separa:

- `LogonUI` presente → sessão bloqueada (ver [[locked-session-screenshots]]).
- `LogonUI` ausente + `GetLastInputInfo` idle ≈ 1s → **Camilo está usando o
  desktop** e a janela do agente ficou atrás.
- `LogonUI` ausente + idle **alto** (20 min ou mais) → o desktop parou de
  compor sozinho (monitor dormindo). Não é lock screen, não é o Camilo, e não
  é o app: `frame_cpu` continua normal e o `fps` fica preso em 19 **mesmo com
  a janela na posição visível**, então mover para dentro da tela não resolve.

**Why:** no segundo caso a skill `ui-harness` proíbe disputar foco, e insistir
com relançamentos só faz janelas piscarem na frente de quem está trabalhando.
No terceiro, insistir também não resolve, e **não dá para contar com o retorno
sozinho**: numa sessão de 18/08 voltou em minutos, mas em 20/08 ficou preso por
**mais de duas horas** — duas capturas brancas, dois vigias de 55 min armados e
expirados, e a composição só voltaria quando o Camilo voltasse à máquina. O
gatilho é a presença dele, não o tempo.

**How to apply:** medir os três sinais antes de culpar o render; se for desktop
ocupado, parar de capturar, guardar a evidência que já existe e perguntar ao
Camilo. Se for ociosidade longa, seguir com o trabalho que não depende de
pixels (testes, medição por `frame_cpu_ms`, revisão), **avisar o Camilo que a
captura está bloqueada e por quê**, e deixar o portão visual explicitamente em
aberto em vez de armar vigia atrás de vigia. Um vigia vale uma vez; a partir do
segundo, o custo é maior que a chance.

Também vale checar instâncias alheias antes de medir performance: outro agente
pode estar com o app aberto (`Get-Process quantick-app | Select Path`), e aí
qualquer frame time medido é ruído. Ver [[no-agent-app-launches]]. E matar
**só** as do próprio worktree: filtrar por `Path`, nunca por nome de processo.
