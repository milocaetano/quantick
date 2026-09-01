"""Time a real bridge process delivering a real session onto a real socket.

Stands in for quantick's listener: binds a port, launches
`bridge/mt5/quantick_bridge.py` exactly as the app's supervisor does, and
records when each part of the opening block arrives. Nothing is stubbed, so
the numbers are what the trader waits through.

    python .claude/evidence/mt5-session-history/open_timing.py WINV26
"""

import json
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BRIDGE = ROOT / "bridge/mt5/quantick_bridge.py"
SYMBOL = sys.argv[1] if len(sys.argv) > 1 else "WINV26"
OFFSET_S = sys.argv[2] if len(sys.argv) > 2 else "-10800"


def main() -> int:
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    port = listener.getsockname()[1]
    listener.settimeout(120)

    started = time.monotonic()
    proc = subprocess.Popen(
        [
            sys.executable,
            str(BRIDGE),
            "--symbol",
            SYMBOL,
            "--port",
            str(port),
            "--no-book",
            # The bridge refuses to guess a server clock, and B3 is shut while
            # this is being measured. -10800 is what its own hint names.
            "--utc-offset-s",
            OFFSET_S,
        ],
        stderr=None,
    )
    try:
        conn, _ = listener.accept()
        conn.settimeout(180)
        marks: dict[str, float] = {"connected": time.monotonic() - started}
        ticks = 0
        opened = 0
        slices = 0
        first_tick_ms = None
        last_tick_ms = None
        buffer = b""
        bytes_in = 0
        while True:
            chunk = conn.recv(1 << 20)
            if not chunk:
                break
            bytes_in += len(chunk)
            buffer += chunk
            *lines, buffer = buffer.split(b"\n")
            for raw in lines:
                if not raw:
                    continue
                message = json.loads(raw)
                kind = message["type"]
                if kind == "tick":
                    ticks += 1
                    stamp_ms = message["time_ms"]
                    if first_tick_ms is None:
                        marks["first tick"] = time.monotonic() - started
                        first_tick_ms = last_tick_ms = stamp_ms
                    # Slices arrive newest-first, so the oldest and newest of
                    # the session are both found by comparing, not by position.
                    first_tick_ms = min(first_tick_ms, stamp_ms)
                    last_tick_ms = max(last_tick_ms, stamp_ms)
                elif kind in ("hello", "backfill_start", "backfill_end"):
                    marks[kind] = time.monotonic() - started
                    if kind == "backfill_end":
                        marks["chart can paint"] = marks[kind]
                        opened = ticks
                elif kind == "history_end" and message.get("opening"):
                    slices += 1
                    if message.get("remaining") == 0:
                        marks["whole session in"] = time.monotonic() - started
                        raise SystemExit(
                            report(
                                marks, ticks, first_tick_ms, last_tick_ms,
                                bytes_in, opened, slices,
                            )
                        )
    finally:
        proc.terminate()
        proc.wait(timeout=10)
    return 1


def report(marks, ticks, first_ms, last_ms, bytes_in, opened, slices) -> int:
    import datetime as dt

    def stamp(ms):
        return dt.datetime.fromtimestamp(ms / 1000, dt.UTC).strftime("%H:%M:%S.%f")[:-3]

    print(f"symbol            : {SYMBOL}")
    print(f"whole session     : {ticks} ticks, {bytes_in / 1e6:.0f} MB")
    print(f"  chart opened on : {opened} ticks, then {slices} slices behind it")
    print(f"  from            : {stamp(first_ms)}")
    print(f"  to              : {stamp(last_ms)}")
    print(f"  span            : {(last_ms - first_ms) / 3_600_000:.2f} h")
    print()
    for name in (
        "connected", "hello", "backfill_start", "first tick",
        "chart can paint", "whole session in",
    ):
        if name in marks:
            print(f"  {name:<15}: {marks[name]:6.2f} s after launch")
    return 0


if __name__ == "__main__":
    sys.exit(main())
