"""Validate raw, SHA-bound timing evidence before proposing or checking budgets."""

from datetime import datetime, timedelta, timezone
import json
from pathlib import Path

import inputs
import sampling
import snapshot

# The campaign requires measurements younger than 30 days. Small clock skew is
# tolerated, not a future-dated run that can remain fresh indefinitely.
MAX_AGE = timedelta(days=30)
MAX_CLOCK_SKEW = timedelta(minutes=5)


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def manifest(output):
    rows = []
    for path in sorted(output.rglob("*")):
        if path.is_file() and path != output / "manifest.json":
            relative = inputs.regular_path(output, path)
            rows.append({"path": relative, "bytes": path.stat().st_size,
                         "sha256": inputs.digest(path.read_bytes())})
    return rows


def publish(output, report):
    inputs.write_json(output / "report.json", report)
    inputs.write_json(output / "manifest.json", manifest(output))


def fresh(report, now=None):
    measured = datetime.fromisoformat(report["measured_utc"])
    if measured.tzinfo is None or measured.utcoffset() != timedelta(0):
        raise ValueError("measurement timestamp must explicitly be UTC")
    age = (now or datetime.now(timezone.utc)) - measured
    if age >= MAX_AGE or age < -MAX_CLOCK_SKEW:
        raise ValueError("measurement is stale or future-dated")


def validate_process(output, recorded, package, touched):
    directory = Path(recorded["directory"])
    if directory.is_absolute() or ".." in directory.parts:
        raise ValueError("raw process directory escapes evidence")
    raw = read_json(output / directory / "process.json")
    if any(recorded.get(key) != value for key, value in raw.items()):
        raise ValueError("reported timing differs from raw process metadata")
    if raw["command"] != ["cargo", "test", "-p", package] or raw.get("interrupted"):
        raise ValueError("sample did not run the exact complete package test command")
    raw["stdout_text"] = (output / directory / "stdout.log").read_text(encoding="utf-8", errors="replace")
    raw["stderr_text"] = (output / directory / "stderr.log").read_text(encoding="utf-8", errors="replace")
    compiled = sampling.validate_sample(raw, package, touched)
    if recorded.get("recompiled") != compiled or recorded.get("tests_passed") is not True:
        raise ValueError("reported compilation/test claims disagree with raw output")
    contention = read_json(output / directory / "contention.json")
    if contention["before"] or contention["after"]:
        raise ValueError("compiler contention invalidates the sample")


def load(report_path, repo, now=None):
    output = report_path.absolute().parent
    rows = read_json(output / "manifest.json")
    names = [row["path"] for row in rows]
    if len(names) != len(set(names)) or "report.json" not in names:
        raise ValueError("artifact manifest is incomplete or has duplicate paths")
    for row in rows:
        path = output / row["path"]
        inputs.regular_path(output, path)
        if path.stat().st_size != row["bytes"] or inputs.digest(path.read_bytes()) != row["sha256"]:
            raise ValueError("raw artifact hash mismatch")
    if rows != manifest(output):
        raise ValueError("artifact manifest does not cover the complete evidence directory")
    report = read_json(report_path)
    fresh(report, now)
    sha = report["source_sha"]
    if not inputs.FULL_SHA.fullmatch(sha):
        raise ValueError("exact source SHA is missing")
    tree = inputs.git(repo, "rev-parse", f"{sha}^{{tree}}").decode().strip()
    if report["source_tree"] != tree:
        raise ValueError("source tree differs from the recorded commit")
    exact = snapshot.facts(repo, sha)
    if report["input_hashes"] != exact["input_hashes"]:
        raise ValueError("complete committed measurement input hash map is required")
    if report["protocol_hash"] != inputs.protocol_hash(exact["input_hashes"]):
        raise ValueError("measurement protocol identity differs from its source SHA")
    if report["profile_hash"] != exact["input_hashes"]["Cargo.toml"]:
        raise ValueError("profile hash differs from the committed Cargo manifest")
    ranking = report["ranking"]
    if ranking != exact["ranking"]:
        raise ValueError("crate ranking differs from exact-SHA frozen-lexer recomputation")
    if len(report["crates"]) != 3 or len(ranking) < 3:
        raise ValueError("complete top-three series is missing")
    for selected, row in zip(ranking, report["crates"]):
        if any(row[key] != value for key, value in selected.items()):
            raise ValueError("measured crates differ from the actual top-three ranking")
        original = inputs.git(repo, "show", f"{sha}:{row['source']}")
        backup = output / row["crate"] / "source-original.bin"
        recovery = read_json(output / row["crate"] / "recovery.json")
        if (backup.read_bytes() != original or recovery["sha256"] != inputs.digest(original)
                or recovery["sha"] != sha or recovery["source"] != row["source"]
                or recovery["state"] != "restored" or row.get("restored") is not True):
            raise ValueError("source restoration/byte identity evidence is invalid")
        validate_process(output, row["warmup"], row["package"], False)
        validate_process(output, row["control"], row["package"], False)
        prior = recovery["mtime_ns"]
        for sample in row["samples"]:
            if sample["touched_mtime_ns"] <= prior:
                raise ValueError("touch timestamps do not strictly advance")
            prior = sample["touched_mtime_ns"]
            validate_process(output, sample, row["package"], True)
        if row["summary"] != sampling.summary(row["samples"]):
            raise ValueError("summary does not retain all measured samples")
    return report
