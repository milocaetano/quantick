"""Review candidate only: finite Windows collection over already built exports.

Never builds, exports, discovers a candidate, retries, or edits source. The
release JSON and every referenced manifest must exist before invocation.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import stat
import subprocess
import sys
import zipfile
import time
import tomllib

from owner_perf_parser01 import ORDER, SELECTORS, InvalidEvidence, compare_actions, parse, require, strict_json

BASELINE = "80da32426e6e2f7a2a962ab8054d986897461b6d"
SUFFIX = "e34b57b032e8d88cb68b85606b7f479b97446db5f5cb93595a053dff9013340a"
PLAN = [(mode, run) for mode in ("timing", "allocations", "dense") for run in ORDER]
PLAN += [("long", "B1"), ("long", "C1")]
BLOCKED = re.compile(r"^(cargo|rustc|rustdoc|cl|link|lld.*|clang.*|gcc.*|cc1.*|msbuild|cmake|ninja|quantick.*|chromedriver|geckodriver|msedgedriver|autohotkey.*|playwright.*|python.*|node|powershell|pwsh)(?:\.exe)?$", re.I)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def load(path):
    return strict_json(Path(path).read_text(encoding="utf-8"))


def save(path, obj):
    Path(path).write_text(json.dumps(obj, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def bound_path(root, name):
    require(type(name) is str and name and "\\" not in name, "noncanonical manifest path")
    path = (root / name).resolve()
    require(path.is_relative_to(root.resolve()) and path != root.resolve(), "manifest path escapes root")
    require(not any(part == ".." for part in Path(name).parts), "parent path forbidden")
    return path


def verify_files(root, mapping, exhaustive=False):
    require(type(mapping) is dict and mapping, "empty identity map")
    for name, sha in mapping.items():
        path = bound_path(root, name)
        require(path.is_file() and not path.is_symlink() and digest(path) == sha, "artifact identity mismatch")
    if exhaustive:
        actual = set()
        for path in root.rglob("*"):
            require(not path.is_symlink() and not path.is_junction(), "source links forbidden")
            if path.is_file():
                actual.add(path.relative_to(root).as_posix())
        require(actual == set(mapping), "source addition/removal")


def verify_zip_archive(path, expected_sha256, product_files):
    """Verify the producer's exact Git ZIP in memory, without extraction."""
    require(digest(path) == expected_sha256, "archive identity mismatch")
    archived = {}
    seen = set()
    with zipfile.ZipFile(path, "r") as archive:
        for member in archive.infolist():
            name = member.orig_filename
            require(name == member.filename and name and "\\" not in name and "\x00" not in name, "noncanonical ZIP member")
            directory = name.endswith("/")
            canonical = name[:-1] if directory else name
            parts = canonical.split("/")
            require(all(part and part not in (".", "..") and ":" not in part for part in parts), "unsafe ZIP member path")
            require(all(part == part.rstrip(" .") and not re.fullmatch(r"(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?", part, re.I) for part in parts), "noncanonical Windows ZIP path")
            require(canonical not in seen, "duplicate ZIP member")
            seen.add(canonical)
            mode = member.external_attr >> 16
            kind = stat.S_IFMT(mode)
            dos = member.external_attr & 0xFFFF
            require(member.create_system in (0, 3), "unsupported ZIP origin")
            if member.create_system == 0:
                require(mode == 0 and dos & ~0x37 == 0, "special DOS ZIP member")
                require(bool(dos & 0x10) == directory, "ZIP directory metadata mismatch")
            else:
                require(kind == (stat.S_IFDIR if directory else stat.S_IFREG), "ZIP link/special member")
            if directory:
                require(member.file_size == 0, "ZIP directory carries content")
            else:
                archived[name] = hashlib.sha256(archive.read(member)).hexdigest()
    require(all("/".join(name.split("/")[:i]) not in archived for name in seen for i in range(1, len(name.split("/")))), "ZIP file/directory conflict")
    require(archived == product_files, "archive/product mapping mismatch")
    return archived


