//+------------------------------------------------------------------+
//| QuantickBridge.mq5 — stream this chart's ticks to quantick.       |
//|                                                                    |
//| Attach this Expert Advisor to a chart of the symbol you want in    |
//| quantick (e.g. WIN$N). It dials the quantick feed's local TCP      |
//| listener and streams newline-delimited JSON: a hello, a backfill   |
//| block from CopyTicks, then live ticks, Depth of Market images and  |
//| heartbeats. The protocol contract lives in PROTOCOL.md next to     |
//| this file; the Rust decoder in crates/feed-mt5 is its executable   |
//| counterpart.                                                       |
//|                                                                    |
//| The book is sent as a *complete image* every time it changes: MT5  |
//| has no incremental book protocol, MarketBookGet only ever returns  |
//| the whole visible DOM. The feed diffs successive images, so only   |
//| real changes travel further into quantick.                         |
//|                                                                    |
//| No credentials are involved anywhere: the terminal is already      |
//| logged in and the socket never leaves this machine.                |
//|                                                                    |
//| Every diagnostic this EA prints to the Experts tab is a single     |
//| JSON object with an event_code, so logs are machine-readable.      |
//+------------------------------------------------------------------+
#property copyright "quantick"
#property version   "1.002"
#property description "Streams ticks to the quantick chart over a local socket"

input string InpHost             = "127.0.0.1"; // Feed host (quantick listener)
input int    InpPort             = 9100;        // Feed port
input int    InpBackfillMinutes  = 30;          // History to send on connect
input int    InpHeartbeatSeconds = 5;           // Heartbeat interval
input int    InpRetrySeconds     = 5;           // Reconnect backoff
input int    InpSendTimeoutMs    = 1000;        // Max ms one send may block
input bool   InpStreamBook       = true;        // Stream Depth of Market
input int    InpBookMinIntervalMs= 20;          // Min ms between book images (0 = every change)
input int    InpPumpIntervalMs   = 25;          // Safety-net pump interval (OnTick is the fast path)

#define SCHEMA_VERSION 1
#define BRIDGE_NAME    "quantick-mt5-bridge"
#define BRIDGE_VERSION "0.4.0"
// How far back to look for one executed trade before declaring a symbol
// tape-less. See DetectTape() for why this errs long.
#define TAPE_PROBE_DAYS 30
// Flush the outgoing buffer once it reaches this many characters. Big enough
// that a busy second costs a handful of syscalls instead of thousands, small
// enough that a burst never grows an unbounded string on the terminal's heap.
#define OUT_BUFFER_FLUSH_CHARS 16384
// Ticks one CopyTicks call may return. The pump keeps calling while a call
// comes back full, so this bounds a single request, not a pass: a burst larger
// than one batch is drained inside the same pass instead of waiting for the
// next event, which is where whole seconds used to be spent.
#define PUMP_TICKS_PER_ROUND 4096
// Hard stop on that loop. Reaching it means the terminal is handing over ticks
// faster than one pass can forward them; the pass ends and says so rather than
// holding the terminal's main thread indefinitely.
#define PUMP_MAX_ROUNDS 16
// A flush slower than this is reported. Nothing on loopback should take a
// millisecond, so this only fires when the reader has stopped reading and the
// send is blocking the very thread that would otherwise collect ticks.
#define SEND_STALL_LOG_MS 25
// Bounds for InpPumpIntervalMs: fast enough that the safety net is a net, slow
// enough that a mistyped 0 cannot spin the terminal's timer thread.
#define PUMP_INTERVAL_MIN_MS 5
#define PUMP_INTERVAL_MAX_MS 1000

int      g_socket           = INVALID_HANDLE;
ulong    g_seq              = 0; // per-session tick sequence, from 1
ulong    g_ticks_sent       = 0;
long     g_last_msc         = 0; // cursor: newest tick time already pumped
int      g_sent_at_last_msc = 0; // ticks already sent sharing g_last_msc
datetime g_last_heartbeat   = 0;
datetime g_next_retry       = 0;
string   g_out              = ""; // queued lines, written by FlushOut
ulong    g_sends            = 0;  // SocketSend batches this session
// Millisecond server clock, anchored on the second boundary (see NowServerMs).
long     g_clock_second     = 0;     // last whole second read from TimeTradeServer
ulong    g_clock_anchor_us  = 0;     // GetMicrosecondCount when that second turned over
bool     g_clock_anchored   = false;
ulong    g_pump_rounds_hit  = 0;     // passes that reached PUMP_MAX_ROUNDS
ulong    g_send_stalls      = 0;     // flushes slower than SEND_STALL_LOG_MS
long     g_cursor_lag_ms    = 0;     // newest tick the terminal holds - newest sent
// Whether this symbol prints executed trades (DetectTape). It decides what
// CopyTicks is asked for: on a printing venue quantick reads `last`/`volume`
// and discards every quote-only tick, so sending them is pure delay.
bool     g_tape_trades      = false;

