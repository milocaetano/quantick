#!/usr/bin/env python3
"""Grade one velocity lever against the protocol, and check the ledger.

``docs/quality/velocity/experiment-protocol.md`` is the specification; where
this module and that document disagree, the document is right and this is the
bug. It exists so that "did the change prove its saving" is a command with an
exit code rather than a paragraph somebody wrote after the fact.

**Nothing here reads a transcript, a registry or the network.** The reading is
taken locally with ``measure.py`` and ``shape.py``, which need the trader's own
session transcripts, and the numbers that come out of that reading are written
into the committed ledger. This module is arithmetic over those numbers, which
is what lets CI run ``verify`` over the committed ledger on a runner where no
transcript exists and none ever will.

The pricing is not re-implemented here. Every counterfactual is a call into
``shape.modelled`` on the coefficients ``shape.json`` already publishes, so a
verdict and the published ranking cannot drift apart.
"""

import argparse
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
VERSION = 1
PROTOCOL = "docs/quality/velocity/experiment-protocol.md"
DEFAULT_LEDGER = os.path.join("docs", "quality", "velocity", "experiments.json")
DEFAULT_LAWS = os.path.join("docs", "quality", "velocity", "shape.json")


def _bootstrap():
    """Load `transcripts.py` beside this file, once, under a stable key."""
    key = "quantick_mission_cost_transcripts"
    if key in sys.modules:
        return sys.modules[key]
    path = os.path.join(HERE, "transcripts.py")
    if not os.path.isfile(path):
        raise RuntimeError(f"cannot load transcripts.py beside {__file__}")
    spec = importlib.util.spec_from_file_location(key, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load transcripts.py beside {__file__}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[key] = module
    spec.loader.exec_module(module)
    return module


TRANSCRIPTS = _bootstrap()
CANONICAL = TRANSCRIPTS.load("canonical")
SHAPE = TRANSCRIPTS.load("shape")

# Section 4 of the protocol, registered before any reading was taken under it
# and mirrored in section 10 of the method. These are constants rather than
# command-line options for the reason the method gives about its own three: a
# threshold that can be passed in is a threshold that can be moved until the
# answer comes out.
MIN_MECHANISM_CHANGE = 0.10
MIN_MODELLED_SAVING = 0.03
MAX_MODEL_RESIDUAL = 0.25
MIN_REFERENCE_N = 3
PROBATION_MISSIONS = 1
MAX_UNPROVEN_EXTENSIONS = 1

# The three admissible proof forms, and what each one measures. The names are
# the ledger's vocabulary, so a lever that cannot say which of the three it is
# cannot be registered at all.
FORMS = {
    "reshape": "longest_context_requests",
    "frame": "frame_tokens_per_request",
    "removal": "removed_context_requests",
}

# Named, so that the refusal says what is wrong rather than "unknown form".
# "This mission cost less than the last one" is the form that certifies noise,
# and section 3 of the protocol rejects it by name.
REFUSED_FORM = "totals"

VERDICTS = ("pending", "proven", "refuted", "unproven", "void")
ATTRIBUTIONS = ("declared", "window")
QUALITY_GUARDS = ("identical", "improved")

# Why a ledger or a reading is refused, as stable names beside the sentence.
# `shape.py` already publishes its defects this way -- `NEGATIVE_FRAME`,
# `DEGENERATE_LAW` -- and for the same reason: a caller that has to act on
# *which* refusal fired should not be matching on prose that a reword breaks.
# A coordinator reverting on a spent probation and only warning on a missing
# honesty field is the case this exists for.
UNREGISTERED_FORM = "unregistered_proof_form"
REFUSED_PROOF_FORM = "refused_proof_form"
FORM_VARIABLE_MISMATCH = "form_and_variable_disagree"
BAD_PREDICTION = "predicted_saving_outside_unit_interval"
BAD_REGISTRATION_INSTANT = "registered_at_is_not_an_instant"
BAD_PROBATION = "probation_is_not_a_positive_count"
PROBATION_OVEREXTENDED = "probation_extended_past_the_limit"
PROBATION_SPENT = "probation_spent_without_a_verdict"
MISSING_HONESTY = "review_reduction_missing_its_honesty_fields"
MISSING_REVERT = "refuted_lever_without_a_recorded_revert"
BAD_VERDICT = "verdict_outside_the_registered_set"
DUPLICATE_ID = "experiment_declared_twice"
DUPLICATE_BRANCH = "one_mission_grades_one_lever"
BAD_REFERENCE = "reference_without_removed_requests"
BAD_ATTRIBUTION = "attribution_outside_the_registered_set"
READING_WITHOUT_BRANCH = "reading_without_a_branch"
READING_WITHOUT_CONTEXTS = "reading_without_a_context_vector"
READING_WITHOUT_TOTAL = "reading_without_a_measured_total"
BAD_READING_INSTANT = "taken_at_is_not_an_instant"
READING_PRECEDES_REGISTRATION = "reading_precedes_its_registration"
CONCURRENT_VARIABLE = "two_open_probations_on_one_variable"
CONCURRENT_REQUEST_TERM = "context_shape_and_request_count_are_not_disjoint"
BAD_SCHEMA = "unknown_ledger_schema"
BAD_EXPERIMENTS = "experiments_is_not_a_list"
NO_ID = "experiment_without_an_id"
UNKNOWN_LEVER = "no_such_lever_in_the_ledger"
DEGENERATE_TRIM = "trim_at_or_past_the_fitted_frame"


def refusal(code, message):
    """One refusal: a stable name for a machine, a sentence for a person."""
    return {"code": code, "message": message}


class LedgerError(ValueError):
    """A ledger that cannot be graded, with every refusal it earned."""

    def __init__(self, refusals):
        self.refusals = list(refusals)
        self.codes = [item["code"] for item in self.refusals]
        self.messages = [item["message"] for item in self.refusals]
        super().__init__("; ".join(self.messages))


def load_ledger(path):
    with open(path, "r", encoding="utf-8") as stream:
        return json.load(stream)


def load_laws(path):
    """The fitted coefficients, keyed by population, straight from shape.json.

    Read rather than re-fitted, and read from the file the published ranking
    was generated from. A lever priced on a law fitted over the mission it is
    grading would be grading itself.
    """
    with open(path, "r", encoding="utf-8") as stream:
        document = json.load(stream)
    populations = document.get("populations") or {}
    return {name: body["cost_law"] for name, body in populations.items()}


def _instant(value):
    """Parse an ISO-8601 instant, or return None."""
    if not isinstance(value, str):
        return None
    try:
        return TRANSCRIPTS.parse_timestamp(value)
    except (ValueError, TypeError):
        return None


def _rows(contexts):
    """The reading's context vector, in the shape ``shape.modelled`` wants."""
    return [
        {"requests": float(item["requests"]), "kind": item["kind"]}
        for item in contexts
    ]


def _population(entry, rows):
    """Which law a reconstructed context is priced on.

    Stated by the lever's policy, because capping *means* handing off to a
    fresh dispatch and a fresh dispatch is a subagent by construction. Falling
    back to the observed vector's own population keeps a lever that says
    nothing from being priced on a population it never ran in.
    """
    named = (entry.get("policy") or {}).get("law")
    if named:
        return named
    return rows[0]["kind"] if rows else "subagent"


def counterfactual_rows(entry, reading):
    """The context vector this mission would have had without the lever.

    One reconstruction per proof form, and all three end in the same place: a
    list of contexts priced on the registered law. That uniformity is the
    protocol's whole trick -- the lever is graded on the shape it changed,
    never on a comparison between two missions that are not interchangeable.
    """
    rows = _rows(reading["contexts"])
    form = entry.get("form")
    if form == "frame":
        # The same contexts. Only the per-request frame differs, and that is
        # applied as a trim at pricing time rather than as a different vector.
        return rows
    if form == "reshape":
        handoff = float((entry.get("policy") or {}).get("handoff", SHAPE.HANDOFF_REQUESTS))
        total = sum(row["requests"] for row in rows)
        # The handoff re-reads exist only because the lever split the context,
        # so the pre-lever run is the same work minus what the splits charged.
        before = total - handoff * (len(rows) - 1)
        return [{"requests": max(before, 0.0), "kind": _population(entry, rows)}]
    if form == "removal":
        return rows + removed_rows(entry, rows)
    raise LedgerError(
        [
            refusal(
                UNREGISTERED_FORM,
                f"{entry['id']}: unregistered proof form {form!r}",
            )
        ]
    )


def smallest_reference(entry):
    """The reference mission that makes the weakest case for the lever.

    A removal borrows the size of what it removed from missions that still ran
    it, and missions are not interchangeable, so the borrowed number is the
    smallest observed and never the mean. `delivery-review` measured 9.1% of
    one mission and 2.9% of another; a lever that cuts it has to beat 2.9%.
    """
    references = entry.get("references") or []
    if not references:
        return None
    return min(references, key=lambda item: float(item["removed_requests"]))


def removed_rows(entry, rows):
    """The contexts a removal took out, added back at their reference length."""
    reference = smallest_reference(entry)
    if reference is None:
        return []
    count = max(int(reference.get("removed_contexts", 1)), 1)
    each = float(reference["removed_requests"]) / count
    kind = _population(entry, rows)
    return [{"requests": each, "kind": kind} for _ in range(count)]


def price(rows, laws, trim=0.0):
    """What a context vector costs on the registered law.

    ``shape.modelled`` is the one owner of that arithmetic, here with no cap
    and no request scaling: this module prices vectors somebody measured, and
    never a policy somebody supposed.
    """
    return SHAPE.modelled(rows, laws, cap=None, trim=trim, scale=1.0)


def _frame_trim(entry, reading, laws, rows):
    """How many tokens a frame lever took off every request, and is it sane."""
    mechanism = reading.get("mechanism") or {}
    before = float(mechanism.get("before", 0.0))
    after = float(mechanism.get("after", 0.0))
    trim = before - after
    smallest = min(
        laws[row["kind"]]["frame_tokens_per_request"] for row in rows
    ) if rows else 0.0
    if trim >= smallest:
        raise LedgerError(
            [
                refusal(
                    DEGENERATE_TRIM,
                    f"{entry['id']}: a trim of {trim:.0f} tokens is at or past "
                    f"the fitted frame of {smallest:.0f}; the row would be "
                    "priced on a degenerate law",
                )
            ]
        )
    return trim


def _void_reasons(entry, reading):
    """Every reason this reading may conclude nothing at all.

    Void is not a weak refutation. It says the reading cannot be read in
    either direction, which is why a void lever is reverted exactly like a
    refuted one rather than given the benefit of the doubt.
    """
    found = []
    form = entry.get("form")
    if form == REFUSED_FORM:
        found.append("proof form totals is refused by protocol section 3")
    elif form not in FORMS:
        found.append(f"unregistered proof form {form!r}")
    if reading.get("attribution") != "declared":
        found.append("attribution is not declared; a window-inferred reading is void")
    if reading.get("contested"):
        found.append("the transcript claim is contested")
    quality = reading.get("quality") or {}
    if quality.get("ci") != "green":
        found.append("CI was not green at the graded head")
    if quality.get("guards_report") not in QUALITY_GUARDS:
        found.append("the guards report neither held nor improved")
    if entry.get("reduces_review"):
        triple = entry.get("reduction") or {}
        missing = [
            name
            for name in ("removes", "risk", "justified_by")
            if not str(triple.get(name) or "").strip()
        ]
        if missing:
            found.append(
                "a reduction in review depth is missing its honesty fields: "
                + ", ".join(missing)
            )
    return found


def _mechanism_move(entry, reading):
    """How far the declared variable moved, as a share of its pre-lever value.

    For a `removal` the pre-lever value is **derived**, not read: it is the
    smallest reference mission's removed-request count. A lever that could
    write its own `before` could make any `after` look like a fall, which is
    the one place a hand-written reading would have been able to grade itself.
    """
    mechanism = reading.get("mechanism") or {}
    after = float(mechanism.get("after", 0.0))
    if entry.get("form") == "removal":
        reference = smallest_reference(entry)
        if reference is None:
            return None
        before = float(reference["removed_requests"])
    else:
        before = float(mechanism.get("before", 0.0))
    if before <= 0:
        return None
    return (before - after) / before


def _removal_keys(entry, reading):
    """The two extra things a removal has to show, and why each one is there.

    The first is that the borrowed number came from enough missions to be worth
    borrowing. The second is the one that rejects a bad round-cut: if the work
    the removed rounds were doing simply reappeared as repair turns, the
    mission's own request count will not have fallen below the missions that
    still ran them, whatever the removal itself measured.
    """
    references = entry.get("references") or []
    enough = len(references) >= MIN_REFERENCE_N
    totals = [float(item["total_requests"]) for item in references if "total_requests" in item]
    measured = float((reading.get("requests") or {}).get("measured", 0.0))
    fell = bool(totals) and measured > 0 and measured < min(totals)
    return {
        "references": len(references),
        "references_enough": enough,
        "smallest_reference_total_requests": min(totals) if totals else None,
        "measured_total_requests": measured or None,
        "total_requests_fell_below_every_reference": fell,
    }


def grade(entry, laws):
    """The verdict for one lever, from its latest reading.

    Section 5 of the protocol, in order: void before anything else, then the
    two keys, then the table. The order matters -- a reading that cannot be
    read must not be able to reach `refuted` and look like evidence against
    the lever.
    """
    result = {
        "id": entry["id"],
        "form": entry.get("form"),
        "variable": FORMS.get(entry.get("form")),
        "protocol": PROTOCOL,
    }
    readings = entry.get("readings") or []
    if not readings:
        result["verdict"] = "pending"
        result["reason"] = "no reading taken"
        return result
    reading = readings[-1]
    result["branch"] = reading.get("branch")
    result["pr"] = reading.get("pr")

    void = _void_reasons(entry, reading)
    if void:
        result["verdict"] = "void"
        result["void_because"] = void
        return result

    rows = _rows(reading["contexts"])
    measured = float(reading["measured_billable"])
    trim = _frame_trim(entry, reading, laws, rows) if entry.get("form") == "frame" else 0.0
    observed = price(rows, laws, trim=trim)
    counter = price(counterfactual_rows(entry, reading), laws, trim=0.0)

    moved = _mechanism_move(entry, reading)
    if moved is None:
        result["verdict"] = "void"
        result["void_because"] = [
            "the mechanism reading is missing or has no pre-lever value, so "
            "the lever cannot be graded in either direction"
        ]
        return result
    first_key = moved >= MIN_MECHANISM_CHANGE
    removal = None
    if entry.get("form") == "removal":
        removal = _removal_keys(entry, reading)
        first_key = (
            first_key
            and removal["references_enough"]
            and removal["total_requests_fell_below_every_reference"]
        )

    residual = abs(observed - measured) / measured if measured else None
    saving = (counter - observed) / counter if counter else None
    direction = measured <= counter
    second_key = (
        residual is not None
        and residual <= MAX_MODEL_RESIDUAL
        and saving is not None
        and saving >= MIN_MODELLED_SAVING
        and direction
    )

    if first_key and second_key:
        verdict = "proven"
    elif not first_key or not direction:
        verdict = "refuted"
    else:
        verdict = "unproven"

    result.update(
        {
            "verdict": verdict,
            "mechanism_moved": round(moved, 4),
            "mechanism_key": bool(first_key),
            "model_key": bool(second_key),
            "measured_billable": measured,
            "modelled_observed": round(observed, 1),
            "modelled_counterfactual": round(counter, 1),
            "model_residual": None if residual is None else round(residual, 4),
            "modelled_saving": None if saving is None else round(saving, 4),
            "measured_at_or_below_counterfactual": direction,
        }
    )
    if trim:
        result["frame_trim_tokens"] = round(trim, 1)
    if removal is not None:
        result["removal"] = removal
    return result


def _reading_refusals(entry, reading, index):
    found = []
    where = f"{entry['id']} reading {index}"
    if reading.get("attribution") not in ATTRIBUTIONS:
        found.append(
            refusal(BAD_ATTRIBUTION, f"{where}: attribution must be one of {ATTRIBUTIONS}")
        )
    if not reading.get("branch"):
        found.append(refusal(READING_WITHOUT_BRANCH, f"{where}: no branch"))
    if not isinstance(reading.get("contexts"), list) or not reading["contexts"]:
        found.append(refusal(READING_WITHOUT_CONTEXTS, f"{where}: no context vector"))
    else:
        for item in reading["contexts"]:
            if float(item.get("requests", 0)) <= 0:
                found.append(
                    refusal(READING_WITHOUT_CONTEXTS, f"{where}: a context with no requests")
                )
                break
            if item.get("kind") not in ("main", "subagent"):
                found.append(
                    refusal(READING_WITHOUT_CONTEXTS, f"{where}: a context with no population")
                )
                break
    if float(reading.get("measured_billable", 0)) <= 0:
        found.append(refusal(READING_WITHOUT_TOTAL, f"{where}: no measured total"))
    taken = _instant(reading.get("taken_at"))
    registered = _instant(entry.get("registered_at"))
    if taken is None:
        found.append(refusal(BAD_READING_INSTANT, f"{where}: taken_at is not an instant"))
    elif registered is not None and taken < registered:
        found.append(
            refusal(
                READING_PRECEDES_REGISTRATION,
                f"{where}: taken before the lever was registered; a rule "
                "written after its reading proves nothing",
            )
        )
    return found


def _entry_refusals(entry, seen_ids, seen_branches):
    found = []
    identifier = entry.get("id")
    if not identifier:
        return [refusal(NO_ID, "an experiment with no id")]
    if identifier in seen_ids:
        found.append(refusal(DUPLICATE_ID, f"{identifier}: declared twice"))
    seen_ids.add(identifier)
    form = entry.get("form")
    if form == REFUSED_FORM:
        found.append(
            refusal(
                REFUSED_PROOF_FORM,
                f"{identifier}: proof form 'totals' is refused. A pair of "
                "mission totals certifies noise at this n; protocol section 3 "
                f"names one of {sorted(FORMS)}",
            )
        )
    elif form not in FORMS:
        found.append(
            refusal(UNREGISTERED_FORM, f"{identifier}: form must be one of {sorted(FORMS)}")
        )
    elif entry.get("variable") != FORMS[form]:
        found.append(
            refusal(
                FORM_VARIABLE_MISMATCH,
                f"{identifier}: form {form} measures {FORMS[form]!r}, "
                f"not {entry.get('variable')!r}",
            )
        )
    predicted = entry.get("predicted_saving")
    if not isinstance(predicted, (int, float)) or not 0 < predicted < 1:
        found.append(
            refusal(BAD_PREDICTION, f"{identifier}: predicted_saving must be a share in (0, 1)")
        )
    if _instant(entry.get("registered_at")) is None:
        found.append(
            refusal(BAD_REGISTRATION_INSTANT, f"{identifier}: registered_at is not an instant")
        )
    probation = entry.get("probation") or {}
    if int(probation.get("missions", 0)) < 1:
        found.append(
            refusal(BAD_PROBATION, f"{identifier}: probation must be at least one mission")
        )
    if int(probation.get("extensions_used", 0)) > MAX_UNPROVEN_EXTENSIONS:
        found.append(
            refusal(
                PROBATION_OVEREXTENDED,
                f"{identifier}: probation extended past MAX_UNPROVEN_EXTENSIONS",
            )
        )
    # The honesty triple is owed by the mission that runs the lever, not by the
    # coordinator who registered it, so it is required from the moment a
    # probation mission is named rather than at registration. Registering the
    # obligation early and discharging it late is the point: the ledger says a
    # round-cut owes these three sentences before anyone starts writing one.
    if entry.get("reduces_review") and (_is_open(entry) or entry.get("readings")):
        triple = entry.get("reduction") or {}
        missing = [
            name
            for name in ("removes", "risk", "justified_by")
            if not str(triple.get(name) or "").strip()
        ]
        if missing:
            found.append(
                refusal(
                    MISSING_HONESTY,
                    f"{identifier}: a lever that reduces review depth must name "
                    + ", ".join(missing),
                )
            )
    if form == "removal":
        for reference in entry.get("references") or []:
            if float(reference.get("removed_requests", 0)) <= 0:
                found.append(
                    refusal(BAD_REFERENCE, f"{identifier}: a reference with no removed requests")
                )
                break
    verdict = entry.get("verdict")
    if verdict is not None and verdict not in VERDICTS:
        found.append(refusal(BAD_VERDICT, f"{identifier}: verdict must be one of {VERDICTS}"))
    if verdict in ("refuted", "void") and not entry.get("reverted_in"):
        found.append(
            refusal(
                MISSING_REVERT,
                f"{identifier}: a {verdict} lever comes out of the campaign "
                "branch; record the revert commit in reverted_in",
            )
        )
    readings = entry.get("readings") or []
    graded_branch = probation.get("graded_branch")
    if graded_branch:
        if graded_branch in seen_branches:
            found.append(
                refusal(
                    DUPLICATE_BRANCH,
                    f"{identifier}: branch {graded_branch} already grades "
                    "another lever; one mission grades one lever",
                )
            )
        else:
            seen_branches.add(graded_branch)
    for index, reading in enumerate(readings):
        found.extend(_reading_refusals(entry, reading, index))
        branch = reading.get("branch")
        if branch and branch == graded_branch:
            continue
        if branch in seen_branches:
            found.append(
                refusal(
                    DUPLICATE_BRANCH,
                    f"{identifier}: branch {branch} already grades another "
                    "lever; one reading grades one lever",
                )
            )
        elif branch:
            seen_branches.add(branch)
    allowed = int(probation.get("missions", PROBATION_MISSIONS)) + int(
        probation.get("extensions_used", 0)
    )
    if len(readings) >= allowed and verdict in (None, "pending"):
        found.append(
            refusal(
                PROBATION_SPENT,
                f"{identifier}: probation is spent and no verdict is recorded; "
                "grade it or revert it",
            )
        )
    return found


def verify(ledger):
    """Every way the committed ledger can be wrong, in one pass.

    Refusals are collected rather than raised one at a time so that a reader
    fixing a ledger sees the whole list, and so the CI step's output is the
    work list rather than the first line of it.
    """
    refusals = []
    if ledger.get("schema") != VERSION:
        refusals.append(refusal(BAD_SCHEMA, f"schema must be {VERSION}"))
    experiments = ledger.get("experiments")
    if not isinstance(experiments, list):
        raise LedgerError(
            refusals + [refusal(BAD_EXPERIMENTS, "experiments must be a list")]
        )
    seen_ids = set()
    seen_branches = set()
    for entry in experiments:
        refusals.extend(_entry_refusals(entry, seen_ids, seen_branches))
    refusals.extend(_concurrency_refusals(experiments))
    if refusals:
        raise LedgerError(refusals)
    return {
        "ok": True,
        "protocol": PROTOCOL,
        "experiments": len(experiments),
        "open_probations": sorted(
            entry["id"] for entry in experiments if _is_open(entry)
        ),
    }


def _is_open(entry):
    """Is this lever *in probation*, as opposed to merely registered?

    Registering a lever costs nothing and blocks nothing -- all three of the
    ranked levers are registered together, before any of them has a mission.
    Probation begins when the coordinator names the mission that will grade it,
    which is the moment section 8's serialization has to start binding, and it
    ends when a verdict is written.
    """
    started = (entry.get("probation") or {}).get("graded_branch")
    return bool(started) and entry.get("verdict") in (None, "pending")


def _concurrency_refusals(experiments):
    """Section 8: two open probations may not share a mechanism variable.

    `contexts` and `requests` both enter N, so two levers moving them at once
    cannot be told apart afterwards. `frame` is disjoint from both and may run
    beside either.
    """
    found = []
    by_variable = {}
    for entry in experiments:
        if not _is_open(entry):
            continue
        by_variable.setdefault(entry.get("variable"), []).append(entry.get("id"))
    for variable, identifiers in sorted(by_variable.items()):
        if len(identifiers) > 1:
            found.append(
                refusal(
                    CONCURRENT_VARIABLE,
                    f"{', '.join(sorted(identifiers))}: two open probations on "
                    f"{variable!r}; protocol section 8 serializes them",
                )
            )
    contexts = by_variable.get(FORMS["reshape"], [])
    requests = by_variable.get(FORMS["removal"], [])
    if contexts and requests:
        found.append(
            refusal(
                CONCURRENT_REQUEST_TERM,
                f"{', '.join(sorted(contexts + requests))}: context shape and "
                "request count are not disjoint; both enter N and a joint "
                "reading cannot be disentangled",
            )
        )
    return found


def _refused(error):
    """One refusal document, whichever command produced it."""
    return {"ok": False, "codes": error.codes, "refusals": error.refusals}


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="grade a velocity lever, or check the experiment ledger",
    )
    modes = parser.add_subparsers(dest="mode", required=True)
    for name, help_text in (
        ("verify", "check the committed ledger; reads no transcript"),
        ("grade", "the verdict for one lever, or for every lever"),
    ):
        mode = modes.add_parser(name, help=help_text)
        mode.add_argument("--ledger", default=DEFAULT_LEDGER)
        mode.add_argument("--out", default="-", help="where to write; - is stdout")
        if name == "grade":
            mode.add_argument("--laws", default=DEFAULT_LAWS)
            mode.add_argument("--id", default=None, help="one lever; default all")
    parsed = parser.parse_args(argv)

    ledger = load_ledger(parsed.ledger)
    try:
        report = verify(ledger)
    except LedgerError as refused:
        CANONICAL.emit(CANONICAL.render(_refused(refused)), parsed.out)
        return 1
    if parsed.mode == "verify":
        CANONICAL.emit(CANONICAL.render(report), parsed.out)
        return 0

    laws = load_laws(parsed.laws)
    wanted = [
        entry
        for entry in ledger["experiments"]
        if parsed.id is None or entry.get("id") == parsed.id
    ]
    if not wanted:
        CANONICAL.emit(
            CANONICAL.render(
                _refused(LedgerError([refusal(UNKNOWN_LEVER, f"no lever {parsed.id!r}")]))
            ),
            parsed.out,
        )
        return 1
    try:
        verdicts = [grade(entry, laws) for entry in wanted]
    except LedgerError as refused:
        CANONICAL.emit(CANONICAL.render(_refused(refused)), parsed.out)
        return 1
    # No instant of its own: section 7 of the method wants the same bytes from
    # the same inputs, and a verdict stamped with the clock would differ every
    # run over an unchanged ledger.
    CANONICAL.emit(
        CANONICAL.render({"protocol": PROTOCOL, "verdicts": verdicts}),
        parsed.out,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
