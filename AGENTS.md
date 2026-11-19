# AGENTS.md — quantick

Before implementation or review, read [CLAUDE.md](CLAUDE.md): shared rules
for Claude and Codex. [Checks](CONTRIBUTING.md); [rationale](docs/agentic-development.md);
[index](docs/README.md). Codex: read [host mappings](.agents/references/codex-compatibility.md)
and `.agents/skills/`.

**App access:** build `quantick-mcp`, run `quantick-mcp setup --client codex`
(or `claude`), enable **Tools → Local agent access**, call `quantick_describe` first.
[Authority](crates/mcp/README.md); [inventory](docs/control-plane/capability-inventory.md);
[catalog](schemas/control/observer-capability-catalog-v1.json).

## The map

`crates/` workspace. Edges: `crates/guards/src/graph.rs`.
`app`, `backtest`, `mcp` and `guards` are leaves.

| Crate | Depends on | What it owns |
| --- | --- | --- |
| `app` | every crate but the other leaves, `trading` and `sources` | Desktop chart. |
| `backtest` | strategy, pine, indicators, replay, sim, paper (dev), engine | Recorded sessions to performance. |
| `mcp` | control-local, control | The MCP adapter; stdout carries MCP frames only. |
| `guards` | — | Ratchets and scans. |
| `control-handlers` | alerts, chart*, civil, control, control-host, control-schema, drawings, engine, orderflow, paper, replay, session, sim, sources, stores, workspace | Port handlers. |
| `control-schema` | control, control-host, stores, pine, layers, operability, trading, orderflow, indicators, orderbook, engine | Control-plane schemas over the domain crates. |
| `stores` | alerts, appearance, chart, drawings, sources, workspace, orderflow, indicators, engine, orderbook (dev) | Cockpit documents; the alert observer cache. |
| `ui-model` | alerts, appearance, chart*, civil, control, control-handlers, control-schema, drawings, engine, indicators, layers, orderflow, paper, replay, session, sources, stores, strategy, workspace, sim (dev), trading (dev) | UI contracts and catalog. |
| `session` | chart*, civil, engine, paper, sim, sources, strategy, workspace | Candles, paging, draft choices. |
| `drawings` | alerts, anchored-studies, appearance, chart*, engine, indicators, workspace | Chart marks: store, update, scenes. |
| `alerts` | civil, workspace | Alerts and sounds. |
| `appearance` | — | Colors and typography. |
| `chart` | appearance, chart-interaction, civil, layers, orderflow, indicator-session, indicators, engine, workspace, orderbook (dev) | Chart model. |
| `indicator-session` | pine, indicators, engine | Bindings, batches, deltas. |
| `anchored-studies` | indicators, engine | Resumable profile and anchored-average state. |
| `pine` | indicators | "Quantick Pine", a Pine v5 subset; zero external dependencies. |
| `strategy` | alerts, sim, workspace, engine | Regions, brackets, `SignalAlarm`, presets, drawing anchors. |
| `control-local` | control | Instance directory and loopback client. |
| `control-host` | control, engine | Projections, admission, idempotency, event journal; told the time. |
| `indicators` | engine | The `Indicator` trait, `ta.*` kernels, draw objects. |
| `replay` | civil, engine | Recorded sessions, playback clock; told the time. Test support: `replay::test_support`. |
| `paper` | sim, trading, replay, workspace, civil, engine | Paper account, risk, journal and report. |
| `sim` | trading, engine | `TradingVenue` impl; fills only on tape, never quotes. |
| `trading` | engine | Venue-neutral order vocabulary and the `TradingVenue` port. |
| `broker-mt5` | trading, engine | MT5 broker `TradingVenue`; owns one socket. |
| `feed` | feed-*, sources, replay, operability, orderbook, engine | The `FeedEvent`/`FeedCommand` port and adapters; owns runtimes, threads, clock. |
| `feed-binance`, `feed-hyperliquid`, `feed-mt5` | engine, orderbook; binance also sources | Produce trades. |
| `sources` | engine | Feed config vocabulary and history reach. |
| `orderflow` | appearance, engine, orderbook | Liquidity history, grouping, timeline and heatmaps. Told the time. |
| `engine` | — | Trades in, bars out. Deterministic, no clock. |
| `orderbook` | — | L2 book core. |
| `control` | — | Control-plane contracts and `fake` ports. |
| `civil` | — | Civil dates and the display offset. |
| `workspace` | — | Layout documents and pane membership. |
| `chart-interaction` | layers, orderflow | Pane state, menus, quick ranges, anchors. |
| `layers` | — | Layer visibility and persistence. |
| `backpressure` | — | Owner-to-worker admission (never drop); told the time. |
| `operability` | — | UI behaviours, capabilities and exclusions. |
| `platform` | — | OS window calls for `app`. |
