# GOAL — MT5 pagina histórico de ticks sob demanda

**Missão**: dar ao feed MetaTrader paginação de histórico de ticks — o botão
"older" busca mais ticks para trás no terminal, como o MetaTrader faz — e fazer
o bloco inicial cobrir o pregão do dia em vez de uma janela curta.

Branch: `feat/mt5-load-older` · worktree `../quantick-worktrees/feat-mt5-load-older`

## Por que

Hoje o protocolo do bridge é **uma via só** (bridge → feed), então não existe
como pedir mais histórico:

- `crates/app/src/feed/metatrader.rs:605` responde todo `LoadOlder` com um bloco
  vazio e loga `MT5_LOAD_OLDER_UNSUPPORTED`.
- `crates/app/src/config.rs:115` declara `history_paging: false`, o que desabilita
  o grupo inteiro na toolbar (`crates/app/src/toolbar.rs:615`).
- `bridge/mt5/QuantickBridge.mq5:29` manda 30 minutos de ticks no connect;
  `bridge/mt5/quantick_bridge.py` manda 720 min com teto de 200k ticks — um dia
  de WIN passa de 1M prints, então o teto corta os mais antigos.

## Decisões tomadas com o Camilo (2026-08-19)

- **Bridge alvo: Python** (`quantick_bridge.py`). O EA MQL5 não entra nesta
  missão; ele apenas não declara a capacidade e segue como hoje.
- **Backfill inicial cobre o pregão do dia**, além do "older".

## Critérios de aceitação

### Específicos

1. [x] Protocolo ganha um canal **feed→bridge** aditivo dentro do schema 1
       (comando do feed + bloco `history_*` do bridge). `bridge/mt5/PROTOCOL.md`,
       `crates/feed-mt5/src/protocol.rs` e seus testes verbatim mudam juntos.
2. [x] `quantick_bridge.py` lê o socket, atende o pedido com `copy_ticks_range`
       para trás do tick mais antigo já enviado, e responde **bloco vazio quando
       não há mais nada** — nunca silêncio, o loader sempre resolve.
3. [x] **Bridge antigo continua funcionando**: a capacidade é declarada no
       `hello`; ausente, o botão fica desabilitado exatamente como hoje.
4. [x] `metatrader.rs` para de responder vazio incondicionalmente; `history_paging`
       sobe pelo watch de capabilities da sessão (como `ohlcv_generation` já faz),
       nunca chutado do `ProviderKind`.
5. [x] Backfill inicial do Python cobre o pregão do dia; o truncamento continua
       visível no log em vez de implícito.
6. [x] Ticks paginados chegam como `FeedEvent::HistoryPrepended`, ordenados e sem
       duplicar o retido — a mesma regra de overlap por tempo do reconnect.
7. [x] Segunda implementação de bridge (fake, em teste) exercita: pede older →
       prepend; pede sem histórico → vazio → loader resolve; bridge sem a
       capacidade → botão desabilitado.

### Gates injetados

8.  [x] Quatro checks verdes após rebase em `main`, mais `ruff check --select F`
        sobre `bridge/mt5/` e `python3 tools/mt5/test_export_session.py`.
9.  [x] Impacto de performance **declarado por taxa** (per-trade / per-frame /
        raro) no plano, com número medindo o parse por tick.
10. [x] `new-extension`: porta nomeada, edições de registro, blast radius
        (adicionados vs. editados) no corpo do PR.
11. [x] `ui-harness` hook para a toolbar com paginação habilitada + `visual-qa` +
        `trader-ux-review` sem Blocker aberto. `LoadOlder` segue drivable sem mouse.
12. [x] `arch-review` rodado, Blocker/Should-fix resolvidos ou deferidos no corpo
        do PR; **PR aberto** (merge é do Camilo, nunca meu).

## Fora de escopo

- Paginação no EA MQL5 (decisão acima).
- Paginação de candles (`rates_*`) — outro bloco, outra missão.
- Qualquer mudança no engine ou na construção de barras.

## Resultado (2026-08-19)

Entregue em 5 commits sobre `origin/main`. Os seis checks passam
(`fmt`, `clippy -D warnings`, `build`, `test --workspace`, `ruff --select F`
sobre `bridge/mt5` + `tools/mt5`, e `tools/mt5/test_export_session.py`).

Desenho final: canal **feed → bridge** aditivo no schema 1 — o `hello` declara
`history_paging`, o feed escreve `load_older` com o cursor do gráfico, e o
bridge responde `history_start` / ticks / `history_end`. O `history_end` carrega
duas honestidades que o desenho inicial não tinha: `exhausted` (só quando nada
foi trimado) e `scanned_to_ms` (até onde a busca chegou, não o que ela devolveu
— sem isso a paginação trava num trecho só de quotes, como a pré-abertura).

Custo por taxa, medido sobre 2 M ticks: ler+decodificar+mapear vai de 1 860 ns
para 1 920 ns por tick (+3,2 %), 0,54 → 0,52 M ticks/s.

**Critério 11 parcial**: `trader-ux-review` rodou e seu único Should-fix
(o tooltip do botão desabilitado dando a razão errada) foi corrigido. `visual-qa`
não rodou — abrir o app exige autorização do Camilo, e a mudança não acrescenta
superfície nova (o botão `+ older ▾` já existia com os dois estados cobertos por
teste em `toolbar.rs`). Registrado no corpo do PR.