def ordinary_default_features(app_manifest, artifact):
    source = tomllib.loads(Path(app_manifest).read_text(encoding="utf-8"))
    require(source.get("features", {}).get("default") == [], "source default features are not explicitly empty")
    require(artifact["features"] == ["default"], "unexpected raw Cargo feature activation")
    require(Path(artifact["manifest_path"]).resolve() == Path(app_manifest).resolve(), "Cargo artifact manifest mismatch")
    require(artifact["target"]["name"] == "quantick-app" and artifact["profile"]["test"], "wrong Cargo test target")
    return []


def verify_side(manifest, commit):
    require(manifest["commit"] == commit and re.fullmatch(r"[0-9a-f]{40}", commit), "wrong exact commit")
    root = Path(manifest["source_root"])
    require(root.is_absolute() and root.is_dir(), "missing export root")
    verify_zip_archive(manifest["archive_path"], manifest["archive_sha256"], manifest["product_files"])
    verify_files(root, manifest["files"], exhaustive=True)
    overlay = manifest["overlay"]
    require(overlay["path"] == "crates/app/src/app/tests/drawings_tests.rs" and overlay["suffix_sha256"] == SUFFIX, "wrong overlay")
    suffix = Path(manifest["suffix_path"]).read_bytes()
    require(hashlib.sha256(suffix).hexdigest() == SUFFIX, "suffix changed")
    product = manifest["product_files"]
    require(set(product) == set(manifest["files"]), "overlay changed file membership")
    for name, sha in product.items():
        actual = bound_path(root, name).read_bytes()
        if name == overlay["path"]:
            require(overlay["separator"] == "\n" and actual.endswith(b"\n" + suffix), "not exact LF plus suffix append")
            actual = actual[:-len(suffix)-1]
        require(hashlib.sha256(actual).hexdigest() == sha, "product preimage mismatch")
    for profile in ("test", "release"):
        binary = manifest["binaries"][profile]
        require(binary["features"] == [] and binary["profile"] == profile, "build mode mismatch")
        require(ordinary_default_features(root / "crates/app/Cargo.toml", binary["cargo_artifact"]) == binary["features"], "effective feature mismatch")
        require(Path(binary["exe"]).suffix.lower() == ".exe" and Path(binary["pdb"]).suffix.lower() == ".pdb", "binary/PDB required")
        for key in ("exe", "pdb"):
            require(digest(binary[key]) == binary[key + "_sha256"], "binary/PDB identity mismatch")
    for label in ("toolchain", "effective_config", "external_dependencies", "build_commands", "source_review"):
        item = manifest[label]
        require(digest(item["path"]) == item["sha256"], "missing build/review identity")
    for item in manifest["receipt_sources"]:
        require(digest(item["path"]) == item["sha256"], "preparation/build receipt changed")


