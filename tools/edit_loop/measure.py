#!/usr/bin/env python3
"""Measure owned, warm-dependency incremental Cargo edit loops; never assign a score.

run requires four disjoint paths: clean source checkout, NEW detached worktree,
NEW target root, NEW evidence directory. No path is deleted or cleaned. Budget
checking follows measurement even for an initially uncalibrated budget, so the
first calibration attempt is honestly red with useful raw artifacts.
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


def version(command, repo):
    return subprocess.check_output(command, cwd=repo, text=True, stderr=subprocess.STDOUT,
                                   timeout=300).strip()


def sample(crate, stage, repo, env, output, touched, process, observe):
    directory = output / stage
    directory.mkdir()
    command = ["cargo", "test", "-p", crate["package"]]
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
        recompiled = sampling.validate_sample(result, crate["package"], touched)
    except ValueError as error:
        inputs.write_json(directory / "validation.json", {"status": "failed", "error": str(error)})
        raise
    return {key: value for key, value in result.items() if not key.endswith("_text")} | {
        "directory": directory.relative_to(output.parent).as_posix(),
        "recompiled": recompiled, "tests_passed": True}


def series(crate, repo, sha, target, output, process=sampling.run_process, sleep=time.sleep,
           observe=inputs.compiler_processes):
    output.mkdir()
    target.mkdir()
    env, normalized = inputs.command_environment(repo, target, inputs.JOBS)
    row = dict(crate, normalized_environment=normalized, target=str(target),
               cache="new target; declared exact-command warm-up then incremental samples", samples=[])
    try:
        with sampling.SourceTouch(repo, repo / crate["source"], sha, output) as source:
            row["warmup"] = sample(crate, "warmup", repo, env, output, False, process, observe)
            row["control"] = sample(crate, "control", repo, env, output, False, process, observe)
            for number in range(inputs.SAMPLES):
                # Separation is outside timed Cargo wall time and is retained in the protocol.
                sleep(sampling.TOUCH_SEPARATION_NS / 1_000_000_000)
                touched_ns = source.touch()
                measured = sample(crate, f"touch-{number + 1}", repo, env, output, True, process, observe)
                measured["touched_mtime_ns"] = touched_ns
                row["samples"].append(measured)
                inputs.write_json(output / "series.json", row)
        row["summary"] = sampling.summary(row["samples"])
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
    env, _ = inputs.command_environment(repo, target / "guard-crosscheck", inputs.JOBS)
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
    inputs.new_paths(repo, worktree, target, output)
    output.mkdir(parents=True)
    report = {"schema": 1, "status": "failed", "source_sha": args.sha, "crates": [],
              "measured_utc": datetime.now(timezone.utc).isoformat(), "restored": False,
              "protocol": "mtime-only; exact package test warm-up/control/five touched samples",
              "jobs": inputs.JOBS, "worktree": str(worktree), "target_root": str(target)}
    try:
        inputs.clean_head(repo, args.sha)
        inputs.regular_path(repo, repo / "Cargo.toml")
        if repo.resolve() != inputs.ROOT:
            raise ValueError("execute the runner from the exact named source checkout")
        budget_path = Path(args.budgets).absolute()
        if budget_path != repo / "tools/edit_loop/budgets.json":
            raise ValueError("use the committed tools/edit_loop/budgets.json, not a replacement budget")
        budget_bytes = inputs.git(repo, "show", f"{args.sha}:tools/edit_loop/budgets.json")
        fixed_budget = json.loads(budget_bytes)
        report["budget_sha256"] = inputs.digest(budget_bytes)
        inputs.git(repo, "worktree", "add", "--detach", str(worktree), args.sha)
        inputs.clean_head(worktree, args.sha)
        target.mkdir(parents=True)
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
            row = series(crate, worktree, args.sha, target / crate["crate"], output / crate["crate"])
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
        budgets.check(validated, fixed_budget)
        report["budget_status"] = "pass"
        return 0
    except (Exception, KeyboardInterrupt) as error:
        report["error"] = f"{type(error).__name__}: {error}"
        report["budget_status"] = "failed"
        print(report["error"], file=sys.stderr)
        return 1
    finally:
        report["finished_utc"] = datetime.now(timezone.utc).isoformat()
        evidence.publish(output, report)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    measure = commands.add_parser("run")
    for name in ("repo", "sha", "worktree", "target", "output", "budgets"):
        measure.add_argument("--" + name, required=True)
    proposal = commands.add_parser("propose", help="print a fixed budget proposal for independent review")
    proposal.add_argument("report")
    proposal.add_argument("--repo", required=True, help="checkout containing the measured Git commit")
    checker = commands.add_parser("check", help="validate retained raw evidence against committed budgets")
    checker.add_argument("report")
    checker.add_argument("--repo", required=True)
    args = parser.parse_args(argv)
    if args.command == "run":
        return run(args)
    try:
        report = evidence.load(Path(args.report), Path(args.repo))
        if args.command == "propose":
            print(json.dumps(budgets.propose(report), indent=2, sort_keys=True, allow_nan=False))
        else:
            committed = json.loads(inputs.git(Path(args.repo), "show", "HEAD:tools/edit_loop/budgets.json"))
            budgets.check(report, committed)
            print("Raw evidence and committed fixed budgets pass.")
        return 0
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    sys.exit(main())
