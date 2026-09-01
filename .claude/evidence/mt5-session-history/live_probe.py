"""Run the real bridge's opening-block logic against the real terminal.

No socket: a Session is built the way the test harness builds one, but with the
genuine MetaTrader5 module behind it, so what is measured is the code that
ships. Prints the old clock-window answer beside the new session-anchored one.
"""

import datetime as dt
import sys
import time
import types
from pathlib import Path

BRIDGE = Path(r"C:\src\quantick-worktrees\feat-mt5-session-history\bridge\mt5")
sys.path.insert(0, str(BRIDGE))

import MetaTrader5 as mt5  # noqa: E402

import quantick_bridge as qb  # noqa: E402

SYMBOL = sys.argv[1] if len(sys.argv) > 1 else "WINV26"


def stamp(ms):
    if ms is None:
        return "none"
    return dt.datetime.fromtimestamp(ms / 1000, dt.UTC).strftime("%Y-%m-%d %H:%M:%S.%f")[:-3]


class Args:
    backfill_minutes = 720
    backfill_max_ticks = 4_000_000
    rates_months = 0
    rates_max_bars = 200_000


def build_session():
    s = object.__new__(qb.Session)
    s.symbol = SYMBOL
    s.args = Args()
    s.offset_s = 0
    s.digits = 0
    s.tape = "trades"
    s.seq = 0
    s.ticks_sent = 0
    s.earliest_ms = None
    s.earliest_known = False
    s.cursor_msc = 0
    s.sent_at_cursor = 0
    s.maybe_heartbeat = lambda: None
    s.sent = []
    s.send = s.sent.append
    return s


def main() -> int:
    if not mt5.initialize():
        print("initialize failed:", mt5.last_error())
        return 1
    mt5.symbol_select(SYMBOL, True)
    now_s = int(time.time())
    print(f"symbol            : {SYMBOL}")
    print(f"host clock        : {dt.datetime.now().isoformat(timespec='seconds')} local")

    # --- what the old code would have sent -------------------------------
    t0 = time.monotonic()
    old = mt5.copy_ticks_range(SYMBOL, now_s - 720 * 60, now_s, mt5.COPY_TICKS_TRADE)
    old_ms = time.monotonic() - t0
    old_n = 0 if old is None else len(old)
    print()
    print("BEFORE - the rolling 720-minute clock window")
    print(f"  ticks           : {old_n}")
    print(f"  oldest          : {stamp(int(old[0]['time_msc'])) if old_n else 'none'}")
    print(f"  newest          : {stamp(int(old[-1]['time_msc'])) if old_n else 'none'}")
    print(f"  fetch           : {old_ms * 1000:.0f} ms")
    if old_n > 1_000_000:
        print(f"  then capped to  : 1000000 (oldest {old_n - 1_000_000} dropped silently)")

    # --- what the branch sends -------------------------------------------
    session = build_session()
    t0 = time.monotonic()
    newest_ms, searched = session.last_print_before(now_s)
    find_ms = time.monotonic() - t0
    if newest_ms is None:
        print("\nAFTER - no prints at all for this symbol")
        mt5.shutdown()
        return 0
    t0 = time.monotonic()
    ticks, windows, stopped_on = session.session_ticks(newest_ms)
    walk_ms = time.monotonic() - t0
    print()
    print("AFTER - the session the tape is in")
    print(f"  ticks           : {len(ticks)}")
    print(f"  oldest          : {stamp(int(ticks[0]['time_msc'])) if len(ticks) else 'none'}")
    print(f"  newest          : {stamp(int(ticks[-1]['time_msc'])) if len(ticks) else 'none'}")
    print(f"  windows walked  : {windows} (search took {searched})")
    print(f"  stopped on      : {stopped_on}")
    print(f"  find + walk     : {find_ms * 1000:.0f} ms + {walk_ms * 1000:.0f} ms")

    if len(ticks) and old_n:
        gained_ms = int(old[0]["time_msc"]) - int(ticks[0]["time_msc"])
        print()
        print(f"  RECOVERED       : {gained_ms / 3_600_000:.2f} h of the session,"
              f" {len(ticks) - old_n} extra prints")
    mt5.shutdown()
    return 0


if __name__ == "__main__":
    sys.exit(main())
