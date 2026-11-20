#!/usr/bin/env python3
"""Check aggregate growth against a Git merge base, outside the process-free guards.

The saved base checkout also supplies QUANTICK_GUARDS_BASE_ROOT for cargo tests.
Measurement changes require exact signed `!measure-change <report-row> +N|-N`
lines in new ratchet fragments. Existing fragments cannot authorize a new change.
"""

import argparse
import os
from pathlib import Path
import subprocess
import sys


def run(args, repo, env=None):
    result = subprocess.run(args, cwd=repo, env=env, text=True, encoding="utf-8",
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    if result.returncode:
        raise ValueError(f"{' '.join(map(str, args))}: {result.stderr or result.stdout}")
    return result.stdout


def measurements(text):
    rows = {}
    for line in text.splitlines():
        key, value = line.split("\t")
        if key in rows:
            raise ValueError(f"duplicate measurement: {key}")
        if key.endswith((".budget", ".recorded")):
            continue
        # Single-consumer rows are names, not numeric measurements.
        if key.startswith("single_consumer."):
            continue
        if not value.isdecimal():
            raise ValueError(f"invalid measurement: {line}")
        rows[key] = int(value)
    if not rows:
        raise ValueError("empty measurement report")
    return rows


def coverage(repo, base):
    result = {}
    for directory in sorted((repo / "crates/guards").glob("*-baseline.d")):
        for fragment in sorted(directory.glob("*.txt")):
            relative = fragment.relative_to(repo)
            if (base / relative).exists():
                continue
            for line in fragment.read_text(encoding="utf-8").splitlines():
                content = line.split("#", 1)[0].strip()
                if not content.startswith("!measure-change"):
                    continue
                fields = content.split()
                if len(fields) != 3 or fields[0] != "!measure-change" or fields[2][:1] not in ("+", "-"):
                    raise ValueError(f"{relative}: invalid measurement coverage")
                key, value = fields[1:]
                if key in result:
                    raise ValueError(f"{relative}: duplicate coverage for {key}")
                result[key] = int(value)
    return result


def compare_measurements(old, new, signed):
    differences = {key: new.get(key, 0) - old.get(key, 0)
                   for key in old.keys() | new.keys()
                   if old.get(key) != new.get(key)}
    if differences != signed:
        missing = {key: value for key, value in differences.items() if signed.get(key) != value}
        stale = {key: value for key, value in signed.items() if differences.get(key) != value}
        raise ValueError(f"measurement disagreement; unsigned or incorrect={missing}; stale coverage={stale}")


def measurement_inputs_changed(repo, base_sha):
    changed = run(["git", "diff", "--name-only", base_sha, "--", "crates/guards"], repo).splitlines()
    changed += run(["git", "ls-files", "--others", "--exclude-standard", "--", "crates/guards"], repo).splitlines()
    return any(path.startswith("crates/guards/src/") or path.endswith(".txt") for path in changed)


def base_checkout(repo, reference):
    sha = run(["git", "merge-base", reference, "HEAD"], repo).strip()
    if len(sha) != 40 or any(char not in "0123456789abcdef" for char in sha):
        raise ValueError("Git did not return a merge-base SHA")
    base = repo / "target/ratchet-delta" / sha / "base"
    if base.exists():
        actual = run(["git", "rev-parse", "HEAD"], base).strip()
        dirty = run(["git", "status", "--porcelain", "--untracked-files=all"], base)
        if actual != sha or dirty:
            raise ValueError(f"cached base checkout is not pristine: {base}")
    else:
        base.parent.mkdir(parents=True, exist_ok=True)
        run(["git", "worktree", "add", "--detach", str(base), sha], repo)
    return sha, base


def build(repo, target):
    run(["cargo", "build", "-q", "-p", "quantick-guards", "--target-dir", str(target)], repo)
    return target / "debug" / ("quantick-guards.exe" if os.name == "nt" else "quantick-guards")


def report(binary, repo):
    env = dict(os.environ, QUANTICK_GUARDS_ROOT=str(repo))
    env.pop("QUANTICK_GUARDS_BASE_ROOT", None)
    return measurements(run([str(binary), "--report"], repo, env))


def check(repo, reference, binary=None, file=None, export_env=None):
    sha, base = base_checkout(repo, reference)
    binary = binary or build(repo, repo / "target")
    if measurement_inputs_changed(repo, sha):
        old_binary = build(base, base.parent / "build")
        compare_measurements(report(old_binary, repo), report(binary, repo), coverage(repo, base))
    args = [str(binary), "--base-root", str(base)]
    if file:
        args += ["--file", file]
    env = dict(os.environ, QUANTICK_GUARDS_ROOT=str(repo))
    output = run(args, repo, env)
    if export_env:
        with Path(export_env).open("a", encoding="utf-8") as stream:
            stream.write(f"QUANTICK_GUARDS_BASE_ROOT={base}\n")
    return f"ratchet delta base={sha} head={run(['git', 'rev-parse', 'HEAD'], repo).strip()}\n{output}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--base", required=True, help="PR target ref; push builds use HEAD^")
    parser.add_argument("--binary", type=Path, help="already built head guard (edit-time hook)")
    parser.add_argument("--file", help="limit non-aggregate guards to this file")
    parser.add_argument("--export-env", help="append the base path to a GitHub environment file")
    args = parser.parse_args()
    try:
        print(check(args.repo.resolve(), args.base, args.binary, args.file, args.export_env), end="")
    except (ValueError, OSError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
