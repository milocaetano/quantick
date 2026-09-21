#!/usr/bin/env python3
"""The cost shape of one agent context, and the price of the ceremony around it.

Why this exists beside ``measure.py`` and ``opening_frame.py``: the registered
method in ``docs/quality/mission-cost/method.md`` grades a *mission*, and #565
showed that this repository cannot place a session on a mission at all. A
ranking needs a unit that needs no attribution. Two such units exist and both
are read here.

**The agent context.** One transcript file is one agent's whole working
context, whether it is a main thread or a dispatched subagent, and nothing has
to be attributed to a branch to count it. What a context costs is not
proportional to the work it does: every request it makes re-reads everything
the context already holds, so the standing prompt grows as the agent works and
the bill grows with the square of the number of requests. ``fit`` measures the
two coefficients of that growth over a whole population of contexts, and
``counterfactual`` prices the one lever the shape implies — capping a context
and handing off to a fresh one.

**The pull request.** What the review chain caught is public: durable review
reports, resolvable AI-review threads, the step-0 finding counts the reports
state themselves, and the commits a branch made after its pull request already
existed, which are repairs by construction. ``ceremony`` reads those from
``gh`` so that a cost-per-catch number has a denominator somebody else can
re-derive.

What this is **not**: a method verdict. Like ``opening_frame.py`` it is
post-hoc — written after the data existed — so every document it writes is
stamped ``registered_comparison: false``. It reuses the registered thresholds
where it compares anything and invents none of its own. Treat it as
corroboration next to a method verdict, never as one.

The privacy boundary is ``transcripts.py``'s, unchanged: this program folds the
five values ``record_from`` returns and opens no transcript line any other way.
A request's *position* in its own file is not a value read from the line, so
nothing here widens what section 3 allows.

What bounds any claim taken from the fit:

- the model is a two-term least squares with no intercept, fitted across
  contexts of very different jobs; it describes the population, not any one
  agent;
- a context that was compacted or resumed reads as one long context, and
  nothing in the five fields can tell that apart from an agent that simply
  worked for a long time;
- the counterfactual is arithmetic over the fitted coefficients, not an
  experiment. It says what the measured shape implies, and #567 owns turning
  that into something a real mission proves.
"""

import argparse
import collections
import hashlib
import importlib.util
import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
VERSION = 1
METHOD = "docs/quality/mission-cost/method.md"


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
DISPERSION = TRANSCRIPTS.load("dispersion")
DELIVERY = TRANSCRIPTS.load("delivery")

# How many requests a handed-off context spends re-reading the pull request and
# its brief before it does any new work. Eight is the number the counterfactual
# charges per extra context; it is a stated assumption, not a measurement, and
# the document records it so a reader can redo the arithmetic with another one.
HANDOFF_REQUESTS = 8

# The context lengths the counterfactual prices. A cap is a rule of the form
# "hand off to a fresh context after this many requests".
CAPS = (40, 60, 80, 120, 150, 200)

# The policy the lever table prices: the cap it proposes, how much of the
# standing frame it supposes can be cut, and what fraction of today's requests
# it supposes remain. Three stated assumptions, in one place, so that a reader
# who disagrees with one can redo the arithmetic with another number.
# How many of a context's first requests count as its opening, before the
# standing prompt has grown, and the context lengths the length-band table
# reports against.
FIRST_REQUESTS = 10
LENGTH_BANDS = (60, 100, 150)

POLICY_CAP = 80
FRAME_TRIM_TOKENS = 10000
REQUEST_SCALE = 0.8


def fit(rows):
    """Least squares for ``billable = frame * n + slope * n^2``, no intercept.

    ``frame`` is what one request costs before the context has grown at all --
    the system prompt, the tool definitions, the always-loaded instructions.
    ``slope`` is half of what each request adds to the standing prompt that
    every later request in the same context pays again.
    """
    s11 = s12 = s22 = s1y = s2y = 0.0
    for row in rows:
        one = float(row["requests"])
        two = one * one
        value = float(row["billable_tokens"])
        s11 += one * one
        s12 += one * two
        s22 += two * two
        s1y += one * value
        s2y += two * value
    determinant = s11 * s22 - s12 * s12
    if determinant == 0:
        raise ValueError("the request counts carry no second-order information")
    frame = (s1y * s22 - s2y * s12) / determinant
    slope = (s2y * s11 - s1y * s12) / determinant
    law = {
        "frame_tokens_per_request": round(frame, 1),
        "slope_tokens_per_request_squared": round(slope, 2),
        "r_squared": round(_r_squared(rows, frame, slope), 4),
        "linear_only_r_squared": round(_linear_r_squared(rows), 4),
        "contexts": len(rows),
    }
    law["validity"] = validity(law)
    return law


# Why a fitted law can fail to be a cost law at all, in the words the document
# publishes. The fit is an unconstrained least squares, so a narrow population
# -- a short window, one campaign's contexts, a main-only run -- can put the
# minimum at a negative coefficient. A request cannot cost less than nothing
# before its context has grown, and a share of a total cannot sit outside
# [0, 1]; both are arithmetic that came out, not measurements.
NEGATIVE_FRAME = "negative_frame_tokens_per_request"
NEGATIVE_SLOPE = "negative_slope_tokens_per_request_squared"
SHARE_OUTSIDE_UNIT = "share_outside_unit_interval"

# Why a *priced* table can fail, which is not the same question. A lever row is
# arithmetic over a fitted law, so it inherits that law's defects, and the
# policy assumptions can add one of their own.
DEGENERATE_LAW = "priced_on_a_degenerate_law"
NEGATIVE_MODELLED = "negative_modelled_tokens"


def validity(coefficients, term=None):
    """Whether a fitted law can be true, and if not, in which way.

    Data honesty: a law that cannot be true is *labelled* rather than printed
    as a number and left to the reader to catch. The judgement belongs in the
    document because a consumer -- #573 is scheduled on a number this function
    produces -- has to be able to branch on it without re-deriving the
    arithmetic that produced it.

    ``term`` is the matching :func:`terms` block when there is one. The two
    shares are the same law seen from the other end, so they are graded here
    rather than in a second field a reader would have to find.
    """
    reasons = []
    if coefficients["frame_tokens_per_request"] < 0:
        reasons.append(NEGATIVE_FRAME)
    if coefficients["slope_tokens_per_request_squared"] < 0:
        reasons.append(NEGATIVE_SLOPE)
    if term is not None and not all(
        0.0 <= term[name] <= 1.0
        for name in ("standing_frame_share", "accumulation_share")
    ):
        reasons.append(SHARE_OUTSIDE_UNIT)
    return {"usable": not reasons, "degenerate_because": reasons}


