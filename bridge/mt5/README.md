# MT5 bridge setup

`QuantickBridge.mq5` streams a chart's ticks to quantick over a local socket.
MQL5 is the terminal's C++-family language; the EA runs inside the already
logged-in terminal, so **no credentials exist anywhere in this path** and
nothing ever leaves `127.0.0.1`.

## Install (once)

1. **Compile** (either way):
   - MetaEditor: open `QuantickBridge.mq5`, press F7; or
   - CLI: `MetaEditor64.exe /compile:"<repo>\bridge\mt5\QuantickBridge.mq5"`
2. **Copy** `QuantickBridge.ex5` into the terminal's `MQL5\Experts\` folder
   (MetaTrader: File → Open Data Folder → MQL5 → Experts), then refresh the
   Navigator (right-click → Refresh).
3. **Allow the socket**: Tools → Options → Expert Advisors →
   ✔ *Allow WebRequest for listed URL* → add `127.0.0.1`.
   Without this, `SocketConnect` fails and the Experts tab shows
   `BRIDGE_CONNECT_FAILED` with that hint.
4. ✔ *Allow Algo Trading* (toolbar button) so the EA runs.

## Run

1. Start quantick with a MetaTrader feed selected — it listens on
   `127.0.0.1:9100` (configurable, `[metatrader]` in `quantick.toml`) and
   logs `MT5_LISTENING`.
2. Open a chart of the symbol quantick expects (e.g. **WIN$N**) and drag
   `QuantickBridge` onto it. Inputs: host/port, backfill minutes (default 30),
   heartbeat seconds.
3. The Experts tab prints `BRIDGE_SESSION_STARTED` with the backfill count;
   quantick logs `MT5_HELLO_OK` and the chart populates.

Symbol must match: the EA streams the chart it is attached to, and the feed
refuses a hello for a different symbol (`MT5_SYMBOL_MISMATCH`).

## Diagnose

Both sides speak structured logs:

- **EA (Experts tab)**: JSON lines with `event_code` — `BRIDGE_STARTING`,
  `BRIDGE_CONNECT_FAILED` (feed not running / URL not allowed),
  `BRIDGE_SESSION_STARTED`, `BRIDGE_DISCONNECTED` (+ retry).
- **quantick (stderr, `QUANTICK_LOG_FORMAT=json`)**: the full `MT5_*` event
  table in `crates/feed-mt5/src/lib.rs`.

No terminal at hand? Replay the committed real recording against a running
quantick instead:

```
cargo run -p quantick-feed-mt5 --example replay_bridge -- \
    crates/feed-mt5/tests/fixtures/win_ticks.ndjson 127.0.0.1:9100 --pace-us 500
```

To record fresh fixtures from a live terminal (one-off, Python):
`python tools/mt5/record_ticks.py --symbol "WIN$N" --out <file>`.