bool     g_book_subscribed  = false; // MarketBookAdd succeeded
ulong    g_book_seq         = 0;     // per-session book image number, from 1
ulong    g_book_sent        = 0;
ulong    g_book_skipped     = 0;     // images identical to the previous one
long     g_book_last_ms     = 0;     // throttle cursor (local ms)
string   g_book_last_body   = "";    // last image's levels, for change detection

//+------------------------------------------------------------------+
//| Structured Experts-tab logging (AI-first: parseable, coded).      |
//+------------------------------------------------------------------+
void LogEvent(const string event_code, const string detail)
  {
   Print(StringFormat("{\"event_code\":\"%s\",\"symbol\":\"%s\",%s}",
                      event_code, _Symbol, detail));
  }

//+------------------------------------------------------------------+
//| Write everything buffered so far. False = socket is broken.       |
//|                                                                   |
//| SocketSend runs on the terminal's main thread and may write only  |
//| part of the buffer (send timeout, full OS buffer — quantick not   |
//| reading). Each attempt is bounded by SocketTimeouts (set at       |
//| connect); the remainder is retried while progress continues.      |
//|                                                                    |
//| Zero progress means the socket is gone, and the peer may already  |
//| hold half a line. That is not repaired here — it is repaired by   |
//| ending the session, which the caller does on false: the decoder   |
//| discards the fragment with the socket and the reconnect re-sends  |
//| the backfill. The buffer therefore survives until the write does. |
//+------------------------------------------------------------------+
bool FlushOut()
  {
   if(StringLen(g_out) <= 0)
      return(true);
   if(g_socket == INVALID_HANDLE)
     {
      g_out = "";
      return(false);
     }
   uchar bytes[];
   int len = StringToCharArray(g_out, bytes, 0, WHOLE_ARRAY, CP_UTF8) - 1;
   // A non-empty buffer that will not convert is a broken session, not an
   // empty write. Returning true here used to throw the batch away and tell
   // the caller it had been sent: the socket stayed up, quantick got a hole
   // in its tape with no BRIDGE_DISCONNECTED to explain it, and the chart
   // showed the exact symptom this whole change exists to remove. One line
   // was the old risk; a full buffer is now up to sixteen thousand characters
   // of it.
   if(len <= 0)
     {
      g_out = "";
      return(false);
     }
   ulong started_us = GetMicrosecondCount();
   int sent = 0;
   while(sent < len)
     {
      int wrote;
      if(sent == 0)
         wrote = SocketSend(g_socket, bytes, len);
      else
        {
         uchar rest[];
         ArrayCopy(rest, bytes, 0, sent, len - sent);
         wrote = SocketSend(g_socket, rest, len - sent);
        }
      // Zero progress after a partial write means the peer has the front half
      // of a line and will never get the rest. The buffer is dropped and the
      // session ends, which is what repairs it: the reconnect re-sends the
      // backfill, and the decoder throws away the fragment with the socket.
      // Keeping the session alive here is what would leave the framing broken.
      if(wrote <= 0)
        {
         g_out = "";
         return(false);
        }
      sent += wrote;
     }
   // Cleared only once every byte is out. Anything short of that is a failure
   // the caller turns into a disconnect, and the disconnect clears it.
   g_out = "";
   g_sends++;
   // A send that blocks is the one delay this bridge can neither shorten nor
   // hide: SocketSend runs on the terminal's main thread, so every millisecond
   // it waits for quantick to read is a millisecond no tick is collected. The
   // tape arrives late and nothing on either side says why — which is exactly
   // the shape of the bug this reporting exists to end. Named, not absorbed.
   long blocked_ms = (long)((GetMicrosecondCount() - started_us) / 1000);
   if(blocked_ms >= SEND_STALL_LOG_MS)
     {
      g_send_stalls++;
      LogEvent("BRIDGE_SEND_STALLED",
               StringFormat("\"blocked_ms\":%I64d,\"bytes\":%d,\"stalls\":%I64u,"
                            "\"hint\":\"quantick stopped reading the socket; ticks are "
                            "not being collected while this blocks\"",
                            blocked_ms, len, g_send_stalls));
     }
   return(true);
  }

