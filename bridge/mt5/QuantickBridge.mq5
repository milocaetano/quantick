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
#property version   "1.001"
#property description "Streams ticks to the quantick chart over a local socket"

input string InpHost             = "127.0.0.1"; // Feed host (quantick listener)
input int    InpPort             = 9100;        // Feed port
input int    InpBackfillMinutes  = 30;          // History to send on connect
input int    InpHeartbeatSeconds = 5;           // Heartbeat interval
input int    InpRetrySeconds     = 5;           // Reconnect backoff
input int    InpSendTimeoutMs    = 5000;        // Max ms one send may block
input bool   InpStreamBook       = true;        // Stream Depth of Market
input int    InpBookMinIntervalMs= 20;          // Min ms between book images (0 = every change)

#define SCHEMA_VERSION 1
#define BRIDGE_NAME    "quantick-mt5-bridge"
#define BRIDGE_VERSION "0.2.0"
// How far back to look for one executed trade before declaring a symbol
// tape-less. See DetectTape() for why this errs long.
#define TAPE_PROBE_DAYS 30

int      g_socket           = INVALID_HANDLE;
ulong    g_seq              = 0; // per-session tick sequence, from 1
ulong    g_ticks_sent       = 0;
long     g_last_msc         = 0; // cursor: newest tick time already pumped
int      g_sent_at_last_msc = 0; // ticks already sent sharing g_last_msc
datetime g_last_heartbeat   = 0;
datetime g_next_retry       = 0;

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
//| Send one NDJSON line. False = socket is broken.                   |
//|                                                                   |
//| SocketSend runs on the terminal's main thread and may write only  |
//| part of the buffer (send timeout, full OS buffer — quantick not   |
//| reading). Each attempt is bounded by SocketTimeouts (set at       |
//| connect); the remainder is retried so a slow read never corrupts  |
//| line framing, and zero progress means the socket is gone.         |
//+------------------------------------------------------------------+
bool SendLine(string payload)
  {
   if(g_socket == INVALID_HANDLE)
      return(false);
   payload += "\n";
   uchar bytes[];
   int len = StringToCharArray(payload, bytes, 0, WHOLE_ARRAY, CP_UTF8) - 1;
   if(len <= 0)
      return(true);
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
      if(wrote <= 0)
         return(false);
      sent += wrote;
     }
   return(true);
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
//| Drop the socket and schedule a reconnect attempt.                 |
//+------------------------------------------------------------------+
void Disconnect(const string why)
  {
   if(g_socket != INVALID_HANDLE)
     {
      SocketClose(g_socket);
      g_socket = INVALID_HANDLE;
     }
   g_next_retry = TimeLocal() + InpRetrySeconds;
   LogEvent("BRIDGE_DISCONNECTED",
            StringFormat("\"reason\":\"%s\",\"retry_in_s\":%d", why, InpRetrySeconds));
  }

