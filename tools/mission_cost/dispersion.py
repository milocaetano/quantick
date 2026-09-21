#!/usr/bin/env python3
"""Dispersion, and the registered rule for when a fall counts as a reduction.

Section 5 of ``docs/quality/mission-cost/method.md``. The three thresholds below
were fixed before any reading was taken; moving one invalidates every reading
that used the old value, which is why they are constants here rather than
command-line options.

A mean on its own hides the shape of a five-mission sample, so every group is
summarised with its count, its quartiles and its extremes, and a group too small
to carry a claim says `inconclusive` instead of producing a percentage.
"""

import statistics

# Section 5 of the method, registered before the first reading.
MIN_GROUP_N = 5
MIN_RELATIVE_CHANGE = 0.10
MAX_UNPLACED_SHARE = 0.20

EMPTY = {
    "n": 0,
    "min": None,
    "p25": None,
    "median": None,
    "p75": None,
    "max": None,
    "mean": None,
}


def summary(values):
    """Count, quartiles, extremes and mean -- never the mean alone."""
    ordered = sorted(float(value) for value in values)
    if not ordered:
        return dict(EMPTY)
    if len(ordered) == 1:
        only = ordered[0]
        return {
            "n": 1,
            "min": only,
            "p25": only,
            "median": only,
            "p75": only,
            "max": only,
            "mean": only,
        }
    lower, middle, upper = statistics.quantiles(ordered, n=4, method="inclusive")
    return {
        "n": len(ordered),
        "min": ordered[0],
        "p25": float(lower),
        "median": float(middle),
        "p75": float(upper),
        "max": ordered[-1],
        "mean": float(statistics.fmean(ordered)),
    }


def compare(
    before,
    after,
    unplaced_share_before=0.0,
    unplaced_share_after=0.0,
    windows_overlap=False,
):
    """Grade one metric's two groups against the registered thresholds."""
    earlier, later = summary(before), summary(after)
    result = {
        "before": earlier,
        "after": later,
        "relative_change": None,
        "verdict": None,
        "reason": None,
        "thresholds": {
            "min_group_n": MIN_GROUP_N,
            "min_relative_change": MIN_RELATIVE_CHANGE,
            "max_unplaced_share": MAX_UNPLACED_SHARE,
        },
    }

    if earlier["median"] not in (None, 0.0) and later["median"] is not None:
        result["relative_change"] = (
            earlier["median"] - later["median"]
        ) / earlier["median"]

    if windows_overlap:
        result["verdict"] = "cannot_be_attributed"
        result["reason"] = "the before and after windows overlap in time"
        return result
    if (
        unplaced_share_before > MAX_UNPLACED_SHARE
        or unplaced_share_after > MAX_UNPLACED_SHARE
    ):
        result["verdict"] = "cannot_be_attributed"
        result["reason"] = (
            "shared and unassigned tokens exceed "
            f"{MAX_UNPLACED_SHARE:.0%} of a group's measured tokens"
        )
        return result
    if earlier["n"] < MIN_GROUP_N or later["n"] < MIN_GROUP_N:
        result["verdict"] = "inconclusive"
        result["reason"] = (
            f"n is {earlier['n']} before and {later['n']} after; "
            f"the registered minimum is {MIN_GROUP_N} in each group"
        )
        result["relative_change"] = None
        return result

    if later["median"] >= earlier["median"]:
        result["verdict"] = "no_reduction"
        result["reason"] = "the after median did not fall"
        return result
    change = result["relative_change"]
    if later["median"] < earlier["p25"] and change is not None and change >= MIN_RELATIVE_CHANGE:
        result["verdict"] = "reduction"
        result["reason"] = (
            "the after median fell below the before lower quartile by at least "
            f"{MIN_RELATIVE_CHANGE:.0%}"
        )
        return result
    result["verdict"] = "directional"
    result["reason"] = (
        "the median fell, but not past the before lower quartile or not by "
        f"{MIN_RELATIVE_CHANGE:.0%}; this is a signal, not a saving"
    )
    return result