//+------------------------------------------------------------------+
//| Queue one NDJSON line. False = socket is broken.                  |
//|                                                                   |
//| Queued rather than written, because SocketSend is a syscall on    |
//| the terminal's *main thread*, and every millisecond spent there   |
//| is a millisecond it is not reading new ticks. Each pass still     |
//| flushes what it queued, so a pass carrying one tick still costs   |
//| one write: the saving is a burst's, not a quiet tape's, and a     |
//| burst is the only time the cost was worth paying attention to.    |
//| The backlog that builds shows up on the chart as a tape running   |
//| behind its own book — the book restamps itself on the way out     |
//| (see SendBook), so only the prints carry the delay, and they      |
//| drift left until they fall out of the lane entirely.              |
//|                                                                   |
//| Framing is unchanged: the buffer is a concatenation of complete   |
//| newline-terminated lines, so a flush lands on a line boundary     |
//| whatever the OS accepted. It is capped so one burst cannot grow   |
//| an unbounded string inside the terminal.                          |
//+------------------------------------------------------------------+
bool SendLine(string payload)
  {
   if(g_socket == INVALID_HANDLE)
      return(false);
   StringAdd(g_out, payload);
   StringAdd(g_out, "\n");
   if(StringLen(g_out) < OUT_BUFFER_FLUSH_CHARS)
      return(true);
   return(FlushOut());
  }

//+------------------------------------------------------------------+
//| server_time - utc, in seconds (B3: -10800). Recomputed on demand  |
//| so a DST-observing broker stays correct across the change.        |
//+------------------------------------------------------------------+
long ServerUtcOffsetSeconds()
  {
   return((long)TimeTradeServer() - (long)TimeGMT());
  }

//+------------------------------------------------------------------+
//| Server time in milliseconds.                                      |
//|                                                                   |
//| MQL5 offers no sub-second wall clock: TimeTradeServer() counts    |
//| whole seconds, and stamping a tick with it reports up to 999 ms   |
//| of delay that does not exist — the exact error the feed's own     |
//| lag readout was already fighting (see MaybeHeartbeat).            |
//|                                                                   |
//| So the millisecond origin is pinned to the second *boundary*: the |
//| first read that sees a new second records the monotonic           |
//| microsecond counter at that instant, and later reads inside the   |
//| same second add the elapsed monotonic time to it. The remaining   |
//| error is the gap between two calls — a tick apart on a live tape, |
//| InpPumpIntervalMs at worst — instead of a full second.            |
//|                                                                   |
//| A server clock that jumps (a DST change, an offset re-sync) turns |
//| the second over early and simply re-anchors, because the anchor   |
//| is refreshed on *any* change, not on an expected increment.       |
//+------------------------------------------------------------------+
long NowServerMs()
  {
   long  now_s  = (long)TimeTradeServer();
   ulong now_us = GetMicrosecondCount();
   if(!g_clock_anchored || now_s != g_clock_second)
     {
      g_clock_second    = now_s;
      g_clock_anchor_us = now_us;
      g_clock_anchored  = true;
      return(now_s * 1000);
     }
   long elapsed_ms = (long)((now_us - g_clock_anchor_us) / 1000);
   // The anchor cannot outlive its own second: without this a quiet tape,
   // where nothing calls in to notice the turnover, would report a stamp in
   // the second that has not started yet.
   if(elapsed_ms > 999)
      elapsed_ms = 999;
   return(g_clock_second * 1000 + elapsed_ms);
  }

//+------------------------------------------------------------------+
//| Newest tick instant the terminal itself holds, in server ms.      |
//|                                                                   |
//| SYMBOL_TIME_MSC is the last quote's instant at the cursor's own   |
//| resolution; in a quiet book it stops moving, so the coarse server |
//| clock stands in and the reading never runs backwards.             |
//+------------------------------------------------------------------+
long TerminalNewestTickMs()
  {
   long newest = (long)SymbolInfoInteger(_Symbol, SYMBOL_TIME_MSC);
   long coarse = (long)TimeTradeServer() * 1000;
   return((coarse > newest) ? coarse : newest);
  }

//+------------------------------------------------------------------+
//| Drop the socket and schedule a reconnect attempt.                 |
//+------------------------------------------------------------------+
void Disconnect(const string why)
  {
   if(g_socket != INVALID_HANDLE)
     {
      SocketClose(g_socket);
      g_socket = INVALID_HANDLE;
     }
   // Whatever was queued belongs to the session that just ended. Carrying it
   // into the next one would put half a dead session's lines in front of the
   // new hello, which the decoder reads as a protocol violation.
   g_out = "";
   g_next_retry = TimeLocal() + InpRetrySeconds;
   LogEvent("BRIDGE_DISCONNECTED",
            StringFormat("\"reason\":\"%s\",\"retry_in_s\":%d", why, InpRetrySeconds));
  }