def policy_validity(policy, graded):
    """Whether the lever table's numbers can be true.

    The same question as :func:`validity`, one surface further on, and it has
    to be asked separately: a lever row is arithmetic *over* a fitted law, so
    the law can be sound and the row still come out impossible, or the law can
    be degenerate and every row inherit it while looking exactly like the real
    table. #573 is scheduled on the L1 row, so that row is the one place a
    verdict may not be left implicit.

    ``graded`` maps each population name to its graded law. The returned block
    carries the same ``usable`` / ``degenerate_because`` pair a ``cost_law``
    does -- one shape to branch on, whichever surface an agent reached -- plus
    the names of the populations at fault, because "which one" is the first
    thing a reader asks next.
    """
    reasons = []
    unusable = sorted(
        name
        for name, law in graded.items()
        if not (law.get("validity") or {}).get("usable", True)
    )
    if unusable:
        reasons.append(DEGENERATE_LAW)
    modelled_tokens = [policy["baseline_modelled_tokens"]]
    modelled_tokens += [row["modelled_tokens"] for row in policy["levers"]]
    if any(value < 0 for value in modelled_tokens):
        reasons.append(NEGATIVE_MODELLED)
    return {
        "usable": not reasons,
        "degenerate_because": reasons,
        "degenerate_populations": unusable,
    }


def _r_squared(rows, frame, slope):
    mean = sum(row["billable_tokens"] for row in rows) / len(rows)
    residual = sum(
        (row["billable_tokens"] - frame * row["requests"] - slope * row["requests"] ** 2)
        ** 2
        for row in rows
    )
    total = sum((row["billable_tokens"] - mean) ** 2 for row in rows)
    return 1.0 - residual / total if total else 0.0


def _linear_r_squared(rows):
    """The same fit with the second term removed, so the reader can see it earn its place."""
    denominator = sum(row["requests"] ** 2 for row in rows)
    if not denominator:
        return 0.0
    coefficient = (
        sum(row["requests"] * row["billable_tokens"] for row in rows) / denominator
    )
    mean = sum(row["billable_tokens"] for row in rows) / len(rows)
    residual = sum(
        (row["billable_tokens"] - coefficient * row["requests"]) ** 2 for row in rows
    )
    total = sum((row["billable_tokens"] - mean) ** 2 for row in rows)
    return 1.0 - residual / total if total else 0.0


def terms(rows, coefficients):
    """Split a population's modelled cost into its standing and growing halves."""
    frame = coefficients["frame_tokens_per_request"]
    slope = coefficients["slope_tokens_per_request_squared"]
    standing = frame * sum(row["requests"] for row in rows)
    growing = slope * sum(row["requests"] ** 2 for row in rows)
    modelled = standing + growing
    return {
        "standing_frame_tokens": round(standing),
        "accumulation_tokens": round(growing),
        "modelled_tokens": round(modelled),
        "actual_tokens": sum(row["billable_tokens"] for row in rows),
        "standing_frame_share": round(standing / modelled, 4) if modelled else 0.0,
        "accumulation_share": round(growing / modelled, 4) if modelled else 0.0,
    }


def concentration(rows, fractions=(0.1, 0.25, 0.5)):
    """How much of a population's cost the most expensive contexts hold."""
    ordered = sorted(rows, key=lambda row: -row["billable_tokens"])
    total = sum(row["billable_tokens"] for row in ordered)
    found = []
    for fraction in fractions:
        take = max(1, int(len(ordered) * fraction))
        held = sum(row["billable_tokens"] for row in ordered[:take])
        found.append(
            {
                "top_fraction_of_contexts": fraction,
                "contexts": take,
                "share_of_tokens": round(held / total, 4) if total else 0.0,
                "smallest_request_count": ordered[take - 1]["requests"],
            }
        )
    return found


def pieces_for(requests, cap):
    """How many contexts a run of ``requests`` becomes under ``cap``."""
    if cap is None:
        return 1
    return max(1, math.ceil(requests / cap))


def modelled(rows, laws, cap=None, handoff=HANDOFF_REQUESTS, trim=0, scale=1.0,
             law=None):
    """What a population costs under one policy, on the fitted coefficients.

    ``cap`` splits each context into equal pieces no longer than it. The
    handoff is charged **once per extra piece**, not once per piece: a run of
    ``N`` requests cut into ``k`` contexts re-reads its brief ``k - 1`` times,
    because the first context is the one that wrote it.

    ``law`` names the cost law every piece is priced on. ``None`` prices each
    context on its own population's law, which is what the baseline does.
    Naming one -- in practice ``"subagent"`` -- says every piece runs as a fresh
    dispatch, which is the whole content of the capping proposal and is
    therefore stated here rather than assumed.
    """
    total = 0.0
    for row in rows:
        chosen = laws[law] if law is not None else laws[row["kind"]]
        frame = chosen["frame_tokens_per_request"] - trim
        slope = chosen["slope_tokens_per_request_squared"]
        requests = row["requests"] * scale
        parts = pieces_for(requests, cap)
        each = (requests + handoff * (parts - 1)) / parts
        total += parts * (frame * each + slope * each * each)
    return total


def length_bands(rows, bands=LENGTH_BANDS):
    """How much of a population's cost sits in its longer contexts.

    The concentration view asks "how much do the dearest contexts hold"; this
    asks "how much sits above a length a cap could actually name", which is the
    question a capping rule is chosen on.
    """
    total = sum(row["billable_tokens"] for row in rows)
    found = []
    for band in bands:
        over = [row for row in rows if row["requests"] > band]
        found.append(
            {
                "requests_over": band,
                "contexts": len(over),
                "of_contexts": len(rows),
                "share_of_tokens": round(
                    sum(row["billable_tokens"] for row in over) / total, 4
                )
                if total
                else 0.0,
            }
        )
    return found


def opening_cache_reads(rows):
    """Mean cache reads per request over each context's first requests.

    The bottom of the accumulation curve, measured rather than modelled. It is
    the number the fitted frame coefficient should be read against.
    """
    reads = sum(row["opening_cache_reads"] for row in rows)
    requests = sum(row["opening_requests"] for row in rows)
    return {
        "first_requests_per_context": FIRST_REQUESTS,
        "requests": requests,
        "mean_cache_read_tokens": round(reads / requests) if requests else 0,
    }


