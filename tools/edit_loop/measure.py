#!/usr/bin/env python3
"""Measure owned, warm-dependency incremental Cargo edit loops; never assign a score.

run requires four disjoint paths: clean source checkout, NEW detached worktree,
target root (new, or a directory restored from the workflow cache), NEW evidence
directory. No path is deleted or cleaned. Timing policy follows strict measurement validation; pull requests are advisory
and main compares recent compatible samples when enough history is available.
"""

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import sys
import time

import budgets
import evidence
import inputs
import sampling

# A package warm-up may compile the app's dependencies from an empty target.
# This timeout is a failure bound, not the calibrated incremental cost budget.
PROCESS_TIMEOUT_SECONDS = 3600
HISTORY_LIMIT = 100


def version(command, repo):
    return subprocess.check_output(command, cwd=repo, text=True, stderr=subprocess.STDOUT,
                                   timeout=300).strip()


def sample(crate, stage, repo, env, output, touched, tests, process, observe):
    directory = output / stage
    directory.mkdir()
    command = sampling.command(crate["package"], tests)
    before = observe()
    if before:
        inputs.write_json(directory / "contention.json", {"before": before})
        raise ValueError("another compiler/cache process is active; sample is not isolated")
    result = process(command, repo, env, directory, PROCESS_TIMEOUT_SECONDS)
    after = observe()
    inputs.write_json(directory / "contention.json", {"before": before, "after": after,
                      "coverage": "boundary snapshots, not proof of exclusive physical hardware"})
    if after:
        raise sampling.ProcessStillRunning("compiler/cache ownership after the sample is uncertain")
    try:
        recompiled = sampling.validate_sample(result, crate["package"], touched, tests)
    except ValueError as error:
        inputs.write_json(directory / "validation.json", {"status": "failed", "error": str(error)})
        raise
    return {key: value for key, value in result.items() if not key.endswith("_text")} | {
        "directory": directory.relative_to(output.parent).as_posix(),
        "recompiled": recompiled} | ({"tests_passed": True} if tests else {})


