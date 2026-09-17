"""Pure fixed-budget validation; calibration proposals never rewrite budgets."""

import math

import inputs

# Operational calibration headroom: 25% of the slowest retained sample plus
# five seconds for runner scheduling noise, rounded upward to a whole second.
HEADROOM_FACTOR = 1.25
HEADROOM_SECONDS = 5


def valid_report(report):
    if report.get("schema") != 1 or report.get("status") != "complete" or not report.get("restored"):
        raise ValueError("measurement is incomplete or source restoration failed")
    if not inputs.FULL_SHA.fullmatch(report.get("source_sha", "")):
        raise ValueError("measurement has no exact source SHA")
    crates = report.get("crates", [])
    if len(crates) != 3 or len({row["crate"] for row in crates}) != 3:
        raise ValueError("measurement must retain exactly the three ranked crates")
    for row in crates:
        samples = row.get("samples", [])
        if len(samples) != inputs.SAMPLES:
            raise ValueError("all five touched samples are required")
        for sample in samples:
            value = sample.get("elapsed_seconds")
            if (type(value) not in (int, float) or not math.isfinite(value) or value < 0
                    or type(sample.get("exit_code")) is not int or sample["exit_code"] != 0
                    or sample.get("timed_out") is not False or sample.get("interrupted") is not None
                    or sample.get("tree_quiescent") is not True or sample.get("orphaned_descendants") is not False
                    or sample.get("recompiled") is not True or sample.get("tests_passed") is not True):
                raise ValueError("invalid touched-sample evidence")


def propose(report):
    valid_report(report)
    return {"schema": 1, "status": "calibrated", "calibration_sha": report["source_sha"],
            "calibration_utc": report["measured_utc"], "host_class": report["host_class"],
            "toolchain": report["toolchain"], "jobs": report["jobs"],
            "profile_hash": report["profile_hash"],
            "protocol_hash": report["protocol_hash"],
            "rationale": "ceil(1.25 * max(all five baseline samples) + 5 seconds)",
            "crates": [{"crate": row["crate"], "package": row["package"], "source": row["source"],
                        "max_seconds": math.ceil(HEADROOM_FACTOR * max(
                            sample["elapsed_seconds"] for sample in row["samples"]) + HEADROOM_SECONDS)}
                       for row in report["crates"]]}


def check(report, budget):
    valid_report(report)
    if budget.get("schema") != 1 or budget.get("status") != "calibrated":
        raise ValueError("budgets are uncalibrated; retain raw run and review a fixed baseline")
    for field in ("host_class", "toolchain", "jobs", "profile_hash", "protocol_hash"):
        if report.get(field) != budget.get(field):
            raise ValueError(f"uncalibrated {field} drift")
    if len(budget.get("crates", [])) != 3:
        raise ValueError("missing top-three budget")
    for row, limit in zip(report["crates"], budget["crates"]):
        if any(row[key] != limit[key] for key in ("crate", "package", "source")):
            raise ValueError("uncalibrated top-three/source drift")
        maximum = limit.get("max_seconds")
        if type(maximum) not in (int, float) or not math.isfinite(maximum) or maximum <= 0:
            raise ValueError("invalid fixed budget")
        if any(sample["elapsed_seconds"] > maximum for sample in row["samples"]):
            raise ValueError(f"edit-loop budget exceeded: {row['package']}")
