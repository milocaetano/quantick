"""Framing and the command back-channel: what goes out on the socket, and
what the trader asks for on the way in.

One half of the session's dealings with the socket. The other half — what the
terminal hands over — lives in the three mixins beside this one.
"""

from __future__ import annotations

import json
import select
import time

from quantick_bridge_core import (
    COMMAND_READ_BYTES,
    LOAD_OLDER_TYPE,
    MAX_COMMAND_LINE_BYTES,
    MAX_COMMAND_READS,
    SEND_BUFFER_BYTES,
    log,
    mt5,
)


class TransportMixin:
    """The session's socket: buffering, flushing, and reading commands back."""

    def send(self, message: dict) -> None:
        """Queue one line. The socket is written by `flush`.

        This used to be a `sendall` per message, which is a syscall per tick.
        On the opening block that is not a rounding error: measured over one
        real WINV26 session of 1 525 621 prints, the per-tick writes cost 47 s
        of the 62 s the whole block took, against 0.08 s for the same 207 MB
        handed over in one piece. A trader watching an empty chart for a minute
        was watching the kernel being asked the same question a million times.

        Buffering only moves *when* bytes leave, never whether they do: every
        path that can block or wait flushes first, and the loop flushes at the
        bottom of every pass.
        """
        self.outbox += json.dumps(message, separators=(",", ":")).encode("utf-8")
        self.outbox += b"\n"
        if len(self.outbox) >= SEND_BUFFER_BYTES:
            self.flush()

    def flush(self) -> None:
        """Hand whatever is queued to the socket.

        Empty is the common case and costs nothing.

        The buffer is dropped **whether or not the write succeeded**, and that
        is deliberate. This socket carries a connect timeout, which puts it in
        timeout mode, and `sendall` there is documented as not saying how much
        it managed to write before raising — so after a failure there is no
        honest way to know what the peer already has. Keeping the bytes to
        retry them is the tempting choice and the wrong one: `close` sends a
        `bye` and flushes, which would splice a repeat of up to a quarter of a
        megabyte of ticks onto whatever half-line the peer received, and the
        feed would chart the duplicates. Losing a block to a dying session is
        recoverable; silently doubling prints on the tape is not.
        """
        if not self.outbox:
            return
        payload = bytes(self.outbox)
        self.outbox.clear()
        self.sock.sendall(payload)

    def price(self, value: float) -> str:
        return f"{value:.{self.digits}f}"

    def server_now_ms(self) -> int:
        return int((time.time() + self.offset_s) * 1000)

    def pump_commands(self) -> None:
        """Serve whatever quantick asked for since the last poll.

        Called from the same loop that pumps ticks, so the terminal is touched
        from one thread only. The read never blocks: `select` with a zero
        timeout answers "is there anything?", and a quiet socket costs one
        syscall per pass.
        """
        for request in self.read_requests():
            if request.get("type") != LOAD_OLDER_TYPE:
                log("BRIDGE_UNKNOWN_COMMAND", got=str(request.get("type")), action="ignore")
                continue
            try:
                count = int(request["count"])
                before_ms = int(request["before_ms"])
            except (KeyError, TypeError, ValueError):
                log("BRIDGE_MALFORMED_COMMAND", got=json.dumps(request), action="ignore")
                continue
            self.serve_load_older(count, before_ms)

    def read_requests(self) -> list[dict]:
        """Every complete NDJSON command quantick has sent, decoded.

        Anything that does not survive the trip is skipped and logged rather
        than taken as a broken connection: the inbound direction carries a
        trader's clicks, and one unreadable click must not cost the session its
        tick stream. Two bounds keep that promise from being turned against the
        bridge — a line that never ends, and a peer that never stops — because
        the socket accepts whatever dialled the port.
        """
        requests: list[dict] = []
        reads = 0
        while reads < MAX_COMMAND_READS and select.select([self.sock], [], [], 0)[0]:
            reads += 1
            chunk = self.sock.recv(COMMAND_READ_BYTES)
            if not chunk:
                # quantick closed its side. The next outbound write raises and
                # the caller reconnects; there is nothing to decide here.
                break
            self.inbox += chunk
            while b"\n" in self.inbox:
                line, self.inbox = self.inbox.split(b"\n", 1)
                text = line.strip()
                if not text:
                    continue
                try:
                    decoded = json.loads(text)
                except (UnicodeDecodeError, json.JSONDecodeError) as error:
                    log("BRIDGE_UNDECODABLE_COMMAND", error=str(error), action="skip")
                    continue
                # `json.loads` accepts bare numbers, strings, lists and null.
                # None of them has `.get`, and reaching for it would raise past
                # every `except` in the session loop and end the bridge.
                if not isinstance(decoded, dict):
                    log(
                        "BRIDGE_UNDECODABLE_COMMAND",
                        error=f"expected an object, got {type(decoded).__name__}",
                        action="skip",
                    )
                    continue
                requests.append(decoded)
            if len(self.inbox) > MAX_COMMAND_LINE_BYTES:
                log(
                    "BRIDGE_COMMAND_LINE_TOO_LONG",
                    bytes_held=len(self.inbox),
                    cap=MAX_COMMAND_LINE_BYTES,
                    action="drop_buffer",
                )
                self.inbox = b""
        return requests

    def close(self, reason: str) -> None:
        if self.book_subscribed:
            mt5.market_book_release(self.symbol)
            self.book_subscribed = False
        try:
            self.send({"type": "bye", "reason": reason})
            self.flush()
        except OSError:
            pass