def from_root_receipts(preparation_path, build_path, archive_path, suffix_path, bindings):
    """Bind root's actual baseline build01 shape; never fills missing identities.

    Also usable for a candidate only if its reviewed receipts use this schema.
    Caller writes/reviews returned collection manifest separately, before release.
    Bindings supplies reviewed toolchain/config/dependency/command/source reports.
    """
    preparation, build = load(preparation_path), load(build_path)
    require(build["status"] == "passed" and build["manifest_sha256"] == digest(preparation_path), "build not terminal or wrong preparation")
    require(build["commit"] == preparation["commit"] and build["overlay_snapshot"] == preparation["overlay_snapshot"], "build source identity mismatch")
    require(preparation["overlay"]["operation"] == "original bytes + LF + identical reviewed suffix02", "unknown overlay recipe")
    require([step["profile"] for step in build["steps"]] == ["test", "release"], "missing build profile")
    binaries = {}
    for step in build["steps"]:
        require(step["exit_code"] == 0 and len(step["artifacts"]) == 1, "build artifact ambiguity")
        require(digest(step["log"]) == step["log_sha256"], "build log changed")
        artifact = step["artifacts"][0]
        cargo = artifact["cargo_artifact"]
        effective = ordinary_default_features(Path(preparation["source_root"]) / "crates/app/Cargo.toml", cargo)
        binaries[step["profile"]] = {"exe": artifact["executable"], "exe_sha256": artifact["executable_sha256"], "pdb": artifact["pdb"], "pdb_sha256": artifact["pdb_sha256"], "features": effective, "cargo_artifact": cargo, "profile": step["profile"]}
    return {**bindings, "commit": preparation["commit"], "source_root": preparation["source_root"],
            "archive_path": str(archive_path), "archive_sha256": preparation["archive_sha256"],
            "files": preparation["overlay_files"], "product_files": preparation["source_files"],
            "suffix_path": str(suffix_path), "overlay": {"path": preparation["overlay"]["path"], "suffix_sha256": preparation["suffix_sha256"], "separator": "\n"},
            "binaries": binaries, "receipt_sources": [{"path": str(p), "sha256": digest(p)} for p in (preparation_path, build_path)]}


def sanitized_environment(original):
    # Values are never included in a receipt. Keep fixture-controlled scratch
    # homes; do not override HOME/APPDATA or replace the per-app store helper.
    removed = []
    result = {}
    for key, value in original.items():
        upper = key.upper()
        if upper.startswith("QUANTICK_") or upper.startswith("RUST_TEST_") or upper in ("RUST_LOG", "RUST_BACKTRACE"):
            removed.append(key)
        else:
            result[key] = value
    return result, sorted(removed)


def process_snapshot():
    # No command lines, paths, owners, titles, handles, environment or tokens.
    script = "$ErrorActionPreference='Stop'; Get-CimInstance Win32_Process | Where-Object ProcessId -ne $PID | ForEach-Object { [pscustomobject]@{pid=$_.ProcessId; name=$_.Name; created=$_.CreationDate.ToUniversalTime().ToString('o')} } | ConvertTo-Json -Compress"
    result = subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script], capture_output=True, check=True, encoding="utf-8")
    rows = strict_json(result.stdout)
    require(type(rows) is list and rows, "process proof unavailable")
    for row in rows:
        require(set(row) == {"pid", "name", "created"} and row["created"], "process creation identity missing")
    return rows


def check_idle(rows, allowed, benchmark_names, child_pid=None):
    identities = {(r["pid"], r["created"]) for r in allowed}
    for row in rows:
        if row["pid"] == child_pid:
            continue  # Popen owns this still-unreaped child during observation.
        hard_block = re.fullmatch(r"(cargo|rustc|rustdoc|cl|link|lld.*|clang.*|gcc.*|cc1.*|msbuild|cmake|ninja|quantick.*)(?:\.exe)?", row["name"], re.I) or row["name"].lower() in benchmark_names
        require(not hard_block, "overlapping compiler/benchmark")
        suspect = BLOCKED.fullmatch(row["name"]) or row["name"].lower() in benchmark_names
        require(not suspect or (row["pid"], row["created"]) in identities, "overlapping compiler/benchmark/automation")


def observe_idle(proof, phase, allowed, names, include_self=False):
    rows = process_snapshot()
    observation = {"phase": phase, "utc_ns": time.time_ns(), "processes": rows}
    # Persist the observation before any exclusion or own-identity check.
    with proof.open("a", encoding="utf-8") as evidence:
        evidence.write(json.dumps(observation) + "\n")
    try:
        if include_self:
            own = [p for p in rows if p["pid"] == os.getpid()]
            require(len(own) == 1, "runner creation identity unavailable")
            allowed = allowed + own
        check_idle(rows, allowed, names)
    except InvalidEvidence as error:
        with proof.open("a", encoding="utf-8") as evidence:
            evidence.write(json.dumps({"phase": phase, "utc_ns": observation["utc_ns"], "reason": str(error)}) + "\n")
        raise
    return allowed


