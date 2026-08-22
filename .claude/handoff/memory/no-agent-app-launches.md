---
name: no-agent-app-launches
description: Não abrir instâncias do quantick-app durante a sessão — o usuário roda a própria e as do agente atrapalham
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 90a1631c-f28e-41ca-9821-0b17c1f83c47
  modified: 2026-08-14T03:18:41.876Z
---

Não lançar `quantick-app.exe` para validação visual sem pedir antes. Numa
sessão de 2026-08-14 o usuário interrompeu o lançamento dizendo que "carregou
tudo errado" e depois "agora parece que acertou a mão" — ele estava com a
própria instância aberta enquanto o agente abria outras.

**Why:** as instâncias do agente competem pelo mesmo desktop e pela porta 9100
do bridge MT5 (ver [[mt5-port-conflict-diagnosis]]), e uma aba MetaTrader
restaurada por engano num run de controle chega a disputar essa porta com o
terminal real do usuário.

**How to apply:** para `visual-qa` / `ui-harness`, perguntar antes de abrir o
app. Quando abrir for autorizado, fixar `QUANTICK_DEFAULT_FEED=binance` (nunca
deixar restaurar uma aba MT5), apontar `QUANTICK_UI_STATE` para o scratchpad e
encerrar toda instância aberta ao terminar. Sem autorização, reportar a célula
da matriz como BLOCKED em vez de PASS.