//+------------------------------------------------------------------+
//| One tick → one NDJSON line. Prices carry exactly _Digits places.  |
//|                                                                   |
//| `sent_ms` is when *this bridge* handed the line over, on the same |
//| server clock as `time_ms`. The difference between the two is the  |
//| delay inside the terminal, and the difference between `sent_ms`   |
//| and the moment quantick sees the line is the delay on the wire.   |
//| One end-to-end number cannot separate those, and separating them  |
//| is the whole reason a late tape can be diagnosed at all. The      |
//| stamp is taken once per pump pass, not once per tick: a pass that |
//| blocks in SocketSend then shows its cost as wire delay, which is  |
//| where it belongs.                                                 |
//+------------------------------------------------------------------+
bool SendTick(const MqlTick &tick, const long sent_ms)
  {
   g_seq++;
   string line = StringFormat(
      "{\"type\":\"tick\",\"seq\":%I64u,\"time_ms\":%I64d,\"sent_ms\":%I64d,"
      "\"bid\":\"%s\",\"ask\":\"%s\","
      "\"last\":\"%s\",\"volume\":%I64u,\"flags\":%u}",
      g_seq,
      tick.time_msc,
      sent_ms,
      DoubleToString(tick.bid, _Digits),
      DoubleToString(tick.ask, _Digits),
      DoubleToString(tick.last, _Digits),
      tick.volume,
      tick.flags);
   if(!SendLine(line))
      return(false);
   g_ticks_sent++;
   return(true);
  }

//+------------------------------------------------------------------+
//| One book level's quantity. volume_real carries the accurate value |
//| where a venue has fractional lots; B3 volumes are whole contracts |
//| and print as integers so images stay small.                       |
//+------------------------------------------------------------------+
string BookVolumeText(const MqlBookInfo &item)
  {
   double v = (item.volume_real > 0.0) ? item.volume_real : (double)item.volume;
   if(v <= 0.0)
      return("0");
   if(MathAbs(v - MathRound(v)) < 1e-9)
      return(IntegerToString((long)MathRound(v)));
   return(DoubleToString(v, 2));
  }

//+------------------------------------------------------------------+
//| Send one complete Depth of Market image.                          |
//|                                                                   |
//| Two filters keep the terminal's main thread and the socket quiet  |
//| without losing anything the chart could show: a minimum interval, |
//| and a comparison against the last image (MT5 fires OnBookEvent    |
//| for changes that leave the visible limit book untouched).         |
//| False = the socket is broken.                                     |
//+------------------------------------------------------------------+
bool SendBook()
  {
   if(g_socket == INVALID_HANDLE || !InpStreamBook || !g_book_subscribed)
      return(true);

   long now_ms = (long)(GetMicrosecondCount() / 1000);
   if(InpBookMinIntervalMs > 0 && (now_ms - g_book_last_ms) < InpBookMinIntervalMs)
      return(true);

   MqlBookInfo book[];
   if(!MarketBookGet(_Symbol, book))
      return(true); // transient; the next book event retries

   string bids = "";
   string asks = "";
   int    n    = ArraySize(book);
   for(int i = 0; i < n; i++)
     {
      // BOOK_TYPE_*_MARKET rows are orders waiting to cross, not resting
      // liquidity at a price: they carry no level to draw.
      if(book[i].type != BOOK_TYPE_BUY && book[i].type != BOOK_TYPE_SELL)
         continue;
      if(book[i].price <= 0.0)
         continue;
      string level = StringFormat("[\"%s\",\"%s\"]",
                                  DoubleToString(book[i].price, _Digits),
                                  BookVolumeText(book[i]));
      if(book[i].type == BOOK_TYPE_BUY)
        {
         if(StringLen(bids) > 0)
            StringAdd(bids, ",");
         StringAdd(bids, level);
        }
      else
        {
         if(StringLen(asks) > 0)
            StringAdd(asks, ",");
         StringAdd(asks, level);
        }
     }

   string body = StringFormat("\"bids\":[%s],\"asks\":[%s]", bids, asks);
   if(body == g_book_last_body)
     {
      g_book_skipped++;
      return(true); // identical image: sending it would only cost bandwidth
     }
   g_book_last_body = body;
   g_book_last_ms   = now_ms;

   // The newest instant the terminal holds, by the one helper that decides
   // what that means — a book stamped by a different rule than the tape is a
   // second clock to keep in step with the first.
   long stamp = TerminalNewestTickMs();

   g_book_seq++;
   string line = StringFormat("{\"type\":\"book\",\"seq\":%I64u,\"time_ms\":%I64d,%s}",
                              g_book_seq, stamp, body);
   if(!SendLine(line))
      return(false);
   g_book_sent++;
   return(true);
  }