def collect_child(command, cwd, env, log, proof, allowed, names):
    child = None
    outcome = {"state": "started", "selector": command[1]}
    save(log.with_suffix(".receipt.json"), outcome)
    try:
        with log.open("xb") as stream:
            observe_idle(proof, "before", allowed, names)
            child = subprocess.Popen(command, cwd=cwd, env=env, stdout=stream, stderr=subprocess.STDOUT)
            outcome["pid"] = child.pid
            code = child.wait()
            outcome["exit_code"] = code
            observe_idle(proof, "after", allowed, names)
            require(code == 0, "benchmark child failed")
        outcome["state"] = "terminal"
    except BaseException as error:
        outcome["state"] = "failed"
        outcome["error_type"] = type(error).__name__
        # No bare process-group or wildcard signalling, only our child handle.
        if child is not None and child.poll() is None:
            try:
                child.terminate()
                outcome["cleanup_exit_code"] = child.wait()
            except BaseException as cleanup_error:
                outcome["secondary_cleanup_error_type"] = type(cleanup_error).__name__
        raise
    finally:
        if log.exists():
            outcome["log_sha256"] = digest(log)
        save(log.with_suffix(".receipt.json"), outcome)


def collect(release_path, output):
    require(sys.platform == "win32", "Windows collection only")
    release = load(release_path)
    require(output.resolve() == Path(release["output_directory"]).resolve(), "output not bound by finite release")
    require(release["authorization"] == "root finite owner-delivery collection" and release["decision_url"].startswith("https://github.com/"), "finite root release required")
    require(release["coordinator_window"] == "exclusive no-build no-benchmark no-native-automation window", "coordinator exclusion window required")
    require(release["baseline"] == BASELINE and re.fullmatch(r"[0-9a-f]{40}", release["candidate"]) and release["candidate"] != BASELINE, "candidate missing")
    require(release["plan"] == [list(p) for p in PLAN], "finite plan changed")
    for name in ("owner-delivery-performance02-proposal.md", "owner-delivery-performance01-source-preflight.md", "owner-action-measurement-suffix02.rs"):
        item = release["protocols"][name]
        require(digest(item["path"]) == item["sha256"], "protocol identity changed")
    # Reviewed policy binds permitted preexisting processes by creation ID,
    # all artifact/build manifests, and these exact runner/parser bytes.
    for name in ("owner_perf_runner01.py", "owner_perf_parser01.py"):
        require(digest(Path(__file__).with_name(name)) == release["tools"][name], "tool identity changed")
    manifests = {}
    for side in ("B", "C"):
        item = release["manifests"][side]
        require(digest(item["path"]) == item["sha256"], "manifest changed")
        manifests[side] = load(item["path"])
        verify_side(manifests[side], release["baseline" if side == "B" else "candidate"])
    for label in ("toolchain", "effective_config", "external_dependencies"):
        require(manifests["B"][label]["sha256"] == manifests["C"][label]["sha256"], "unequal toolchain/config/external dependencies")
    names = {Path(b["exe"]).name.lower() for m in manifests.values() for b in m["binaries"].values()}
    environment, removed = sanitized_environment(os.environ)
    allowed = release["allowed_processes"]
    proof = output / "boundaries.processes.jsonl"
    allowed = observe_idle(proof, "admission", allowed, names, include_self=True)
    status = {"state": "started", "release_sha256": digest(release_path), "removed_environment_names": removed, "completed": []}
    save(output / "terminal.json", status)
    results = {mode: {} for mode in SELECTORS}
    try:
        for position, (mode, name) in enumerate(PLAN):
            for side in ("B", "C"):
                verify_side(manifests[side], release["baseline" if side == "B" else "candidate"])
            observe_idle(proof, f"{position:02d}-{mode}-{name}-before-source-admitted-child", allowed, names)
            manifest = manifests[name[0]]
            binary = manifest["binaries"]["release" if mode == "long" else "test"]
            command = [binary["exe"], SELECTORS[mode], "--exact", "--ignored", "--nocapture", "--test-threads=1"]
            stem = output / f"{position:02d}-{mode}-{name}"
            child_proof = stem.with_suffix(".processes.jsonl")
            save(stem.with_suffix(".command.json"), {"binary_sha256": binary["exe_sha256"], "args": command[1:], "manifest_sha256": release["manifests"][name[0]]["sha256"]})
            collect_child(command, manifest["source_root"], environment, stem.with_suffix(".log"), child_proof, allowed, names)
            verify_side(manifest, release["baseline" if name[0] == "B" else "candidate"])
            observe_idle(proof, f"{position:02d}-{mode}-{name}-after-source-verified-child", allowed, names)
            rows = parse(stem.with_suffix(".log").read_text(encoding="utf-8"), mode)
            if mode in ("timing", "allocations"):
                # Stop at the first semantic difference, not at end of mode.
                prior = next(iter(results[mode].values()), None)
                if prior is not None:
                    require([r["output"] for r in prior] == [r["output"] for r in rows], "semantic identity mismatch")
                if mode == "allocations":
                    require([r["output"] for r in results["timing"][name]] == [r["output"] for r in rows], "cross-mode semantic mismatch")
            results[mode][name] = rows
            save(stem.with_suffix(".parsed.json"), rows)
            status["completed"].append([mode, name])
            save(output / "terminal.json", status)
        summary = {mode: compare_actions(results[mode]) for mode in ("timing", "allocations")}
        summary["dense"] = results["dense"]
        baseline_cpu = statistics.median(results["dense"][r]["frame_cpu_ms"] for r in ("B1", "B2", "B3"))
        candidate_cpu = statistics.median(results["dense"][r]["frame_cpu_ms"] for r in ("C1", "C2", "C3"))
        summary["dense_investigation"] = {"baseline_median_cpu_average": baseline_cpu, "candidate_median_cpu_average": candidate_cpu,
                                          "greater_than_10_percent": candidate_cpu > baseline_cpu * 1.10,
                                          "below_trigger_is_not_acceptance": True}
        summary["long"] = results["long"]
        summary["assessment"] = "Collection only; action shifts/tails and dense regression require independent analysis. No performance PASS."
        save(output / "summary.json", summary)
        status["state"] = "terminal"
    except BaseException as error:
        status["state"] = "failed"
        status["error_type"] = type(error).__name__
        # Stable coded parser exceptions are safe; never dump raw OS errors,
        # environment values, command lines, or artifact/trader paths.
        if isinstance(error, InvalidEvidence):
            status["reason"] = str(error)
        raise
    finally:
        save(output / "terminal.json", status)


def run(release_path, output):
    require(not output.exists(), "output already exists; retries forbidden")
    output.mkdir()
    admission = {"state": "started", "utc_ns": time.time_ns()}
    save(output / "admission.json", admission)
    try:
        collect(release_path, output)
        admission["state"] = "terminal"
    except BaseException as error:
        admission.update(state="failed", error_type=type(error).__name__)
        if isinstance(error, InvalidEvidence):
            admission["reason"] = str(error)
        raise
    finally:
        admission["terminal_utc_ns"] = time.time_ns()
        save(output / "admission.json", admission)
        terminal_path = output / "terminal.json"
        terminal = load(terminal_path) if terminal_path.exists() else {}
        terminal.update(state=admission["state"])
        for key in ("error_type", "reason"):
            if key in admission:
                terminal[key] = admission[key]
        terminal["artifact_hashes"] = {p.name: digest(p) for p in output.iterdir() if p.is_file() and p.name != "terminal.json"}
        save(terminal_path, terminal)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("release", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        run(args.release, args.output)
    except BaseException as failure:
        print("Collection stopped: " + type(failure).__name__, file=sys.stderr)
        sys.exit(1)
