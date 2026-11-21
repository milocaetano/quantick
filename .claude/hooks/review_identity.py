#!/usr/bin/env python3
"""Stable reviewed changes, retaining whitespace, file modes and binary content."""

import hashlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile

LEDGER = "docs/quality/read-cost/ledger.md"
CONTROL_DOCS = {
    "docs/control-plane/capability-inventory.md",
    "docs/control-plane/retry-matrix.md",
}


def git(repo, *args, data=None):
    return subprocess.check_output(["git", "-C", str(repo), *args], input=data)


def generated_candidate(path):
    return path in CONTROL_DOCS or (
        path.startswith("schemas/control/")
        and "/" not in path[len("schemas/control/"):]
        and (path.endswith(".schema.json") or path.endswith("-catalog-v1.json"))
    )


def verified_outputs(repo, paths):
    """Only B2 output bytes regenerated from the exact committed source qualify.

    Failure is conservative: every unproved output remains in the review.
    GUI outputs and generator inputs are never candidates. An immutable tree
    identifies the proof cache, as it identifies all generator inputs too.
    """
    candidates = {path for path in paths if generated_candidate(path)}
    if not candidates:
        return set()
    tree = git(repo, "rev-parse", "HEAD^{tree}").decode().strip()
    git_dir = Path(git(repo, "rev-parse", "--absolute-git-dir").decode().strip())
    proof = git_dir / "review-generated" / tree
    if proof.exists():
        return candidates.intersection(proof.read_text(encoding="utf-8").splitlines())
    try:
        with tempfile.TemporaryDirectory(prefix="quantick-review-") as temporary:
            snapshot = Path(temporary) / "source"
            output = Path(temporary) / "output"
            snapshot.mkdir()
            archive = git(repo, "archive", "--format=tar", "HEAD")
            with tarfile.open(fileobj=io.BytesIO(archive)) as files:
                files.extractall(snapshot, filter="data")
            environment = os.environ.copy()
            environment.setdefault("CARGO_TARGET_DIR", str(Path(repo).resolve() / "target"))
            subprocess.run(
                ["cargo", "run", "--locked", "--offline", "-q", "-p",
                 "quantick-control-handlers", "--bin", "regen-control", "--",
                 "--write", "--root", str(output)],
                cwd=snapshot, env=environment, check=True,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            verified = set()
            for produced in output.rglob("*"):
                if produced.is_file():
                    path = produced.relative_to(output).as_posix()
                    committed = snapshot / path
                    if generated_candidate(path) and committed.is_file() and (
                        produced.read_bytes() == committed.read_bytes()
                    ):
                        verified.add(path)
            proof.parent.mkdir(exist_ok=True)
            proof.write_text("\n".join(sorted(verified)), encoding="utf-8")
            return candidates.intersection(verified)
    except (OSError, subprocess.SubprocessError, tarfile.TarError, ValueError):
        return set()


def identity(repo, base):
    ancestor = git(repo, "merge-base", base, "HEAD").decode().strip()
    # Keep both sides of a rename. The patch below deliberately disables rename
    # collapsing, so path discovery must use the same view or source deletions
    # (including moves into an excluded generated destination) disappear.
    paths = git(repo, "diff", "--name-only", "--no-renames", "-z",
                ancestor, "HEAD").decode().split("\0")
    raw = git(repo, "diff", "--raw", "--no-renames", "-z", ancestor, "HEAD").split(b"\0")
    modes = {raw[index + 1].decode() for index in range(0, len(raw) - 1, 2)
             if raw[index].split()[1] != b"100644"
             or raw[index].split()[0][1:] not in (b"000000", raw[index].split()[1])}
    eligible = set(paths) - modes
    paths = sorted(set(paths) - {"", LEDGER} - verified_outputs(repo, eligible))
    patch = b""
    if paths:
        patch = git(repo, "diff", "--no-ext-diff", "--no-textconv", "--no-renames",
                    "--binary", "--full-index", "--unified=0", ancestor, "HEAD", "--", *paths)
    stable = git(repo, "patch-id", "--stable", data=patch).split()
    # Coordinates and blob IDs move on context-only synchronization. Retain the
    # coordinate-free hunk label, however: it identifies the enclosing function
    # and prevents an identical edit moved to another semantic location from
    # inheriting approval. Exact changed lines retain indentation, modes and
    # binary patches beside patch-id, which alone ignores whitespace.
    changed_lines = []
    for line in patch.splitlines(keepends=True):
        if line.startswith(b"@@ "):
            marker = line.find(b" @@", 3)
            changed_lines.append(line[marker + 3:] if marker >= 0 else b"\n")
        elif not line.startswith(b"index "):
            changed_lines.append(line)
    changed = b"".join(changed_lines)
    return hashlib.sha256((stable[0] if stable else b"") + b"\0" + changed).hexdigest()


if __name__ == "__main__":
    try:
        print(identity(sys.argv[1], sys.argv[2]))
    except (OSError, subprocess.SubprocessError, IndexError) as error:
        print(f"Review identity unavailable: {error}", file=sys.stderr)
        sys.exit(1)
