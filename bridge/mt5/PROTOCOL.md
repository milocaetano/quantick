# QuantickBridge wire protocol (schema 1)

Newline-delimited JSON (NDJSON, UTF-8) over a local TCP socket. The **bridge
dials out** (MQL5 sockets are client-only); the quantick feed listens
(default `127.0.0.1:9100`). One connection = one session. The stream is
one-way: bridge → feed.

**One port carries one symbol.** A listener serves a single session at a time
and refuses the `hello` of any other symbol, so streaming several symbols means
several listeners, each on its own port with its own bridge (quantick:
`[metatrader.ports]`; EA: `InpPort`).

Two bridges implement it — `quantick_bridge.py` (outside the terminal, via the
official Python package) and `QuantickBridge.mq5` (inside it) — and the feed
accepts either without knowing the difference. The protocol is the contract;
the bridges are interchangeable.

The executable counterpart of this document is
`crates/feed-mt5/src/protocol.rs` — its tests parse the verbatim lines shown
here. If you change one, change all three.

## Session shape

```
hello                        exactly once, first line
backfill_start               optional block, at most once, right after hello
  tick × N                   historical ticks (CopyTicks)
backfill_end
rates_start                  optional block, at most once, after backfill_end
  rate × N                   batched historical candles (CopyRates)
rates_end
tick | book | heartbeat × …  live, until the session ends
bye                          optional clean goodbye
```

A session lasts until the connection ends; the feed then goes back to
accepting. While one is being served, **a second connection to the same port is
accepted and closed**, not queued: it gets a short window (250 ms) to send its
`hello` so the feed's log can name the symbol that dialed the wrong port, then
the socket is closed with no reply — the stream is one-way, so a close is the
only answer the protocol has. The served session is untouched. A bridge sees
this as an ordinary disconnect and retries on its own schedule; the feed logs
`MT5_SESSION_BUSY` every time, which is what makes two EAs sharing one
`InpPort` visible instead of looking like a hang.

## Messages

### hello

```json
{"type":"hello","schema":1,"bridge":"quantick-mt5-bridge","bridge_version":"0.2.0","symbol":"WIN$N","broker_symbol":"WINQ26","digits":0,"server_utc_offset_s":-10800,"book_levels":20,"tick_size":"5"}
```

- `schema` — protocol version; the feed refuses a mismatch.
- `symbol` — what this stream is; the feed refuses it if it expects another.
- `digits` — decimal places every price string carries.
- `server_utc_offset_s` — **the honesty field**: MT5 stamps ticks in *server
  wall time encoded as epoch*. True UTC = `time_ms − server_utc_offset_s×1000`.
  Computed live as `TimeTradeServer() − TimeGMT()` (B3: −10800).
- `tape` — *optional*, `"trades"` or `"quotes"`. What the venue actually
  prints. An exchange feed prints executed trades; a broker-quoted CFD prints
  nothing at all — Tickmill's `US500` sent 100 000 consecutive ticks with no
  LAST bit and no volume, and `COPY_TICKS_TRADE` over the preceding five days
  returned nothing. Both bridges decide it by asking the terminal for one trade
  tick in the last 30 days — the window errs long because mislabelling a real
  tape as quotes looks like nothing is wrong. On `"quotes"` the feed charts one synthetic print
  per tick, at the mid, sized 1 (see `crates/feed-mt5/src/map.rs`), and the
  chart withholds everything that would need a traded volume. **Absent means
  `"trades"`**, so a bridge written before this field behaves exactly as before.
- `book_levels` — *optional*. Present only when this session can actually send
  depth (`MarketBookAdd` succeeded): `SYMBOL_TICKS_BOOKDEPTH`, i.e. the most
  levels per side the terminal exposes. **Absent means no book** — either a
  bridge older than schema 1's depth support, or a symbol/account without a
  DOM. The feed reports that instead of drawing an empty heatmap.
