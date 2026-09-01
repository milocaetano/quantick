"""The fake terminal every bridge test runs against.

The bridge imports `MetaTrader5`, which exists only on Windows next to an
installed terminal — so CI can never import the real thing. This stubs the
module in `sys.modules` before importing the bridge, with the documented
semantics of the calls the bridge makes, *including* the refusals that caused
the bugs those tests exist for.

It lives apart from any one test file because two suites now share it:
`test_paging.py` (candle paging and the load-older walk) and
`test_session_backfill.py` (where the opening block starts). A second copy of
a fake terminal is a second definition of what MetaTrader does, and the two
would drift the first time one of them learned something.
"""


import sys
import time
import types
from pathlib import Path

BRIDGE_DIR = Path(__file__).resolve().parent.parent

# Bars the fake terminal will serve, newest last.
M1 = 60


#: Aggressor/kind flags, as the real module exposes them. Only the bit the
#: bridge passes through matters here.
COPY_TICKS_ALL = 0
COPY_TICKS_TRADE = 1


class FakeTerminal:
    """The subset of the MetaTrader5 API the candle path touches."""

    TIMEFRAME_M1 = 1

    def __init__(self, available: int, newest_s: int, maxbars: int = 100_000) -> None:
        self.times = [newest_s - i * M1 for i in range(available)][::-1]
        self.maxbars = maxbars
        self.calls: list[tuple[int, int]] = []
        self.error = (0, "ok")
        # Pages that should fail outright, by 1-based call index.
        self.fail_on: set[int] = set()
        # Ticks the terminal holds, ascending by time_msc. Empty unless a test
        # gives it some; the candle tests never touch this half.
        self.ticks: list[dict] = []
        # Every (from_s, to_s, flags) the walk asked for.
        self.tick_calls: list[tuple[int, int, int]] = []
        # Every (from_s, count, flags) the live pump asked for.
        self.from_calls: list[tuple[int, int, int]] = []
        # Ranges answered with a failure, by 1-based call index.
        self.tick_fail_on: set[int] = set()

    def copy_ticks_range(self, _symbol, from_s, to_s, flags):
        self.tick_calls.append((int(from_s), int(to_s), int(flags)))
        if len(self.tick_calls) in self.tick_fail_on:
            self.error = (-1, "Terminal: no history")
            return None
        self.error = (0, "ok")
        lo, hi = int(from_s) * 1000, int(to_s) * 1000
        return [
            tick
            for tick in self.ticks
            if lo <= tick["time_msc"] <= hi
            and (flags != COPY_TICKS_TRADE or tick.get("is_trade", True))
        ]

    def copy_ticks_from(self, _symbol, from_s, count, flags):
        """The oldest tick held, and the live pump's own request."""
        self.error = (0, "ok")
        self.from_calls.append((int(from_s), int(count), int(flags)))
        lo = int(from_s) * 1000
        return [
            tick
            for tick in self.ticks
            if tick["time_msc"] >= lo
            and (flags != COPY_TICKS_TRADE or tick.get("is_trade", True))
        ][: int(count)]

    def copy_rates_from(self, _symbol, _timeframe, anchor, count):
        self.calls.append((int(anchor), int(count)))
        if len(self.calls) in self.fail_on:
            self.error = (-1, "Terminal: some other failure")
            return None
        if count > self.maxbars:
            # The bug: refused on the size of the request, not on the data.
            self.error = (-2, "Terminal: Invalid params")
            return None
        self.error = (0, "ok")
        upto = [t for t in self.times if t <= anchor]
        return [
            {
                "time": t,
                "open": 100.0,
                "high": 101.0,
                "low": 99.0,
                "close": 100.5,
                "tick_volume": 7,
                "real_volume": 3,
            }
            for t in upto[-count:]
        ]

    def last_error(self):
        return self.error


def load_bridge(terminal: FakeTerminal):
    """Import the bridge against `terminal`, fresh each time."""
    module = types.ModuleType("MetaTrader5")
    module.TIMEFRAME_M1 = FakeTerminal.TIMEFRAME_M1
    module.copy_rates_from = terminal.copy_rates_from
    module.copy_ticks_range = terminal.copy_ticks_range
    module.copy_ticks_from = terminal.copy_ticks_from
    module.COPY_TICKS_ALL = COPY_TICKS_ALL
    module.COPY_TICKS_TRADE = COPY_TICKS_TRADE
    module.last_error = terminal.last_error
    sys.modules["MetaTrader5"] = module
    sys.path.insert(0, str(BRIDGE_DIR))
    sys.modules.pop("quantick_bridge", None)
    import quantick_bridge  # noqa: PLC0415  (deliberately late, after the stub)

    return quantick_bridge


