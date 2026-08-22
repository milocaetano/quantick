---
name: win-nao-tem-tick-de-cotacao
description: WIN$N pela XP entrega 100% flags=1080; COPY_TICKS_TRADE não economiza nada lá
metadata: 
  node_type: memory
  type: reference
  originSessionId: 9892839e-28c2-457f-90a1-25a752119b6f
  modified: 2026-08-21T14:25:01.068Z
---

A gravação real commitada em `crates/feed-mt5/tests/fixtures/win_ticks.ndjson`
(1500 ticks ao vivo do WIN$N, puxados com `COPY_TICKS_ALL`, 2026-07-23) é
**100% `flags=1080`**: todo tick carrega LAST, e não existe um único tick de
cotação. Densidade: 33 ticks/s, gap entre prints p50=20 ms, p99=146 ms,
max=285 ms.

Consequências ao diagnosticar latência do MT5:

- Filtrar por `COPY_TICKS_TRADE` é neutro nesse símbolo — não corta tráfego.
  Vale por princípio e por outros brokers, não como cura de atraso.
- 33 ticks/s não constrói backlog de socket. Uma syscall por tick não é o
  gargalo nessa carga.
- **Um gap de mais de 1 s entre prints não é o WIN em pregão ativo.** Se
  aparecer, ou o mercado parou (almoço) ou algo segurou a entrega — medir,
  não supor.

Contar os flags da fixture antes de acusar a ponte custa segundos e evitou uma
causa raiz inventada. Ver [[gap-da-fita-sao-dois-relogios]].
