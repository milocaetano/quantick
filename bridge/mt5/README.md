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

Two things make that work outside a developer's checkout:

- **The script is found, not assumed.** A relative `bridge_command` path is
  resolved against the working directory *and* against the folder holding the
  quantick executable and its ancestors, so a build launched from a shortcut
  still finds the `bridge/` folder shipped beside it.
- **`python` is a first guess, not a requirement.** With no `python` on PATH,
  the Windows `py` launcher is tried next. A `bridge_command` you configured
  yourself is never second-guessed — it is an instruction, not a hint.

**You should not have to read this file to fix a broken setup.** Whatever stops
the bridge is reported on the chart, with the one next step: terminal closed,
package missing, contract not in Market Watch, server clock unmeasurable, no
Depth of Market for that symbol. The log keeps the full detail; the chart
carries the part you have to act on.

To run it by hand anyway — to see its log lines up close, or to feed a quantick
that is not yours:

```
python bridge/mt5/quantick_bridge.py --symbol WINQ26
```

Options worth knowing:
`--port` (default 9100), `--backfill-minutes` (720 — a whole B3 session),
`--backfill-max-ticks` (1 000 000; the newest win and the log says how many were
left behind), `--no-book`, `--book-poll-ms` (5), `--utc-offset-s`.

The terminal keeps far more history than you would guess — a probe on
2026-07-24 found 1.25 M ticks for that day alone and 36 M over 30 days, with a
full day returning in under a second. The cap exists for what happens after:
every backfilled tick becomes a line on the socket and a bar on the chart, so
the opening block is sized to hold a trading day rather than every day the
terminal keeps.

**Reaching further back.** This bridge answers quantick's *load older* button:
click it and the chart asks for ticks from before its oldest, this bridge walks
the terminal backwards and sends them, and they land in front of what is
already drawn. So the cap above is a pace, not a ceiling — whatever it left
behind is one click away. The Expert Advisor does not implement this; quantick
disables the button rather than offering one that returns nothing. The wire
details are in `PROTOCOL.md` (`load_older`, `history_start`/`history_end`).

The one thing it will refuse to do is guess: MetaTrader stamps everything in
server wall time and exposes no server clock to outside processes, so the
offset is measured from a *moving* tick. Run it once while the market trades
and it caches the value; start it cold outside market hours and it stops with
`BRIDGE_UTC_OFFSET_UNKNOWN` rather than mislabelling every timestamp. Pass
`--utc-offset-s -10800` (B3) to skip the wait.

The rest of this page is the Expert Advisor, which trades setup cost for
event-accurate book updates.

## Install the EA (once)

> **Already running an older EA?** Recompile and re-drag it. From **0.3.0** the
> bridge asks a printing venue for executed trades only and writes them in
> batches instead of one socket call per tick. Both cut delay that used to show
> up on the chart as aggression bubbles trailing the tape's right edge — see
> *What the tape costs the terminal* below. quantick reports the version it is
> talking to in `MT5_HELLO_OK` (`bridge_version`).

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
   `127.0.0.1:9100`, or on this symbol's own port if it has one (`[metatrader]`
   in `quantick.toml`; see *Multiple symbols* below). It logs the address it
   resolved as `MT5_ENDPOINT_RESOLVED`, then `MT5_LISTENING`.
2. Open a chart of the symbol quantick expects (e.g. **WIN$N**) and drag
   `QuantickBridge` onto it. Inputs: host/port, backfill minutes (default 30),
   heartbeat seconds, plus the depth pair below. Raise the backfill minutes if
   you want the chart to open on the whole session: unlike the Python bridge,
   the EA cannot be asked for more afterwards, so what it sends on connect is
   what that session has.
3. The Experts tab prints `BRIDGE_SESSION_STARTED` with the backfill count;
   quantick logs `MT5_HELLO_OK` and the chart populates.

## What the tape costs the terminal

