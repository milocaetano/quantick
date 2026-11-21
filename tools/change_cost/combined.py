"""Exact acceptance arithmetic and evidence-state validation for C14."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Counts:
    production: int
    tests: int
    panic: int

    def validate(self) -> None:
        if self.production <= 0 or self.tests < 0 or self.panic < 0:
            raise ValueError("counts require positive production and nonnegative tests/sites")


def compare(base: Counts, head: Counts) -> dict[str, int | bool]:
    base.validate()
    head.validate()
    test_left = head.tests * base.production
    test_right = base.tests * head.production
    panic_left = head.panic * base.production
    panic_right = base.panic * head.production
    return {
        "test_left": test_left,
        "test_right": test_right,
        "test_non_regression": test_left >= test_right,
        "test_deficit": max(0, test_right - test_left),
        "panic_left": panic_left,
        "panic_right": panic_right,
        "panic_non_regression": panic_left <= panic_right,
    }


def validate_results(required: list[str], results: list[dict]) -> None:
    names = [result.get("command") for result in results]
    duplicates = sorted({name for name in names if names.count(name) > 1})
    missing = sorted(set(required) - set(names))
    extras = sorted(set(names) - set(required))
    failed = sorted(result["command"] for result in results if result.get("exit_code") != 0)
    unpinned = sorted(result["command"] for result in results if not result.get("sha"))
    if duplicates or missing or extras or failed or unpinned:
        raise ValueError(
            f"invalid command evidence: duplicates={duplicates} missing={missing} "
            f"extras={extras} failed={failed} unpinned={unpinned}"
        )


def validate_history(rows: list[dict]) -> None:
    for row in rows:
        if not row.get("metric") or not row.get("sources"):
            raise ValueError("historical rows require a metric and source provenance")
        after = row.get("after")
        if after and row.get("state") != "unobservable":
            raise ValueError(f"future window {row['metric']} must be explicitly unobservable")
        if row.get("state") == "unobservable" and not after:
            raise ValueError(f"unobservable row {row['metric']} requires its observation condition")
        if row.get("state") == "measured" and row.get("value") is None:
            raise ValueError(f"measured row {row['metric']} has no value")
