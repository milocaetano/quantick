"""Exact acceptance arithmetic and evidence-state validation for C14."""

from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import re


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


def validate_results(
    required: list[str],
    results: list[dict],
    head: str,
    tree: str,
    base: str,
    evidence_root: Path,
) -> None:
    names = [result.get("command") for result in results]
    duplicates = sorted({name for name in names if names.count(name) > 1})
    missing = sorted(set(required) - set(names))
    extras = sorted(set(names) - set(required))
    failed = sorted(result["command"] for result in results if result.get("exit_code") != 0)
    malformed = sorted(
        result["command"]
        for result in results
        if not re.fullmatch(r"[0-9a-f]{40}", str(result.get("sha", "")))
    )
    stale = sorted(
        result["command"]
        for result in results
        if result.get("sha") != head
        or result.get("tree") != tree
        or result.get("base") != base
    )
    if duplicates or missing or extras or failed or malformed or stale:
        raise ValueError(
            f"invalid command evidence: duplicates={duplicates} missing={missing} "
            f"extras={extras} failed={failed} malformed={malformed} stale={stale}"
        )
    for result in results:
        name = result["command"]
        capture_path = evidence_root / str(result.get("capture", ""))
        try:
            capture = json.loads(capture_path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            raise ValueError(
                f"invalid command evidence for {name}: capture is unavailable: {error}"
            ) from error
        expected = {"command": name, "sha": head, "tree": tree, "base": base}
        if any(capture.get(key) != value for key, value in expected.items()):
            raise ValueError(
                f"invalid command evidence for {name}: capture metadata does not match"
            )
        outputs = capture.get("outputs")
        if not isinstance(outputs, list) or not outputs:
            raise ValueError(f"invalid command evidence for {name}: captured output is missing")
        for output in outputs:
            if not isinstance(output, dict):
                raise ValueError(
                    f"invalid command evidence for {name}: captured output is malformed"
                )
            output_path = evidence_root / str(output.get("path", ""))
            try:
                digest = hashlib.sha256(output_path.read_bytes()).hexdigest()
            except OSError as error:
                raise ValueError(
                    f"invalid command evidence for {name}: output is unavailable: {error}"
                ) from error
            if digest != output.get("sha256"):
                raise ValueError(f"invalid command evidence for {name}: output digest does not match")


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


def history_rows(source: list[dict]) -> list[dict]:
    rows = []
    for source_row in source:
        expected = "unobservable" if source_row.get("after") else "measured"
        if source_row.get("state", expected) != expected:
            raise ValueError(
                f"history row {source_row.get('metric')} contradicts derived state {expected}"
            )
        rows.append({**source_row, "state": expected})
    validate_history(rows)
    return rows
