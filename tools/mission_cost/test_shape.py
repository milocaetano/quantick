#!/usr/bin/env python3
"""Offline tests for `shape.py`. No transcript outside the fixture, no network."""

import builtins
import json
import os
import subprocess
import sys
import tempfile
import unittest

import transcripts as TRANSCRIPTS_MODULE  # noqa: F401  (import guard, see below)

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))

SHAPE = TRANSCRIPTS_MODULE.load("shape")

FIXTURES = os.path.join(HERE, "fixtures", "transcripts")
SHAPE_MODULE = os.path.join(HERE, "shape.py")

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


class Validity(unittest.TestCase):
    """A law that cannot be true says so in the document, not to the reader.

    The fit is an unconstrained least squares, so a narrow population can put
    its minimum at a negative coefficient. A request cannot cost less than
    nothing before its context has grown; publishing that as a number and
    leaving a consumer to notice is the data-honesty failure this closes.
    """

    def test_a_law_that_can_be_true_is_marked_usable(self):
        found = SHAPE.fit(synthetic(60000, 500, [10, 25, 50, 100]))
        self.assertTrue(found["validity"]["usable"])
        self.assertEqual(found["validity"]["degenerate_because"], [])

    def test_a_negative_frame_is_named_rather_than_printed(self):
        found = SHAPE.fit(synthetic(-1000, 500, [10, 25, 50, 100]))
        self.assertLess(found["frame_tokens_per_request"], 0)
        self.assertFalse(found["validity"]["usable"])
        self.assertIn(SHAPE.NEGATIVE_FRAME, found["validity"]["degenerate_because"])

    def test_a_negative_slope_is_named_too(self):
        found = SHAPE.fit(synthetic(60000, -50, [10, 25, 50, 100]))
        self.assertFalse(found["validity"]["usable"])
        self.assertIn(SHAPE.NEGATIVE_SLOPE, found["validity"]["degenerate_because"])

    def test_a_share_outside_the_unit_interval_is_named(self):
        rows = synthetic(-1000, 500, [10, 25, 50, 100])
        law = SHAPE.fit(rows)
        term = SHAPE.terms(rows, law)
        self.assertLess(term["standing_frame_share"], 0.0)
        judged = SHAPE.validity(law, term)
        self.assertFalse(judged["usable"])
        self.assertIn(SHAPE.SHARE_OUTSIDE_UNIT, judged["degenerate_because"])

    def test_the_shares_alone_cannot_condemn_a_sound_law(self):
        rows = synthetic(60000, 500, [10, 25, 50, 100])
        law = SHAPE.fit(rows)
        judged = SHAPE.validity(law, SHAPE.terms(rows, law))
        self.assertTrue(judged["usable"])


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