def counterfactual(rows, coefficients, caps=CAPS, handoff=HANDOFF_REQUESTS):
    """One population's cap sweep, priced on its own law throughout.

    This is the *within-population* view: it answers "what would capping have
    done to these contexts", and it never borrows another population's law.
    The cross-population policy view is :func:`levers`, which says in its own
    output which law it used.
    """
    laws = {"only": coefficients}
    rows = [dict(row, kind="only") for row in rows]
    baseline = modelled(rows, laws, cap=None, handoff=0)
    found = []
    for cap in caps:
        total = modelled(rows, laws, cap=cap, handoff=handoff)
        found.append(
            {
                "cap_requests": cap,
                "modelled_tokens": round(total),
                "change": round(total / baseline - 1.0, 4) if baseline else 0.0,
            }
        )
    return {
        "handoff_requests": handoff,
        "baseline_modelled_tokens": round(baseline),
        "caps": found,
    }


def levers(rows, laws, cap=POLICY_CAP, handoff=HANDOFF_REQUESTS,
           trim=FRAME_TRIM_TOKENS, scale=REQUEST_SCALE):
    """The three levers the ranking proposes, alone and stacked.

    The baseline prices every context on its own population's law. So does
    every row that does not cap, because trimming the frame or asking for fewer
    requests does not change where an agent runs. Every row that **does** cap
    prices its pieces on the subagent law, because a handed-off piece is a
    fresh dispatch by construction -- and each row says which law it used, so a
    reader never has to infer it.
    """
    baseline = modelled(rows, laws, cap=None, handoff=0)
    # A scale above 1 is a legal question -- "what if we made more requests?" --
    # and "-20% fewer requests" is not the way to ask it.
    fewer = (
        f"{1 - scale:.0%} fewer requests"
        if scale <= 1
        else f"{scale - 1:.0%} more requests"
    )
    plans = (
        ("L1", f"cap every context at {cap} requests, handing off to a fresh "
               "dispatch", dict(cap=cap, law="subagent")),
        ("L1+L2", f"the same, and {trim:,} tokens off the standing frame",
         dict(cap=cap, law="subagent", trim=trim)),
        ("L1+L3", f"the same as L1, and {fewer}",
         dict(cap=cap, law="subagent", scale=scale)),
        ("L1+L2+L3", "all three together",
         dict(cap=cap, law="subagent", trim=trim, scale=scale)),
        ("L2", f"{trim:,} tokens off the standing frame, no cap",
         dict(trim=trim)),
        ("L3", f"{fewer}, no cap", dict(scale=scale)),
    )
    found = []
    for name, description, plan in plans:
        total = modelled(rows, laws, handoff=handoff, **plan)
        found.append(
            {
                "lever": name,
                "description": description,
                "cap_requests": plan.get("cap"),
                "law": plan.get("law", "each population's own"),
                "frame_trim_tokens": plan.get("trim", 0),
                "request_scale": plan.get("scale", 1.0),
                "modelled_tokens": round(total),
                "change": round(total / baseline - 1.0, 4) if baseline else 0.0,
            }
        )
    return {
        # The four stated assumptions this table was priced on, recorded here
        # rather than recoverable only by reading the source at the right
        # commit. Every one of them is a flag on `measure`.
        "handoff_requests": handoff,
        "cap_requests": cap,
        "frame_trim_tokens": trim,
        "request_scale": scale,
        "baseline_modelled_tokens": round(baseline),
        "baseline_law": "each population's own",
        "levers": found,
    }


def bounded(path, since, until):
    """Fold one transcript, counting only the requests inside the window.

    The window has to cut **requests**, not whole files. The transcript
    directory is live and append-only, so a context that is still running when
    a reading is taken keeps growing afterwards; dropping or keeping the whole
    file either way makes the reading unrepeatable, because tomorrow's run over
    the same closed window would see a longer file. Counting only the requests
    at or before ``until`` makes the answer stable, since a request already
    made never changes.

    The privacy boundary is ``transcripts.record_from``, as everywhere else in
    this module: five values come out of a line and the line is dropped.
    """
    requests = 0
    opening = opened = 0
    counters = {name: 0 for name in TRANSCRIPTS.COUNTERS}
    first = last = None
    active = 0.0
    previous = None
    with open(path, "r", encoding="utf-8", errors="replace") as stream:
        for line in stream:
            if not line.strip():
                continue
            try:
                parsed = json.loads(line)
            except ValueError:
                continue
            record = TRANSCRIPTS.record_from(parsed)
            if record is None:
                continue
            when = record.timestamp
            if since is not None and when < since:
                continue
            if until is not None and when > until:
                continue
            requests += 1
            if requests <= FIRST_REQUESTS:
                opening += record.cache_read_input_tokens
                opened += 1
            for name in TRANSCRIPTS.COUNTERS:
                counters[name] += getattr(record, name)
            if first is None or when < first:
                first = when
            last = when if last is None or when > last else last
            if previous is not None:
                gap = (when - previous).total_seconds()
                if 0 <= gap <= TRANSCRIPTS.IDLE_GAP_SECONDS:
                    # A running total, not a list: `Transcript.add` keeps work
                    # blocks rather than requests for a reason, and a buffer
                    # proportional to the number of requests ever made would
                    # give that back.
                    active += gap
            previous = when
    if not requests:
        return None
    return {
        "requests": requests,
        "billable_tokens": sum(counters.values()),
        "cache_read_input_tokens": counters["cache_read_input_tokens"],
        "output_tokens": counters["output_tokens"],
        "first": first,
        "last": last,
        "active_seconds": active,
        "opening_cache_reads": opening,
        "opening_requests": opened,
    }


def contexts(roots, since, until):
    """Every transcript with a request in the window, folded to one row each.

    ``locate`` rather than ``discover``: this module folds each file itself,
    through :func:`bounded`, because the window has to cut requests rather than
    whole files. ``discover`` would fold every file first and this would then
    throw that fold away and read the same lines again -- two passes over a
    directory that is append-only and never pruned. ``locate`` walks and
    classifies without opening anything, so each file is read exactly once and
    ``transcripts.classify`` stays the one owner of the layout rule.
    """
    rows = []
    for root in roots:
        found, _ = TRANSCRIPTS.locate(root)
        for item in found:
            folded = bounded(item.path, since, until)
            if folded is None:
                continue
            rows.append(
                {
                    "relative": item.relative,
                    "root": item.root,
                    "kind": item.kind,
                    "requests": folded["requests"],
                    "billable_tokens": folded["billable_tokens"],
                    "cache_read_input_tokens": folded["cache_read_input_tokens"],
                    "output_tokens": folded["output_tokens"],
                    "first": folded["first"].isoformat(),
                    "last": folded["last"].isoformat(),
                    "active_seconds": round(folded["active_seconds"]),
                    "opening_cache_reads": folded["opening_cache_reads"],
                    "opening_requests": folded["opening_requests"],
                }
            )
    rows.sort(key=lambda row: (row["root"], row["relative"]))
    return rows


