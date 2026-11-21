"""Collect C14 at an exact clean commit using outside-score v1."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
from combined import Counts, compare, validate_history

BASE_SHA = "9719d691f22285b5234b99aeb81fa91d785da667"
BASE = Counts(268631, 311966, 111)
EVIDENCE = Path.home() / "AppData/Local/Temp/quantick-change-cost/acceptance-evidence-index.json"


def command(*args: str) -> str:
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True).stdout.strip()


def main() -> int:
    if command("git", "status", "--porcelain"):
        print("measure.py refuses to attribute a dirty tree to a commit", file=sys.stderr)
        return 2
    head = command("git", "rev-parse", "HEAD")
    tree = command("git", "rev-parse", "HEAD^{tree}")
    raw = command(sys.executable, "tools/outside_score/measure.py", ".")
    values = dict(line.split("\t", 1) for line in raw.splitlines() if "\t" in line)
    if values.get("measure.version") != "1":
        raise ValueError("canonical outside-score measure.version must be 1")
    measured = Counts(int(values["lines.production"]), int(values["lines.test"]), int(values["panic_sites"]))
    comparison = compare(BASE, measured)
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8-sig"))
    canonical = evidence["canonical"]
    if canonical["tool"] != "tools/outside_score/measure.py" or canonical["version"] != 1:
        raise ValueError("pinned baseline calculator identity changed")
    if (canonical["production"], canonical["test"], canonical["panic"]) != (BASE.production, BASE.tests, BASE.panic):
        raise ValueError("pinned baseline counts changed")
    rows = []
    for row in evidence["history"]:
        rows.append({**row, "state": "unobservable" if row.get("after") else "measured"})
    validate_history(rows)
    fragments = sum(1 for path in ROOT.glob("crates/guards/*-baseline.d/*.txt"))
    report = {
        "criterion": "C14",
        "head": head,
        "tree": tree,
        "baseline": BASE_SHA,
        "calculator": {"path": canonical["tool"], "version": 1},
        "counts": {"production": measured.production, "test": measured.tests, "panic": measured.panic, "expect": int(values["expect_sites"])},
        "exact": comparison,
        "fragment_count": fragments,
        "history": rows,
    }
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if comparison["test_non_regression"] and comparison["panic_non_regression"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