`SocketSend` runs on the terminal's *main thread* — the same thread that picks
up new ticks. Two things used to make a busy tape expensive there, and both
showed up on the chart the same way: the bubbles falling behind the tape's
right edge, and past the lane's window disappearing from it altogether.

- **Quote updates travelled too.** quantick charts a printing venue from `last`
  and `volume` and discards every tick without a LAST bit, so a quote-only tick
  is data the other end deletes on arrival. The EA now asks for
  `COPY_TICKS_TRADE` whenever `DetectTape` says the symbol prints. **How much
  this saves is per broker, and on the one measured here it saves nothing**:
  the committed WIN$N recording is 100% `flags=1080`, every tick carrying LAST.
  It is kept because it is free where there is nothing to filter and a bound on
  the wire where a broker does quote separately. A broker-quoted symbol is
  unchanged: there the quotes are the whole tape.
- **One syscall per tick.** Lines are queued and written in batches now, capped
  so a burst never grows an unbounded string, and flushed at the end of every
  pass so batching never becomes latency of its own. **The flush-per-pass is
  the point and also the limit**: when the terminal hands over one tick per
  `OnTick`, one line is queued and one write goes out, exactly as before. The
  saving is real only when a pass carries several ticks — a burst, an opening
  auction, or the 200 ms `OnTimer` sweeping up what `OnTick` did not. At the 33
  ticks/s the WIN$N recording averages there is little to batch, and little
  being delayed either.

`BRIDGE_TAPE_STATS` reports it every heartbeat: `ticks_sent` against
`socket_writes` (well apart is the batching working), plus `quiet_ms` — how long
since the last print this bridge forwarded — and `send_stalls` and `pump_limits`,
the two counts below. `quiet_ms` used to be called `tick_lag_ms` and read as how
far the pump trails the terminal, which it never was: the clock behind it
advances whether or not a newer tick exists, so on a quiet market it counts the
quiet and during a stall it counts the stall. On the quantick side the tape's own
age appears as `tape_newest_print_age_ms` in `APP_HEALTH_SUMMARY`, and on the
chart as the caption under the tape.

## Why a late tape says who is late

A chart running seconds behind the terminal used to be one number in a corner —
`arrival 18112 ms` — and that number reads the same whether the terminal got the
print late, this bridge's pump is trailing the terminal, or quantick read the
socket late. Three faults, three different fixes, one indistinguishable symptom.

So the bridge stamps what it sends. Every tick carries `sent_ms`: when the
bridge handed the line over, on the same server clock as `time_ms`. quantick
subtracts the two and gets the delay inside MetaTrader; its own arrival instant
minus `sent_ms` is the delay on the wire. The two halves add up to the whole,
and the larger one is the name that reaches the status bar and
`quantick_get_diagnostics`. The accounting is in
`crates/feed-mt5/src/latency.rs`; `PROTOCOL.md` defines the field.

`sent_ms` is additive within schema 1: a bridge that predates it still streams,
and the split is then reported unavailable rather than as a zero nobody
measured.

**Two halves, and deliberately not three.** Splitting MetaTrader's own half
again — what the terminal cost against what this pump cost — was built and then
taken back out. A bridge has no cheap way to ask the terminal "what is the
newest tick you hold that I have not sent yet", and every approximation of it
collapses into *time since the last print*, which during a stall equals the
delay itself and so blames the pump for all of it. A figure that names the wrong
hop is worse than no figure. The pump instead reports its own health from things
it can really see:

- `BRIDGE_PUMP_LIMIT` — one pass ended on a bound (`PUMP_MAX_ROUNDS` rounds, or
  `PUMP_MAX_MS` of main-thread time) rather than because the terminal was
  drained. A pass keeps calling `CopyTicks` while the answers come back full, so
  this is the tape genuinely arriving faster than one bounded pass can forward
  it. A pass that caught up on its last permitted round does **not** report
  here, and that distinction is what makes the count worth reading.
