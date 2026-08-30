# QuantickBridge wire protocol (schema 1)

Newline-delimited JSON (NDJSON, UTF-8) over a local TCP socket. The **bridge
dials out** (MQL5 sockets are client-only); the quantick feed listens
(default `127.0.0.1:9100`). One connection = one session.

Almost everything travels bridge → feed: the terminal volunteers ticks, book
images, candles and heartbeats, and the feed only listens. The one exception is
history the chart has scrolled past the start of — nothing volunteers that,
because nothing knows the trader wants it until they ask. So a bridge may
declare `history_paging` in its hello, and a bridge that does gets written to:
one `load_older` line in, one `history_start` … `history_end` block back.

**A bridge that does not declare it is never written to.** This is not
politeness: a peer that never reads fills its receive buffer, and in the Expert
Advisor a full buffer eventually blocks the terminal thread that sends ticks —
so a chart asking for history would stop the chart. Silence is a hard "do not",
not "probably fine to try".

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

and, interleaved with the live phase, on a bridge that declared
`history_paging`:

```
                             ◀─ load_older            feed → bridge, one at a time
history_start                one block per request, in order
  tick × N                   older than the request's `before_ms`, ascending
history_end
```

The feed keeps **one request outstanding at a time** and answers a click that
arrives while one is in flight by dropping it, so a bridge never has to
correlate replies with requests — the next block it sends is always the answer
to the last line it read. A session that ends holding a request is answered
with an empty block by the feed itself; the trader's spinner stops either way.

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
- `history_paging` — *optional*, `true` when this session **reads its socket**
  and answers `load_older`. **Absent means no**, and absent is what every bridge
  written before the back-channel says, so they keep working unchanged: the feed
  never writes to them, and quantick disables the chart's "load older" affordance
  rather than offering a button that returns nothing. See the warning at the top
  for why the feed will not just try.
- `rates` — *optional*, `true` when this session sends the historical candle
  block below. **Absent means no candles**: an older bridge, the Expert Advisor
  (which does not implement them), or `--rates-months 0`. The feed reports the
  absence instead of leaving a time pane waiting for a block that is not coming.

Depth and candle fields are all additive within schema 1: a bridge that predates
any of them still connects and still streams ticks.

### tick

```json
{"type":"tick","seq":1,"time_ms":1784824300802,"sent_ms":1784824300815,"bid":"0","ask":"0","last":"177795","volume":3,"flags":1080}
```

- `seq` — bridge-assigned, monotonic from 1 per session. **Synthetic** (MT5
  has no exchange trade id): good for gap detection, not stable across
  sessions.
- `time_ms` — `MqlTick.time_msc`, in **server time** (see hello).
- `sent_ms` — *optional*, **server time**: when the bridge handed this line
  over. It exists so a late tape can be diagnosed instead of merely noticed.
  A chart can subtract a print's `time_ms` from its own clock and get one
  end-to-end number, and that number cannot tell a terminal that received the
  tick late from a socket that delivered it late — opposite faults with
  opposite fixes. `sent_ms` cuts the chain in two: `sent_ms - time_ms` is the
  delay inside the terminal, and the reader's own arrival minus `sent_ms` is
  the delay on the wire. A bridge may stamp once per batch rather than once per
  tick, so the figure is the instant the *batch* left, never later than the
  line itself. **Absent means an older bridge**: the reader reports the split
  as unavailable rather than inventing a zero. Live prints only — on a backfill
  or paged tick the stamp is honest and the difference is the age of the
  history, not a latency.

  **Two halves, and deliberately not three.** The obvious next cut — what the
  terminal cost against what the bridge's own pump cost — is not on this wire.
  A bridge has no cheap way to ask the terminal "what is the newest tick you
  hold that I have not sent yet", and every approximation of it collapses into
  *time since the last print*, which during a stall equals the delay itself and
  so blames the pump for all of it. A field that names the wrong hop is worse
  than no field. The pump reports its own health where it can actually measure
  it, in the terminal's own log: see `BRIDGE_PUMP_LIMIT` and
  `BRIDGE_SEND_STALLED` in README.md.
