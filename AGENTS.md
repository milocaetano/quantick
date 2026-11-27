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

`crates/` workspace. Ranks: `RANK` in `crates/guards/src/graph.rs`; edges point lower.

| Crate | What it owns |
| --- | --- |
| `app` | Desktop chart. |
| `backtest` | Recorded sessions to performance. |
| `mcp` | The MCP adapter; stdout carries MCP frames only. |
| `guards` | Ratchets and scans. |
| `control-handlers` | Port handlers. |
| `control-schema` | Wire schemas over the domain. |
| `stores` | Cockpit documents; the alert observer cache. |
| `ui-model` | UI contracts and catalog. |
| `session` | Candles, paging, draft choices. |
| `drawings` | Chart marks: store, update, scenes. |
| `alerts` | Alerts and sounds. |
| `appearance` | Colors and typography. |
| `chart` | Chart model. |
| `indicator-session` | Bindings, batches, deltas. |
| `anchored-studies` | Resumable profile and anchored-average state. |
| `pine` | "Quantick Pine", a Pine v5 subset; zero external dependencies. |
| `strategy` | Regions, brackets, `SignalAlarm`, presets, drawing anchors. |
| `control-local` | Instance directory and loopback client. |
| `control-host` | Projections, admission, idempotency, event journal; told the time. |
| `indicators` | The `Indicator` trait, `ta.*` kernels, draw objects. |
| `replay` | Recorded sessions, playback clock; told the time. Test support: `replay::test_support`. |
| `paper` | Paper account, risk, journal and report. |
| `sim` | `TradingVenue` impl; fills only on tape, never quotes. |
| `trading` | Orders, `TradingVenue` port, rates. |
| `broker-mt5` | MT5 broker `TradingVenue`; owns one socket. |
| `feed` | `FeedEvent`/`FeedCommand`, adapters; owns runtimes, threads, clock |
| `feed-binance`, `feed-hyperliquid`, `feed-mt5` | Produce trades. |
| `sources` | Feed config vocabulary and history reach. |
| `orderflow` | Liquidity history, grouping, timeline, heatmaps; told the time. |
| `engine` | Trades in, bars out. Deterministic, no clock. |
| `orderbook` | L2 book core. |
| `control` | Control-plane contracts and `fake` ports. |
| `civil` | Civil dates and the display offset. |
| `workspace` | Layout documents and pane membership. |
| `chart-interaction` | Pane state, menus, quick ranges, anchors. |
| `layers` | Layer visibility and persistence. |
| `backpressure` | Owner-to-worker admission (never drop); told the time. |
| `operability` | UI behaviours, capabilities and exclusions. |
| `platform` | OS window calls for `app`. |