def digest(rows):
    """A fingerprint of the input set, for the reason section 7 gives."""
    hasher = hashlib.sha256()
    for row in rows:
        hasher.update(
            f"{row['root']}/{row['relative']}:{row['requests']}:"
            f"{row['billable_tokens']}\n".encode("ascii", "replace")
        )
    return hasher.hexdigest()


class PolicyError(ValueError):
    """A policy assumption that cannot be priced on the population measured."""


def check_policy(laws, trim):
    """Refuse a frame trim the fitted population cannot absorb.

    The cheap domains of the four flags are ``argparse``'s, because they need
    nothing but the value. This one needs the fit, so it lives here: trimming
    more than a request costs before its context has grown prices that request
    at nothing or less, and the lever table would then report a saving larger
    than the whole bill.

    Only *usable* laws bound the trim. A degenerate law has a negative frame
    already, and refusing the run over it would blame the flag for the fit;
    :func:`validity` and :func:`policy_validity` label that case instead, which
    is the honest answer rather than a refusal aimed at the wrong thing.
    """
    frames = [
        law["frame_tokens_per_request"]
        for law in laws.values()
        if (law.get("validity") or {}).get("usable", True)
    ]
    if frames and trim >= min(frames):
        raise PolicyError(
            f"--frame-trim {trim} is not below the smallest fitted frame "
            f"({min(frames)} tokens per request); trimming that much prices a "
            "request at nothing or less"
        )


def build_shape(rows, since, until, cap=POLICY_CAP, handoff=HANDOFF_REQUESTS,
                trim=FRAME_TRIM_TOKENS, scale=REQUEST_SCALE):
    """The whole shape document: population, fit, terms, concentration, caps.

    The four policy assumptions arrive as arguments rather than being read off
    the module, because ``measure`` gives each of them a flag: a reader who
    disagrees with one redoes the arithmetic from the command line, and the
    ``policy`` block records what the run actually used.
    """
    population = {
        "subagent": [row for row in rows if row["kind"] == "subagent"],
        "main": [row for row in rows if row["kind"] == "main"],
    }
    document = {
        "schema": VERSION,
        "method": METHOD,
        "metric": "context_cost_shape",
        "registered_comparison": False,
        "window": {"since": since, "until": until},
        "inputs": {
            "contexts": len(rows),
            "requests": sum(row["requests"] for row in rows),
            "billable_tokens": sum(row["billable_tokens"] for row in rows),
            "digest": digest(rows),
        },
        "populations": {},
    }
    laws = {
        name: fit(group) for name, group in population.items() if group
    }
    # Grade every law before anything is priced on one. The republished law
    # carries the judgement the shares complete: a negative coefficient and a
    # share outside [0, 1] are the same defect seen twice, so they are graded
    # in one field rather than two a reader would have to find.
    graded = {}
    for name, group in population.items():
        if not group:
            continue
        term = terms(group, laws[name])
        graded[name] = (
            dict(laws[name], validity=validity(laws[name], term)),
            term,
        )
    judged = {name: law for name, (law, _) in graded.items()}
    check_policy(judged, trim)
    if "subagent" in laws:
        document["policy"] = levers(
            rows, laws, cap=cap, handoff=handoff, trim=trim, scale=scale
        )
        # The verdict has to reach the table the campaign acts on, not stop at
        # the coefficients it came from. Two surfaces reading one document must
        # not disagree about whether its numbers are real.
        document["policy"]["validity"] = policy_validity(document["policy"], judged)
    for name, group in population.items():
        if not group:
            continue
        coefficients, term = graded[name]
        document["populations"][name] = {
            "contexts": len(group),
            "requests": sum(row["requests"] for row in group),
            "billable_tokens": sum(row["billable_tokens"] for row in group),
            "active_seconds": sum(row["active_seconds"] for row in group),
            "requests_per_context": DISPERSION.summary(
                [row["requests"] for row in group]
            ),
            "billable_per_context": DISPERSION.summary(
                [row["billable_tokens"] for row in group]
            ),
            "cost_law": coefficients,
            "terms": term,
            "concentration": concentration(group),
            "length_bands": length_bands(group),
            "opening": opening_cache_reads(group),
            "counterfactual": counterfactual(group, coefficients, handoff=handoff),
        }
    return document


# ---------------------------------------------------------------- ceremony ---

# How many nodes each connection is asked for. Named here rather than spelled
# into the query alone, because the truncation check compares against them and
# a cap that drifted from its own check would be worse than no check at all.
PAGES = {"comments": 60, "commits": 100, "reviewThreads": 100}

CEREMONY_QUERY = """
query($owner:String!,$repo:String!,$num:Int!){
  repository(owner:$owner,name:$repo){
    pullRequest(number:$num){
      number additions deletions createdAt bodyText
      comments(first:%(comments)d){totalCount nodes{createdAt body}}
      commits(first:%(commits)d){totalCount nodes{commit{committedDate}}}
      reviewThreads(first:%(reviewThreads)d){totalCount nodes{isResolved}}
    }
  }
}
""" % PAGES

REPORT_MARKER = "<!-- quantick-review-report"


def truncation(pull):
    """Which connections came back short, and by how much.

    A cost-per-catch number is a ratio whose denominator is counted here, so a
    page that silently dropped a durable report or a hundred commits would move
    the answer with nothing saying it had. PR #542 has 176 commits against a
    page of 100, and the ``comments`` cap is sharper still because the report
    filter runs *after* the page. So each connection is asked for its
    ``totalCount`` and the document records what it did not see, rather than
    reporting a truncated list as the whole.

    Returns a mapping of connection name to ``{fetched, total}``, empty when
    everything asked for arrived.
    """
    short = {}
    for name in sorted(PAGES):
        connection = pull.get(name) or {}
        total = connection.get("totalCount")
        fetched = len(connection.get("nodes") or ())
        if isinstance(total, int) and total > fetched:
            short[name] = {"fetched": fetched, "total": total}
    return short