//+------------------------------------------------------------------+
//| One tick → one NDJSON line. Prices carry exactly _Digits places.  |
//+------------------------------------------------------------------+
bool SendTick(const MqlTick &tick)
  {
   g_seq++;
   string line = StringFormat(
      "{\"type\":\"tick\",\"seq\":%I64u,\"time_ms\":%I64d,\"bid\":\"%s\",\"ask\":\"%s\","
      "\"last\":\"%s\",\"volume\":%I64u,\"flags\":%u}",
      g_seq,
      tick.time_msc,
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

   // SYMBOL_TIME_MSC is the last quote's instant; in a quiet book it stops
   // moving, so the coarser server clock takes over and the book timeline
   // never stalls behind the trade timeline.
   long stamp     = (long)SymbolInfoInteger(_Symbol, SYMBOL_TIME_MSC);
   long server_ms = (long)TimeTradeServer() * 1000;
   if(server_ms > stamp)
      stamp = server_ms;

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
   string tape    = (found > 0) ? "trades" : "quotes";
   LogEvent("BRIDGE_TAPE_DETECTED",
            StringFormat("\"tape\":\"%s\",\"probe_days\":%d,\"trade_ticks_found\":%d,"
                         "\"note\":\"quotes = the broker prices this symbol but "
                         "prints no trades\"",
                         tape, TAPE_PROBE_DAYS, (found > 0) ? found : 0));
   return(tape);
  }

//+------------------------------------------------------------------+
//| Session preamble + recent history, right after connecting.        |
//+------------------------------------------------------------------+
bool StartSession()
  {
   g_seq              = 0;
   g_ticks_sent       = 0;
   g_sent_at_last_msc = 0;
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
   if(!SendLine(hello))
      return(false);

   // Backfill: recent ticks so the chart opens populated. An empty block is
   // still announced — the feed treats backfill_end as "history is done".
   long now_msc  = (long)TimeTradeServer() * 1000;
   long from_msc = now_msc - (long)InpBackfillMinutes * 60 * 1000;
   MqlTick history[];
   int fetched = CopyTicksRange(_Symbol, history, COPY_TICKS_ALL, from_msc, now_msc);
   if(fetched < 0)
     {
      LogEvent("BRIDGE_BACKFILL_FAILED",
               StringFormat("\"mql_error\":%d", GetLastError()));
      fetched = 0;
     }
   if(!SendLine(StringFormat("{\"type\":\"backfill_start\",\"count_hint\":%d}", fetched)))
      return(false);
   for(int i = 0; i < fetched; i++)
      if(!SendTick(history[i]))
         return(false);
   if(!SendLine("{\"type\":\"backfill_end\"}"))
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
//+------------------------------------------------------------------+
void Pump()
  {
   if(g_socket == INVALID_HANDLE)
      return;
   MqlTick ticks[];
   int n = CopyTicks(_Symbol, ticks, COPY_TICKS_ALL, (ulong)g_last_msc, 4096);
   if(n <= 0)
      return;
   int at_cursor_seen = 0;
   for(int i = 0; i < n; i++)
     {
      if(ticks[i].time_msc < g_last_msc)
         continue; // older than the cursor: already sent
      if(ticks[i].time_msc == g_last_msc)
        {
         at_cursor_seen++;
         if(at_cursor_seen <= g_sent_at_last_msc)
            continue; // already sent this one
         if(!SendTick(ticks[i]))
           {
            Disconnect("send failed");
            return;
           }
         g_sent_at_last_msc++;
        }
      else
        {
         if(!SendTick(ticks[i]))
           {
            Disconnect("send failed");
            return;
           }
         g_last_msc = ticks[i].time_msc;
         g_sent_at_last_msc = 1;
         at_cursor_seen = 0;
        }
     }
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
   string line = StringFormat(
      "{\"type\":\"heartbeat\",\"seq_last\":%I64u,\"time_ms\":%I64d,"
      "\"ticks_sent\":%I64u,\"server_utc_offset_s\":%I64d}",
      g_seq, (long)TimeTradeServer() * 1000, g_ticks_sent, ServerUtcOffsetSeconds());
   if(!SendLine(line))
     {
      Disconnect("heartbeat send failed");
      return;
     }
   if(g_book_subscribed)
      LogEvent("BRIDGE_BOOK_STATS",
               StringFormat("\"images_sent\":%I64u,\"images_skipped\":%I64u",
                            g_book_sent, g_book_skipped));
  }

//+------------------------------------------------------------------+
int OnInit()
  {
   EventSetMillisecondTimer(200);
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
            StringFormat("\"host\":\"%s\",\"port\":%d,\"backfill_minutes\":%d,\"stream_book\":%s",
                         InpHost, InpPort, InpBackfillMinutes,
                         (g_book_subscribed ? "true" : "false")));
   return(INIT_SUCCEEDED);
  }

//+------------------------------------------------------------------+
void OnDeinit(const int reason)
  {
   if(g_socket != INVALID_HANDLE)
     {
      SendLine("{\"type\":\"bye\",\"reason\":\"deinit\"}");
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
   if(!SendBook())
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
  }
//+------------------------------------------------------------------+