//+------------------------------------------------------------------+
//| Does this venue print trades for the symbol, or only quote it?    |
//|                                                                   |
//| Asks the terminal for one executed trade tick in the recent past: |
//| cheap, and decisive. An exchange-fed instrument has printed       |
//| something; a broker-quoted CFD never prints at all — its ticks    |
//| carry a bid and an ask and nothing else.                          |
//|                                                                   |
//| The window errs long on purpose: mislabelling a real tape as      |
//| quotes would chart one-unit synthetic prints with volume bars off |
//| and nothing would look broken, and a month of lookback costs the  |
//| same single tick request as a day.                                |
//+------------------------------------------------------------------+
string DetectTape()
  {
   MqlTick probe[];
   ulong from_msc = (ulong)(TimeTradeServer() - TAPE_PROBE_DAYS * 24 * 60 * 60) * 1000;
   int   found    = CopyTicks(_Symbol, probe, COPY_TICKS_TRADE, from_msc, 1);
   g_tape_trades  = (found > 0);
   string tape    = g_tape_trades ? "trades" : "quotes";
   LogEvent("BRIDGE_TAPE_DETECTED",
            StringFormat("\"tape\":\"%s\",\"probe_days\":%d,\"trade_ticks_found\":%d,"
                         "\"note\":\"quotes = the broker prices this symbol but "
                         "prints no trades\"",
                         tape, TAPE_PROBE_DAYS, (found > 0) ? found : 0));
   return(tape);
  }

//+------------------------------------------------------------------+
//| Which ticks this session streams.                                 |
//|                                                                   |
//| A printing venue is charted from `last` and `volume` alone: the   |
//| feed's mapper turns every tick without the LAST flag into a       |
//| QuoteOnly and drops it (crates/feed-mt5/src/map.rs), so asking    |
//| the terminal for one is asking it to fill a socket with data the  |
//| other end deletes on arrival.                                     |
//|                                                                    |
//| How much that saves is per broker, and on the one measured here   |
//| it saves nothing: the committed WIN$N recording (1500 live ticks, |
//| pulled with COPY_TICKS_ALL) is 100% flags=1080 — every tick        |
//| carries LAST and there are no quote-only ticks to drop. This is   |
//| still the right request: it is what the same bridge's load-older  |
//| path already asks for, it costs nothing where there is nothing to |
//| filter, and it bounds the wire on a broker that does quote        |
//| separately. A quote-only symbol is charted from the quotes        |
//| themselves, so there they are the whole tape and stay.            |
//+------------------------------------------------------------------+
uint TickFlagsWanted()
  {
   // Cast explicitly: COPY_TICKS_ALL is -1 and the parameter is a uint, so
   // leaving the conversion implicit is a compiler warning about the very
   // value that means "everything".
   return(g_tape_trades ? (uint)COPY_TICKS_TRADE : (uint)COPY_TICKS_ALL);
  }

