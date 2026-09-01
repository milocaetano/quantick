"""When bytes actually leave the bridge.

Outbound lines are buffered rather than written one `sendall` at a time. That
is worth 47 of the 62 seconds a real WINV26 session took to reach the chart —
and it is also the kind of change that breaks liveness quietly, because
everything still *arrives*, just later than something was counting on.

So these are the tests for *when*, not for what. Every one of them is a case
where a line sitting in a buffer would be read by the other end as the bridge
having died.

Run directly (`python bridge/mt5/tests/test_wire.py`) or through
`cargo test -p quantick-feed-mt5 --test bridge_paging`, which discovers and
runs every suite in this folder so the four checks cover them.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from harness import (  # noqa: E402  (deliberately after the path insert)
    FakeTerminal,
    check,
    load_bridge,
    run_tests,
    session_at,
    tick_at,
)

NOW = 1_784_824_260


class FakeSocket:
    """A socket that records each handover instead of making one."""

    def __init__(self) -> None:
        #: One entry per `sendall`, which is the thing under test.
        self.writes: list[bytes] = []

    def sendall(self, payload: bytes) -> None:
        self.writes.append(payload)

    def lines(self) -> list[dict]:
        blob = b"".join(self.writes).decode("utf-8")
        return [json.loads(line) for line in blob.splitlines() if line]

    def types(self) -> list[str]:
        return [message["type"] for message in self.lines()]


def wired(bridge, term, now_s: int, **args):
    """A session that writes through a real `send`, onto a fake socket."""
    session = session_at(bridge, term, now_s, **args)
    sock = FakeSocket()
    session.sock = sock
    session.outbox = bytearray()
    # `session_at` replaces `send` with a list append; put the real one back,
    # because the buffering is exactly what is being measured.
    session.send = lambda message: bridge.Session.send(session, message)
    session.flush = lambda: bridge.Session.flush(session)
    # `session_for` stubs the heartbeat out so the backfill tests can assert
    # silence. Restoring it is the point here.
    session.maybe_heartbeat = lambda: bridge.Session.maybe_heartbeat(session)
    return session, sock


def test_a_block_leaves_in_far_fewer_writes_than_it_has_ticks():
    """The whole point: a syscall per tick is what made a session take a
    minute, so the count of handovers must not track the count of prints."""
    term = FakeTerminal(0, NOW)
    term.ticks = [tick_at((NOW - 3600 + i) * 1000) for i in range(3600)]
    bridge = load_bridge(term)
    session, sock = wired(bridge, term, NOW, backfill_minutes=720)
    session.backfill()

    ticks_sent = sum(1 for message in sock.lines() if message["type"] == "tick")
    check("the whole block went out", ticks_sent == 3600, ticks_sent)
    check(
        "but not one write per tick",
        len(sock.writes) < ticks_sent // 10,
        (len(sock.writes), ticks_sent),
    )


def test_the_hello_leaves_before_the_block_is_built():
    """The feed reads a silent socket as a bridge that failed to start, and
    building the opening block takes seconds. The hello cannot wait for it."""
    term = FakeTerminal(0, NOW)
    term.ticks = [tick_at((NOW - 3600 + i) * 1000) for i in range(3600)]
    bridge = load_bridge(term)
    session, sock = wired(bridge, term, NOW, backfill_minutes=720)

    session.send({"type": "hello", "schema": 1})
    session.flush()
    check(
        "the hello is on the wire on its own",
        sock.types() == ["hello"],
        sock.types(),
    )

    before = len(sock.writes)
    session.backfill()
    check(
        "and the block follows it",
        len(sock.writes) > before,
        (before, len(sock.writes)),
    )


def test_a_heartbeat_does_not_wait_in_the_buffer():
    """`walk_back` beats from inside its own search, precisely because that
    search does not return to the loop for seconds. A buffered heartbeat is
    the silence it exists to prevent."""
    term = FakeTerminal(0, NOW)
    term.ticks = [tick_at((NOW - 10 + i) * 1000) for i in range(10)]
    bridge = load_bridge(term)
    session, sock = wired(bridge, term, NOW)
    session.last_heartbeat = 0.0
    session.book_subscribed = False
    session.book_sent = session.book_skipped = 0
    session.args.heartbeat_seconds = 0.0

    session.maybe_heartbeat()
    check(
        "the heartbeat is on the wire without anything flushing it",
        sock.types() == ["heartbeat"],
        sock.types(),
    )


def test_an_empty_flush_never_touches_the_socket():
    """The loop flushes on every pass, and most passes produce nothing. That
    must cost no syscall at all."""
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session, sock = wired(bridge, term, NOW)

    for _ in range(10):
        session.flush()
    check("an idle loop writes nothing", sock.writes == [], sock.writes)


def test_a_failed_write_does_not_lose_the_buffer_twice():
    """`sendall` is all-or-raise. The buffer is dropped when the bytes are
    gone, so a raise leaves them queued rather than silently discarded."""
    term = FakeTerminal(0, NOW)
    bridge = load_bridge(term)
    session, sock = wired(bridge, term, NOW)

    def refuse(_payload):
        raise OSError("connection reset")

    sock.sendall = refuse
    session.send({"type": "tick", "seq": 1})
    raised = False
    try:
        session.flush()
    except OSError:
        raised = True
    check("the failure is not swallowed", raised, raised)
    check(
        "and the line is still queued rather than lost",
        b'"seq":1' in bytes(session.outbox),
        bytes(session.outbox),
    )


def main() -> int:
    return run_tests(globals())


if __name__ == "__main__":
    sys.exit(main())
