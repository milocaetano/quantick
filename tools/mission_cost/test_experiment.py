#!/usr/bin/env python3
"""The protocol's decision rule, executed.

Every test here is one clause of
``docs/quality/velocity/experiment-protocol.md``. The point is not coverage: it
is that the rule three blocked children are graded against cannot quietly stop
meaning what it says, and that the refusals which protect the campaign from its
own conflict of interest are executable rather than advisory.

Offline. No transcript, no network, no `gh`.
"""

import copy
import importlib.util
import json
import os
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))


def _load(name):
    spec = importlib.util.spec_from_file_location(
        f"quantick_mission_cost_{name}_under_test", os.path.join(HERE, f"{name}.py")
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


EXPERIMENT = _load("experiment")

# The fitted coefficients the ranking publishes, as a literal, so these tests
# are arithmetic over a known law rather than over whatever shape.json holds
# today. `LedgerIsCommitted` checks the committed file separately.
LAWS = {
    "subagent": {
        "frame_tokens_per_request": 64223.5,
        "slope_tokens_per_request_squared": 530.98,
    },
    "main": {
        "frame_tokens_per_request": 150062.2,
        "slope_tokens_per_request_squared": 233.14,
    },
}

GREEN = {"ci": "green", "guards_report": "identical", "findings": {"arch_rounds": 2}}


def _price(requests, kind="subagent", trim=0.0):
    law = LAWS[kind]
    frame = law["frame_tokens_per_request"] - trim
    return frame * requests + law["slope_tokens_per_request_squared"] * requests**2


def reshape_entry(**overrides):
    entry = {
        "id": "L1",
        "form": "reshape",
        "variable": "longest_context_requests",
        "predicted_saving": 0.459,
        "policy": {"cap": 80, "handoff": 8, "law": "subagent"},
        "registered_at": "2026-09-21T12:00:00Z",
        "reduces_review": False,
        "probation": {"missions": 1, "extensions_used": 0, "graded_branch": None},
        "readings": [],
        "verdict": None,
        "reverted_in": None,
    }
    entry.update(overrides)
    return entry


def reshape_reading(**overrides):
    """Four capped contexts of 80 requests each, priced on the law exactly."""
    contexts = [{"requests": 80, "kind": "subagent"} for _ in range(4)]
    measured = sum(_price(80) for _ in contexts)
    reading = {
        "branch": "feat/context-handoff",
        "pr": 900,
        "taken_at": "2026-09-22T12:00:00Z",
        "attribution": "declared",
        "contexts": contexts,
        "measured_billable": measured,
        # The longest context would have been the whole run in one piece, less
        # the re-reads the three handoffs charged.
        "mechanism": {"before": 320 - 24, "after": 80},
        "quality": dict(GREEN),
    }
    reading.update(overrides)
    return reading


class ReshapeVerdicts(unittest.TestCase):
    """Protocol section 5, on the form whose counterfactual borrows nothing."""

    def test_a_cap_that_bound_is_proven(self):
        entry = reshape_entry(readings=[reshape_reading()])
        verdict = EXPERIMENT.grade(entry, LAWS)
        self.assertEqual(verdict["verdict"], "proven")
        self.assertTrue(verdict["mechanism_key"])
        self.assertTrue(verdict["model_key"])
        # 296 requests in one context against four of 80: the quadratic term is
        # what the saving is made of, and it is most of the bill.
        self.assertGreater(verdict["modelled_saving"], 0.30)

    def test_a_cap_that_never_bound_is_refuted(self):
        """One context, no handoff, nothing moved. K1 fails, and it must."""
        reading = reshape_reading(
            contexts=[{"requests": 80, "kind": "subagent"}],
            measured_billable=_price(80),
            mechanism={"before": 80, "after": 80},
        )
        verdict = EXPERIMENT.grade(reshape_entry(readings=[reading]), LAWS)
        self.assertEqual(verdict["verdict"], "refuted")
        self.assertFalse(verdict["mechanism_key"])

    def test_a_mission_the_law_cannot_predict_is_unproven(self):
        """K2's teeth. The mechanism moved; the model does not fit this mission.

        A counterfactual built from a law that mispredicts the very mission it
        is grading is arithmetic, not evidence, so the verdict stops short of
        `proven` instead of crediting a saving nobody can stand behind.
        """
        reading = reshape_reading(measured_billable=sum(_price(80) for _ in range(4)) * 0.5)
        verdict = EXPERIMENT.grade(reshape_entry(readings=[reading]), LAWS)
        self.assertEqual(verdict["verdict"], "unproven")
        self.assertTrue(verdict["mechanism_key"])
        self.assertGreater(verdict["model_residual"], EXPERIMENT.MAX_MODEL_RESIDUAL)

    def test_a_mission_dearer_than_its_counterfactual_is_refuted(self):
        reading = reshape_reading(measured_billable=_price(296) * 1.10)
        verdict = EXPERIMENT.grade(reshape_entry(readings=[reading]), LAWS)
        self.assertEqual(verdict["verdict"], "refuted")
        self.assertFalse(verdict["measured_at_or_below_counterfactual"])

    def test_no_reading_is_pending_not_a_verdict(self):
        self.assertEqual(EXPERIMENT.grade(reshape_entry(), LAWS)["verdict"], "pending")


class FrameVerdicts(unittest.TestCase):
    """Protocol section 5 on the trim, and its one degenerate case."""

    def _entry(self, **overrides):
        entry = reshape_entry(
            id="L2",
            form="frame",
            variable="frame_tokens_per_request",
            predicted_saving=0.051,
            policy={"trim": 10000},
        )
        entry.update(overrides)
        return entry

    def _reading(self, before=64223.5, after=54223.5, **overrides):
        contexts = [{"requests": 100, "kind": "subagent"} for _ in range(3)]
        measured = sum(_price(100, trim=before - after) for _ in contexts)
        reading = {
            "branch": "feat/thin-frame",
            "pr": 901,
            "taken_at": "2026-09-22T12:00:00Z",
            "attribution": "declared",
            "contexts": contexts,
            "measured_billable": measured,
            "mechanism": {"before": before, "after": after},
            "quality": dict(GREEN),
        }
        reading.update(overrides)
        return reading

    def test_a_measured_trim_is_proven(self):
        verdict = EXPERIMENT.grade(self._entry(readings=[self._reading()]), LAWS)
        self.assertEqual(verdict["verdict"], "proven")
        self.assertEqual(verdict["frame_trim_tokens"], 10000.0)

    def test_a_trim_below_the_mechanism_floor_is_refuted(self):
        """MIN_MECHANISM_CHANGE. A frame that barely moved did not move."""
        reading = self._reading(after=64223.5 - 6000)
        entry = self._entry(readings=[reading])
        verdict = EXPERIMENT.grade(entry, LAWS)
        # The mechanism moved less than a tenth of the frame, so K1 refuses.
        self.assertEqual(verdict["verdict"], "refuted")
        self.assertLess(verdict["mechanism_moved"], EXPERIMENT.MIN_MECHANISM_CHANGE)

    def test_a_trim_past_the_fitted_frame_is_refused(self):
        """A claim in the wrong units would price a request below nothing."""
        entry = self._entry(readings=[self._reading(before=100000.0, after=30000.0)])
        with self.assertRaises(EXPERIMENT.LedgerError):
            EXPERIMENT.grade(entry, LAWS)


class RemovalVerdicts(unittest.TestCase):
    """The form a round-cut needs, and the two extra things it has to show.

    This class is the answer to the conflict of interest the dispatch named:
    #575 will propose cutting rounds and #567 writes the rule that judges it.
    Every test below is a way a round-cut can look good and be worthless.
    """

    def _entry(self, references=None, **overrides):
        entry = reshape_entry(
            id="L3",
            form="removal",
            variable="removed_context_requests",
            predicted_saving=0.295,
            policy={"law": "subagent"},
            reduces_review=True,
            reduction={
                "removes": "the second and later rounds of arch-review",
                "risk": "a defect that only a second reading would have caught",
                "justified_by": "docs/quality/velocity/ranking.md, round two of five returned nothing new",
            },
            references=references
            if references is not None
            else [
                {"branch": "a", "removed_contexts": 2, "removed_requests": 120, "total_requests": 400},
                {"branch": "b", "removed_contexts": 2, "removed_requests": 160, "total_requests": 420},
                {"branch": "c", "removed_contexts": 2, "removed_requests": 200, "total_requests": 460},
            ],
        )
        entry.update(overrides)
        return entry

    def _reading(self, total_requests=300, **overrides):
        contexts = [{"requests": 100, "kind": "subagent"} for _ in range(3)]
        reading = {
            "branch": "feat/fewer-rounds",
            "pr": 902,
            "taken_at": "2026-09-22T12:00:00Z",
            "attribution": "declared",
            "contexts": contexts,
            "measured_billable": sum(_price(100) for _ in contexts),
            "mechanism": {"before": 120, "after": 0},
            "requests": {"measured": total_requests},
            "quality": dict(GREEN),
        }
        reading.update(overrides)
        return reading

    def test_a_real_removal_is_proven(self):
        verdict = EXPERIMENT.grade(self._entry(readings=[self._reading()]), LAWS)
        self.assertEqual(verdict["verdict"], "proven")
        self.assertEqual(verdict["removal"]["references"], 3)

    def test_the_work_reappearing_elsewhere_is_refuted(self):
        """The bad round-cut. Rounds gone, repair turns take their place.

        Nothing about the removal itself changed -- the rounds really are
        absent -- but the mission is not shorter than the missions that ran
        them, so the saving did not happen and the verdict says so.
        """
        entry = self._entry(readings=[self._reading(total_requests=430)])
        verdict = EXPERIMENT.grade(entry, LAWS)
        self.assertEqual(verdict["verdict"], "refuted")
        self.assertFalse(
            verdict["removal"]["total_requests_fell_below_every_reference"]
        )

    def test_too_few_references_cannot_reach_proven(self):
        entry = self._entry(
            references=[
                {"branch": "a", "removed_contexts": 2, "removed_requests": 120, "total_requests": 400},
            ],
            readings=[self._reading()],
        )
        verdict = EXPERIMENT.grade(entry, LAWS)
        self.assertEqual(verdict["verdict"], "refuted")
        self.assertFalse(verdict["removal"]["references_enough"])

    def test_the_smallest_reference_is_the_one_priced(self):
        """Missions are not interchangeable, so the weakest case is the case."""
        entry = self._entry(readings=[self._reading()])
        rows = EXPERIMENT.removed_rows(entry, [{"requests": 100, "kind": "subagent"}])
        self.assertEqual([row["requests"] for row in rows], [60.0, 60.0])

    def test_a_reduction_without_its_honesty_triple_is_void(self):
        entry = self._entry(readings=[self._reading()])
        entry["reduction"]["justified_by"] = "  "
        verdict = EXPERIMENT.grade(entry, LAWS)
        self.assertEqual(verdict["verdict"], "void")
        self.assertTrue(
            any("justified_by" in reason for reason in verdict["void_because"])
        )


class TheQualityFloor(unittest.TestCase):
    """Protocol section 6, decision D8. Two things are not negotiable."""

    def _verdict(self, quality):
        reading = reshape_reading(quality=quality)
        return EXPERIMENT.grade(reshape_entry(readings=[reading]), LAWS)

    def test_red_ci_voids_the_reading(self):
        verdict = self._verdict({"ci": "red", "guards_report": "identical"})
        self.assertEqual(verdict["verdict"], "void")

    def test_a_guards_report_that_slipped_voids_the_reading(self):
        verdict = self._verdict({"ci": "green", "guards_report": "worse"})
        self.assertEqual(verdict["verdict"], "void")

    def test_void_is_not_a_refutation(self):
        """A void reading concludes nothing in either direction.

        It matters because section 7 reverts a void lever exactly like a
        refuted one: a change nobody could grade has not earned its place, and
        `void` must not be readable as evidence against the lever either.
        """
        verdict = self._verdict({"ci": "red", "guards_report": "identical"})
        self.assertNotIn("modelled_saving", verdict)
        self.assertNotIn("mechanism_key", verdict)


class AttributionMustBeExact(unittest.TestCase):
    """Protocol section 5, and what makes section 9 load-bearing."""

    def test_a_window_inferred_reading_is_void(self):
        reading = reshape_reading(attribution="window")
        verdict = EXPERIMENT.grade(reshape_entry(readings=[reading]), LAWS)
        self.assertEqual(verdict["verdict"], "void")

    def test_a_contested_claim_is_void(self):
        reading = reshape_reading(contested=True)
        verdict = EXPERIMENT.grade(reshape_entry(readings=[reading]), LAWS)
        self.assertEqual(verdict["verdict"], "void")


class TheRefusedForm(unittest.TestCase):
    """Protocol section 3. The form that certifies noise, refused by name."""

    def test_totals_cannot_be_registered(self):
        ledger = {
            "schema": 1,
            "experiments": [reshape_entry(form="totals", variable="billable_tokens")],
        }
        with self.assertRaises(EXPERIMENT.LedgerError) as refused:
            EXPERIMENT.verify(ledger)
        joined = "; ".join(refused.exception.refusals)
        self.assertIn("totals", joined)
        self.assertIn("certifies noise", joined)

    def test_no_verdict_path_reaches_proven_without_a_form(self):
        for form in sorted(EXPERIMENT.FORMS):
            self.assertIn(form, ("reshape", "frame", "removal"))
        self.assertNotIn(EXPERIMENT.REFUSED_FORM, EXPERIMENT.FORMS)


class LedgerRefusals(unittest.TestCase):
    """Protocol sections 7 and 8, as `verify` enforces them."""

    def _ledger(self, *entries):
        return {"schema": 1, "experiments": list(entries)}

    def test_a_clean_ledger_passes(self):
        report = EXPERIMENT.verify(self._ledger(reshape_entry()))
        self.assertTrue(report["ok"])
        self.assertEqual(report["open_probations"], [])

    def test_a_reading_taken_before_registration_is_refused(self):
        entry = reshape_entry(
            readings=[reshape_reading(taken_at="2026-09-20T12:00:00Z")]
        )
        with self.assertRaises(EXPERIMENT.LedgerError) as refused:
            EXPERIMENT.verify(self._ledger(entry))
        self.assertIn(
            "taken before the lever was registered",
            "; ".join(refused.exception.refusals),
        )

    def test_a_spent_probation_without_a_verdict_is_refused(self):
        entry = reshape_entry(readings=[reshape_reading()])
        with self.assertRaises(EXPERIMENT.LedgerError) as refused:
            EXPERIMENT.verify(self._ledger(entry))
        self.assertIn("grade it or revert it", "; ".join(refused.exception.refusals))

    def test_a_refuted_lever_must_record_its_revert(self):
        entry = reshape_entry(readings=[reshape_reading()], verdict="refuted")
        with self.assertRaises(EXPERIMENT.LedgerError) as refused:
            EXPERIMENT.verify(self._ledger(entry))
        self.assertIn("comes out of the campaign branch", "; ".join(refused.exception.refusals))
        entry["reverted_in"] = "deadbeef"
        self.assertTrue(EXPERIMENT.verify(self._ledger(entry))["ok"])

    def test_a_void_lever_is_reverted_like_a_refuted_one(self):
        entry = reshape_entry(readings=[reshape_reading()], verdict="void")
        with self.assertRaises(EXPERIMENT.LedgerError):
            EXPERIMENT.verify(self._ledger(entry))

    def test_registration_alone_starts_no_probation(self):
        """All three levers are registered together and none of them blocks."""
        first = reshape_entry(id="L1")
        second = reshape_entry(
            id="L3", form="removal", variable="removed_context_requests"
        )
        report = EXPERIMENT.verify(self._ledger(first, second))
        self.assertEqual(report["open_probations"], [])

    def test_two_open_probations_on_the_same_variable_are_refused(self):
        first = reshape_entry(
            id="L1", probation={"missions": 1, "extensions_used": 0, "graded_branch": "a"}
        )
        second = reshape_entry(
            id="L1b", probation={"missions": 1, "extensions_used": 0, "graded_branch": "b"}
        )
        with self.assertRaises(EXPERIMENT.LedgerError) as refused:
            EXPERIMENT.verify(self._ledger(first, second))
        self.assertIn("serializes them", "; ".join(refused.exception.refusals))

    def test_context_shape_and_request_count_are_not_disjoint(self):
        shape = reshape_entry(
            id="L1", probation={"missions": 1, "extensions_used": 0, "graded_branch": "a"}
        )
        count = reshape_entry(
            id="L3",
            form="removal",
            variable="removed_context_requests",
            probation={"missions": 1, "extensions_used": 0, "graded_branch": "b"},
        )
        with self.assertRaises(EXPERIMENT.LedgerError) as refused:
            EXPERIMENT.verify(self._ledger(shape, count))
        self.assertIn("both enter N", "; ".join(refused.exception.refusals))

    def test_a_trim_is_disjoint_from_both(self):
        shape = reshape_entry(
            id="L1", probation={"missions": 1, "extensions_used": 0, "graded_branch": "a"}
        )
        trim = reshape_entry(
            id="L2",
            form="frame",
            variable="frame_tokens_per_request",
            probation={"missions": 1, "extensions_used": 0, "graded_branch": "b"},
        )
        report = EXPERIMENT.verify(self._ledger(shape, trim))
        self.assertEqual(report["open_probations"], ["L1", "L2"])

    def test_one_mission_grades_one_lever(self):
        first = reshape_entry(
            id="L1", probation={"missions": 1, "extensions_used": 0, "graded_branch": "same"}
        )
        second = reshape_entry(
            id="L2",
            form="frame",
            variable="frame_tokens_per_request",
            probation={"missions": 1, "extensions_used": 0, "graded_branch": "same"},
        )
        with self.assertRaises(EXPERIMENT.LedgerError) as refused:
            EXPERIMENT.verify(self._ledger(first, second))
        self.assertIn("one mission grades one lever", "; ".join(refused.exception.refusals))

    def test_a_form_and_a_variable_that_disagree_are_refused(self):
        entry = reshape_entry(variable="frame_tokens_per_request")
        with self.assertRaises(EXPERIMENT.LedgerError):
            EXPERIMENT.verify(self._ledger(entry))

    def test_the_honesty_triple_is_owed_from_probation_not_registration(self):
        entry = reshape_entry(
            id="L3",
            form="removal",
            variable="removed_context_requests",
            reduces_review=True,
            reduction={"removes": "", "risk": "", "justified_by": ""},
        )
        self.assertTrue(EXPERIMENT.verify(self._ledger(entry))["ok"])
        entry["probation"]["graded_branch"] = "feat/fewer-rounds"
        with self.assertRaises(EXPERIMENT.LedgerError) as refused:
            EXPERIMENT.verify(self._ledger(entry))
        self.assertIn("must name", "; ".join(refused.exception.refusals))


class LedgerIsCommitted(unittest.TestCase):
    """The committed ledger and the committed laws, checked as they ship.

    CI runs the same `verify` over the same file, so this test failing and the
    CI step failing are the same fact reported twice -- which is the point: a
    later session that edits the ledger by hand finds out here.
    """

    LEDGER = os.path.join(REPO, "docs", "quality", "velocity", "experiments.json")
    LAWS = os.path.join(REPO, "docs", "quality", "velocity", "shape.json")

    def test_the_committed_ledger_verifies(self):
        report = EXPERIMENT.verify(EXPERIMENT.load_ledger(self.LEDGER))
        self.assertTrue(report["ok"])

    def test_every_ranked_lever_is_registered_before_any_reading(self):
        ledger = EXPERIMENT.load_ledger(self.LEDGER)
        issues = sorted(entry["issue"] for entry in ledger["experiments"])
        self.assertEqual(issues, [573, 574, 575])
        for entry in ledger["experiments"]:
            self.assertEqual(entry["readings"], [])
            self.assertIsNone(entry["verdict"])

    def test_the_laws_come_from_the_published_shape(self):
        laws = EXPERIMENT.load_laws(self.LAWS)
        self.assertEqual(
            round(laws["subagent"]["frame_tokens_per_request"]), 64224
        )
        self.assertEqual(
            round(laws["main"]["frame_tokens_per_request"]), 150062
        )

    def test_grading_the_committed_ledger_is_all_pending(self):
        ledger = EXPERIMENT.load_ledger(self.LEDGER)
        laws = EXPERIMENT.load_laws(self.LAWS)
        for entry in ledger["experiments"]:
            self.assertEqual(EXPERIMENT.grade(entry, laws)["verdict"], "pending")


class TheCommandLine(unittest.TestCase):
    """Exit codes, because the CI step is the enforcement."""

    def _write(self, document):
        handle = tempfile.NamedTemporaryFile(
            "w", suffix=".json", delete=False, encoding="utf-8"
        )
        json.dump(document, handle)
        handle.close()
        self.addCleanup(os.unlink, handle.name)
        return handle.name

    def test_verify_returns_zero_on_the_committed_ledger(self):
        out = self._write({"schema": 1, "experiments": []})
        self.assertEqual(
            EXPERIMENT.main(["verify", "--ledger", out, "--out", self._write({})]), 0
        )

    def test_verify_returns_one_on_a_broken_ledger(self):
        broken = self._write(
            {"schema": 1, "experiments": [reshape_entry(form="totals")]}
        )
        self.assertEqual(
            EXPERIMENT.main(["verify", "--ledger", broken, "--out", self._write({})]), 1
        )

    def test_a_verdict_is_byte_identical_across_runs(self):
        """Method section 7. Nothing in a verdict comes from the clock."""
        ledger = self._write(
            {"schema": 1, "experiments": [copy.deepcopy(reshape_entry())]}
        )
        laws = os.path.join(REPO, "docs", "quality", "velocity", "shape.json")
        first, second = self._write({}), self._write({})
        EXPERIMENT.main(["grade", "--ledger", ledger, "--laws", laws, "--out", first])
        EXPERIMENT.main(["grade", "--ledger", ledger, "--laws", laws, "--out", second])
        with open(first, "rb") as one, open(second, "rb") as two:
            self.assertEqual(one.read(), two.read())


if __name__ == "__main__":
    unittest.main()
