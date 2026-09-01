"""What it actually costs to put one session on the wire.

Uses real WINV26 ticks and a real loopback socket, so the numbers are the
trader's, not a model's.
"""

import socket
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(r"C:\src\quantick-worktrees\feat-mt5-session-history\bridge\mt5")))

import MetaTrader5 as mt5  # noqa: E402

import quantick_bridge as qb  # noqa: E402

SYMBOL = "WINV26"


class Args:
    backfill_minutes = 720
    backfill_max_ticks = 4_000_000


def drain(sock, done):
    total = 0
    while True:
        chunk = sock.recv(1 << 20)
        if not chunk:
            break
        total += len(chunk)
    done.append(total)


def main():
    mt5.initialize()
    mt5.symbol_select(SYMBOL, True)
    s = object.__new__(qb.Session)
    s.symbol, s.args, s.offset_s, s.digits, s.tape = SYMBOL, Args(), 0, 0, "trades"
    s.seq = 0
    s.ticks_sent = 0
    s.earliest_ms, s.earliest_known = None, False
    s.maybe_heartbeat = lambda: None
    newest, _ = s.last_print_before(int(time.time()))
    ticks, _, _ = s.session_ticks(newest)
    print(f"session: {len(ticks)} ticks")

    # (a) serialise only, nothing else
    s.sock = None
    lines = []
    s.send = lambda m: lines.append(m)
    s.seq = 0
    t0 = time.monotonic()
    for tick in ticks:
        s.send_tick(tick, 0)
    build = time.monotonic() - t0
    print(f"  build dicts        : {build:6.2f} s")

    t0 = time.monotonic()
    import json

    blob = "".join(json.dumps(m, separators=(",", ":")) + "\n" for m in lines)
    dumps = time.monotonic() - t0
    payload = blob.encode("utf-8")
    print(f"  json.dumps each    : {dumps:6.2f} s  ({len(payload) / 1e6:.0f} MB)")

    # (b) one sendall per message, which is what ships today
    server = socket.socket()
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", 0))
    server.listen(1)
    port = server.getsockname()[1]
    done = []
    client = socket.create_connection(("127.0.0.1", port))
    conn, _ = server.accept()
    reader = threading.Thread(target=drain, args=(client, done), daemon=True)
    reader.start()

    sample = lines[:200_000]
    t0 = time.monotonic()
    for message in sample:
        conn.sendall((json.dumps(message, separators=(",", ":")) + "\n").encode("utf-8"))
    per_message = time.monotonic() - t0
    print(
        f"  per-tick sendall   : {per_message:6.2f} s for {len(sample)}"
        f"  -> {per_message * len(ticks) / len(sample):6.2f} s extrapolated"
    )

    # (c) one buffered write for the whole block
    t0 = time.monotonic()
    conn.sendall(payload)
    bulk = time.monotonic() - t0
    print(f"  one bulk sendall   : {bulk:6.2f} s for the whole {len(payload) / 1e6:.0f} MB")
    conn.close()
    client.close()
    server.close()
    mt5.shutdown()

    today = build + dumps + per_message * len(ticks) / len(sample)
    print()
    print(f"  TODAY  (per-tick)  : {today:6.2f} s to put the session on the wire")
    print(f"  BUFFERED           : {build + dumps + bulk:6.2f} s")


if __name__ == "__main__":
    main()
