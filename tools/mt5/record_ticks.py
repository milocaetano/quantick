"""Record real MT5 ticks as quantick bridge-protocol NDJSON fixtures.

One-off developer/AI tool, NOT the production bridge (that is the MQL5 EA in
bridge/mt5/). It attaches to the already-running, already-logged-in MetaTrader 5
terminal via the official MetaTrader5 package — no credentials are read, stored
or written anywhere.

The output is byte-for-byte the wire format the QuantickBridge EA emits (see
bridge/mt5/PROTOCOL.md), so recorded files serve two purposes:
  1. committed test fixtures for quantick-feed-mt5 (small, real market data);
  2. replayable streams to feed the chart without a live market.

Usage:
  python tools/mt5/record_ticks.py --symbol "WIN$N" --minutes 15 \
      --max-ticks 1500 --out crates/feed-mt5/tests/fixtures/win_ticks.ndjson

Every diagnostic line this tool prints is a single JSON object with an
`event_code`, so an AI (or a human with jq) can read the run without scraping
prose.
"""
from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

SCHEMA_VERSION = 1

# MQL5 tick flags (shared vocabulary with the bridge and the Rust decoder).
TICK_FLAG_BID = 2
TICK_FLAG_ASK = 4
TICK_FLAG_LAST = 8
TICK_FLAG_VOLUME = 16
TICK_FLAG_BUY = 32
TICK_FLAG_SELL = 64


def emit(event_code: str, **fields) -> None:
    """Print one structured JSON diagnostic line to stderr."""
    print(
        json.dumps({"event_code": event_code, **fields}, ensure_ascii=False),
        file=sys.stderr,
        flush=True,
    )


def fmt_price(value: float, digits: int) -> str:
    """Format a price exactly as the bridge does: fixed `digits` decimals."""
    return f"{value:.{digits}f}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--symbol", default="WIN$N", help="MT5 symbol (default WIN$N)")
    parser.add_argument("--minutes", type=int, default=15, help="lookback window")
    parser.add_argument("--max-ticks", type=int, default=1500, help="keep the most recent N")
    parser.add_argument("--out", required=True, help="output NDJSON path")
    args = parser.parse_args()

    try:
        import MetaTrader5 as mt5
    except ImportError:
        emit("MT5_PKG_MISSING", fix="pip install MetaTrader5")
        return 2

    if not mt5.initialize():
        code, msg = mt5.last_error()
        emit(
            "MT5_ATTACH_FAILED",
            last_error_code=code,
            last_error_msg=msg,
            fix="open and log in the MetaTrader 5 terminal, then retry",
        )
        return 2

    try:
        info = mt5.symbol_info(args.symbol)
        if info is None:
            emit(
                "MT5_SYMBOL_NOT_FOUND",
                symbol=args.symbol,
                fix="check the exact symbol name in the terminal's Market Watch",
            )
            return 2
        if not info.visible:
            mt5.symbol_select(args.symbol, True)

        # Server-vs-UTC offset. Brazilian brokers stamp ticks in server wall
        # time encoded as epoch; the bridge computes this precisely via
        # TimeTradeServer()-TimeGMT(). Here we estimate from the freshest tick
        # versus real UTC now, rounded to 30 min. Honest label, not a patch.
        probe = mt5.symbol_info_tick(args.symbol)
        if probe is None:
            emit("MT5_NO_CURRENT_TICK", symbol=args.symbol)
            return 2
        now_utc = datetime.now(timezone.utc).timestamp()
        raw_offset = probe.time - now_utc
        offset_s = int(round(raw_offset / 1800.0) * 1800)
        emit(
            "MT5_OFFSET_ESTIMATED",
            server_utc_offset_s=offset_s,
            raw_offset_s=round(raw_offset, 3),
            note="ticks are stamped in server time; consumer subtracts offset",
        )

        # Ticks are requested in server-time coordinates too.
        server_now = datetime.fromtimestamp(probe.time, tz=timezone.utc)
        from_dt = server_now - timedelta(minutes=args.minutes)
        ticks = mt5.copy_ticks_range(args.symbol, from_dt, server_now, mt5.COPY_TICKS_ALL)
        if ticks is None:
            code, msg = mt5.last_error()
            emit("MT5_COPY_TICKS_FAILED", last_error_code=code, last_error_msg=msg)
            return 2
        total = len(ticks)
        kept = ticks[-args.max_ticks :] if total > args.max_ticks else ticks
        emit(
            "MT5_TICKS_FETCHED",
            symbol=args.symbol,
            window_minutes=args.minutes,
            fetched=total,
            kept=len(kept),
        )
        if total == 0:
            emit(
                "MT5_NO_TICKS_IN_WINDOW",
                hint="market closed or symbol not streaming; try a bigger --minutes",
            )
            return 3

        out = Path(args.out)
        out.parent.mkdir(parents=True, exist_ok=True)
        digits = info.digits
        trade_ticks = 0
        with out.open("w", encoding="utf-8", newline="\n") as f:
            hello = {
                "type": "hello",
                "schema": SCHEMA_VERSION,
                "bridge": "record_ticks.py",
                "bridge_version": "0.1.0",
                "symbol": args.symbol,
                "broker_symbol": args.symbol,
                "digits": digits,
                "server_utc_offset_s": offset_s,
            }
            f.write(json.dumps(hello, ensure_ascii=False) + "\n")
            for seq, t in enumerate(kept, start=1):
                flags = int(t["flags"])
                volume = int(t["volume"])
                if flags & TICK_FLAG_LAST and volume > 0:
                    trade_ticks += 1
                msg = {
                    "type": "tick",
                    "seq": seq,
                    "time_ms": int(t["time_msc"]),
                    "bid": fmt_price(float(t["bid"]), digits),
                    "ask": fmt_price(float(t["ask"]), digits),
                    "last": fmt_price(float(t["last"]), digits),
                    "volume": volume,
                    "flags": flags,
                }
                f.write(json.dumps(msg, ensure_ascii=False) + "\n")
            f.write(
                json.dumps({"type": "bye", "reason": "recording_complete"}) + "\n"
            )
        emit(
            "RECORDING_WRITTEN",
            out=str(out),
            messages=len(kept) + 2,
            trade_ticks=trade_ticks,
            quote_only_ticks=len(kept) - trade_ticks,
        )
        return 0
    finally:
        mt5.shutdown()


if __name__ == "__main__":
    sys.exit(main())
