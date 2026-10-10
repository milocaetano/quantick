# AGENTS.md — quantick for AI agents

The crate map. [CLAUDE.md](CLAUDE.md) owns the rules, [CONTRIBUTING.md](CONTRIBUTING.md) the verification loop,
[docs/agentic-development.md](docs/agentic-development.md) their reasons,
[docs/README.md](docs/README.md) the docs index.

**Driving the running app?** Build `quantick-mcp`, run `quantick-mcp setup
--client claude` (or `codex`), enable **Tools → Local agent access**, and call
`quantick_describe` first. Tools and authority:
[crates/mcp/README.md](crates/mcp/README.md); capabilities: the generated
[capability inventory](docs/control-plane/capability-inventory.md) and
[capability catalog](schemas/control/observer-capability-catalog-v1.json).

## The map

A Cargo workspace under `crates/`. *Depends on* is one-way
(`crates/guards/src/graph.rs`). `app`, `backtest`, `mcp` and `guards` are
leaves; `guards` has no edges either way.

| Crate | Depends on | What it owns |
| --- | --- | --- |
| `app` | every crate but the other leaves, `trading` and `sources` | The desktop chart (egui). |
| `backtest` | strategy, pine, indicators, replay, sim, paper (dev), engine | Recorded sessions in, performance out, over the chart's engine and indicators. |
| `mcp` | control-local, control | The MCP adapter; stdout carries MCP frames only. |
| `guards` | — | Size, context, cycle and UI-free ratchets; English and encoding scans. |
| `control-handlers` | control-schema, control-host, control, chart, sources, engine | Control handlers over headless ports. |
| `control-schema` | control, control-host, stores, pine, layers, sim, orderflow, indicators, orderbook, engine | Control-plane schemas over the domain crates. |
| `stores` | chart, sources, workspace, orderflow, indicators, engine | Cockpit documents: catalogue, symbols, footprint, presets, scripts, arrangement, home, bundle. |
| `chart` | chart-interaction, civil, orderflow, indicator-session, indicators, engine | State/geometry/viewport/style, indicator views |
| `indicator-session` | pine, indicators, engine | Source binding, batches, deltas. |
| `anchored-studies` | indicators, engine | Resumable profile and anchored-average state; the caller schedules and paints. |
| `pine` | indicators | "Quantick Pine", a Pine v5 subset; zero external dependencies. |
| `strategy` | sim, workspace, engine | Armed regions, brackets, the armed-instance state machine, `SignalAlarm` and sounds, preset bank, drawing anchors. |
| `control-local` | control | Instance-descriptor directory and blocking loopback client, under one ownership check. |
| `control-host` | control, engine | Projection registry, admission, idempotency store, event journal. Told the time. |
| `indicators` | engine | The `Indicator` trait (commit/preview, rollback), incremental `ta.*` kernels, draw objects. |
| `replay` | civil, engine | Recorded sessions: CSV, folder scan, deal recorder, playback clock; told the time. Test support is `replay::test_support`. |
| `paper` | sim, replay, workspace, civil, engine | One paper account (orders, risk sizing, journal, home, report), three drivers. |
| `sim` | trading, engine | `TradingVenue` impl; fills only on what the tape proves, never on quotes. |
| `trading` | engine | Venue-neutral order vocabulary and the `TradingVenue` port. |
| `feed` | feed-*, sources, replay, orderbook, engine | The `FeedEvent`/`FeedCommand` port, its adapters, session export. Owns runtimes, threads, clock. |
| `feed-binance`, `feed-hyperliquid`, `feed-mt5` | engine, orderbook | Venue sources; produce trades. |
| `sources` | engine | Feed config vocabulary and the history reach, below `feed`. |
| `orderflow` | engine, orderbook | Liquidity history, grouping, timeline, settled/live heatmap projections. Told the time. |
| `engine` | — | Trades in, bars out. Deterministic, no clock. |
| `orderbook` | — | L2 book core: validated snapshots, absolute level updates, update-id continuity. |
| `control` | — | Control-plane contracts and the published `fake` host/client ports. |
| `civil` | — | Civil dates and the display offset the journal, report and axis share. |
| `workspace` | — | Layout documents and pane membership transitions. |
| `chart-interaction` | layers, orderflow | Pane state, menus, quick ranges, anchors. |
| `layers` | — | Layer catalog, visibility, availability, inheritance, persistence policy. |
| `backpressure` | — | Owner-to-worker admission (park, fold, never drop), progress; told the time. |
| `operability` | — | Each UI behaviour and the capability reaching it, or its exclusion. |