def series(crate, repo, sha, target, output, process=sampling.run_process, sleep=time.sleep,
           observe=inputs.compiler_processes):
    output.mkdir()
    target.mkdir(parents=True, exist_ok=True)
    env, normalized = inputs.command_environment(repo, target, inputs.JOBS)
    row = dict(crate, normalized_environment=normalized, target=str(target),
               cache="shared target, restored from the workflow cache when present; exact-command "
                     "build warm-up, touched incremental builds, then one complete test run",
               samples=[])
    try:
        with sampling.SourceTouch(repo, repo / crate["source"], sha, output) as source:
            row["warmup"] = sample(crate, "warmup", repo, env, output, False, False, process, observe)
            row["control"] = sample(crate, "control", repo, env, output, False, False, process, observe)
            for number in range(inputs.SAMPLES):
                # Separation is outside timed Cargo wall time and is retained in the protocol.
                sleep(sampling.TOUCH_SEPARATION_NS / 1_000_000_000)
                touched_ns = source.touch()
                measured = sample(crate, f"touch-{number + 1}", repo, env, output, True, False,
                                  process, observe)
                measured["touched_mtime_ns"] = touched_ns
                row["samples"].append(measured)
                inputs.write_json(output / "series.json", row)
            # The test binary is already built, so this times the run alone. The
            # suite is the same for every touch; running it once instead of five
            # times is most of what keeps the workflow inside seven minutes.
            row["tests"] = sample(crate, "tests", repo, env, output, False, True, process, observe)
        row["summary"] = sampling.summary(row["samples"], row["tests"]["elapsed_seconds"])
        row["restored"] = True
    except (Exception, KeyboardInterrupt) as error:
        row["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        inputs.write_json(output / "series.json", row)
    return row


def guard_crosscheck(repo, target, output):
    directory = output / "guard-crosscheck"
    directory.mkdir()
    env, _ = inputs.command_environment(repo, target, inputs.JOBS)
    result = sampling.run_process(["cargo", "run", "-p", "quantick-guards", "--", "--report"],
                                  repo, env, directory, PROCESS_TIMEOUT_SECONDS)
    if (result["exit_code"] != 0 or result["timed_out"]
            or not result["tree_quiescent"] or result["orphaned_descendants"]):
        raise ValueError("guard cross-check failed; retain its raw setup output")
    counts = {}
    for line in result["stdout_text"].splitlines():
        key, _, value = line.partition("\t")
        if key.startswith("crate.lines.") and value.isdigit():
            counts[key] = int(value)
    return {"counts": counts, "directory": "guard-crosscheck",
            "interpretation": "guard physical-line totals differ from frozen lexer production lines; "
                              "retained cross-check only, never the crate-selection definition"}


def run(args):
    repo, worktree, target, output = (Path(getattr(args, name)).absolute()
                                      for name in ("repo", "worktree", "target", "output"))
    target_restored = inputs.new_paths(repo, worktree, target, output)
    output.mkdir(parents=True)
    report = {"schema": 1, "status": "failed", "source_sha": args.sha, "crates": [],
              "measured_utc": datetime.now(timezone.utc).isoformat(), "restored": False,
              "protocol": "mtime-only; package test-build warm-up/control, five touched builds, "
                          "one complete package test run",
              "jobs": inputs.JOBS, "debug_info": inputs.DEBUG_INFO, "worktree": str(worktree),
              "target_root": str(target), "target_restored": target_restored}
    try:
        if args.history and any(Path(args.history).resolve().is_relative_to(path.resolve())
                                for path in (repo, worktree, target, output)):
            raise ValueError("timing history must be outside source, worktree, target and evidence paths")
        inputs.clean_head(repo, args.sha)
        inputs.regular_path(repo, repo / "Cargo.toml")
        if repo.resolve() != inputs.ROOT:
            raise ValueError("execute the runner from the exact named source checkout")
        budget_path = Path(args.budgets).absolute()
        if budget_path != repo / "tools/edit_loop/budgets.json":
            raise ValueError("use the committed tools/edit_loop/budgets.json, not a replacement budget")
        budget_bytes = inputs.git(repo, "show", f"{args.sha}:tools/edit_loop/budgets.json")
        json.loads(budget_bytes)  # Retain the historical calibration input identity.
        report["budget_sha256"] = inputs.digest(budget_bytes)
        inputs.git(repo, "worktree", "add", "--detach", str(worktree), args.sha)
        inputs.clean_head(worktree, args.sha)
        target.mkdir(parents=True, exist_ok=True)
        report["source_tree"] = inputs.git(worktree, "rev-parse", "HEAD^{tree}").decode().strip()
        report["input_hashes"] = inputs.input_hashes(worktree)
        report["protocol_hash"] = inputs.protocol_hash(report["input_hashes"])
        report["cargo_configuration"] = inputs.cargo_configuration(worktree)
        report["profile_hash"] = report["input_hashes"]["Cargo.toml"]
        report["host"] = inputs.host_identity()
        report["host_class"] = report["host"]["class"]
        report["versions"] = {"python": sys.version, "python_executable": sys.executable,
                              "rustc": version(["rustc", "-Vv"], worktree),
                              "cargo": version(["cargo", "-V"], worktree),
                              "clippy": version(["cargo", "clippy", "--version"], worktree),
                              "fmt": version(["cargo", "fmt", "--version"], worktree)}
        report["toolchain"] = report["versions"]["rustc"]
        report["ranking"] = inputs.ranked_crates(worktree)
        report["guard_crosscheck"] = guard_crosscheck(worktree, target, output)
        for row in report["ranking"][:3]:
            if report["guard_crosscheck"]["counts"].get("crate.lines." + row["crate"], 0) < row["production_lines"]:
                raise ValueError("guard physical-line cross-check cannot support the frozen selection")
        inputs.write_json(output / "report.json", report)
        for crate in report["ranking"][:3]:
            row = series(crate, worktree, args.sha, target, output / crate["crate"])
            report["crates"].append(row)
            inputs.write_json(output / "report.json", report)
        inputs.clean_head(worktree, args.sha)
        inputs.clean_head(repo, args.sha)
        if inputs.input_hashes(worktree) != report["input_hashes"]:
            raise ValueError("measured inputs changed during the experiment")
        if inputs.cargo_configuration(worktree) != report["cargo_configuration"]:
            raise ValueError("Cargo configuration changed during the experiment")
        report.update(status="complete", restored=True)
        evidence.publish(output, report)
        validated = evidence.load(output / "report.json", repo)
        report["timing_context"] = args.context
        report["timing_policy"] = budgets.check(validated, read_history(args.history), args.context)
        report["budget_status"] = report["timing_policy"]["status"]
        print(json.dumps(report["timing_policy"], indent=2))
        if args.context == "main" and args.history:
            save_history(Path(args.history), report)
        return 1 if report["budget_status"] == "failed" else 0
    except (Exception, KeyboardInterrupt) as error:
        report["error"] = f"{type(error).__name__}: {error}"
        report["budget_status"] = "failed"
        print(report["error"], file=sys.stderr)
        return 1
    finally:
        report["finished_utc"] = datetime.now(timezone.utc).isoformat()
        evidence.publish(output, report)


def read_history(path):
    if not path or not Path(path).exists():
        return []
    value = evidence.read_json(Path(path))
    if not isinstance(value, list):
        raise ValueError("timing history must be a list of main reports")
    return value


def save_history(path, report):
    prior = [row for row in read_history(path) if row["source_sha"] != report["source_sha"]]
    # Bound cache size while keeping enough recent runs for per-crate selection changes.
    rows = sorted(prior + [report], key=lambda row: (row["measured_utc"], row["source_sha"]))[-HISTORY_LIMIT:]
    path.parent.mkdir(parents=True, exist_ok=True)
    inputs.write_json(path, rows)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    measure = commands.add_parser("run")
    for name in ("repo", "sha", "worktree", "target", "output", "budgets"):
        measure.add_argument("--" + name, required=True)
    measure.add_argument("--context", choices=("pr", "main"), default="pr")
    measure.add_argument("--history", help="main-only report history; absent means cold start")
    proposal = commands.add_parser("propose", help="print a fixed budget proposal for independent review")
    proposal.add_argument("report")
    proposal.add_argument("--repo", required=True, help="checkout containing the measured Git commit")
    checker = commands.add_parser("check", help="validate retained raw evidence and report timing policy")
    checker.add_argument("report")
    checker.add_argument("--repo", required=True)
    checker.add_argument("--context", choices=("pr", "main"), default="pr")
    checker.add_argument("--history")
    args = parser.parse_args(argv)
    if args.command == "run":
        return run(args)
    try:
        report = evidence.load(Path(args.report), Path(args.repo))
        if args.command == "propose":
            print(json.dumps(budgets.propose(report), indent=2, sort_keys=True, allow_nan=False))
        else:
            result = budgets.check(report, read_history(args.history), args.context)
            print(json.dumps(result, indent=2))
            return 1 if result["status"] == "failed" else 0
        return 0
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    sys.exit(main())
