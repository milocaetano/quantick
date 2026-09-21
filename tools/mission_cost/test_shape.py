#!/usr/bin/env python3
"""Offline tests for `shape.py`. No transcript outside the fixture, no network."""

import json
import os
import unittest

import transcripts as TRANSCRIPTS_MODULE  # noqa: F401  (import guard, see below)

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))

SHAPE = TRANSCRIPTS_MODULE.load("shape")

SHAPE_DOCUMENT = os.path.join(REPO, "docs", "quality", "velocity", "shape.json")
CEREMONY_DOCUMENT = os.path.join(REPO, "docs", "quality", "velocity", "ceremony.json")
RANKING = os.path.join(REPO, "docs", "quality", "velocity", "ranking.md")


def synthetic(frame, slope, counts):
    """Contexts whose cost is exactly the law, so a fit must recover it."""
    return [
        {"requests": n, "billable_tokens": frame * n + slope * n * n, "kind": "subagent"}
        for n in counts
    ]


class Fit(unittest.TestCase):
    def test_recovers_the_coefficients_it_was_built_from(self):
        rows = synthetic(60000, 500, [10, 25, 50, 100, 200, 400])
        found = SHAPE.fit(rows)
        self.assertAlmostEqual(found["frame_tokens_per_request"], 60000, places=0)
        self.assertAlmostEqual(found["slope_tokens_per_request_squared"], 500, places=0)
        self.assertAlmostEqual(found["r_squared"], 1.0, places=4)

    def test_reports_the_linear_only_fit_beside_it(self):
        rows = synthetic(60000, 500, [10, 25, 50, 100, 200, 400])
        found = SHAPE.fit(rows)
        # The second term has to earn its place in front of the reader, so the
        # weaker model's score is published rather than implied.
        self.assertLess(found["linear_only_r_squared"], found["r_squared"])

    def test_refuses_a_population_with_no_second_order_information(self):
        with self.assertRaises(ValueError):
            SHAPE.fit(synthetic(1000, 1, [7, 7, 7]))


class Terms(unittest.TestCase):
    def test_the_two_shares_are_the_whole_modelled_cost(self):
        rows = synthetic(60000, 500, [10, 50, 200])
        found = SHAPE.terms(rows, SHAPE.fit(rows))
        self.assertAlmostEqual(
            found["standing_frame_share"] + found["accumulation_share"], 1.0, places=3
        )
        self.assertEqual(
            found["standing_frame_tokens"] + found["accumulation_tokens"],
            found["modelled_tokens"],
        )


class Counterfactual(unittest.TestCase):
    def setUp(self):
        self.rows = synthetic(60000, 500, [10, 50, 120, 400])
        self.law = SHAPE.fit(self.rows)

    def test_a_cap_no_context_reaches_changes_nothing(self):
        found = SHAPE.counterfactual(self.rows, self.law, caps=(1000,))
        self.assertAlmostEqual(found["caps"][0]["change"], 0.0, places=3)

    def test_a_cap_that_bites_lowers_the_bill(self):
        found = SHAPE.counterfactual(self.rows, self.law, caps=(50,))
        self.assertLess(found["caps"][0]["change"], 0.0)

    def test_the_handoff_charge_is_recorded_not_hidden(self):
        found = SHAPE.counterfactual(self.rows, self.law, caps=(50,), handoff=25)
        looser = SHAPE.counterfactual(self.rows, self.law, caps=(50,), handoff=0)
        self.assertEqual(found["handoff_requests"], 25)
        self.assertGreater(found["caps"][0]["change"], looser["caps"][0]["change"])


class Concentration(unittest.TestCase):
    def test_the_top_decile_share_never_exceeds_the_whole(self):
        rows = synthetic(60000, 500, [5, 10, 20, 40, 80, 160, 320, 640, 900, 1200])
        for entry in SHAPE.concentration(rows):
            self.assertGreater(entry["share_of_tokens"], 0.0)
            self.assertLessEqual(entry["share_of_tokens"], 1.0)


class TierReading(unittest.TestCase):
    def test_reads_both_spellings_in_use(self):
        self.assertEqual(SHAPE._tier_of("Objective: x\nTier: medium\nSource: y"), "medium")
        # The goal archive's `**Tier:** `medium`.` arrives with its full stop.
        self.assertEqual(SHAPE._tier_of("Tier: medium. Three new guards"), "medium")

    def test_rejects_a_word_that_is_not_a_tier(self):
        self.assertIsNone(SHAPE._tier_of("Tier: unknown"))
        self.assertIsNone(SHAPE._tier_of("no tier here"))


class StepZero(unittest.TestCase):
    def test_reads_the_count_the_report_states(self):
        self.assertEqual(
            SHAPE._step_zero("step 0: code-review at `low`, 3 findings. The level"), 3
        )
        self.assertEqual(SHAPE._step_zero("step 0: code-review at low, 0 findings."), 0)

    def test_returns_none_when_the_report_does_not_state_one(self):
        self.assertIsNone(SHAPE._step_zero("## Findings\n\nBlockers: none."))


