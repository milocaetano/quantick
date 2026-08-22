---
name: bench-headless-via-backtest
description: "Bench de sim/strategy sem abrir o app: quantick-backtest release + prints_per_second do BACKTEST_DONE; medir main e branch na mesma janela de condições"
metadata: 
  node_type: memory
  type: project
  originSessionId: 59b0055b-8ef0-4268-b26a-9e3d1846ac17
  modified: 2026-08-18T06:56:10.856Z
---

Para o gate de performance de hot path sem abrir o app (que exige
autorização): compilar `quantick-backtest` em release nas duas árvores (main
checkout e worktree) e rodar 3× cada sobre uma sessão real de
[[pasta-de-dados-de-replay]], por exemplo
`--dir C:\Users\Camillo\Quantick\replay --symbol WINV26 --from 2026-08-12
--to 2026-08-12 --strategy force-region --region 171000:171500:sell`.
O número é `"prints_per_second"` no evento JSON `BACKTEST_DONE` do stderr
(a linha final; runs também emitem um evento por sessão antes).

**Why:** na sessão de 2026-08-18 os mesmos binários mediram ~983k e ~1.9M
prints/s em janelas diferentes — o disco a 100% e carga concorrente dobram o
tempo. Comparar main de uma janela com branch de outra é lixo.

**How to apply:** medir control e branch **na mesma janela**, intercalados,
e reportar mediana de 3 runs. Rodar o binário da main direto de
`C:\src\quantick\target\release\` não escreve na árvore (o hook de worktree
não barra). Disco cheio aparece antes como panic do rustc em
`optimize module` — ver [[worktree-targets-enchem-o-disco]].