def ceremony_command(owner, repo, number):
    return [
        "gh",
        "api",
        "graphql",
        "-f",
        f"query={CEREMONY_QUERY}",
        "-F",
        f"owner={owner}",
        "-F",
        f"repo={repo}",
        "-F",
        f"num={number}",
    ]


def _kind_of(body):
    marker = "kind="
    start = body.find(marker)
    if start < 0:
        return None
    start += len(marker)
    end = start
    while end < len(body) and (body[end].isalpha() or body[end] == "-"):
        end += 1
    return body[start:end] or None


def _step_zero(body):
    """The finding count a durable arch-review report states about its own step 0.

    The skill writes the sentence; this reads the integer in front of the word
    ``findings`` on the ``step 0`` line and nothing else on it.
    """
    for line in body.splitlines():
        lowered = line.lower()
        if not lowered.startswith("step 0"):
            continue
        words = lowered.replace(",", " ").split()
        for index, word in enumerate(words):
            if word.startswith("finding") and index:
                try:
                    return int(words[index - 1])
                except ValueError:
                    return None
    return None


TIERS = ("small", "medium", "high", "max")


def _tier_of(text):
    """The tier a pull request body declares, or ``None``.

    Both spellings in use reach here as ``Tier: `` once the markdown is
    stripped -- the mission-summary block's plain ``Tier: medium`` and the goal
    archive's ``**Tier:** `medium`.`` -- and the second one arrives with the
    sentence's full stop still attached, so the word is cut at the first
    character that is not a letter rather than at the first space.
    """
    marker = "Tier: "
    start = text.find(marker)
    if start < 0:
        return None
    start += len(marker)
    end = start
    while end < len(text) and text[end].isalpha():
        end += 1
    word = text[start:end].lower()
    return word if word in TIERS else None


def pull_ceremony(owner, repo, number, runner=DELIVERY.shell):
    """One pull request's ceremony facts. No prose leaves this function."""
    found = json.loads(runner(ceremony_command(owner, repo, number)))
    pull = found["data"]["repository"]["pullRequest"]
    opened = TRANSCRIPTS.require_instant(pull["createdAt"], f"#{number}: createdAt")
    reports = {}
    step_zero = []
    for node in pull["comments"]["nodes"]:
        body = node["body"]
        if not body.startswith(REPORT_MARKER):
            continue
        kind = _kind_of(body)
        if kind is None:
            continue
        reports[kind] = reports.get(kind, 0) + 1
        if kind == "arch-review":
            count = _step_zero(body)
            if count is not None:
                step_zero.append(count)
    before = after = 0
    for node in pull["commits"]["nodes"]:
        when = TRANSCRIPTS.require_instant(
            node["commit"]["committedDate"], f"#{number}: committedDate"
        )
        if when < opened:
            before += 1
        else:
            after += 1
    tier = _tier_of(pull.get("bodyText") or "")
    threads = pull["reviewThreads"]["nodes"]
    return {
        "pr": pull["number"],
        "tier": tier,
        "churn": pull["additions"] + pull["deletions"],
        "reports": reports,
        "step_zero_rounds": len(step_zero),
        "step_zero_findings": sum(step_zero),
        "step_zero_empty_rounds": sum(1 for count in step_zero if count == 0),
        "ai_review_threads": len(threads),
        "ai_review_threads_open": sum(1 for one in threads if not one["isResolved"]),
        "commits_before_pr": before,
        "commits_after_pr": after,
        "truncated": truncation(pull),
    }


def ceremony_digest(rows):
    """A fingerprint of the pull requests read, for the reason section 7 gives.

    The counterpart of :func:`digest`, over ``gh`` facts rather than
    transcripts. It covers every value a total is summed from, so a reopened
    pull request, an edited comment, one more durable report or one more
    registry entry changes the digest and a reader can tell a changed input
    from a changed harness.
    """
    hasher = hashlib.sha256()
    for row in sorted(rows, key=lambda row: row["pr"]):
        reports = ";".join(f"{k}={v}" for k, v in sorted(row["reports"].items()))
        hasher.update(
            f"{row['pr']}:{row['tier']}:{row['churn']}:{reports}:"
            f"{row['step_zero_rounds']}:{row['step_zero_findings']}:"
            f"{row['step_zero_empty_rounds']}:{row['ai_review_threads']}:"
            f"{row['ai_review_threads_open']}:{row['commits_before_pr']}:"
            # The counts, not only the connection names: a pull request whose
            # commits grow 176 -> 300 stays truncated on the same connection,
            # and a digest over names alone would not move although the input
            # did.
            f"{row['commits_after_pr']}:{sorted(row['truncated'].items())}\n".encode(
                "ascii", "replace"
            )
        )
    return hasher.hexdigest()


def build_ceremony(rows, registry=None):
    """Totals and per-tier rollups over the pull requests read."""
    document = {
        "schema": VERSION,
        "method": METHOD,
        "metric": "ceremony_cost_per_catch",
        "registered_comparison": False,
        "missions": len(rows),
        "inputs": {
            "missions": len(rows),
            # Forward slashes whatever the host: a document read on one
            # platform and re-derived on another must not differ by a
            # separator.
            "registry": registry.replace(os.sep, "/") if registry else registry,
            "pulls": sorted(row["pr"] for row in rows),
            "digest": ceremony_digest(rows),
        },
        "totals": {
            "arch_review_reports": sum(r["reports"].get("arch-review", 0) for r in rows),
            "ai_review_reports": sum(r["reports"].get("ai-review", 0) for r in rows),
            "delivery_review_reports": sum(
                r["reports"].get("delivery-review", 0) for r in rows
            ),
            "step_zero_rounds": sum(r["step_zero_rounds"] for r in rows),
            "step_zero_findings": sum(r["step_zero_findings"] for r in rows),
            "step_zero_empty_rounds": sum(r["step_zero_empty_rounds"] for r in rows),
            "ai_review_threads": sum(r["ai_review_threads"] for r in rows),
            "ai_review_threads_open": sum(r["ai_review_threads_open"] for r in rows),
            "commits_before_pr": sum(r["commits_before_pr"] for r in rows),
            "commits_after_pr": sum(r["commits_after_pr"] for r in rows),
            # Not a rollup of the others: the count of pull requests whose
            # facts arrived short. Above zero, every total on this document is
            # a floor rather than a number.
            "truncated_pulls": sum(1 for r in rows if r["truncated"]),
        },
        "tiers": {},
    }
    for tier in ("small", "medium", "high", "max", "unrecorded"):
        group = [r for r in rows if (r["tier"] or "unrecorded") == tier]
        if not group:
            continue
        document["tiers"][tier] = {
            "missions": len(group),
            "churn": DISPERSION.summary([r["churn"] for r in group]),
            "ai_review_threads": sum(r["ai_review_threads"] for r in group),
            "ai_review_threads_per_mission": round(
                sum(r["ai_review_threads"] for r in group) / len(group), 3
            ),
            "arch_review_reports": sum(
                r["reports"].get("arch-review", 0) for r in group
            ),
            "delivery_review_reports": sum(
                r["reports"].get("delivery-review", 0) for r in group
            ),
            "step_zero_findings": sum(r["step_zero_findings"] for r in group),
            "repair_commits_per_mission": round(
                sum(r["commits_after_pr"] for r in group) / len(group), 3
            ),
        }
    document["pulls"] = sorted(rows, key=lambda row: row["pr"])
    return document


