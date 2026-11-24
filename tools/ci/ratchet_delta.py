#!/usr/bin/env python3
"""Run the HEAD guard against a pristine checkout of the PR merge base."""

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


def base_checkout(repo, reference):
    sha = run(["git", "merge-base", reference, "HEAD"], repo).strip()
    if len(sha) != 40 or any(char not in "0123456789abcdef" for char in sha):
        raise ValueError("Git did not return a merge-base SHA")
    base = repo / "target/ratchet-delta" / sha / "base"
    if base.exists():
        actual = run(["git", "rev-parse", "HEAD"], base).strip()
        root = Path(run(["git", "rev-parse", "--show-toplevel"], base).strip()).resolve()
        dirty = run(["git", "status", "--porcelain", "--untracked-files=all", "--ignored"], base)
        if root != base.resolve() or actual != sha or dirty:
            raise ValueError(f"cached base checkout is not pristine: {base}")
    else:
        base.parent.mkdir(parents=True, exist_ok=True)
        run(["git", "worktree", "add", "--detach", str(base), sha], repo)
    return sha, base


def build(repo):
    target = repo / "target"
    run(["cargo", "build", "-q", "-p", "quantick-guards", "--target-dir", str(target)], repo)
    return target / "debug" / ("quantick-guards.exe" if os.name == "nt" else "quantick-guards")


def check(repo, reference, binary=None, file=None, export_env=None):
    sha, base = base_checkout(repo, reference)
    binary = binary or build(repo)
    args = [str(binary), "--base-root", str(base)]
    if file:
        args += ["--file", file]
    output = run(args, repo, dict(os.environ, QUANTICK_GUARDS_ROOT=str(repo)))
    if export_env:
        with Path(export_env).open("a", encoding="utf-8") as stream:
            stream.write(f"QUANTICK_GUARDS_BASE_ROOT={base}\n")
    return f"ratchet delta base={sha} head={run(['git', 'rev-parse', 'HEAD'], repo).strip()}\n{output}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--base", required=True, help="PR target ref; push builds use HEAD^")
    parser.add_argument("--binary", type=Path, help="already built HEAD guard")
    parser.add_argument("--file", help="limit non-aggregate guards to this file")
    parser.add_argument("--export-env", type=Path, help="append the base path to a GitHub environment file")
    args = parser.parse_args()
    try:
        binary = args.binary.resolve() if args.binary else None
        print(check(args.repo.resolve(), args.base, binary, args.file, args.export_env), end="")
    except (ValueError, OSError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