- `bid`/`ask`/`last` — price strings with exactly `digits` decimals; `"0"`
  when the feed carries none (B3 history ticks have no quotes).
- `volume` — contracts; `0` on quote-only ticks.
- **Which ticks travel.** On a `"trades"` tape the bridge asks the terminal
  for `COPY_TICKS_TRADE` and sends nothing else: the feed already discards
  every tick without a LAST bit, and on a busy exchange tape those quotes
  outnumber the prints several times over. Sending them cost the one thing the
  tape cannot spare — they share the socket with the prints and delay them,
  and because a book image restamps itself on the way out (see `book`) the
  delay is invisible in the depth map and fully visible in the bubbles, which
  drift left of the tape's edge until they fall off it. On a `"quotes"` tape
  every tick still travels: there the quotes *are* the data.
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
  names which was used, and counts the per-bar substitutions in
  `fell_back_to_tick_volume`: a tape instrument whose terminal reported no real
  volume on some bars is a different chart from one that reported it on all of
  them, and once the bars are drawn the two look identical.
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

**Why the bridge pages, and why the block can be shorter than asked.** The
terminal validates a `CopyRates` request's *potential* bar count against its
"Max bars in chart" setting and returns a hard error rather than truncating to
what it will serve. Probed 2026-08-03 against a terminal capped at 100 000: a
90-day M1 range is 129 600 potential slots and came back
`(-2, 'Terminal: Invalid params')`, while the same call over a single day
returned its 563 bars. The bridge therefore walks backwards in counted pages,
merging by bar time — and because its page size is itself a value the Max-bars
dialog offers, a page the terminal still refuses is halved and retried rather
than trusted to be small enough. Two consequences are visible on
the wire: the block is assembled from several terminal calls, so a page failing
partway delivers the newer part rather than nothing; and a young contract simply
has less history than was asked for. `BRIDGE_RATES_SENT` reports
`requested_span_days` against `covered_span_days` so the second case reads as a
fact about the instrument rather than as a bug.

`rates_end` carries **`partial: true`** when the bridge knows the block is short
of what was asked: a page failed after others had landed, the walk hit its page
budget, or the block was clipped to `--rates-max-bars`. **Absent means
complete**, so a bridge predating the field is unaffected. Note what the flag is
*not*: a contract younger than the request has fewer candles and is still
complete — that case shows up as `covered_span_days` below `requested_span_days`,
and the two facts are separate on purpose. The feed sets it too when its own
per-block cap clips the series, so either side knowing is enough.

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

**What the block covers is the bridge's business, not the feed's.** Both
bridges send a window of the width their operator asked for
(`--backfill-minutes`, `InpBackfillMinutes`), and both move that window's *end*
from the clock to the tape when the clock's own window holds nothing: outside a
session, `now` names hours in which nothing printed, while the terminal still
holds the last session on disk. The block is then the newest session there is,
and the feed charts it exactly as it charts any other backfill — the ticks
carry their real timestamps, so how far behind they are is something the chart
reads off them rather than something this protocol has to say. A symbol the
terminal has never held is still an empty block; the search finds history, it
does not invent any.

### load_older (feed → bridge)

```json
{"type":"load_older","count":2000,"before_ms":1784824300802}
```

The only message that travels this way. Sent when the trader asks the chart for
more history than it holds, and only to a session whose hello declared
`history_paging`.

- `count` — how many ticks the chart wants. A bound, not a promise: the bridge
  sends what the terminal has and says so with `history_end`. A bridge should
  cap it against its own limit (the Python bridge: 200 000) rather than trust
  it — the number comes from a field a trader can type into.
- `before_ms` — **exclusive** upper bound, in server time (see hello): the
  timestamp of the oldest tick the chart holds. Every tick in the answer must be
  strictly older. Server time because every other timestamp in this protocol is;
  quantick converts from UTC on the way out, where the live offset is tracked.

Note the unit mismatch the bridge has to absorb: `CopyTicksRange` takes whole
seconds, so a bridge asking for `before_ms / 1000` would drop every tick sharing
that second and one asking for the second *containing* it must filter the
surplus by millisecond. The Python bridge rounds the range outward and filters
(`walk_back`); the feed drops any overlap that survives anyway, because two
implementations of one boundary is one too many to trust.

