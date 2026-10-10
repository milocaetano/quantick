"""Pure fixed-budget validation; calibration proposals never rewrite budgets."""

import math

import inputs

# Operational calibration headroom: 25% above the slowest retained loop,
# rounded upward to the millisecond, with no additive allowance. It is an
# explicit policy, not a measured confidence interval.
HEADROOM_FACTOR = 1.25


def valid_process(record, tests):
    value = record.get("elapsed_seconds")
    return (type(value) in (int, float) and math.isfinite(value) and value >= 0
            and type(record.get("exit_code")) is int and record["exit_code"] == 0
            and record.get("timed_out") is False and record.get("interrupted") is None
            and record.get("tree_quiescent") is True and record.get("orphaned_descendants") is False
            and (record.get("tests_passed") is True if tests else record.get("recompiled") is True))


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
        if not all(valid_process(sample, tests=False) for sample in samples):
            raise ValueError("invalid touched-sample evidence")
        if not valid_process(row.get("tests", {}), tests=True):
            raise ValueError("invalid package test-run evidence")


def loop_seconds(row):
    """Each touched build plus the package's one test run: the loop a developer waits on."""
    return [sample["elapsed_seconds"] + row["tests"]["elapsed_seconds"] for sample in row["samples"]]


def propose(report):
    valid_report(report)
    return {"schema": 1, "status": "calibrated", "calibration_sha": report["source_sha"],
            "calibration_utc": report["measured_utc"], "host_class": report["host_class"],
            "toolchain": report["toolchain"], "jobs": report["jobs"],
            "profile_hash": report["profile_hash"],
            "protocol_hash": report["protocol_hash"],
            "rationale": "ceil(1.25 * max(touched build + package test run) * 1000) / 1000",
            "crates": [{"crate": row["crate"], "package": row["package"], "source": row["source"],
                        "max_seconds": math.ceil(HEADROOM_FACTOR * max(loop_seconds(row)) * 1000) / 1000}
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
        if any(value > maximum for value in loop_seconds(row)):
            raise ValueError(f"edit-loop budget exceeded: {row['package']}")