//+------------------------------------------------------------------+
//| Session preamble + recent history, right after connecting.        |
//+------------------------------------------------------------------+
bool StartSession()
  {
   g_seq              = 0;
   g_ticks_sent       = 0;
   g_sent_at_last_msc = 0;
   // Reset beside the counter it is read against: BRIDGE_TAPE_STATS diagnoses
   // by comparing socket_writes to ticks_sent, and a writes count that
   // survived a reconnect while the ticks count restarted inverts the reading.
   g_sends            = 0;
   g_send_stalls      = 0;
   g_pump_rounds_hit  = 0;
   g_cursor_lag_ms    = 0;
   g_book_seq         = 0;
   g_book_sent        = 0;
   g_book_skipped     = 0;
   g_book_last_body   = "";

   string basis = SymbolInfoString(_Symbol, SYMBOL_BASIS);
   if(basis == "")
      basis = _Symbol;

   // Depth fields are announced only when this session can actually deliver
   // depth. Omitting them is the honest "no book here" the feed relies on to
   // tell the chart why the heatmap is empty.
   string depth = "";
   if(g_book_subscribed)
     {
      long   levels    = SymbolInfoInteger(_Symbol, SYMBOL_TICKS_BOOKDEPTH);
      double tick_size = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);
      depth = StringFormat(",\"book_levels\":%I64d", levels);
      if(tick_size > 0.0)
         StringAdd(depth, StringFormat(",\"tick_size\":\"%s\"",
                                       DoubleToString(tick_size, _Digits)));
     }

   string hello = StringFormat(
      "{\"type\":\"hello\",\"schema\":%d,\"bridge\":\"%s\",\"bridge_version\":\"%s\","
      "\"symbol\":\"%s\",\"broker_symbol\":\"%s\",\"digits\":%d,\"server_utc_offset_s\":%I64d,"
      "\"tape\":\"%s\"%s}",
      SCHEMA_VERSION, BRIDGE_NAME, BRIDGE_VERSION,
      _Symbol, basis, _Digits, ServerUtcOffsetSeconds(), DetectTape(), depth);
   // Written, not merely queued: the feed's contract is that a bridge says
   // hello *the moment it connects* (crates/feed-mt5/src/stream.rs), and it
   // gives the greeting ten seconds before dropping the connection. The next
   // statement is a CopyTicksRange that blocks while a cold terminal syncs
   // tick history from the trade server — exactly the kind of wait that would
   // spend that budget with the hello still sitting in a buffer.
   if(!SendLine(hello) || !FlushOut())
      return(false);

   // Backfill: recent ticks so the chart opens populated. An empty block is
   // still announced — the feed treats backfill_end as "history is done".
   long now_msc  = (long)TimeTradeServer() * 1000;
   long from_msc = now_msc - (long)InpBackfillMinutes * 60 * 1000;
   MqlTick history[];
   int fetched = CopyTicksRange(_Symbol, history, TickFlagsWanted(), from_msc, now_msc);
   if(fetched < 0)
     {
      LogEvent("BRIDGE_BACKFILL_FAILED",
               StringFormat("\"mql_error\":%d", GetLastError()));
      fetched = 0;
     }
   if(!SendLine(StringFormat("{\"type\":\"backfill_start\",\"count_hint\":%d}", fetched)))
      return(false);
   // History is stamped like anything else: `sent_ms` says when the bridge
   // handed the line over, which for a backfill tick is minutes after it
   // happened. The feed reads the split from live prints only, so a backfill
   // stamp is a true statement nobody measures latency with.
   long backfill_sent_ms = NowServerMs();
   for(int i = 0; i < fetched; i++)
      if(!SendTick(history[i], backfill_sent_ms))
         return(false);
   if(!SendLine("{\"type\":\"backfill_end\"}") || !FlushOut())
      return(false);

   // Position the live cursor after the last history tick.
   if(fetched > 0)
     {
      g_last_msc = history[fetched - 1].time_msc;
      g_sent_at_last_msc = 0;
      for(int i = fetched - 1; i >= 0 && history[i].time_msc == g_last_msc; i--)
         g_sent_at_last_msc++;
     }
   else
     {
      g_last_msc = now_msc;
      g_sent_at_last_msc = 0;
     }

   LogEvent("BRIDGE_SESSION_STARTED",
            StringFormat("\"backfill_ticks\":%d,\"host\":\"%s\",\"port\":%d",
                         fetched, InpHost, InpPort));
   return(true);
  }

//+------------------------------------------------------------------+
//| Try to dial the quantick feed.                                    |
//+------------------------------------------------------------------+
void TryConnect()
  {
   g_socket = SocketCreate();
   if(g_socket == INVALID_HANDLE)
     {
      LogEvent("BRIDGE_SOCKET_CREATE_FAILED",
               StringFormat("\"mql_error\":%d", GetLastError()));
      g_next_retry = TimeLocal() + InpRetrySeconds;
      return;
     }
   if(!SocketConnect(g_socket, InpHost, InpPort, 2000))
     {
      LogEvent("BRIDGE_CONNECT_FAILED",
               StringFormat("\"host\":\"%s\",\"port\":%d,\"mql_error\":%d,"
                            "\"hint\":\"is quantick running? is %s allowed in "
                            "Tools>Options>Expert Advisors?\"",
                            InpHost, InpPort, GetLastError(), InpHost));
      SocketClose(g_socket);
      g_socket = INVALID_HANDLE;
      g_next_retry = TimeLocal() + InpRetrySeconds;
      return;
     }
   // Bound every send: without this, a stalled reader can freeze the
   // terminal's main thread inside SocketSend indefinitely.
   SocketTimeouts(g_socket, (uint)InpSendTimeoutMs, (uint)InpSendTimeoutMs);
   if(!StartSession())
      Disconnect("send failed during session start");
  }

