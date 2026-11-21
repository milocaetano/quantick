#!/usr/bin/env python3
"""Replay a read capability and a new mutation through disposable real consumers."""
from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = Path("crates/control/src/families.rs")
FIXTURES = ROOT / "tools/control_docking"
CONTEXT_FILES = [
    Path(f"crates/{crate}/src/{name}.rs")
    for crate in ("control-schema", "control-handlers")
    for name in ("chart_context", "chart_context_tests")
]
FIXTURE_ROW = '            "zz_fixture" { schemas: [zz_fixture], actions: [zz_fixture], projections: [zz_fixture], authority: [zz_fixture], readbacks: [zz_fixture], behaviours: [zz_fixture], samples: [zz_fixture], }\n'


def run(root: Path, *args: str, capture: bool = False) -> str:
    result = subprocess.run(args, cwd=root, text=True, capture_output=capture)
    if result.returncode:
        if capture:
            print(result.stdout, flush=True)
            print(result.stderr, flush=True)
        result.check_returncode()
    return result.stdout if capture else ""


def cargo(root: Path, *args: str, capture: bool = False) -> str:
    return run(root, "cargo", "--offline", *args, capture=capture)


def emit(root: Path) -> dict[str, bytes]:
    command = ("run", "-q", "-p", "quantick-control-handlers", "--bin", "regen-control", "--")
    paths = cargo(root, *command, "--list", capture=True).splitlines()
    assert len(paths) == len(set(paths)), "duplicate artifact paths"
    cargo(root, *command, "--write")
    return {path: (root / path).read_bytes() for path in paths}


def digest(files: dict[str, bytes]) -> str:
    result = hashlib.sha256()
    for name, data in sorted(files.items()):
        result.update(name.encode() + b"\0" + data + b"\0")
    return result.hexdigest()


def snapshot(root: Path) -> dict[str, bytes]:
    paths = run(ROOT, "git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", capture=True)
    return {path: (root / path).read_bytes() for path in paths.split("\0") if path and (root / path).is_file()}


def replay_chart(root: Path, expected: dict[str, bytes]) -> dict[str, object]:
    source = (root / MANIFEST).read_text(encoding="utf-8")
    row = next(line for line in source.splitlines(keepends=True) if '"chart_context" {' in line)
    files = {path: (root / path).read_bytes() for path in CONTEXT_FILES}
    (root / MANIFEST).write_text(source.replace(row, ""), encoding="utf-8")
    for path in files:
        (root / path).unlink()
    baseline_artifacts = emit(root)
    assert b'"chart.context.read"' not in baseline_artifacts["schemas/control/observer-capability-catalog-v1.json"]
    # The baseline keeps #1113's drawing/strategy owners and app adapters buildable.
    cargo(root, "check", "-q", "-p", "quantick-app")
    for path in expected.keys() - baseline_artifacts.keys():
        (root / path).unlink()
    baseline = snapshot(root)
    for path, data in files.items():
        (root / path).write_bytes(data)
    (root / MANIFEST).write_text(source, encoding="utf-8")
    restored = emit(root)
    assert restored == expected, "capability replay changed emitted artifact paths or bytes"
    after = snapshot(root)
    changed = {path for path in baseline.keys() | after.keys() if baseline.get(path) != after.get(path)}
    handwritten = changed - baseline_artifacts.keys() - restored.keys()
    existing_lines = 0
    for path in handwritten & baseline.keys():
        before_lines = baseline[path].decode().splitlines()
        after_lines = after[path].decode().splitlines()
        existing_lines += sum(1 for line in difflib.ndiff(before_lines, after_lines) if line[:2] in ("+ ", "- "))
    assert existing_lines <= 2, (existing_lines, sorted(handwritten))
    assert len(changed) <= 25, sorted(changed)
    cargo(root, "test", "-q", "-p", "quantick-control-handlers", "--lib", "chart_context")
    return {"existing_handwritten_lines": existing_lines, "changed_files": len(changed), "artifacts": len(restored), "artifact_sha256": digest(restored)}