def _payload(body, tier_text, commits, threads, totals=None):
    """One `gh` answer. ``totals`` overrides a connection's `totalCount`.

    Left alone, every `totalCount` equals the number of nodes returned, which
    is what an untruncated page looks like.
    """
    totals = totals or {}
    comments = [{"createdAt": "2026-09-20T11:00:00Z", "body": body}]
    commit_nodes = [{"commit": {"committedDate": when}} for when in commits]
    thread_nodes = [{"isResolved": one} for one in threads]
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
                        "comments": {
                            "totalCount": totals.get("comments", len(comments)),
                            "nodes": comments,
                        },
                        "commits": {
                            "totalCount": totals.get("commits", len(commit_nodes)),
                            "nodes": commit_nodes,
                        },
                        "reviewThreads": {
                            "totalCount": totals.get(
                                "reviewThreads", len(thread_nodes)
                            ),
                            "nodes": thread_nodes,
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

    def test_a_complete_page_records_no_truncation(self):
        self.assertEqual(self.record()["truncated"], {})


class Truncation(unittest.TestCase):
    """A short page is recorded, never reported as the whole.

    The ceremony totals are the denominator of every cost-per-catch number. PR
    #542 has 176 commits against a page of 100, and the `comments` cap is
    sharper still because the durable-report filter runs after the page: a
    pull request whose 61st comment is a review report would lose that report
    from `reports`, `step_zero_rounds` and `step_zero_findings` in silence.
    """

    def payload(self, totals):
        return _payload(
            "<!-- quantick-review-report:v1 kind=arch-review -->\n"
            "step 0: code-review at low, 4 findings.\n",
            "Tier: medium",
            ["2026-09-20T09:00:00Z"],
            [True],
            totals=totals,
        )

    def record(self, totals):
        raw = self.payload(totals)
        return SHAPE.pull_ceremony("owner", "repo", 42, runner=lambda args: raw)

    def test_the_query_asks_every_connection_for_its_total(self):
        for name in SHAPE.PAGES:
            self.assertIn(f"{name}(first:{SHAPE.PAGES[name]}){{totalCount", SHAPE.CEREMONY_QUERY)

    def test_a_short_page_is_named_with_what_was_missed(self):
        found = self.record({"commits": 176})
        self.assertEqual(found["truncated"], {"commits": {"fetched": 1, "total": 176}})

    def test_every_connection_is_checked_not_only_the_first(self):
        found = self.record({"comments": 61, "reviewThreads": 400})
        self.assertEqual(sorted(found["truncated"]), ["comments", "reviewThreads"])

    def test_the_document_totals_count_the_truncated_pull_requests(self):
        rows = [self.record({}), self.record({"commits": 176})]
        rows[1]["pr"] = 43
        document = SHAPE.build_ceremony(rows)
        self.assertEqual(document["totals"]["truncated_pulls"], 1)


class CeremonyProvenance(unittest.TestCase):
    """`build_ceremony` stamps what it read, the way `build_shape` does.

    Its input is live GitHub state: one reopened pull request, one edited
    comment or one more registry entry moves every total. Without a digest the
    document cannot be told apart from a changed harness.
    """

    def row(self, pr, threads=3):
        return {
            "pr": pr,
            "tier": "medium",
            "churn": 100,
            "reports": {"arch-review": 1},
            "step_zero_rounds": 1,
            "step_zero_findings": 2,
            "step_zero_empty_rounds": 0,
            "ai_review_threads": threads,
            "ai_review_threads_open": 0,
            "commits_before_pr": 4,
            "commits_after_pr": 6,
            "truncated": {},
        }

    def test_the_document_carries_an_inputs_block(self):
        document = SHAPE.build_ceremony([self.row(7), self.row(9)], registry="r.json")
        self.assertEqual(document["inputs"]["missions"], 2)
        self.assertEqual(document["inputs"]["pulls"], [7, 9])
        self.assertEqual(document["inputs"]["registry"], "r.json")
        self.assertEqual(len(document["inputs"]["digest"]), 64)

    def test_the_digest_does_not_depend_on_the_order_the_pulls_arrived(self):
        one = SHAPE.build_ceremony([self.row(7), self.row(9)])
        other = SHAPE.build_ceremony([self.row(9), self.row(7)])
        self.assertEqual(one["inputs"]["digest"], other["inputs"]["digest"])

    def test_a_changed_fact_changes_the_digest(self):
        one = SHAPE.build_ceremony([self.row(7)])
        other = SHAPE.build_ceremony([self.row(7, threads=4)])
        self.assertNotEqual(one["inputs"]["digest"], other["inputs"]["digest"])


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
        self.assertEqual(
            SHAPE.extract("before\n" + rendered + "after", "figures"), rendered
        )

    def test_a_degenerate_law_is_labelled_in_the_report_too(self):
        """`-112.6% / 212.6%` must never render as though it were a reading."""
        shape = self.shape()
        rows = synthetic(-1000, 500, [10, 25, 50, 100])
        law = SHAPE.fit(rows)
        term = SHAPE.terms(rows, law)
        shape["populations"]["subagent"]["cost_law"] = dict(
            law, validity=SHAPE.validity(law, term)
        )
        rendered = SHAPE.figures(shape, self.ceremony())
        self.assertIn("degenerate", rendered)
        self.assertIn(SHAPE.NEGATIVE_FRAME, rendered)

    def test_a_sound_law_carries_no_warning(self):
        self.assertNotIn("degenerate", SHAPE.figures(self.shape(), self.ceremony()))

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

    def test_the_committed_report_carries_every_generated_block(self):
        """Every block in the registry, not one test method per block.

        The Blocker #572's review found lived in a table computed beside the
        document rather than out of it, with one row priced on a law its own
        baseline did not use. Generating the table is the durable fix; looping
        over `BLOCKS` is what keeps the next block covered for free.
        """
        for path in (SHAPE_DOCUMENT, CEREMONY_DOCUMENT, RANKING):
            if not os.path.isfile(path):
                self.skipTest(f"{path} is not committed here")
        with open(SHAPE_DOCUMENT, "r", encoding="utf-8") as stream:
            shape = json.load(stream)
        with open(CEREMONY_DOCUMENT, "r", encoding="utf-8") as stream:
            ceremony = json.load(stream)
        with open(RANKING, "r", encoding="utf-8") as stream:
            markdown = stream.read()
        for name, block in sorted(SHAPE.BLOCKS.items()):
            with self.subTest(block=name):
                carried = SHAPE.extract(markdown, name)
                self.assertIsNotNone(
                    carried, f"the ranking carries no {name} block"
                )
                self.assertEqual(
                    carried,
                    block.render(shape, ceremony),
                    f"the ranking's {name} block no longer matches the committed "
                    f"JSON; regenerate it with `shape.py table --block {name}`",
                )


class Bounded(unittest.TestCase):
    """The window cut itself, over the committed fixture.

    `bounded()` is the repair that makes a reading over a live, append-only
    directory repeatable: a context still running when the reading is taken
    keeps growing afterwards, so the window has to cut **requests** rather than
    whole files. These pin that semantics, because the whole report rests on a
    closed window giving the same answer tomorrow.
    """

    # The four requests of the fixture's first main thread.
    MAIN = os.path.join(FIXTURES, "aaaaaaaa-0000-4000-8000-000000000001.jsonl")

    def test_a_context_wholly_inside_the_window_is_read_whole(self):
        found = SHAPE.bounded(self.MAIN, None, None)
        self.assertEqual(found["requests"], 4)

    def test_a_context_wholly_outside_the_window_is_dropped(self):
        since = TRANSCRIPTS_MODULE.require_instant("2026-01-02T00:00:00Z", "since")
        self.assertIsNone(SHAPE.bounded(self.MAIN, since, None))

    def test_a_context_straddling_the_boundary_is_truncated_not_dropped(self):
        until = TRANSCRIPTS_MODULE.require_instant("2026-01-01T00:15:00Z", "until")
        found = SHAPE.bounded(self.MAIN, None, until)
        self.assertEqual(found["requests"], 3)
        self.assertEqual(found["last"].isoformat(), "2026-01-01T00:15:00+00:00")

    def test_the_boundary_instant_itself_is_inside_the_window(self):
        at = TRANSCRIPTS_MODULE.require_instant("2026-01-01T00:10:30Z", "at")
        found = SHAPE.bounded(self.MAIN, at, at)
        self.assertEqual(found["requests"], 1)

    def test_only_the_requests_inside_the_window_are_billed(self):
        until = TRANSCRIPTS_MODULE.require_instant("2026-01-01T00:15:00Z", "until")
        whole = SHAPE.bounded(self.MAIN, None, None)
        cut = SHAPE.bounded(self.MAIN, None, until)
        self.assertLess(cut["billable_tokens"], whole["billable_tokens"])
        # The 00:59 request is more than the idle bound past 00:15, so dropping
        # it must not change the active time either way.
        self.assertEqual(cut["active_seconds"], whole["active_seconds"])


class Contexts(unittest.TestCase):
    """Discovery, the per-context row and the order, over the fixture."""

    def rows(self, since=None, until=None):
        return SHAPE.contexts([FIXTURES], since, until)

    def test_every_fixture_transcript_becomes_one_row(self):
        rows = self.rows()
        self.assertEqual(len(rows), 7)
        self.assertEqual(
            sorted(row["kind"] for row in rows),
            ["main", "main", "main", "main", "subagent", "subagent", "subagent"],
        )

    def test_the_rows_are_ordered_so_two_runs_agree(self):
        rows = self.rows()
        self.assertEqual(
            rows, sorted(rows, key=lambda row: (row["root"], row["relative"]))
        )

    def test_a_row_carries_the_counts_bounded_folded(self):
        rows = {row["relative"]: row for row in self.rows()}
        one = rows["aaaaaaaa-0000-4000-8000-000000000001.jsonl"]
        self.assertEqual(one["requests"], 4)
        self.assertEqual(one["kind"], "main")
        self.assertEqual(one["first"], "2026-01-01T00:10:00+00:00")

    def test_a_window_drops_the_contexts_outside_it(self):
        since = TRANSCRIPTS_MODULE.require_instant("2026-01-02T00:00:00Z", "since")
        until = TRANSCRIPTS_MODULE.require_instant("2026-01-03T00:00:00Z", "until")
        rows = self.rows(since, until)
        self.assertEqual([row["relative"] for row in rows],
                         ["dddddddd-0000-4000-8000-000000000004.jsonl"])

    def test_each_transcript_is_opened_exactly_once(self):
        """The repair: `locate` walks, `bounded` reads, and nothing reads twice.

        `discover` would fold every file and `contexts` would then throw that
        fold away and read the same lines again. Counting the opens is the only
        way to say so without reading the implementation.
        """
        opened = []

        def counting(path, *args, **kwargs):
            opened.append(os.path.abspath(path))
            return builtins.open(path, *args, **kwargs)

        # A module global shadows the builtin for code inside that module, so
        # this counts `bounded`'s opens and nothing else's.
        SHAPE.open = counting
        try:
            self.rows()
        finally:
            del SHAPE.open
        self.assertEqual(len(opened), 7)
        self.assertEqual(len(set(opened)), 7)


class Command(unittest.TestCase):
    """`main()` end to end over the fixture, the way the sibling modules do."""

    def measure(self, *extra):
        with tempfile.TemporaryDirectory() as folder:
            out = os.path.join(folder, "shape.json")
            self.assertEqual(
                SHAPE.main(["measure", "--transcripts", FIXTURES, "--out", out,
                            *extra]),
                0,
            )
            with open(out, "rb") as stream:
                return stream.read()

    def test_measure_writes_a_shape_document_over_the_fixture(self):
        document = json.loads(self.measure().decode("ascii"))
        self.assertEqual(document["metric"], "context_cost_shape")
        self.assertFalse(document["registered_comparison"])
        self.assertEqual(sorted(document["populations"]), ["main", "subagent"])
        self.assertEqual(document["inputs"]["contexts"], 7)

    def test_the_fixture_law_is_published_as_degenerate_rather_than_as_a_number(self):
        """The fixture is exactly the narrow population that breaks the fit.

        Seven short contexts of very different jobs put the least-squares
        minimum at a negative frame coefficient. The document has to say so.
        """
        document = json.loads(self.measure().decode("ascii"))
        judged = document["populations"]["subagent"]["cost_law"]["validity"]
        self.assertFalse(judged["usable"])
        self.assertTrue(judged["degenerate_because"])

    def test_two_runs_over_one_closed_window_are_byte_identical(self):
        """The property the whole reading rests on.

        The transcript directory is live and append-only, so a reading is only
        repeatable if a closed window gives the same bytes tomorrow.
        """
        window = ("--since", "2026-01-01T00:00:00Z", "--until", "2026-01-01T23:59:59Z")
        self.assertEqual(self.measure(*window), self.measure(*window))

    def test_the_policy_block_records_the_assumptions_the_run_used(self):
        document = json.loads(
            self.measure(
                "--cap", "120", "--handoff", "15",
                "--frame-trim", "25000", "--request-scale", "0.5",
            ).decode("ascii")
        )
        policy = document["policy"]
        self.assertEqual(policy["cap_requests"], 120)
        self.assertEqual(policy["handoff_requests"], 15)
        self.assertEqual(policy["frame_trim_tokens"], 25000)
        self.assertEqual(policy["request_scale"], 0.5)
        self.assertTrue(
            all(row["cap_requests"] in (None, 120) for row in policy["levers"])
        )

    def test_a_flag_left_off_defaults_to_the_stated_assumption(self):
        policy = json.loads(self.measure().decode("ascii"))["policy"]
        self.assertEqual(policy["cap_requests"], SHAPE.POLICY_CAP)
        self.assertEqual(policy["handoff_requests"], SHAPE.HANDOFF_REQUESTS)
        self.assertEqual(policy["frame_trim_tokens"], SHAPE.FRAME_TRIM_TOKENS)
        self.assertEqual(policy["request_scale"], SHAPE.REQUEST_SCALE)

    def test_a_missing_transcript_directory_is_refused(self):
        with self.assertRaises(SystemExit):
            SHAPE.main(["measure", "--transcripts", os.path.join(HERE, "nowhere")])

    def test_a_window_no_transcript_falls_inside_is_refused(self):
        with self.assertRaises(SystemExit):
            SHAPE.main(["measure", "--transcripts", FIXTURES,
                        "--since", "2030-01-01T00:00:00Z"])

    def test_the_figures_block_refuses_to_render_without_a_ceremony_document(self):
        with self.assertRaises(SystemExit):
            SHAPE.main(["table", "--block", "figures", "--shape", SHAPE_DOCUMENT])

    def test_every_block_in_the_registry_is_a_choice_the_command_takes(self):
        if not os.path.isfile(SHAPE_DOCUMENT):
            self.skipTest(f"{SHAPE_DOCUMENT} is not committed here")
        with tempfile.TemporaryDirectory() as folder:
            for name, block in sorted(SHAPE.BLOCKS.items()):
                if block.needs_ceremony:
                    continue
                out = os.path.join(folder, f"{name}.md")
                self.assertEqual(
                    SHAPE.main(["table", "--block", name, "--shape", SHAPE_DOCUMENT,
                                "--out", out]),
                    0,
                )
                with open(out, "r", encoding="utf-8") as stream:
                    self.assertTrue(stream.read().startswith(block.begin))

    def test_the_module_runs_as_a_subprocess(self):
        """The real entry point, not just the function behind it."""
        with tempfile.TemporaryDirectory() as folder:
            out = os.path.join(folder, "shape.json")
            result = subprocess.run(
                [sys.executable, SHAPE_MODULE, "measure",
                 "--transcripts", FIXTURES, "--out", out],
                capture_output=True, text=True, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            with open(out, "r", encoding="utf-8") as stream:
                self.assertEqual(json.load(stream)["inputs"]["contexts"], 7)


if __name__ == "__main__":
    unittest.main()