//+------------------------------------------------------------------+
//| Forward every tick newer than the cursor. MT5 ticks can share a   |
//| millisecond, so the cursor is (msc, count-at-msc), not just msc.  |
//|                                                                   |
//| One CopyTicks call returns at most PUMP_TICKS_PER_ROUND ticks, so |
//| a pass that filled its batch has not necessarily caught up: it    |
//| asks again from the advanced cursor and keeps asking while the    |
//| answers come back full. Stopping after one batch left the         |
//| remainder for the next event, and on a burst that is how a tape   |
//| falls whole seconds behind a book that never batches at all.      |
//+------------------------------------------------------------------+
void Pump()
  {
   if(g_socket == INVALID_HANDLE)
      return;
   // One stamp for the pass. Taken before the work, so a flush that blocks
   // pays for itself in wire delay rather than hiding inside terminal delay.
   long sent_ms = NowServerMs();
   int  rounds  = 0;
   while(rounds < PUMP_MAX_ROUNDS)
     {
      rounds++;
      MqlTick ticks[];
      int n = CopyTicks(_Symbol, ticks, TickFlagsWanted(), (ulong)g_last_msc,
                        PUMP_TICKS_PER_ROUND);
      if(n <= 0)
         break;
      int at_cursor_seen = 0;
      int forwarded      = 0;
      for(int i = 0; i < n; i++)
        {
         if(ticks[i].time_msc < g_last_msc)
            continue; // older than the cursor: already sent
         if(ticks[i].time_msc == g_last_msc)
           {
            at_cursor_seen++;
            if(at_cursor_seen <= g_sent_at_last_msc)
               continue; // already sent this one
            if(!SendTick(ticks[i], sent_ms))
              {
               Disconnect("send failed");
               return;
              }
            g_sent_at_last_msc++;
            forwarded++;
           }
         else
           {
            if(!SendTick(ticks[i], sent_ms))
              {
               Disconnect("send failed");
               return;
              }
            g_last_msc = ticks[i].time_msc;
            g_sent_at_last_msc = 1;
            at_cursor_seen = 0;
            forwarded++;
           }
        }
      // A short batch is the terminal saying it has nothing more; a batch that
      // forwarded nothing means every tick in it was already sent, and asking
      // again from the same cursor would return the same ticks forever.
      if(n < PUMP_TICKS_PER_ROUND || forwarded == 0)
         break;
     }
   if(rounds >= PUMP_MAX_ROUNDS)
     {
      // The loop is bounded so one pass can never own the terminal's main
      // thread. Hitting the bound is not a failure, but it is the tape
      // arriving faster than a pass can forward it, and that is the reading
      // behind a late chart — so it is logged rather than silently retried.
      g_pump_rounds_hit++;
      LogEvent("BRIDGE_PUMP_ROUND_LIMIT",
               StringFormat("\"rounds\":%d,\"ticks_per_round\":%d,\"passes\":%I64u,"
                            "\"cursor_lag_ms\":%I64d",
                            PUMP_MAX_ROUNDS, PUMP_TICKS_PER_ROUND, g_pump_rounds_hit,
                            TerminalNewestTickMs() - g_last_msc));
     }
   // Whatever this pass queued goes out now, in one write. Leaving it for the
   // next event would trade syscalls for exactly the latency this buffer
   // exists to remove.
   if(!FlushOut())
      Disconnect("send failed");
  }

//+------------------------------------------------------------------+
//| Heartbeat: liveness + a fresh server-time offset.                 |
//+------------------------------------------------------------------+
void MaybeHeartbeat()
  {
   if(g_socket == INVALID_HANDLE)
      return;
   if(TimeLocal() - g_last_heartbeat < InpHeartbeatSeconds)
      return;
   g_last_heartbeat = TimeLocal();
   // How far the bridge's cursor trails the newest tick the terminal itself
   // holds. It is the one hop no timestamp comparison downstream can see: a
   // tape late *inside* the terminal and a tape late on the wire look the same
   // from the chart, and this is what tells them apart. Sent on the heartbeat
   // rather than per tick — it is a property of the pump, not of a print.
   g_cursor_lag_ms = TerminalNewestTickMs() - g_last_msc;
   if(g_cursor_lag_ms < 0)
      g_cursor_lag_ms = 0;
   string line = StringFormat(
      "{\"type\":\"heartbeat\",\"seq_last\":%I64u,\"time_ms\":%I64d,"
      "\"ticks_sent\":%I64u,\"server_utc_offset_s\":%I64d,\"cursor_lag_ms\":%I64d}",
      g_seq, NowServerMs(), g_ticks_sent, ServerUtcOffsetSeconds(), g_cursor_lag_ms);
   if(!SendLine(line))
     {
      Disconnect("heartbeat send failed");
      return;
     }
   if(g_book_subscribed)
      LogEvent("BRIDGE_BOOK_STATS",
               StringFormat("\"images_sent\":%I64u,\"images_skipped\":%I64u",
                            g_book_sent, g_book_skipped));
   // What the tape cost the terminal's main thread. `socket_writes` well
   // below `ticks_sent` is the batching working; the two converging again
   // means the flush threshold is being reached every pass, which is the
   // shape a genuinely overloaded tape has.
   // `tick_lag_ms` is the same figure the heartbeat now carries: TimeTradeServer()
   // alone is whole seconds and would report up to 999 ms of lag that does not
   // exist, so TerminalNewestTickMs() reads the cursor's own resolution first.
   LogEvent("BRIDGE_TAPE_STATS",
            StringFormat("\"tape\":\"%s\",\"ticks_sent\":%I64u,\"socket_writes\":%I64u,"
                         "\"tick_lag_ms\":%I64d,\"send_stalls\":%I64u,\"pump_round_limits\":%I64u",
                         (g_tape_trades ? "trades" : "quotes"), g_ticks_sent, g_sends,
                         g_cursor_lag_ms, g_send_stalls, g_pump_rounds_hit));
  }

