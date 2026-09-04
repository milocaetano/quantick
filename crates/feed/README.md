# quantick-feed

The **feed host** — the port every market-data source implements, and the
adapters that run one:

- `lib` — the port itself: `FeedEvent`, `FeedCommand`, `FeedCapabilities`,
  `FeedHandle`, `FeedSource`, `FeedNotice`, `FeedGap`, `FeedLatency`, and the
  `spawn` that turns a `FeedSource` into a running handle.
- `binance` — public aggTrades and depth, straight from the venue.
- `hyperliquid` — public perpetual trades and complete L2 images.
- `metatrader` — the local QuantickBridge EA listener (see `bridge/mt5/`).
- `mt5_bridge` — supervising the bridge process itself.
- `replay` — a recorded session played back through the very same channel,
  which is what lets market replay reuse the whole chart untouched.
- `stall` — deciding when a quiet socket has stopped being quiet and started
  being wedged.
- `ohlcv_plan` — how a candle-history request is cut into venue-sized pages.
- `config` — the feed-shaped half of the TOML config: `ProviderKind`,
  `FeedCapabilities`, `MetaTraderSettings`, `Mt5SideSource` and `Mt5Endpoint`.
  They describe these adapters, so they are declared beside them;
  `quantick-app`'s `config` re-exports them for its own callers.
- `hooks` — `HookSpec` and `declare_hooks!`, the one-line declaration a module
  makes beside the `QUANTICK_*` it reads. Declared here rather than in the
  application because four of these adapters read a hook and the application's
  registry has to hold one `HookSpec` type for all of them; `quantick-app`'s
  `hooks` module re-exports both and keeps the `OWNERS` table.

## This is the level that owns runtimes

`quantick-feed` is deliberately **not** headless. It starts `tokio` runtimes,
spawns `std::thread`s, opens sockets, and reads the wall clock — `wall_clock_ms`
is defined here, in `clock`, because this is the layer where real time enters
quantick.

**Everything below it stays clock-free.** `quantick-engine`,
`quantick-orderbook` and `quantick-replay` are *told* what time it is by the
trades and events they receive and never ask; that is what keeps one fixture
producing one set of bars. The three `feed-*` venue crates stamp arrival from
the clock, and that stamp never crosses the `FeedEvent` channel as engine
input. Nothing in this crate may leak a clock read downward.

It depends on `quantick-engine` (bars, trades), `quantick-orderbook` (depth
events), `quantick-replay` (recorded sessions) and the three venue crates. It
never depends on `egui`, on `eframe`, or on the script language: no UI type
reaches this crate, which is what lets a headless consumer — a backtest, a bot,
issue #273's native adapter protocol — be tested against the same port the
chart drains.

`quantick-app` is the first consumer: it calls `spawn` and drains the resulting
`FeedHandle` once a frame.
