---
name: mt5-port-conflict-diagnosis
description: "Como diagnosticar \"connecting to <símbolo> ...\" eterno no feed MT5 — quase sempre é a porta 9100 tomada, não o terminal/bridge"
metadata: 
  node_type: memory
  type: project
  originSessionId: 8978af6f-e877-42d6-a7c0-550fc829ceac
  modified: 2026-08-13T15:50:50.752Z
---

Sintoma: chart preso em "connecting to <símbolo> …" com bolha "the bridge lost its connection — reconnecting" repetindo. Em 2026-08-13 (roll WINQ26→WINV26) a causa foi a porta 9100 tomada por outro holder (aba antiga/instância/processo morto); o terminal MT5, o contrato e o bridge Python estavam saudáveis.

**Why:** A bolha "reconnecting" vem do stderr do bridge (`BRIDGE_DISCONNECTED`) e enterra o card de atenção do `MT5_BIND_FAILED`; o sintoma visível aponta para o lado errado.

**How to apply:** Diagnóstico rápido: `netstat -ano | findstr :9100` + procurar processos quantick/python; rodar o bridge à mão (`python bridge/mt5/quantick_bridge.py --symbol X`) contra um listener de teste separa lado-terminal de lado-app. Desde o PR #173 o feed re-tenta o bind a cada 2 s — liberar a porta basta, o chart volta sozinho. Logs JSON: `QUANTICK_LOG_FORMAT=json`, eventos `MT5_*` tabulados em `crates/feed-mt5/src/lib.rs`.
