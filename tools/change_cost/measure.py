"""Collect C14 at an exact clean commit using outside-score v1."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
from combined import Counts, compare, history_rows, validate_results

BASE_SHA = "9719d691f22285b5234b99aeb81fa91d785da667"
BASE = Counts(268631, 311966, 111)
DEFAULT_EVIDENCE_DIR = Path(tempfile.gettempdir()) / "quantick-change-cost"
INTEGRATION_BASE = "827b83cf0d324dc6b498f9ce0205ddfdd57c277c"
REQUIRED_COMMANDS = ["C0", "C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C9a", "C10", "C11", "C12", "C13"]


def command(*args: str) -> str:
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True).stdout.strip()


def validate_evidence(
    evidence: dict, command_results: list[dict], head: str, tree: str, evidence_root: Path
) -> list[dict]:
    validate_results(
        REQUIRED_COMMANDS, command_results, head, tree, INTEGRATION_BASE, evidence_root
    )
    return history_rows(evidence["history"])


def validate_canonical(canonical: dict) -> None:
    if (
        canonical.get("tool") != "tools/outside_score/measure.py"
        or canonical.get("version") != 1
    ):
        raise ValueError("pinned baseline calculator identity changed")
    if (canonical.get("production"), canonical.get("test"), canonical.get("panic")) != (
        BASE.production,
        BASE.tests,
        BASE.panic,
    ):
        raise ValueError("pinned baseline counts changed")


def arguments(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--evidence",
        type=Path,
        default=DEFAULT_EVIDENCE_DIR / "acceptance-evidence-index.json",
    )
    parser.add_argument(
        "--command-results",
        type=Path,
        default=DEFAULT_EVIDENCE_DIR / "c3-command-results.json",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = arguments(argv)
    if command("git", "status", "--porcelain"):
        print("measure.py refuses to attribute a dirty tree to a commit", file=sys.stderr)
        return 2
    head = command("git", "rev-parse", "HEAD")
    tree = command("git", "rev-parse", "HEAD^{tree}")
    if not args.command_results.is_file():
        print(
            f"C14 command evidence is missing: {args.command_results}; run C0-C13 at {head} and record exact head/tree/base results",
            file=sys.stderr,
        )
        return 2
    command_results = json.loads(args.command_results.read_text(encoding="utf-8-sig"))
    raw = command(sys.executable, "tools/outside_score/measure.py", ".")
    values = dict(line.split("\t", 1) for line in raw.splitlines() if "\t" in line)
    if values.get("measure.version") != "1":
        raise ValueError("canonical outside-score measure.version must be 1")
    measured = Counts(int(values["lines.production"]), int(values["lines.test"]), int(values["panic_sites"]))
    comparison = compare(BASE, measured)
    evidence = json.loads(args.evidence.read_text(encoding="utf-8-sig"))
    evidence_root = args.command_results.resolve().parent
    rows = validate_evidence(evidence, command_results, head, tree, evidence_root)
    canonical = evidence["canonical"]
    validate_canonical(canonical)
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
