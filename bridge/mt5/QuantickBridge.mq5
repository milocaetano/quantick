//+------------------------------------------------------------------+
//| QuantickBridge.mq5 — stream this chart's ticks to quantick.       |
//|                                                                    |
//| Attach this Expert Advisor to a chart of the symbol you want in    |
//| quantick (e.g. WIN$N). It dials the quantick feed's local TCP      |
//| listener and streams newline-delimited JSON: a hello, a backfill   |
//| block from CopyTicks, then live ticks and heartbeats. The protocol |
//| contract lives in PROTOCOL.md next to this file; the Rust decoder  |
//| in crates/feed-mt5 is its executable counterpart.                  |
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

#define SCHEMA_VERSION 1
#define BRIDGE_NAME    "quantick-mt5-bridge"
#define BRIDGE_VERSION "0.1.1"

int      g_socket           = INVALID_HANDLE;
ulong    g_seq              = 0; // per-session tick sequence, from 1
ulong    g_ticks_sent       = 0;
long     g_last_msc         = 0; // cursor: newest tick time already pumped
int      g_sent_at_last_msc = 0; // ticks already sent sharing g_last_msc
datetime g_last_heartbeat   = 0;
datetime g_next_retry       = 0;

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
//| Session preamble + recent history, right after connecting.        |
//+------------------------------------------------------------------+
bool StartSession()
  {
   g_seq              = 0;
   g_ticks_sent       = 0;
   g_sent_at_last_msc = 0;

   string basis = SymbolInfoString(_Symbol, SYMBOL_BASIS);
   if(basis == "")
      basis = _Symbol;

   string hello = StringFormat(
      "{\"type\":\"hello\",\"schema\":%d,\"bridge\":\"%s\",\"bridge_version\":\"%s\","
      "\"symbol\":\"%s\",\"broker_symbol\":\"%s\",\"digits\":%d,\"server_utc_offset_s\":%I64d}",
      SCHEMA_VERSION, BRIDGE_NAME, BRIDGE_VERSION,
      _Symbol, basis, _Digits, ServerUtcOffsetSeconds());
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
      Disconnect("heartbeat send failed");
  }

//+------------------------------------------------------------------+
int OnInit()
  {
   EventSetMillisecondTimer(200);
   LogEvent("BRIDGE_STARTING",
            StringFormat("\"host\":\"%s\",\"port\":%d,\"backfill_minutes\":%d",
                         InpHost, InpPort, InpBackfillMinutes));
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
   EventKillTimer();
   LogEvent("BRIDGE_STOPPED", StringFormat("\"deinit_reason\":%d", reason));
  }

//+------------------------------------------------------------------+
void OnTick()
  {
   Pump(); // low latency path; OnTimer is the safety net
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