//+------------------------------------------------------------------+
int OnInit()
  {
   // OnTick is the fast path; this timer is the safety net for everything it
   // does not cover — a symbol whose chart is not receiving ticks, a terminal
   // that coalesced the callback, a reconnect. It used to be a fifth of a
   // second, which is a fifth of a second the net itself could cost.
   int pump_ms = InpPumpIntervalMs;
   if(pump_ms < PUMP_INTERVAL_MIN_MS)
      pump_ms = PUMP_INTERVAL_MIN_MS;
   if(pump_ms > PUMP_INTERVAL_MAX_MS)
      pump_ms = PUMP_INTERVAL_MAX_MS;
   EventSetMillisecondTimer(pump_ms);
   if(InpStreamBook)
     {
      // Subscribing before any connection means the terminal is already
      // maintaining the DOM when the first quantick session starts.
      g_book_subscribed = MarketBookAdd(_Symbol);
      if(g_book_subscribed)
         LogEvent("BRIDGE_BOOK_SUBSCRIBED",
                  StringFormat("\"book_levels\":%I64d,\"min_interval_ms\":%d",
                               SymbolInfoInteger(_Symbol, SYMBOL_TICKS_BOOKDEPTH),
                               InpBookMinIntervalMs));
      else
         LogEvent("BRIDGE_BOOK_SUBSCRIBE_FAILED",
                  StringFormat("\"mql_error\":%d,\"hint\":\"this symbol may have no Depth "
                               "of Market on this account; ticks still stream\"",
                               GetLastError()));
     }
   LogEvent("BRIDGE_STARTING",
            StringFormat("\"host\":\"%s\",\"port\":%d,\"backfill_minutes\":%d,\"stream_book\":%s,"
                         "\"pump_interval_ms\":%d,\"send_timeout_ms\":%d",
                         InpHost, InpPort, InpBackfillMinutes,
                         (g_book_subscribed ? "true" : "false"),
                         pump_ms, InpSendTimeoutMs));
   return(INIT_SUCCEEDED);
  }

//+------------------------------------------------------------------+
void OnDeinit(const int reason)
  {
   if(g_socket != INVALID_HANDLE)
     {
      SendLine("{\"type\":\"bye\",\"reason\":\"deinit\"}");
      FlushOut();
      SocketClose(g_socket);
      g_socket = INVALID_HANDLE;
     }
   if(g_book_subscribed)
     {
      MarketBookRelease(_Symbol);
      g_book_subscribed = false;
     }
   EventKillTimer();
   LogEvent("BRIDGE_STOPPED", StringFormat("\"deinit_reason\":%d", reason));
  }

//+------------------------------------------------------------------+
void OnTick()
  {
   Pump(); // low latency path; OnTimer is the safety net
  }

//+------------------------------------------------------------------+
//| The DOM changed: forward the new image.                           |
//+------------------------------------------------------------------+
void OnBookEvent(const string &symbol)
  {
   if(symbol != _Symbol)
      return;
   if(!SendBook() || !FlushOut())
      Disconnect("book send failed");
  }

//+------------------------------------------------------------------+
void OnTimer()
  {
   if(g_socket == INVALID_HANDLE)
     {
      if(TimeLocal() >= g_next_retry)
         TryConnect();
      return;
     }
   Pump();
   MaybeHeartbeat();
   if(!FlushOut())
      Disconnect("send failed");
  }
//+------------------------------------------------------------------+
