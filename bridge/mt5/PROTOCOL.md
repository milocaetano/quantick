# QuantickBridge wire protocol (schema 1)

Newline-delimited JSON (NDJSON, UTF-8) over a local TCP socket. The **bridge
dials out** (MQL5 sockets are client-only); the quantick feed listens
(default `127.0.0.1:9100`). One connection = one session. The stream is
one-way: bridge → feed.

The executable counterpart of this document is
`crates/feed-mt5/src/protocol.rs` — its tests parse the verbatim lines shown
here. If you change one, change both.

## Session shape

```
hello                    exactly once, first line
backfill_start           optional block, at most once, right after hello
  tick × N               historical ticks (CopyTicks)
backfill_end
tick | heartbeat × …     live, until the session ends
bye                      optional clean goodbye
```

## Messages

### hello

```json
{"type":"hello","schema":1,"bridge":"quantick-mt5-bridge","bridge_version":"0.1.0","symbol":"WIN$N","broker_symbol":"WINQ26","digits":0,"server_utc_offset_s":-10800}
```

- `schema` — protocol version; the feed refuses a mismatch.
- `symbol` — what this stream is; the feed refuses it if it expects another.
- `digits` — decimal places every price string carries.
- `server_utc_offset_s` — **the honesty field**: MT5 stamps ticks in *server
  wall time encoded as epoch*. True UTC = `time_ms − server_utc_offset_s×1000`.
  Computed live as `TimeTradeServer() − TimeGMT()` (B3: −10800).

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
- A session that dies mid-backfill discards the partial block; the next
  connection re-sends history.