# ------------------------------------------------------------------- table ---

FIGURES_BEGIN = "<!-- shape-figures:v1 -->"
FIGURES_END = "<!-- end shape-figures:v1 -->"
LEVERS_BEGIN = "<!-- shape-levers:v1 -->"
LEVERS_END = "<!-- end shape-levers:v1 -->"


def _thousands(value):
    return f"{round(value):,}"


# The three states a graded block can be in, for every renderer at once.
# "Was it graded" and "did it pass" are two questions, and collapsing them is
# the same mistake as reading an absent `truncated_pulls` as "nothing was
# missed": a document that predates the field says nothing either way, and a
# block reporting that silence as a pass has invented the reading this whole
# mechanism exists to prevent.
GRADED = "graded"
UNGRADED = "ungraded"
DEGENERATE = "degenerate"

UNGRADED_NOTE = "verdict not recorded; this document predates the check"


def verdict(block):
    """Which of three states a block carrying a ``validity`` field is in.

    The single owner of that question. The two generated blocks drifted apart
    on it inside one commit -- :func:`_truncation_figure` kept *absent* and
    *zero* apart while the lever table read *absent* as sound, and the table is
    the surface #573 is scheduled on. Rendering an older document rather than
    crashing is right; asserting it was graded and passed is not.

    Returns ``(state, judged)``, so a caller can name the reasons without
    reaching for the field a second time.
    """
    judged = block.get("validity")
    if not isinstance(judged, dict) or not isinstance(judged.get("usable"), bool):
        return UNGRADED, {}
    return (GRADED if judged["usable"] else DEGENERATE), judged


def _warning(law):
    """What a law's cell is labelled with: nothing, ungraded, or degenerate.

    One owner, because every cell rendered off a law has to carry the same
    verdict. A document that grades a law and a report that prints its numbers
    unmarked are two surfaces disagreeing about whether a number is real.
    """
    state, judged = verdict(law)
    if state == GRADED:
        return ""
    if state == UNGRADED:
        return f" *— {UNGRADED_NOTE}*"
    return " **— degenerate: " + ", ".join(judged["degenerate_because"]) + "**"


def _population_states(shape):
    """Each population's verdict state, so one row can speak for all of them."""
    found = {}
    for name, group in shape["populations"].items():
        state, _ = verdict(group["cost_law"])
        found.setdefault(state, []).append(name)
    return {state: sorted(names) for state, names in found.items()}


def figures(shape, ceremony):
    """The load-bearing figures, rendered from the committed JSON.

    #565 lost three review rounds to figures retyped into prose out of a file
    that already held them. The answer is a generator rather than a checker: a
    checker that stops matching the prose goes quiet, while a generator that
    stops matching fails its test loudly.
    """
    lines = [FIGURES_BEGIN, ""]
    lines.append("| Figure | Value |")
    lines.append("| --- | ---: |")
    inputs = shape["inputs"]
    lines.append(f"| Agent contexts read | {inputs['contexts']:,} |")
    lines.append(f"| Requests | {inputs['requests']:,} |")
    lines.append(f"| Billable tokens | {inputs['billable_tokens']:,} |")
    for name in ("subagent", "main"):
        group = shape["populations"].get(name)
        if group is None:
            continue
        law = group["cost_law"]
        term = group["terms"]
        lines.append(
            f"| {name.capitalize()} contexts | {group['contexts']:,} "
            f"({group['requests']:,} requests, {group['billable_tokens']:,} tokens) |"
        )
        # A law the document itself calls degenerate says so in the report too.
        # Rendering `-112.6% / 212.6%` as though it were a reading is the exact
        # data-honesty failure the `validity` field exists to make impossible.
        warning = _warning(law)
        lines.append(
            f"| {name.capitalize()} cost law, tokens | "
            f"{_thousands(law['frame_tokens_per_request'])}·N + "
            f"{law['slope_tokens_per_request_squared']}·N² "
            f"(R² {law['r_squared']}, linear-only R² {law['linear_only_r_squared']})"
            f"{warning} |"
        )
        # The shares are the same law seen from the other end, so a warning on
        # the line above does not cover them: `-112.6% / 212.6%` has to carry
        # its own, or a reader skimming this row alone takes it for a reading.
        lines.append(
            f"| {name.capitalize()} standing frame / accumulation | "
            f"{term['standing_frame_share']:.1%} / {term['accumulation_share']:.1%}"
            f"{warning} |"
        )
    both = _combined(shape)
    # The headline row sums every population, so one degenerate population is
    # enough to make it arithmetic rather than a reading -- and one ungraded
    # population is enough to make the sum ungraded.
    states = _population_states(shape)
    combined_warning = ""
    if states.get(DEGENERATE):
        combined_warning = f" **— degenerate: {', '.join(states[DEGENERATE])}**"
    elif states.get(UNGRADED):
        combined_warning = f" *— {UNGRADED_NOTE}: {', '.join(states[UNGRADED])}*"
    lines.append(
        f"| **All contexts, standing frame / accumulation** | "
        f"**{both['standing_frame_share']:.1%} / {both['accumulation_share']:.1%}**"
        f"{combined_warning} |"
    )
    opening = shape["populations"]["subagent"]["opening"]
    lines.append(
        f"| Mean cache reads per request over a subagent context's first "
        f"{opening['first_requests_per_context']} requests | "
        f"{opening['mean_cache_read_tokens']:,} |"
    )
    for entry in shape["populations"]["subagent"]["length_bands"]:
        lines.append(
            f"| Subagent contexts over {entry['requests_over']} requests | "
            f"{entry['contexts']} of {entry['of_contexts']}, holding "
            f"{entry['share_of_tokens']:.1%} |"
        )
    for entry in shape["populations"]["subagent"]["concentration"]:
        lines.append(
            f"| Top {entry['top_fraction_of_contexts']:.0%} of subagent contexts "
            f"({entry['contexts']}) hold | {entry['share_of_tokens']:.1%} |"
        )
    totals = ceremony["totals"]
    lines.append(f"| Missions read for ceremony | {ceremony['missions']:,} |")
    lines.append(
        f"| arch-review / ai-review / delivery-review durable reports | "
        f"{totals['arch_review_reports']} / {totals['ai_review_reports']} / "
        f"{totals['delivery_review_reports']} |"
    )
    lines.append(
        f"| step-0 findings over reported rounds | "
        f"{totals['step_zero_findings']} over {totals['step_zero_rounds']} "
        f"({totals['step_zero_empty_rounds']} empty) |"
    )
    lines.append(
        f"| AI-review threads, of which open | "
        f"{totals['ai_review_threads']} / {totals['ai_review_threads_open']} |"
    )
    commits = totals["commits_before_pr"] + totals["commits_after_pr"]
    repair = totals["commits_after_pr"] / commits if commits else 0.0
    lines.append(
        f"| Commits before / after the pull request existed | "
        f"{totals['commits_before_pr']} / {totals['commits_after_pr']} "
        f"({repair:.1%} repair) |"
    )
    lines.append(
        f"| Pull requests whose ceremony facts arrived short | "
        f"{_truncation_figure(ceremony)} |"
    )
    lines.append("")
    lines.append(FIGURES_END)
    return "\n".join(lines) + "\n"


