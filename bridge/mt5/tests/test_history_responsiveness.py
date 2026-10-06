"""History boundaries and requests while the opening session is still arriving."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from harness import FakeTerminal, load_bridge, session_at, tick_at  # noqa: E402


class HistoryResponsiveness(unittest.TestCase):
    def setUp(self):
        self.terminal = FakeTerminal(0, 10_000)
        self.bridge = load_bridge(self.terminal)
        self.session = session_at(self.bridge, self.terminal, 10_000, opening_slice_ticks=5)

    def test_opening_slices_keep_same_millisecond_prints_whole(self):
        ticks = [tick_at(stamp) for stamp in [1000] * 6 + [2000] * 6 + [3000] * 6]
        end = len(ticks)
        blocks = []
        while end:
            start = self.session.page_start(ticks, end, 5)
            blocks.append(ticks[start:end])
            end = start
        self.assertEqual([len(block) for block in blocks], [6, 6, 6])
        for newer, older in zip(blocks, blocks[1:]):
            self.assertLess(older[-1]["time_msc"], newer[0]["time_msc"])
        self.assertEqual(sum(len(block) for block in blocks), len(ticks))

    def test_trimmed_pages_never_skip_unsent_prints_at_the_cursor(self):
        self.terminal.ticks = [tick_at(stamp) for stamp in [1000] * 3 + [2000] * 3 + [3000] * 3]
        cursor = 4000
        served = []
        for _ in range(4):
            ticks, exhausted, cursor, _ = self.session.walk_back(4, cursor)
            served[0:0] = ticks
            if exhausted:
                break
        self.assertEqual([tick["time_msc"] for tick in served], [1000] * 3 + [2000] * 3 + [3000] * 3)

    def test_an_older_request_waits_for_opening_without_repeating_the_morning(self):
        self.session.pending_opening = [[tick_at(3000)], [tick_at(2000)]]
        requests = []
        self.session.walk_back = lambda wanted, before: (requests.append((wanted, before)) or ([], False, before, 0))
        self.session.serve_load_older(10, 4000)
        self.assertEqual(requests, [])
        self.session.pump_opening()
        self.assertEqual(requests, [])
        self.session.pump_opening()
        self.assertEqual(requests, [(10, 2000)])
        self.assertEqual(sum(message["type"] == "history_end" and not message.get("opening", False) for message in self.session.sent), 1)

    def test_a_group_larger_than_the_consumer_cap_is_refused_honestly(self):
        ticks = [tick_at(1000)] * 250_001
        with self.assertRaises(self.bridge.BridgeExit):
            self.session.page_start(ticks, len(ticks), 200_000)

    def test_a_cold_older_walk_validates_a_distant_terminal_floor(self):
        claimed = 300_000_000
        self.terminal.ticks = [tick_at(claimed - 1000)]
        self.bridge.mt5.copy_ticks_from = lambda *args: [tick_at(claimed)]
        self.session.walk_back(10, claimed + 3 * 86_400_000)
        self.assertIsNone(self.session.earliest_ms)
        self.assertTrue(any(to_s * 1000 <= claimed for _, to_s, _ in self.terminal.tick_calls))


if __name__ == "__main__":
    unittest.main()
