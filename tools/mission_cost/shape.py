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
    return {
        "frame_tokens_per_request": round(frame, 1),
        "slope_tokens_per_request_squared": round(slope, 2),
        "r_squared": round(_r_squared(rows, frame, slope), 4),
        "linear_only_r_squared": round(_linear_r_squared(rows), 4),
        "contexts": len(rows),
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
    plans = (
        ("L1", f"cap every context at {cap} requests, handing off to a fresh "
               "dispatch", dict(cap=cap, law="subagent")),
        ("L1+L2", f"the same, and {trim:,} tokens off the standing frame",
         dict(cap=cap, law="subagent", trim=trim)),
        ("L1+L3", f"the same as L1, and {1 - scale:.0%} fewer requests",
         dict(cap=cap, law="subagent", scale=scale)),
        ("L1+L2+L3", "all three together",
         dict(cap=cap, law="subagent", trim=trim, scale=scale)),
        ("L2", f"{trim:,} tokens off the standing frame, no cap",
         dict(trim=trim)),
        ("L3", f"{1 - scale:.0%} fewer requests, no cap", dict(scale=scale)),
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
        "handoff_requests": handoff,
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
    spans = []
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
                    spans.append(gap)
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
        "active_seconds": sum(spans),
        "opening_cache_reads": opening,
        "opening_requests": opened,
    }


def contexts(roots, since, until):
    """Every transcript with a request in the window, folded to one row each."""
    rows = []
    for root in roots:
        found, _ = TRANSCRIPTS.discover(root)
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


def build_shape(rows, since, until):
    """The whole shape document: population, fit, terms, concentration, caps."""
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
    if "subagent" in laws:
        document["policy"] = levers(rows, laws)
    for name, group in population.items():
        if not group:
            continue
        coefficients = laws[name]
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
            "terms": terms(group, coefficients),
            "concentration": concentration(group),
            "length_bands": length_bands(group),
            "opening": opening_cache_reads(group),
            "counterfactual": counterfactual(group, coefficients),
        }
    return document


# ---------------------------------------------------------------- ceremony ---

CEREMONY_QUERY = """
query($owner:String!,$repo:String!,$num:Int!){
  repository(owner:$owner,name:$repo){
    pullRequest(number:$num){
      number additions deletions createdAt bodyText
      comments(first:60){nodes{createdAt body}}
      commits(first:100){nodes{commit{committedDate}}}
      reviewThreads(first:100){nodes{isResolved}}
    }
  }
}
"""

REPORT_MARKER = "<!-- quantick-review-report"


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
    }


def build_ceremony(rows):
    """Totals and per-tier rollups over the pull requests read."""
    document = {
        "schema": VERSION,
        "method": METHOD,
        "metric": "ceremony_cost_per_catch",
        "registered_comparison": False,
        "missions": len(rows),
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
        lines.append(
            f"| {name.capitalize()} cost law, tokens | "
            f"{_thousands(law['frame_tokens_per_request'])}·N + "
            f"{law['slope_tokens_per_request_squared']}·N² "
            f"(R² {law['r_squared']}, linear-only R² {law['linear_only_r_squared']}) |"
        )
        lines.append(
            f"| {name.capitalize()} standing frame / accumulation | "
            f"{term['standing_frame_share']:.1%} / {term['accumulation_share']:.1%} |"
        )
    both = _combined(shape)
    lines.append(
        f"| **All contexts, standing frame / accumulation** | "
        f"**{both['standing_frame_share']:.1%} / {both['accumulation_share']:.1%}** |"
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
    lines.append("")
    lines.append(FIGURES_END)
    return "\n".join(lines) + "\n"


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


def lever_table(shape):
    """The lever table, rendered from the committed policy block.

    #572's architecture review found the first version of this table computed
    by hand beside the document rather than out of it, with one row silently
    priced on a different cost law than the baseline it was compared against.
    The fix is not a sharper reviewer: it is that this table, like the figures
    block, is generated, and that every row carries the law it used.
    """
    policy = shape["policy"]
    baseline = policy["baseline_modelled_tokens"]
    lines = [LEVERS_BEGIN, ""]
    lines.append("| Levers applied | Cost law | Modelled tokens | Change |")
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


def extract_figures(markdown):
    """The figures block a document carries, or ``None`` when it carries none."""
    return extract_block(markdown, FIGURES_BEGIN, FIGURES_END)


def extract_levers(markdown):
    """The lever block a document carries, or ``None`` when it carries none."""
    return extract_block(markdown, LEVERS_BEGIN, LEVERS_END)


# --------------------------------------------------------------- commands ---


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
        choices=("figures", "levers"),
        help="which generated block to render",
    )
    table.add_argument("--out", default="-", help="where to write; - is stdout")

    options = parser.parse_args(argv)

    if options.mode == "measure":
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
        document = build_shape(rows, options.since, options.until)
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
        CANONICAL.emit(CANONICAL.render(build_ceremony(rows)), options.out)
        return 0

    with open(options.shape, "r", encoding="utf-8") as stream:
        shape = json.load(stream)
    if options.block == "levers":
        CANONICAL.emit(lever_table(shape), options.out)
        return 0
    if options.ceremony is None:
        parser.error("the figures block needs --ceremony")
    with open(options.ceremony, "r", encoding="utf-8") as stream:
        ceremony = json.load(stream)
    CANONICAL.emit(figures(shape, ceremony), options.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
