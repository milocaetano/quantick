---
name: locked-session-screenshots
description: Sessão Windows bloqueada = capturas impossíveis (branco/lock screen); usar frame_cpu_ms e render-proof off-screen
metadata: 
  node_type: memory
  type: project
  originSessionId: 0ba72494-a104-4b3e-ba7f-68969d340e79
  modified: 2026-08-14T13:37:42.394Z
---

Com a sessão do Windows bloqueada (madrugada), nenhuma janela apresenta:
`PrintWindow` (mesmo com PW_RENDERFULLCONTENT) captura branco puro,
`CopyFromScreen` fotografa o backdrop azul da lock screen, e
`SetForegroundWindow` falha pelo foreground-lock (processo em background não
pode tomar foco). `SetWindowPos` topmost também não faz o GL apresentar.
O `APP_HEALTH_SUMMARY` fica preso em `fps=19 / frame_avg≈51` — é estado
ambiental, idêntico para qualquer build.

**Why:** em 2026-08-14 a missão do paper trading gastou várias tentativas de
captura até diagnosticar que era a lock screen, não o app.

**How to apply:** para medir performance nesse estado, comparar
`frame_cpu_ms` (imune ao throttle do compositor) entre branch e controle da
main sob env idêntico. Para evidência visual, usar render-proof off-screen no
padrão do repo (`egui::Context::default().run(...)` + inspecionar
`output.shapes` — ver `the_cmd_preview_paints_line_label_and_price_chip` em
paper_trading.rs), e declarar as capturas como BLOCKED-ambiente no PR, a
refazer na primeira sessão desbloqueada. Ver também [[no-agent-app-launches]].