def _truncation_figure(ceremony):
    """What the ceremony document says about its own completeness.

    Three different claims, and the report may not collapse them. A count of
    zero says the reading was checked and nothing was missed. A count above
    zero says every ceremony total above is a floor rather than a number. An
    **absent** field says neither -- the document predates the check, so its
    completeness is simply unknown, and printing that as "none" would be the
    invention this whole block exists to prevent.
    """
    short = ceremony["totals"].get("truncated_pulls")
    if short is None:
        return "not recorded — this reading predates the check, so completeness is unknown"
    if short:
        return f"{short} of {ceremony['missions']:,} — **every ceremony total above is a floor**"
    return f"0 of {ceremony['missions']:,}"


def _combined(shape):
    """The two populations' terms added, because the ranking is over both."""
    standing = sum(
        group["terms"]["standing_frame_tokens"]
        for group in shape["populations"].values()
    )  # every population present contributes; an absent one simply is not there
    growing = sum(
        group["terms"]["accumulation_tokens"] for group in shape["populations"].values()
    )
    total = standing + growing
    return {
        "standing_frame_tokens": standing,
        "accumulation_tokens": growing,
        "standing_frame_share": standing / total if total else 0.0,
        "accumulation_share": growing / total if total else 0.0,
    }


def lever_table(shape, ceremony=None):
    """The lever table, rendered from the committed policy block.

    ``ceremony`` is accepted and unused so that every renderer in ``BLOCKS``
    has one signature and the registry needs no per-block calling convention.

    #572's architecture review found the first version of this table computed
    by hand beside the document rather than out of it, with one row silently
    priced on a different cost law than the baseline it was compared against.
    The fix is not a sharper reviewer: it is that this table, like the figures
    block, is generated, and that every row carries the law it used.
    """
    policy = shape["policy"]
    baseline = policy["baseline_modelled_tokens"]
    lines = [LEVERS_BEGIN, ""]
    state, judged = verdict(policy)
    columns = "Modelled tokens | Change"
    if state == UNGRADED:
        # The third state, in the same voice as the degenerate one and with the
        # same standard as the truncation row: a document that predates the
        # check was not graded, which is not the same claim as graded and
        # sound. The columns stay unmarked, because nothing here is known to
        # be wrong either.
        lines.append(
            f"> **Not graded.** This table carries no verdict at all — "
            f"{UNGRADED_NOTE} — so nothing here is claimed either way: not "
            f"that its numbers can be true, and not that they cannot. Re-run "
            f"`shape.py measure` to grade it."
        )
        lines.append("")
    elif state == DEGENERATE:
        # The numbers stay, because the arithmetic has to remain inspectable;
        # what changes is that nothing here is offered as a reading. A row of
        # this table is the number #573 is scheduled on.
        at_fault = ", ".join(judged.get("degenerate_populations") or ()) or "unknown"
        lines.append(
            f"> **Not a reading.** This table is priced on a cost law the "
            f"document itself grades as degenerate ({at_fault}: "
            f"{', '.join(judged['degenerate_because'])}), so every number below "
            f"is arithmetic over a law that cannot be true. Read "
            f"`policy.validity` before quoting any of it."
        )
        lines.append("")
        columns = "Modelled tokens (not a reading) | Change (not a reading)"
    lines.append(f"| Levers applied | Cost law | {columns} |")
    lines.append("| --- | --- | ---: | ---: |")
    lines.append(
        f"| Baseline (modelled) | {policy['baseline_law']} | "
        f"{baseline:,} | — |"
    )
    for row in policy["levers"]:
        lines.append(
            f"| **{row['lever']}** — {row['description']} | {row['law']} | "
            f"{row['modelled_tokens']:,} | **{row['change']:+.1%}** |"
        )
    lines.append("")
    lines.append(
        f"Handoff charged at {policy['handoff_requests']} requests per *extra* "
        "context, once per handoff rather than once per piece."
    )
    lines.append("")
    lines.append(LEVERS_END)
    return "\n".join(lines) + "\n"


def extract_block(markdown, begin, end):
    """The delimited block a document carries, or ``None`` when it carries none."""
    start = markdown.find(begin)
    stop = markdown.find(end)
    if start < 0 or stop < 0:
        return None
    return markdown[start : stop + len(end)] + "\n"


Block = collections.namedtuple("Block", ("begin", "end", "render", "needs_ceremony"))
"""One generated block: its delimiters, what writes it, and what it reads."""