def _payload(body, tier_text, commits, threads):
    return json.dumps(
        {
            "data": {
                "repository": {
                    "pullRequest": {
                        "number": 42,
                        "additions": 10,
                        "deletions": 2,
                        "createdAt": "2026-09-20T10:00:00Z",
                        "bodyText": tier_text,
                        "comments": {"nodes": [{"createdAt": "2026-09-20T11:00:00Z", "body": body}]},
                        "commits": {
                            "nodes": [{"commit": {"committedDate": when}} for when in commits]
                        },
                        "reviewThreads": {
                            "nodes": [{"isResolved": one} for one in threads]
                        },
                    }
                }
            }
        }
    )


class PullCeremony(unittest.TestCase):
    BAIT = "SECRET-CONVERSATION-TEXT"

    def setUp(self):
        self.body = (
            "<!-- quantick-review-report:v1 kind=arch-review verdict=PASS -->\n"
            "# arch-review\n"
            f"step 0: code-review at low, 4 findings. {self.BAIT}\n"
        )
        self.raw = _payload(
            self.body,
            f"Tier: medium\n{self.BAIT}",
            ["2026-09-20T09:00:00Z", "2026-09-20T12:00:00Z", "2026-09-20T13:00:00Z"],
            [True, False],
        )

    def record(self):
        return SHAPE.pull_ceremony("owner", "repo", 42, runner=lambda args: self.raw)

    def test_counts_what_the_chain_produced(self):
        found = self.record()
        self.assertEqual(found["reports"], {"arch-review": 1})
        self.assertEqual(found["step_zero_findings"], 4)
        self.assertEqual(found["step_zero_rounds"], 1)
        self.assertEqual(found["tier"], "medium")
        self.assertEqual(found["churn"], 12)

    def test_splits_commits_at_the_instant_the_pull_request_opened(self):
        found = self.record()
        self.assertEqual(found["commits_before_pr"], 1)
        self.assertEqual(found["commits_after_pr"], 2)

    def test_counts_threads_and_the_ones_still_open(self):
        found = self.record()
        self.assertEqual(found["ai_review_threads"], 2)
        self.assertEqual(found["ai_review_threads_open"], 1)

    def test_no_prose_survives_into_the_record(self):
        self.assertNotIn(self.BAIT, json.dumps(self.record()))


class Figures(unittest.TestCase):
    def shape(self):
        rows = synthetic(60000, 500, [10, 50, 120, 400])
        law = SHAPE.fit(rows)
        group = {
            "contexts": len(rows),
            "requests": sum(row["requests"] for row in rows),
            "billable_tokens": sum(row["billable_tokens"] for row in rows),
            "cost_law": law,
            "terms": SHAPE.terms(rows, law),
            "concentration": SHAPE.concentration(rows),
        }
        return {
            "inputs": {"contexts": 8, "requests": 100, "billable_tokens": 999},
            "populations": {"subagent": group, "main": dict(group)},
        }

    def ceremony(self):
        return {
            "missions": 3,
            "totals": {
                "arch_review_reports": 4,
                "ai_review_reports": 5,
                "delivery_review_reports": 1,
                "step_zero_rounds": 4,
                "step_zero_findings": 6,
                "step_zero_empty_rounds": 2,
                "ai_review_threads": 7,
                "ai_review_threads_open": 0,
                "commits_before_pr": 10,
                "commits_after_pr": 30,
            },
        }

    def test_the_block_is_delimited_and_round_trips(self):
        rendered = SHAPE.figures(self.shape(), self.ceremony())
        self.assertTrue(rendered.startswith(SHAPE.FIGURES_BEGIN))
        self.assertTrue(rendered.endswith(SHAPE.FIGURES_END + "\n"))
        self.assertEqual(SHAPE.extract_figures("before\n" + rendered + "after"), rendered)

    def test_it_ends_in_one_newline_and_carries_no_carriage_return(self):
        # Markdown, not canonical JSON, so `canonical.py`'s ASCII rule does not
        # apply; what has to hold is that the bytes compared against the
        # committed document are the same bytes on either platform.
        rendered = SHAPE.figures(self.shape(), self.ceremony())
        rendered.encode("utf-8")
        self.assertNotIn("\r", rendered)
        self.assertFalse(rendered.endswith("\n\n"))


class ReportFigures(unittest.TestCase):
    """The answer to #565's prose-drift question, in working form.

    #565 lost three review rounds to figures retyped into prose out of a file
    that already held them. A checker over prose goes stale quietly; a
    generator does not, because the moment it stops matching, this fails.
    """

    def test_the_committed_report_carries_the_generated_block(self):
        for path in (SHAPE_DOCUMENT, CEREMONY_DOCUMENT, RANKING):
            if not os.path.isfile(path):
                self.skipTest(f"{path} is not committed here")
        with open(SHAPE_DOCUMENT, "r", encoding="utf-8") as stream:
            shape = json.load(stream)
        with open(CEREMONY_DOCUMENT, "r", encoding="utf-8") as stream:
            ceremony = json.load(stream)
        with open(RANKING, "r", encoding="utf-8") as stream:
            markdown = stream.read()
        carried = SHAPE.extract_figures(markdown)
        self.assertIsNotNone(carried, "the ranking carries no shape-figures block")
        self.assertEqual(
            carried,
            SHAPE.figures(shape, ceremony),
            "the ranking's figures block no longer matches the committed JSON; "
            "regenerate it with `shape.py table`",
        )


if __name__ == "__main__":
    unittest.main()