class FakeArgs:
    def __init__(
        self,
        rates_max_bars=200_000,
        rates_months=3,
        backfill_minutes=720,
        backfill_max_ticks=1_000_000,
    ):
        self.rates_max_bars = rates_max_bars
        self.rates_months = rates_months
        self.backfill_minutes = backfill_minutes
        self.backfill_max_ticks = backfill_max_ticks


def session_for(bridge, terminal, **args):
    """A Session wired to `terminal`, bypassing __init__'s socket."""
    session = object.__new__(bridge.Session)
    session.symbol = "WINQ26"
    session.args = FakeArgs(**args)
    session.offset_s = 0
    session.digits = 0
    session.tape = "trades"
    session.sent: list[dict] = []
    session.send = session.sent.append
    # The tick half's own state, which `__init__` would have set.
    session.seq = 0
    session.ticks_sent = 0
    session.pump_round_limits = 0
    session.earliest_ms = None
    session.earliest_known = False
    session.inbox = b""
    session.last_heartbeat = 0.0
    session.cursor_msc = 0
    session.sent_at_cursor = 0
    session.maybe_heartbeat = lambda: None
    return session


def session_at(bridge, terminal, now_s: int, **args):
    """A session whose server clock reads exactly `now_s`.

    The clock is frozen rather than offset. Deriving the offset from one
    `time.time()` while the code under test takes another leaves the two in
    different seconds whenever the first read lands late enough in one, and
    the failure surfaces as an off-by-one assertion inside the bridge — which
    is the wrong place to go looking for a flaky helper. `monotonic` is passed
    through: the heartbeat and the load-older walk measure elapsed time with
    it, and freezing that would be a different lie.
    """
    session = session_for(bridge, terminal, **args)
    bridge.time = types.SimpleNamespace(
        time=lambda: float(now_s), monotonic=time.monotonic
    )
    session.offset_s = 0
    return session


def block_ticks(session) -> list[dict]:
    """The tick lines of the backfill block a session just sent."""
    return [msg for msg in session.sent if msg["type"] == "tick"]


def tick_at(time_msc: int, last: float = 100.0, is_trade: bool = True) -> dict:
    """One terminal tick. `is_trade` decides whether COPY_TICKS_TRADE sees it."""
    return {
        "time_msc": time_msc,
        "bid": 99.0,
        "ask": 101.0,
        "last": last if is_trade else 0.0,
        "volume": 1 if is_trade else 0,
        "flags": 1080 if is_trade else 6,
        "is_trade": is_trade,
    }


NOW = 1_784_824_260
SPAN = 93 * 86_400
FROM = NOW - SPAN

FAILURES: list[str] = []


def check(name, condition, detail=""):
    if condition:
        print(f"  ok   {name}")
    else:
        FAILURES.append(f"{name}: {detail}")
        print(f"  FAIL {name}: {detail}")


#: A session's worth of prints, one every two seconds for twenty minutes.
CLOSED_SESSION_TICKS = 600
CLOSED_SESSION_STEP_MS = 2_000


def session_ending_at(last_s: int) -> list[dict]:
    """Prints ending at `last_s`, ascending, as the terminal would hold them."""
    last_ms = last_s * 1000
    return [
        tick_at(last_ms - (CLOSED_SESSION_TICKS - 1 - i) * CLOSED_SESSION_STEP_MS)
        for i in range(CLOSED_SESSION_TICKS)
    ]


def run_tests(namespace) -> int:
    """Run every `test_*` in `namespace`, newest failure list last.

    The suites are plain scripts rather than a pytest tree: the bridge has no
    test dependency and CI runs it with the interpreter it already needs for
    the bridge itself. This is the whole framework.
    """
    tests = [value for name, value in sorted(namespace.items()) if name.startswith("test_")]
    for test in tests:
        print(f"{test.__name__}:")
        test()
    if FAILURES:
        print()
        print(f"{len(FAILURES)} check(s) failed:")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print()
    print(f"all checks passed across {len(tests)} tests")
    return 0
