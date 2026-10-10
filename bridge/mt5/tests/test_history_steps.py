"""Older history yields terminal work to the live loop without splitting a block."""

import json
import sys
import types
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
from harness import FakeTerminal, load_bridge, patch_bridge, session_at, tick_at  # noqa: E402


class HistorySteps(unittest.TestCase):
    def setUp(self):
        self.terminal = FakeTerminal(0, 10_000)
        self.terminal.ticks = [tick_at(stamp) for stamp in range(1_000_000, 2_000_000, 1000)]
        self.bridge = load_bridge(self.terminal)
        self.session = session_at(self.bridge, self.terminal, 10_000)
        self.session.earliest_known = True
        self.session.earliest_ms = 1_000_000

    def request(self, count=800, before=2_000_000):
        self.session.read_requests = lambda: [{"type": "load_older", "count": count, "before_ms": before}]
        self.session.pump_commands()
        self.session.read_requests = lambda: []

    def finish(self):
        for _ in range(40):
            if self.session.pending_history_request is None:
                return
            calls = len(self.terminal.tick_calls) + len(self.terminal.from_calls)
            self.session.pump_history()
            self.assertLessEqual(len(self.terminal.tick_calls) + len(self.terminal.from_calls) - calls, 1)
        self.fail("history did not finish within the terminal window budget")

    def test_commands_admit_without_searching_and_do_not_replace_a_pending_request(self):
        self.session.pending_opening = [[tick_at(2_500_000)], [tick_at(2_000_000)]]
        self.request(before=3_000_000)
        self.request(count=1, before=1_100_000)
        self.assertEqual(self.terminal.tick_calls, [])
        self.assertEqual(self.session.pending_history_request, (800, 2_000_000))
        self.session.pump_history()
        self.assertEqual(self.session.sent, [])
        self.session.pump_opening()
        self.session.pump_opening()
        self.assertEqual(self.terminal.tick_calls, [])
        self.finish()
        ends = [message for message in self.session.sent if message["type"] == "history_end"]
        self.assertEqual(len(ends), 3)
        self.assertEqual(sum(not message.get("opening", False) for message in ends), 1)
        self.assertEqual(ends[-1]["scanned_to_ms"], 1_200_000)
        self.request(count=100, before=1_200_000)
        self.session.pump_history()
        self.request(count=1, before=1_100_000)
        self.assertEqual(self.session.pending_history_request, (100, 1_200_000))
        self.finish()
        self.assertEqual(sum(message["type"] == "history_end" and not message.get("opening", False)
                             for message in self.session.sent), 2)

    def test_cold_floor_validation_and_each_window_use_separate_turns(self):
        self.session.earliest_known = False
        self.session.earliest_ms = None
        self.request()
        self.assertEqual(self.terminal.tick_calls + self.terminal.from_calls, [])
        self.finish()
        self.assertEqual(len(self.terminal.from_calls), 1)
        self.assertGreaterEqual(len(self.terminal.tick_calls), 4)
        self.assertEqual(sum(message["type"] == "tick" for message in self.session.sent), 800)

    def test_block_is_contiguous_and_matches_the_synchronous_wire_bytes(self):
        expected = session_at(self.bridge, self.terminal, 10_000)
        expected.earliest_known, expected.earliest_ms = True, 1_000_000
        writes = [[], []]
        for session, output in zip([expected, self.session], writes):
            del session.send
            session.sock = types.SimpleNamespace(sendall=output.append)
        expected.serve_load_older(800, 2_000_000)
        expected.flush()
        self.request()
        self.finish()
        self.session.flush()
        self.assertEqual(b"".join(writes[0]), b"".join(writes[1]))
        messages = [json.loads(line) for line in b"".join(writes[1]).splitlines()]
        self.assertEqual(messages[0], {"type": "history_start"})
        self.assertEqual([message["time_ms"] for message in messages[1:-1]], list(range(1_200_000, 2_000_000, 1000)))
        self.assertEqual([message["seq"] for message in messages[1:-1]], list(range(1, 801)))
        self.assertTrue(all(message["type"] == "tick" for message in messages[1:-1]))

    def test_empty_exhausted_error_and_group_boundaries_match_the_walk(self):
        cases = [([], None, set()), ([], 1_000_000, set()),
                 (self.terminal.ticks, 1_000_000, {1}),
                 (self.terminal.ticks, 1_000_000, {2}),
                 ([tick_at(stamp) for stamp in [1000] * 3 + [2000] * 3 + [3000] * 3], 1000, set())]
        for ticks, floor, failures in cases:
            with self.subTest(floor=floor, failures=failures, count=len(ticks)):
                self.terminal.ticks = ticks
                self.terminal.tick_calls.clear()
                self.terminal.tick_fail_on = failures
                self.session.sent.clear()
                self.session.earliest_ms = floor
                before = 4000 if len(ticks) == 9 else 2_000_000
                count = 4 if len(ticks) == 9 else 800
                expected, exhausted, scanned, _ = self.session.walk_back(count, before)
                self.terminal.tick_calls.clear()
                self.request(count, before)
                self.finish()
                actual = [message["time_ms"] for message in self.session.sent if message["type"] == "tick"]
                self.assertEqual(actual, [tick["time_msc"] for tick in expected])
                self.assertEqual(self.session.sent[-1], {"type": "history_end", "exhausted": exhausted, "scanned_to_ms": scanned})

    def test_an_empty_unknown_floor_stops_at_the_window_budget_without_claiming_exhaustion(self):
        self.terminal.ticks = []
        self.session.earliest_ms = None
        self.request(before=1_000_000_000_000)
        self.finish()
        self.assertEqual(len(self.terminal.tick_calls), 24)
        self.assertFalse(self.session.sent[-1]["exhausted"])
        self.assertLess(self.session.sent[-1]["scanned_to_ms"], 1_000_000_000_000)

    def test_command_eof_stops_searching_before_another_window(self):
        self.request()
        self.session.pump_history()
        calls = len(self.terminal.tick_calls)
        del self.session.read_requests
        self.session.sock = types.SimpleNamespace(recv=lambda count: b"")
        with patch.object(self.bridge.select, "select", return_value=([self.session.sock], [], [])):
            with self.assertRaises(ConnectionError):
                self.session.pump_commands()
        self.session.book_subscribed = False
        self.session.close("socket error")
        self.session.pump_history()
        self.assertEqual(len(self.terminal.tick_calls), calls)
        self.assertFalse(any(message["type"] == "history_start" for message in self.session.sent))

    def test_closing_discards_an_unfinished_walk_and_a_fresh_session_has_no_reply(self):
        self.request()
        self.session.pump_history()
        self.session.book_subscribed = False
        self.session.close("socket error")
        self.assertIsNone(self.session.pending_history_request)
        self.assertIsNone(self.session.history_steps)
        self.session.pump_history()
        self.assertEqual([message["type"] for message in self.session.sent], ["bye"])
        fresh = session_at(self.bridge, self.terminal, 10_000)
        fresh.pump_history()
        self.assertEqual(fresh.sent, [])

    def test_run_session_pumps_real_live_ticks_book_and_heartbeat_between_windows(self):
        session, bridge, terminal = self.session, self.bridge, self.terminal
        clock = [100.0]
        patch_bridge("time", types.SimpleNamespace(time=lambda: 10_000 + clock[0],
                     monotonic=lambda: clock[0], perf_counter=lambda: clock[0], sleep=lambda seconds: clock.__setitem__(0, clock[0] + seconds)))
        session.args.host, session.args.port = "127.0.0.1", 19619
        session.args.symbol = session.symbol
        session.args.tick_poll_ms = session.args.book_poll_ms = 10
        session.args.book_min_interval_ms = 0
        session.args.heartbeat_seconds = 0.1
        session.book_subscribed = True
        session.last_book_body = None
        session.last_book_ms = 0.0
        session.book_seq = session.book_sent = session.book_skipped = 0
        session.cursor_msc = 10_000_000
        session.start = lambda offset: None
        del session.maybe_heartbeat
        bridge.mt5.BOOK_TYPE_BUY, bridge.mt5.BOOK_TYPE_SELL = 2, 1
        bridge.mt5.market_book_release = lambda symbol: None
        bridge.mt5.symbol_info_tick = lambda symbol: None
        bridge.mt5.market_book_get = lambda symbol: [types.SimpleNamespace(
            type=2, price=100, volume_dbl=1 + len(terminal.tick_calls), volume=1)]
        ranges = terminal.copy_ticks_range

        def delayed_range(*args):
            result = ranges(*args)
            clock[0] += 0.25
            terminal.ticks.append(tick_at(10_000_000 + len(terminal.tick_calls)))
            return result

        bridge.mt5.copy_ticks_range = delayed_range
        first = [True]

        def commands():
            if any(message["type"] == "history_end" for message in session.sent):
                raise KeyboardInterrupt
            if first:
                first.pop()
                return [{"type": "load_older", "count": 800, "before_ms": 2_000_000}]
            return []

        session.read_requests = commands
        connection = types.SimpleNamespace(setsockopt=lambda *args: None)
        with patch.object(bridge.socket, "create_connection") as connect, patch.object(bridge, "Session", return_value=session):
            connect.return_value.__enter__.return_value = connection
            with self.assertRaises(KeyboardInterrupt):
                bridge.run_session(session.args, 0)
        start = next(index for index, message in enumerate(session.sent) if message["type"] == "history_start")
        before = session.sent[:start]
        live = [message for message in before if message["type"] == "tick"]
        self.assertEqual([message["time_ms"] for message in live], [10_000_001, 10_000_002, 10_000_003])
        self.assertGreaterEqual(sum(message["type"] == "book" for message in before), 4)
        self.assertGreaterEqual(sum(message["type"] == "heartbeat" for message in before), 3)
        end = next(index for index, message in enumerate(session.sent) if message["type"] == "history_end")
        self.assertEqual(end - start - 1, 800)
        self.assertTrue(all(message["type"] == "tick" and message["time_ms"] < 2_000_000 for message in session.sent[start + 1:end]))
        self.assertEqual(session.cursor_msc, 10_000_003)


if __name__ == "__main__":
    unittest.main()
