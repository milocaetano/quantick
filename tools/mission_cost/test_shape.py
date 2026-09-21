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

    def test_the_handoff_is_charged_once_per_extra_context_not_once_per_piece(self):
        """The bug #572's review found: `k` handoffs charged where `k - 1` happen.

        One context of 100 requests capped at 50 becomes two contexts and one
        handoff, so the modelled total is exactly two pieces of 54 requests --
        not two of 58.
        """
        rows = [{"requests": 100, "billable_tokens": 0, "kind": "a"}]
        laws = {"a": {"frame_tokens_per_request": 1000,
                      "slope_tokens_per_request_squared": 0}}
        found = SHAPE.modelled(rows, laws, cap=50, handoff=8)
        self.assertAlmostEqual(found, 2 * 54 * 1000, places=6)

    def test_a_cap_no_context_reaches_charges_no_handoff_at_all(self):
        rows = [{"requests": 30, "billable_tokens": 0, "kind": "a"}]
        laws = {"a": {"frame_tokens_per_request": 1000,
                      "slope_tokens_per_request_squared": 0}}
        self.assertAlmostEqual(
            SHAPE.modelled(rows, laws, cap=50, handoff=8), 30 * 1000, places=6
        )


class Levers(unittest.TestCase):
    """The cross-population policy table, and the law each row is priced on.

    #572's architecture review found a headline row priced on the subagent law
    while the baseline it was compared against used each population's own. The
    fix is that the law is an output, not an assumption, and these tests hold
    it there.
    """

    def setUp(self):
        self.laws = {
            "subagent": {"frame_tokens_per_request": 60000,
                         "slope_tokens_per_request_squared": 500},
            "main": {"frame_tokens_per_request": 150000,
                     "slope_tokens_per_request_squared": 200},
        }
        self.rows = (
            [{"requests": n, "billable_tokens": 0, "kind": "subagent"}
             for n in (20, 120, 400)]
            + [{"requests": n, "billable_tokens": 0, "kind": "main"}
               for n in (50, 300)]
        )

    def test_the_baseline_prices_every_context_on_its_own_law(self):
        found = SHAPE.levers(self.rows, self.laws)
        expected = sum(
            self.laws[row["kind"]]["frame_tokens_per_request"] * row["requests"]
            + self.laws[row["kind"]]["slope_tokens_per_request_squared"]
            * row["requests"] ** 2
            for row in self.rows
        )
        self.assertEqual(found["baseline_modelled_tokens"], round(expected))
        self.assertEqual(found["baseline_law"], "each population's own")

    def test_every_row_says_which_law_it_used(self):
        found = SHAPE.levers(self.rows, self.laws)
        by_name = {row["lever"]: row for row in found["levers"]}
        for name in ("L1", "L1+L2", "L1+L3", "L1+L2+L3"):
            self.assertEqual(by_name[name]["law"], "subagent")
        for name in ("L2", "L3"):
            self.assertEqual(by_name[name]["law"], "each population's own")

    def test_a_row_that_does_not_cap_is_reproducible_from_the_baseline_law(self):
        found = SHAPE.levers(self.rows, self.laws, trim=10000)
        trimmed = next(r for r in found["levers"] if r["lever"] == "L2")
        expected = found["baseline_modelled_tokens"] - 10000 * sum(
            row["requests"] for row in self.rows
        )
        self.assertEqual(trimmed["modelled_tokens"], round(expected))

    def test_stacking_never_costs_more_than_the_lever_alone(self):
        found = SHAPE.levers(self.rows, self.laws)
        by_name = {row["lever"]: row["change"] for row in found["levers"]}
        self.assertLess(by_name["L1+L2"], by_name["L1"])
        self.assertLess(by_name["L1+L3"], by_name["L1"])
        self.assertLessEqual(by_name["L1+L2+L3"], by_name["L1+L2"])
        self.assertLessEqual(by_name["L1+L2+L3"], by_name["L1+L3"])


class LengthBands(unittest.TestCase):
    def test_a_band_no_context_exceeds_holds_nothing(self):
        rows = [{"requests": 10, "billable_tokens": 100, "kind": "a"}]
        found = SHAPE.length_bands(rows, bands=(50,))
        self.assertEqual(found[0]["contexts"], 0)
        self.assertEqual(found[0]["share_of_tokens"], 0.0)

    def test_a_band_every_context_exceeds_holds_all_of_it(self):
        rows = [{"requests": 90, "billable_tokens": 100, "kind": "a"}]
        found = SHAPE.length_bands(rows, bands=(50,))
        self.assertEqual(found[0]["contexts"], 1)
        self.assertEqual(found[0]["share_of_tokens"], 1.0)


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
        for row in rows:
            row["opening_cache_reads"] = 40000 * min(row["requests"], 10)
            row["opening_requests"] = min(row["requests"], 10)
        group = {
            "contexts": len(rows),
            "requests": sum(row["requests"] for row in rows),
            "billable_tokens": sum(row["billable_tokens"] for row in rows),
            "cost_law": law,
            "terms": SHAPE.terms(rows, law),
            "concentration": SHAPE.concentration(rows),
            "length_bands": SHAPE.length_bands(rows),
            "opening": SHAPE.opening_cache_reads(rows),
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
            "regenerate it with `shape.py table --block figures`",
        )

    def test_the_committed_report_carries_the_generated_lever_table(self):
        """The Blocker #572's review found lived in a table like this one.

        It was computed beside the document rather than out of it, and one row
        was priced on a law the baseline did not use. Generating it is the
        durable fix; this is what makes the fix stick.
        """
        for path in (SHAPE_DOCUMENT, RANKING):
            if not os.path.isfile(path):
                self.skipTest(f"{path} is not committed here")
        with open(SHAPE_DOCUMENT, "r", encoding="utf-8") as stream:
            shape = json.load(stream)
        with open(RANKING, "r", encoding="utf-8") as stream:
            markdown = stream.read()
        carried = SHAPE.extract_levers(markdown)
        self.assertIsNotNone(carried, "the ranking carries no shape-levers block")
        self.assertEqual(
            carried,
            SHAPE.lever_table(shape),
            "the ranking's lever table no longer matches the committed JSON; "
            "regenerate it with `shape.py table --block levers`",
        )


if __name__ == "__main__":
    unittest.main()