- `tick_size` — *optional*, `SYMBOL_TRADE_TICK_SIZE` as an exact decimal
  string. The instrument's real price grid (WIN: `"5"`), so the consumer does
  not render liquidity on rows that can never hold any.
- `rates` — *optional*, `true` when this session sends the historical candle
  block below. **Absent means no candles**: an older bridge, the Expert Advisor
  (which does not implement them), or `--rates-months 0`. The feed reports the
  absence instead of leaving a time pane waiting for a block that is not coming.

Depth and candle fields are all additive within schema 1: a bridge that predates
any of them still connects and still streams ticks.

### tick

```json
{"type":"tick","seq":1,"time_ms":1784824300802,"bid":"0","ask":"0","last":"177795","volume":3,"flags":1080}
```

- `seq` — bridge-assigned, monotonic from 1 per session. **Synthetic** (MT5
  has no exchange trade id): good for gap detection, not stable across
  sessions.
- `time_ms` — `MqlTick.time_msc`, in **server time** (see hello).
- `bid`/`ask`/`last` — price strings with exactly `digits` decimals; `"0"`
  when the feed carries none (B3 history ticks have no quotes).
- `volume` — contracts; `0` on quote-only ticks.
- `flags` — raw `MqlTick.flags`: BID=2 ASK=4 LAST=8 VOLUME=16 BUY=32 SELL=64.
  Real feeds set undocumented extra bits (B3 sets 1024); consumers must mask,
  not reject. **Known pathology**: some B3 brokers set BUY on every tick —
  the feed's tick-rule side policy exists because of this (verified
  2026-07-23 on WIN$N: 100% of live and history ticks carried `flags=1080`).

### book

```json
{"type":"book","seq":7,"time_ms":1784824300802,"bids":[["177795","3"],["177790","12"]],"asks":[["177800","5"]]}
```

One **complete image** of the Depth of Market. MT5 has no incremental book
protocol: `OnBookEvent` fires and `MarketBookGet` returns the whole visible
DOM, with no update ids of any kind. The feed diffs successive images into the
snapshot-plus-delta form the rest of quantick speaks
(`quantick_orderbook::SnapshotDiffer`).

- `seq` — bridge-assigned, monotonic from 1 per session, **independent of tick
  `seq`**. Only for detecting images lost in transport; a gap makes the feed
  open a new capture generation rather than diff across an unobserved moment.
- `time_ms` — server time (see hello), taken as
  `max(SYMBOL_TIME_MSC, TimeTradeServer()×1000)` so the book timeline keeps
  moving when quotes go quiet.
- `bids`/`asks` — `["price","quantity"]` pairs, exact decimal strings, in
  whatever order the terminal returned them (the feed sorts and sums
  duplicates). Prices carry `digits` decimals.
- Limit levels only. `BOOK_TYPE_*_MARKET` rows are orders waiting to cross,
  not resting liquidity, and the bridge excludes them.
- The bridge sends an image only when it differs from the previous one, and at
  most every `InpBookMinIntervalMs` (default 20 ms).

Empty sides are legitimate (auction, halted book), not an error. A **crossed**
image (best bid ≥ best ask) is real during B3's pre-open auction; the feed
rejects and counts it, keeping the last uncrossed image.

### rates_start / rate / rates_end

```json
{"type":"rates_start","interval_ms":60000,"count_hint":129600}
{"type":"rate","bars":[[1784824260000,"177790","177850","177780","177800","1234"],…]}
{"type":"rates_end"}
```

Historical candles, so a time pane opens with months of context rather than
with the tick window. Sent **once, after `backfill_end`**, by a bridge whose
hello declared `"rates": true`; the transport is one-way, so this block arrives
unasked and the feed holds it until something requests it.

- `interval_ms` — what each candle covers. `60000` (M1) is what ships: one base
  series, resampled locally to whatever the pane shows, which is the only
  contract a push-only transport can keep.
