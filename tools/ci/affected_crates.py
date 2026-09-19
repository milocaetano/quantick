#!/usr/bin/env python3
"""The workspace crates a diff can affect, for the draft-PR fast CI job.

    python tools/ci/affected_crates.py --base <rev> [--head <rev>] [--cargo-args]

Prints one package name per line (or `-p a -p b ...` with --cargo-args) and
explains every selection on stderr, so a run's log says why a crate ran.

The crate graph comes from `cargo metadata`, never from a list in this file:
a crate added to the workspace is covered the moment it exists. A changed file
selects

  * every crate, when it is a workspace-wide cargo input (WORKSPACE_INPUTS);
  * the crate whose manifest directory holds it;
  * any crate whose tracked files name it by path: a fixture, a script or a
    doc a test reads with `include_str!` or opens at run time, or a Rust file
    pulled in with `#[path]`, belongs to that test's crate too, whoever owns
    the directory (see `needles`);

and then every crate that depends on a selected one, through normal, dev or
build dependencies, transitively. `quantick-guards` always runs: its tests are
repository-wide ratchets that read files no crate graph reaches.

This is a fast signal, not the verdict. Full CI still runs everything before a
PR can be made ready; see .claude/hooks/README.md.
"""

import argparse
import json
import os
import subprocess
import sys

ALWAYS = ("quantick-guards",)

# Inputs every crate's build reads. A change to any of them selects the whole
# workspace.
WORKSPACE_INPUTS = (
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "clippy.toml",
    ".clippy.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
)
WORKSPACE_INPUT_DIRS = (".cargo/",)


def workspace(metadata):
    """(root, {name: {"dir": repo-relative dir, "deps": {workspace dep names}}})."""
    root = os.path.normpath(metadata["workspace_root"])
    members = set(metadata["workspace_members"])
    packages = {}
    for package in metadata["packages"]:
        if package["id"] not in members:
            continue
        directory = os.path.relpath(os.path.dirname(package["manifest_path"]), root)
        packages[package["name"]] = {
            "dir": directory.replace(os.sep, "/"),
            "deps": {d["name"] for d in package["dependencies"] if d.get("path")},
        }
    for info in packages.values():
        info["deps"] &= packages.keys()
    return root, packages


def owner(path, packages):
    """The crate whose manifest directory holds `path`, the deepest one."""
    best = None
    for name, info in packages.items():
        prefix = info["dir"] + "/"
        if path.startswith(prefix) and (best is None or len(prefix) > len(packages[best]["dir"]) + 1):
            best = name
    return best


def needles(path):
    """Literal strings a crate would use to name `path`: the repository path,
    and the path relative to `crates/` (how `include_str!("../../x/...")` or
    `#[path = ...]` spells a sibling crate's file). For a non-Rust file, also
    each ancestor directory at least two levels deep: a test that walks a
    fixture directory names the directory, not the file. A Rust file's
    ancestors are its crate's source tree, which comments everywhere name.
    """
    found = set()
    for candidate in (path, path[len("crates/"):] if path.startswith("crates/") else None):
        if not candidate:
            continue
        found.add(candidate)
        if path.endswith(".rs"):
            continue
        parts = candidate.split("/")
        for depth in range(len(parts) - 1, 1, -1):
            found.add("/".join(parts[:depth]))
    # Longest first, so the log names the most specific match.
    return sorted(found, key=lambda s: (-len(s), s))


def select(changed, packages, referrers):
    """{crate: reason} for the crates `changed` reaches directly.

    `referrers(needle)` returns the repository paths of tracked files, under a
    crate, that contain `needle` literally.
    """
    reasons = {}
    for path in changed:
        if path in WORKSPACE_INPUTS or path.startswith(WORKSPACE_INPUT_DIRS):
            for name in packages:
                reasons.setdefault(name, f"workspace input {path} changed")
            continue
        name = owner(path, packages)
        if name:
            reasons.setdefault(name, f"{path} changed")
        for needle in needles(path):
            for referrer in referrers(needle):
                name = owner(referrer, packages)
                if name:
                    reasons.setdefault(name, f"{referrer} names {needle}")
    return reasons


def reverse_closure(reasons, packages):
    """Add every crate that depends, transitively, on one already selected."""
    dependents = {name: set() for name in packages}
    for name, info in packages.items():
        for dep in info["deps"]:
            dependents[dep].add(name)
    selected = dict(reasons)
    frontier = sorted(selected)
    while frontier:
        current = frontier.pop()
        for dependent in sorted(dependents[current]):
            if dependent not in selected:
                selected[dependent] = f"depends on {current}"
                frontier.append(dependent)
    return selected


def affected(changed, packages, referrers):
    selected = reverse_closure(select(changed, packages, referrers), packages)
    for name in ALWAYS:
        if name in packages:
            selected.setdefault(name, "always runs: repository-wide ratchets")
    return selected


def git(*args, cwd):
    return subprocess.run(
        ("git",) + args, cwd=cwd, check=True, capture_output=True, text=True
    ).stdout


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--base", required=True, help="the PR's base commit")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--cargo-args", action="store_true", help="print -p flags")
    args = parser.parse_args(argv)

    metadata = json.loads(subprocess.run(
        ("cargo", "metadata", "--format-version", "1", "--no-deps"),
        check=True, capture_output=True, text=True,
    ).stdout)
    root, packages = workspace(metadata)

    # --no-renames reports both sides of a move: the crate that lost the file is
    # affected as much as the one that gained it.
    changed = git("diff", "--name-only", "--no-renames", f"{args.base}...{args.head}", cwd=root).split()

    def referrers(needle):
        result = subprocess.run(
            ("git", "grep", "-l", "-F", "-e", needle, args.head, "--", "crates/"),
            cwd=root, capture_output=True, text=True,
        )
        if result.returncode not in (0, 1):
            raise SystemExit(f"git grep failed: {result.stderr.strip()}")
        # `git grep <rev>` prefixes each path with `<rev>:`.
        return [line.split(":", 1)[1] for line in result.stdout.splitlines()]

    selected = affected(changed, packages, referrers)
    for name in sorted(selected):
        print(f"{name}: {selected[name]}", file=sys.stderr)
    if args.cargo_args:
        print(" ".join(f"-p {name}" for name in sorted(selected)))
    else:
        print("\n".join(sorted(selected)))


if __name__ == "__main__":
    main()