- `BRIDGE_SEND_STALLED` — `SocketSend` blocked for `blocked_ms`. It runs on the
  terminal's main thread, so that is time no tick was being collected: the cause
  is quantick not reading, and the symptom looks like a slow terminal.
- `InpPumpIntervalMs` (default 25 ms) — the safety net under `OnTick`, which is
  the fast path. It was a fifth of a second, which is a fifth of a second the
  net itself could cost on any symbol whose chart is not receiving `OnTick`.

Read those beside quantick's own `MT5_TAPE_LATE`, which names the half it
measured, and `MT5_CONSUMER_BACKPRESSURE` — the case where quantick's own stall
is what makes the terminal look slow.

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

## Multiple symbols

MQL5 sockets are client-only, so a "connection" is an EA on a chart dialing
quantick, and one port carries one symbol's stream. Charting XAUUSD and US500
from the same terminal at once is therefore not a setting — it is two of
everything: two ports, two charts, two EAs.

**The port is the only thing pairing a chart to a feed.** Per symbol:

1. Give it a port in your configuration, under `[metatrader.ports]`:

   ```toml
   [metatrader.ports]
   XAUUSD = 9101
   US500  = 9102
   US30   = 9103
   ```

   Two symbols may not share a port, and none may reuse the `listen_addr` port
   (9100) that every unmapped symbol falls back to. quantick refuses a config
   that breaks either rule, naming both claimants — the alternative is a chart
   that is simply empty and does not say why.
2. Open that symbol's chart and drag `QuantickBridge` onto it.
3. Set that chart's **`InpPort` to the same number**. Nothing else has to
   match; the symbol comes from the chart the EA sits on.

One EA per chart, one chart per port. Symbols with no entry share 9100, which
is fine as long as only one of them streams at a time.

**When the ports collide**, both sides say so rather than going quiet:

- *Two EAs on one port.* The first is served. The second is accepted, given a
  moment to identify itself, then closed: quantick logs `MT5_SESSION_BUSY` with
  the peer address and the symbol its hello declared, and the EA's Experts tab
  shows `BRIDGE_DISCONNECTED` as it retries. The established session keeps
  streaming throughout — the intruder never interrupts it.
- *Two quantick feeds on one port.* Whoever bound it first keeps it; the second
  logs `MT5_BIND_FAILED` and puts a notice on that chart naming the address and
  pointing at `[metatrader.ports]`. Charting the same symbol twice hits this,
  and so does forgetting to map a second symbol. The losing feed keeps retrying
  the bind, so freeing the port — closing the other tab or instance — is
  enough: that chart reconnects on its own.

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
  `BRIDGE_BOOK_STATS` every heartbeat (images sent vs skipped as unchanged),
  plus `BRIDGE_PUMP_LIMIT` and `BRIDGE_SEND_STALLED` when the tape or the
  socket is the reason a chart is late.
- **quantick (stderr, `QUANTICK_LOG_FORMAT=json`)**: `MT5_*` events. The
  feed's own — `MT5_LISTENING`, `MT5_SESSION_BUSY`, `MT5_HELLO_OK` and the
  rest — are tabulated in `crates/feed-mt5/src/lib.rs`. The app layer emits a
  few more around them, notably `MT5_ENDPOINT_RESOLVED` (which port this
  symbol got, and whether it came from `[metatrader.ports]`); those live in
  `crates/app/src/feed/metatrader.rs`.

No terminal at hand? Replay the committed real recording against a running
quantick instead:

```
cargo run -p quantick-feed-mt5 --example replay_bridge -- \
    crates/feed-mt5/tests/fixtures/win_ticks.ndjson 127.0.0.1:9100 --pace-us 500
```

To record fresh fixtures from a live terminal (one-off, Python):
`python tools/mt5/record_ticks.py --symbol "WIN$N" --out <file>`.