- `count_hint` — *optional*, like `backfill_start`'s.
- `bars` — `[time_ms, open, high, low, close, volume]`, ascending. `time_ms` is
  the **bucket start in server time** (see hello), prices are decimal strings
  with `digits` decimals, and volume is a decimal string.
- **Volume follows what the venue prints.** An exchange tape reports traded size
  (`real_volume`); a broker-quoted CFD prints nothing at all and reports its
  tick count instead — the same one-synthetic-unit-per-tick the live path
  charts for that instrument (see `tape`). The `BRIDGE_RATES_SENT` log line
  names which was used.
- No aggressor split exists in a MetaTrader candle. The feed puts half the
  volume on each side, so total volume stays exact and delta is identically
  zero — read as *not measured*, never as a balanced market.
- Candles the terminal reports with no volume are dropped by the feed, never
  emitted: an empty bucket is a gap, and a carried-forward price would be an
  invented one.

**Why a batch per line.** A `rate` message carries many candles because the
alternatives both fail. One candle per line spends the block in newline framing
— 130 000 lines for a quarter. The whole block on one line breaks the 64 KiB
line cap, which does not truncate the line but *ends the session*. So a batch is
bounded at **300 candles**: a row is a ≤20-character timestamp plus five quoted
decimals of at most 22 characters, separators included — under 150 bytes even
for an instrument quoting eight integer and eight fractional digits. Three
hundred of those is ~44 KiB, about two thirds of the cap.
`rates_line_stays_under_the_cap` in `crates/feed-mt5/src/protocol.rs` pins it,
and `MAX_BARS_PER_RATE_LINE` appears on both sides of the wire.

A block that never reaches `rates_end` is discarded whole rather than delivered
short: a candle series with a hole in it reads as a market that stopped trading.
The next session re-sends it.

### heartbeat

```json
{"type":"heartbeat","seq_last":42,"time_ms":1784824301000,"ticks_sent":42,"server_utc_offset_s":-10800}
```

Sent every ~5 s. Refreshes the offset (DST-safe). A feed hearing nothing for
its read timeout (default 30 s) presumes the bridge dead.

### backfill_start / backfill_end

```json
{"type":"backfill_start","count_hint":500}
{"type":"backfill_end"}
```

Bracket the historical block. An empty block still sends both markers —
`backfill_end` is the "history is done" signal.

### bye

```json
{"type":"bye","reason":"deinit"}
```

Clean goodbye (EA removed, terminal closing). Anything after it is ignored.

## Error handling contract

- Unknown **fields** in a known message: ignored (forward compatibility).
- Unknown message **type** / malformed line / invalid UTF-8 line: the feed
  skips and counts it (`MT5_UNDECODABLE_LINE`), the session survives.
- A line longer than **64 KiB** (orders of magnitude above any protocol
  line): the session is dropped (`MT5_LINE_TOO_LONG`) — an unbounded buffer
  would let any local process exhaust the feed's memory.
- Wrong first message, schema or symbol mismatch: session refused, feed keeps
  listening.
- A connection arriving while a session is being served: refused and closed
  (`MT5_SESSION_BUSY`), the running session unaffected.
- A session that dies mid-backfill, or mid-candle-block, discards the partial
  block; the next connection re-sends it.
- A `rate` batch arriving outside a `rates_start`/`rates_end` pair is dropped
  and counted: it has no declared interval, and guessing one would misdate
  every candle in it.

## Compatibility

Schema 1 grows by optional fields and optional message types rather than by
version bumps, so the two sides can be upgraded independently:

- **New bridge, old feed.** The candle block is unknown to a feed that predates
  it, so it lands in the existing "unknown message type" path: skipped and
  counted per line (`MT5_UNDECODABLE_LINE`), session unharmed. Batching keeps
  the cost of that visible-but-harmless case small — a quarter of M1 candles is
  ~440 warnings rather than 130 000.
- **Old bridge, new feed.** No `rates` in the hello, so the feed reports no
  candle history and the time pane says so rather than waiting.