def mutation(root: Path, expected: dict[str, bytes]) -> dict[str, object]:
    destinations = {
        "schema": "crates/control-schema/src/zz_fixture.rs",
        "handler": "crates/control-handlers/src/zz_fixture.rs",
        "tests": "crates/control-handlers/src/zz_fixture/tests.rs",
        "authority": "crates/control-host/src/authority/families/zz_fixture.rs",
        "behaviour": "crates/operability/src/registry/zz_fixture.rs",
        "samples": "crates/app/src/app/tests/retry_readback_tests/samples/zz_fixture.rs",
    }
    for name, relative in destinations.items():
        destination = root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(FIXTURES / f"zz_fixture_{name}.rs", destination)
    manifest = (root / MANIFEST).read_text(encoding="utf-8")
    marker = "        }\n    };"
    assert manifest.count(marker) == 1
    (root / MANIFEST).write_text(manifest.replace(marker, FIXTURE_ROW + marker), encoding="utf-8")
    artifacts = emit(root)
    mutable = {"schemas/control/observer-capability-catalog-v1.json", "docs/control-plane/capability-inventory.md", "docs/control-plane/retry-matrix.md"}
    assert artifacts.keys() - expected.keys() == {"schemas/control/fixture-input-v1.schema.json", "schemas/control/fixture-output-v1.schema.json"}
    assert expected.keys() <= artifacts.keys()
    for path in expected.keys() - mutable:
        assert artifacts[path] == expected[path], f"unrelated emitted bytes changed: {path}"
    old_catalog = json.loads(expected["schemas/control/observer-capability-catalog-v1.json"])
    new_catalog = json.loads(artifacts["schemas/control/observer-capability-catalog-v1.json"])
    for key, identity in {"capabilities": "fixture.magnet.set", "modules": "fixture", "permissions": "cockpit.fixture", "snapshot_scopes": "fixture.available"}.items():
        additions = [row for row in new_catalog[key] if row["id"] == identity]
        assert len(additions) == 1, (key, identity)
        new_catalog[key] = [row for row in new_catalog[key] if row["id"] != identity]
    assert new_catalog == old_catalog, "existing catalog semantics changed"
    for path in mutable - {"schemas/control/observer-capability-catalog-v1.json"}:
        # Remove the one new row, and account for its exact summary increments.
        lines = artifacts[path].split(b"\n")
        assert sum(b"fixture.magnet.set" in line for line in lines) == 1
        stripped = b"\n".join(line for line in lines if b"fixture.magnet.set" not in line)
        wanted = expected[path]
        patterns = ([rb"(\n)(\d+)( capabilities registered\.)"] if path.endswith("capability-inventory.md") else [
            rb"(\| deduplicated per connection \| )(\d+)( \|)",
            rb"(\| \*\*Total\*\* \| \*\*)(\d+)(\*\* \|)",
            rb"(\n)(\d+)( mutable capabilities registered,)",
            rb"( mutable capabilities registered, )(\d+)( with a readback\.)",
        ])
        for pattern in patterns:
            wanted, count = re.subn(pattern, lambda match: match[1] + str(int(match[2]) + 1).encode() + match[3], wanted)
            assert count == 1, pattern
        assert stripped == wanted, f"existing table bytes changed: {path}"
    cargo(root, "test", "-q", "-p", "quantick-control-handlers", "--lib", "fake_mutating_family_reaches_every_manifest_consumer")
    # This invokes the actual app action manifest and ordered retry sample consumer.
    cargo(root, "test", "-q", "-p", "quantick-app", "--bin", "quantick-app", "every_reachable_optional_row_replays_a_dropped_answer_and_begins_once")
    return {"artifacts": len(artifacts), "artifact_sha256": digest(artifacts), "synthetic_family": "zz_fixture"}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", choices=("all", "chart", "mutation"), default="all")
    parser.add_argument("--keep", action="store_true", help="keep the disposable checkout for inspection")
    parser.add_argument(
        "--scratch",
        type=Path,
        help="rebuild this quantick-control-docking-* path instead of choosing a random path",
    )
    args = parser.parse_args()
    os.environ.setdefault("CARGO_TARGET_DIR", str(ROOT / "target"))
    if args.scratch:
        scratch = args.scratch.resolve()
        scratch_parent = Path(tempfile.gettempdir()).resolve()
        assert scratch.parent == scratch_parent, (scratch, scratch_parent)
        assert scratch.name.startswith("quantick-control-docking-"), scratch
        if scratch.exists():
            shutil.rmtree(scratch)
        scratch.mkdir(parents=True)
    else:
        scratch = Path(tempfile.mkdtemp(prefix="quantick-control-docking-"))
    print(f"scratch={scratch}", flush=True)
    try:
        source = snapshot(ROOT)
        for path, data in source.items():
            destination = scratch / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        expected = emit(scratch)
        checked_in = {path: (ROOT / path).read_bytes() for path in expected}
        assert expected == checked_in, "starting tree has stale emitted artifacts"
        report = {
            "source_sha": run(ROOT, "git", "rev-parse", "HEAD", capture=True).strip(),
            "source_snapshot_sha256": digest(source),
        }
        if args.phase in ("all", "chart"):
            report["chart_context"] = replay_chart(scratch, expected)
        if args.phase in ("all", "mutation"):
            report["mutation"] = mutation(scratch, expected)
        print(json.dumps(report, sort_keys=True, indent=2), flush=True)
    finally:
        if not args.keep:
            shutil.rmtree(scratch)


if __name__ == "__main__":
    main()