### history_start / tick / history_end

```json
{"type":"history_start","count_hint":2000}
{"type":"history_end","exhausted":true,"scanned_to_ms":1784824200000}
```

The answer to one `load_older`. The ticks between the markers are ordinary
`tick` messages — same shape, same `seq` counter, continuing monotonically — so
the mapping stays one code path. They are *history*, and the markers are the
only thing that says so.

Deliberately **not** `backfill_start`/`backfill_end`. That block is the session's
opening window: it arrives unasked, exactly once, and lands at the front of an
empty chart. This one arrives on demand, repeatedly, and lands *before*
everything already charted. A bridge that reused the backfill markers would have
quantick prepend the opening window on every reconnect and append a paged block
to the live tape.

- `count_hint` — *optional*, like `backfill_start`'s. A bridge that walks the
  terminal before it knows the count should send `history_start` **first and
  bare**, then walk: the search is the slow part, the feed drops a session it
  has heard nothing from for its read timeout, and a bridge that waits until it
  can fill in a count spends that whole wait silent.
- `scanned_to_ms` — *optional*: the oldest instant the search actually
  **reached**, in server time. Not the oldest tick sent — the two come apart
  constantly, and the difference is what keeps paging moving.

  A stretch of quote-only ticks maps to no trades at all; a window over a closed
  market holds nothing to begin with. In both cases the block is empty while the
  search moved hours. A consumer paging from its oldest *trade* would then ask
  for the identical window on the next click, forever — a trader clicking into a
  pre-open session would never get past it. quantick pages from this instead,
  falling back to its oldest trade when the field is absent (which is what a
  bridge predating it gets, unchanged).

  Report only what was **delivered**. A bridge that found more than `count` and
  trimmed the surplus has not delivered what it searched past, so it reports the
  oldest tick it actually sent — otherwise the consumer skips over history that
  never crossed the wire.
- `exhausted` — *optional*. `true` means the terminal has **nothing older at
  all** for this symbol: the walk reached the first tick the terminal holds.
  **Absent means "there may be more"**, so a bridge that cannot tell simply
  never claims the end and the trader keeps a live button.

  Same trimming rule as `scanned_to_ms`: a walk that reached the terminal's
  floor but trimmed its surplus has **not** delivered the end of the tape, and
  must not claim it. Both halves are needed — the floor reached, and nothing
  dropped.

  An empty block is *not* by itself the end either, and must not be sent with
  `exhausted` on that basis. A page that failed, or one that crossed a weekend
  without finding anything, returns nothing and has plenty older behind it —
  and **`exhausted` really does retire the button**: quantick withdraws the
  `history_paging` capability on it, so the chart's "load older" greys out until
  the next session's hello re-publishes it. That is the right answer for a tape
  that has genuinely run out, and the one mistake here that cannot be undone by
  clicking again.

An empty block still sends **both** markers. `history_end` is what stops the
chart's loading indicator, exactly as `backfill_end` is what resolves its
opening load.

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
- A session that dies mid-**page**, or holding a request it never sent, has that
  request answered with an empty block by the feed. The request is *not*
  replayed against the next session: it names a cursor that session never sent.
- A `history_start`/`history_end` block nobody asked for is collected and
  discarded rather than charted — its ticks are history, and charting history at
  the front of the live tape is the one outcome worse than dropping it.
- A `load_older` a bridge cannot parse is skipped and counted
  (`BRIDGE_UNDECODABLE_COMMAND` / `BRIDGE_MALFORMED_COMMAND`), the session
  survives. One unreadable click must not cost the trader their tick stream —
  but note the consequence: no block comes back, so the feed's request stays
  outstanding until the read timeout ends the session and answers it empty.
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
  candle history and the time pane says so rather than waiting. No
  `history_paging` either, so the feed never writes to the socket and the chart
  disables "load older" — the behaviour that build had before the back-channel
  existed, unchanged.
- **New bridge, old feed.** A feed predating the back-channel never sends
  `load_older`, so the bridge's reader simply never has anything to read and its
  `history_start`/`history_end` path never runs. The extra hello field lands in
  the "unknown fields are ignored" rule.