BLOCKS = {
    "figures": Block(FIGURES_BEGIN, FIGURES_END, figures, True),
    "levers": Block(LEVERS_BEGIN, LEVERS_END, lever_table, False),
}
"""Every generated block, keyed by the name ``--block`` takes.

A registry rather than a closed choice, because the data is uniform and the
next block should cost one entry. Adding ``levers`` beside ``figures`` during
this pull request's own repair round cost five edits -- two constants, a
renderer, an ``extract_*`` wrapper, a ``choices`` tuple and a branch in
``main``. Four of those five were bookkeeping; this table is the four.
"""


def extract(markdown, name):
    """The named block a document carries, or ``None`` when it carries none."""
    block = BLOCKS[name]
    return extract_block(markdown, block.begin, block.end)


# --------------------------------------------------------------- commands ---

# The domain of each policy flag that can be checked from the value alone,
# lowest legal value first. A flag added to let an operator vary a stated
# assumption must not also let them drive the published document to a value
# that cannot mean anything -- or, for `--cap 0`, to a traceback. The bound
# that needs the fit is `check_policy`'s, not argparse's.
POLICY_DOMAINS = (
    ("cap", 1, "at least 1 request"),
    ("handoff", 0, "not negative"),
    ("frame_trim", 0, "not negative"),
)


def check_domains(parser, options):
    """Refuse a policy flag outside its domain, by name and by bound.

    A named refusal, the way ``measure`` already refuses a missing transcript
    directory. A traceback tells an operator only that something broke; a
    document quietly priced at ``--cap -5`` tells them nothing at all, because
    ``max(1, ceil(negative))`` is one piece and the row then reports a
    plausible number that means nothing.
    """
    for name, lowest, described in POLICY_DOMAINS:
        value = getattr(options, name)
        if value < lowest:
            parser.error(f"--{name.replace('_', '-')} must be {described}; got {value}")
    if options.request_scale <= 0:
        parser.error(
            f"--request-scale must be greater than 0; got {options.request_scale}"
        )


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Context cost shape and ceremony price; not a method verdict"
    )
    modes = parser.add_subparsers(dest="mode", required=True)

    measure = modes.add_parser("measure", help="the cost shape, from local transcripts")
    measure.add_argument("--repo", default=".", help="the repository to measure from")
    measure.add_argument(
        "--transcripts",
        action="append",
        default=None,
        metavar="DIR",
        help="a session transcript directory; repeatable",
    )
    measure.add_argument("--since", default=None, help="drop contexts that ended before this")
    measure.add_argument("--until", default=None, help="drop contexts that began after this")
    # The four stated assumptions the lever table is priced on. They are
    # assumptions rather than measurements, and the documents say so, so
    # disagreeing with one has to be a named call rather than an edit to this
    # file. The document records what was used.
    measure.add_argument(
        "--cap",
        type=int,
        default=POLICY_CAP,
        help=f"requests after which a context hands off (default {POLICY_CAP})",
    )
    measure.add_argument(
        "--handoff",
        type=int,
        default=HANDOFF_REQUESTS,
        help="requests a handed-off context spends re-reading its brief "
        f"(default {HANDOFF_REQUESTS})",
    )
    measure.add_argument(
        "--frame-trim",
        type=int,
        default=FRAME_TRIM_TOKENS,
        help=f"tokens off the standing frame (default {FRAME_TRIM_TOKENS})",
    )
    measure.add_argument(
        "--request-scale",
        type=float,
        default=REQUEST_SCALE,
        help=f"fraction of today's requests that remain (default {REQUEST_SCALE})",
    )
    measure.add_argument("--out", default="-", help="where to write; - is stdout")

    price = modes.add_parser("ceremony", help="what the review chain caught, from gh")
    price.add_argument("--repo", default=".", help="the repository to measure from")
    price.add_argument("--owner", default="milocaetano")
    price.add_argument("--name", default="quantick")
    price.add_argument(
        "--registry",
        default=os.path.join("docs", "quality", "mission-cost", "missions.json"),
    )
    price.add_argument("--out", default="-", help="where to write; - is stdout")

    table = modes.add_parser("table", help="render a report block from committed JSON")
    table.add_argument("--shape", required=True, help="a shape document")
    table.add_argument(
        "--ceremony",
        default=None,
        help="a ceremony document; the figures block needs one",
    )
    table.add_argument(
        "--block",
        default="figures",
        choices=sorted(BLOCKS),
        help="which generated block to render",
    )
    table.add_argument("--out", default="-", help="where to write; - is stdout")

    options = parser.parse_args(argv)

    if options.mode == "measure":
        check_domains(parser, options)
        roots = options.transcripts or [
            TRANSCRIPTS.default_transcripts(options.repo)
        ]
        for root in roots:
            if not os.path.isdir(root):
                parser.error(
                    f"no transcript directory at {root}. Transcripts are local to "
                    "the trader's machine and are not in the repository; see "
                    "tools/mission_cost/README.md"
                )
        since = (
            TRANSCRIPTS.require_instant(options.since, "--since")
            if options.since
            else None
        )
        until = (
            TRANSCRIPTS.require_instant(options.until, "--until")
            if options.until
            else None
        )
        rows = contexts(roots, since, until)
        if not rows:
            parser.error("no transcript fell inside the window")
        try:
            document = build_shape(
                rows,
                options.since,
                options.until,
                cap=options.cap,
                handoff=options.handoff,
                trim=options.frame_trim,
                scale=options.request_scale,
            )
        except PolicyError as refused:
            # The one bound that is only knowable after the fit, refused in the
            # same voice as the rest rather than as a traceback.
            parser.error(str(refused))
        CANONICAL.emit(CANONICAL.render(document), options.out)
        return 0

    if options.mode == "ceremony":
        with open(options.registry, "r", encoding="utf-8") as stream:
            registry = json.load(stream)
        rows = [
            pull_ceremony(options.owner, options.name, mission["pr"])
            for mission in registry["missions"]
            if mission.get("pr")
        ]
        document = build_ceremony(rows, registry=options.registry)
        CANONICAL.emit(CANONICAL.render(document), options.out)
        return 0

    block = BLOCKS[options.block]
    if block.needs_ceremony and options.ceremony is None:
        parser.error(f"the {options.block} block needs --ceremony")
    with open(options.shape, "r", encoding="utf-8") as stream:
        shape = json.load(stream)
    ceremony = None
    if options.ceremony is not None:
        with open(options.ceremony, "r", encoding="utf-8") as stream:
            ceremony = json.load(stream)
    CANONICAL.emit(block.render(shape, ceremony), options.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
