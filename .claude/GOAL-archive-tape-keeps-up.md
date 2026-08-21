# Missão: a fita acompanha a fita

Camilo, operando WIN ao vivo no preset `live lane pie`, reportou que as bolhas
de agressão ficam para trás da borda direita da fita e chegam a sumir dela.
Prioridade declarada por ele: performance e leitura instantânea — quem opera
fluxo não pode ler informação falsa.

## Diagnóstico (concluído)

O gráfico tem dois relógios e a fita usa o mais adiantado como "agora":

- `crates/app/src/orderflow/history.rs:379` — `latest_ms()` = `max(book, print)`
- `crates/app/src/orderflow_engine.rs:1016` — a borda da fita é esse máximo
- `crates/app/src/orderflow/projection.rs:548` — o mapa L2 vai até `latest_book_ms`

Logo: a distância entre a última bolha e a borda **é** `latest_book_ms −
latest_print_ms`. Passando a janela da fita, não sobra bolha nenhuma nela — os
prints vão para o slot da barra (`projection.rs:1605`) e a fita fica muda.

Causa do atraso, na ponte MT5:

- `QuantickBridge.mq5` pedia `COPY_TICKS_ALL` e o app descarta todo tick sem
  LAST (`crates/feed-mt5/src/map.rs:313`) — no WIN, a maioria do tráfego.
- `SendTick` fazia uma syscall `SocketSend` por tick, no thread principal do
  terminal, que é o mesmo que busca ticks novos.
- O book se re-carimba no envio (`QuantickBridge.mq5:224`), então o atraso é
  invisível nele e visível só nas bolhas.

Descartados com leitura de código: âncora de cluster (ponto médio do intervalo,
`interaction.rs:180`), fold do orçamento (`OldestFirst` poupa o novo), atraso do
worker (coalesce latest-wins, no máximo um frame, e o frame é esticado — não
gera gap).

## Critérios

1. Os dois bridges pedem só o que o app usa num símbolo que imprime negócios;
   um símbolo só de cotação segue inalterado. Teste que falha sem a correção.
2. Envio em lote no EA, framing intacto, flush no fim de cada passagem.
3. Teste na projeção provando gap == distância entre relógios, e provando que
   passado o window o print continua no gráfico (slot da barra).
4. A fita declara a idade do print mais novo em vez de esvaziar em silêncio.
5. `tape_newest_print_age_ms` no APP_HEALTH_SUMMARY; `BRIDGE_TAPE_STATS` no EA.
6. Quatro checks verdes + `ruff --select F` + `test_export_session.py`.
7. Números: custo por frame do half vivo sob o preset `live lane pie`.
8. arch-review com todo Blocker/Should-fix resolvido ou deferido; PR aberto.

## Estado

Worktree `../quantick-worktrees/fix-tape-keeps-up`, branch
`fix/tape-keeps-up-with-the-tape`. 1–5 implementados, checks em andamento.
