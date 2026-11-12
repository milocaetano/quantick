# AGENTS.md — quantick for AI agents

[CLAUDE.md](CLAUDE.md): rules; [CONTRIBUTING.md](CONTRIBUTING.md): checks;
[docs/agentic-development.md](docs/agentic-development.md): rationale;
[docs/README.md](docs/README.md): index.

**Driving the running app?** Build `quantick-mcp`, run `quantick-mcp setup
--client claude` (or `codex`), enable **Tools → Local agent access**, and call
`quantick_describe` first. Tools and authority:
[crates/mcp/README.md](crates/mcp/README.md); capabilities:
[capability inventory](docs/control-plane/capability-inventory.md) and
[capability catalog](schemas/control/observer-capability-catalog-v1.json).

## The map

`crates/` workspace. Edges: `crates/guards/src/graph.rs`.
`app`, `backtest`, `mcp` and `guards` are leaves; `guards` has no edges.

| Crate | Depends on | What it owns |
| --- | --- | --- |
| `app` | every crate but the other leaves, `trading` and `sources` | Desktop chart. |
| `backtest` | strategy, pine, indicators, replay, sim, paper (dev), engine | Recorded-session performance over shared engine and indicators. |
| `mcp` | control-local, control | The MCP adapter; stdout carries MCP frames only. |
| `guards` | — | Size, context, cycle and UI-free ratchets; English and encoding scans. |
| `control-handlers` | control*, chart*, civil, drawings, engine, orderflow, paper, replay, session, sim, sources, stores, workspace | Port handlers. |
| `control-schema` | control, control-host, stores, pine, layers, trading, orderflow, indicators, orderbook, engine | Control-plane schemas over the domain crates. |
| `stores` | appearance, chart, sources, workspace, orderflow, indicators, engine | Cockpit documents: catalogue, symbols, footprint, presets, scripts, arrangement, home, bundle. |
| `ui-model` | none | UI model contract and catalog. |
| `appearance` | — | Versioned colors and typography. |
| `chart` | appearance, chart-interaction, civil, orderflow, indicator-session, indicators, engine, workspace | Chart model. |
| `indicator-session` | pine, indicators, engine | Source binding, batches, deltas. |
| `anchored-studies` | indicators, engine | Resumable profile and anchored-average state; the caller schedules and paints. |
| `pine` | indicators | "Quantick Pine", a Pine v5 subset; zero external dependencies. |
| `strategy` | sim, workspace, engine | Armed regions, brackets, the armed-instance state machine, `SignalAlarm` and sounds, preset bank, drawing anchors. |
| `control-local` | control | Instance-descriptor directory and blocking loopback client, under one ownership check. |
| `control-host` | control, engine | Projection registry, admission, idempotency store, event journal. Told the time. |
| `indicators` | engine | The `Indicator` trait (commit/preview, rollback), incremental `ta.*` kernels, draw objects. |
| `replay` | civil, engine | Recorded sessions: CSV, folder scan, deal recorder, playback clock; told the time. Test support is `replay::test_support`. |
| `paper` | sim, trading, replay, workspace, civil, engine | Paper account, risk, journal and report; three drivers. |
| `sim` | trading, engine | `TradingVenue` impl; fills only on what the tape proves, never on quotes. |
| `trading` | engine | Venue-neutral order vocabulary and the `TradingVenue` port. |
| `feed` | feed-*, sources, replay, orderbook, engine | The `FeedEvent`/`FeedCommand` port, its adapters, session export. Owns runtimes, threads, clock. |
| `feed-binance`, `feed-hyperliquid`, `feed-mt5` | engine, orderbook, sources | Produce trades. |
| `sources` | engine | Feed config vocabulary and the history reach, below `feed`. |
| `orderflow` | appearance, engine, orderbook | Liquidity history, grouping, timeline and heatmaps. Told the time. |
| `engine` | — | Trades in, bars out. Deterministic, no clock. |
| `orderbook` | — | L2 book core: validated snapshots, absolute level updates, update-id continuity. |
| `control` | — | Control-plane contracts and the published `fake` host/client ports. |
| `civil` | — | Civil dates and the display offset the journal, report and axis share. |
| `workspace` | — | Layout documents and pane membership transitions. |
| `chart-interaction` | layers, orderflow | Pane state, menus, quick ranges, anchors. |
| `layers` | — | Layers, visibility, availability, inheritance, persistence. |
| `backpressure` | — | Owner-to-worker admission (park, fold, never drop), progress; told the time. |
| `operability` | — | UI behaviours, capabilities and exclusions. |
