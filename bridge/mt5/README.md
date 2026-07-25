# MT5 bridge setup

Two bridges stream a symbol's ticks **and its Depth of Market** into quantick.
They speak the same wire protocol (PROTOCOL.md) into the same local socket, so
quantick cannot tell which one dialed it — pick whichever fits the day. Neither
involves credentials: the terminal is already logged in and nothing ever leaves
`127.0.0.1`.

| | `quantick_bridge.py` | `QuantickBridge.mq5` |
|---|---|---|
| Setup | one command | compile, copy, drag onto a chart |
| Runs | outside, attaches by IPC | inside the terminal |
| Book updates | polls (~5 ms) | pushed by `OnBookEvent` |
| Survives terminal restart | reconnects | restarts with the terminal |

**Start here — you probably do not have to start anything.** Selecting a
MetaTrader feed makes quantick launch `quantick_bridge.py` itself: it waits a
few seconds first, so a bridge you already started, or an EA sitting on a
chart, is used instead of being fought. All it needs is the official package
(`pip install MetaTrader5`) and a running, logged-in terminal. Turn it off with
`bridge_autostart = false`, or point `bridge_command` somewhere else, under
`[metatrader]` in your config.

To run it by hand anyway — to see its log lines up close, or to feed a quantick
that is not yours:

```
python bridge/mt5/quantick_bridge.py --symbol WINQ26
```

Options worth knowing:
`--port` (default 9100), `--backfill-minutes` (720 — a whole B3 session),
`--backfill-max-ticks` (200 000; the newest win and the log says how many were
left behind), `--no-book`, `--book-poll-ms` (5), `--utc-offset-s`.

The terminal keeps far more history than you would guess — a probe on
2026-07-24 found 1.25 M ticks for that day alone and 36 M over 30 days, with a
full day returning in under a second. The cap exists for what happens after:
every backfilled tick becomes a line on the socket and a bar on the chart.

The one thing it will refuse to do is guess: MetaTrader stamps everything in
server wall time and exposes no server clock to outside processes, so the
offset is measured from a *moving* tick. Run it once while the market trades
and it caches the value; start it cold outside market hours and it stops with
`BRIDGE_UTC_OFFSET_UNKNOWN` rather than mislabelling every timestamp. Pass
`--utc-offset-s -10800` (B3) to skip the wait.

The rest of this page is the Expert Advisor, which trades setup cost for
event-accurate book updates.

## Install the EA (once)

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

## Run the EA

1. Start quantick with a MetaTrader feed selected — it listens on
   `127.0.0.1:9100` (configurable, `[metatrader]` in `quantick.toml`) and
   logs `MT5_LISTENING`.
2. Open a chart of the symbol quantick expects (e.g. **WIN$N**) and drag
   `QuantickBridge` onto it. Inputs: host/port, backfill minutes (default 30),
   heartbeat seconds, plus the depth pair below.
3. The Experts tab prints `BRIDGE_SESSION_STARTED` with the backfill count;
   quantick logs `MT5_HELLO_OK` and the chart populates.

## Depth of Market (book heatmap)

`InpStreamBook` (default on) subscribes to the symbol's DOM and sends a
complete book image whenever it changes; `InpBookMinIntervalMs` (default 20)
caps how often. MT5 has no incremental book protocol — `MarketBookGet` only
ever returns the whole visible DOM — so the feed diffs successive images into
the same snapshot-plus-delta stream the Binance path produces, and the heatmap
runs on one shared pipeline.

Two limits are real and are labelled rather than hidden:

- The terminal exposes only the top levels (`SYMBOL_TICKS_BOOKDEPTH`), so
  coverage is reported as *limited* and liquidity leaving that window shows as
  removed. It is not the whole exchange book and quantick never says it is.
- Not every symbol or account has a DOM. When `MarketBookAdd` fails the EA
  logs `BRIDGE_BOOK_SUBSCRIBE_FAILED`, ticks keep streaming, and quantick says
  `MT5_BOOK_UNSUPPORTED_BY_BRIDGE` instead of drawing an empty heatmap. The
  same message appears for an EA compiled before depth support.

Symbol must match: the EA streams the chart it is attached to, and the feed
refuses a hello for a different symbol (`MT5_SYMBOL_MISMATCH`).

## Diagnose

Both sides speak structured logs:

- **Python bridge (stderr)**: JSON lines with the same `event_code`
  vocabulary — `BRIDGE_STARTING`, `BRIDGE_TERMINAL_ATTACH_FAILED`,
  `BRIDGE_UTC_OFFSET*`, `BRIDGE_SESSION_STARTED`, `BRIDGE_BOOK_STATS`,
  `BRIDGE_DISCONNECTED`.
- **EA (Experts tab)**: JSON lines with `event_code` — `BRIDGE_STARTING`,
  `BRIDGE_CONNECT_FAILED` (feed not running / URL not allowed),
  `BRIDGE_SESSION_STARTED`, `BRIDGE_DISCONNECTED` (+ retry),
  `BRIDGE_BOOK_SUBSCRIBED` / `BRIDGE_BOOK_SUBSCRIBE_FAILED`, and
  `BRIDGE_BOOK_STATS` every heartbeat (images sent vs skipped as unchanged).
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
