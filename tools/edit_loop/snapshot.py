"""Materialize exact Git blobs for read-only selection; never execute archived code."""

import hashlib
import io
from pathlib import PurePosixPath
import tarfile
import tempfile

import inputs

ROOTS = ("crates", "tools/edit_loop", "tools/outside_score/measure.py",
         "Cargo.toml", "Cargo.lock", "rust-toolchain.toml")


def relative_path(name):
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or "\\" in name or ":" in name:
        raise ValueError("Git archive entry escapes the temporary read-only materialization")
    return path


def facts(repo, sha):
    if not inputs.FULL_SHA.fullmatch(sha):
        raise ValueError("exact SHA required for archived input validation")
    if inputs.git(repo, "cat-file", "-t", sha).strip() != b"commit":
        raise ValueError("measurement source SHA must identify a commit")
    entries = inputs.git(repo, "ls-tree", "-r", "-z", sha, "--", *ROOTS)
    expected = {}
    for entry in entries.split(b"\0"):
        if not entry:
            continue
        metadata, name = entry.split(b"\t", 1)
        mode, kind, oid = metadata.decode().split()
        relative = relative_path(name.decode("utf-8")).as_posix()
        if kind != "blob" or mode not in {"100644", "100755"}:
            raise ValueError("measurement inputs must be regular Git blobs, not links/submodules")
        expected[relative] = oid
    archive = inputs.git(repo, "archive", "--format=tar", sha, "--", *ROOTS)
    # This fresh library-owned temporary directory is the only disposable
    # filesystem material. Existing source/worktrees/targets are never modified.
    with tempfile.TemporaryDirectory(prefix="quantick-edit-loop-read-") as temporary:
        from pathlib import Path
        root = Path(temporary)
        found = set()
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as stream:
            for entry in stream:
                relative = relative_path(entry.name).as_posix()
                if entry.isdir():
                    continue
                if not entry.isfile() or relative not in expected or relative in found:
                    raise ValueError("unexpected or duplicate Git archive entry")
                data = stream.extractfile(entry).read()
                # Detect export-subst transformations as well as omitted export-ignore
                # paths: the lexer must read actual blobs, not Git archive substitutions.
                identity = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
                if identity != expected[relative]:
                    raise ValueError("archive bytes differ from the exact Git blob")
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
                found.add(relative)
        if found != set(expected):
            raise ValueError("Git archive omitted required measurement inputs")
        if (root / "tools/outside_score/measure.py").read_bytes() != inputs.FROZEN_LEXER.read_bytes():
            raise ValueError("the exact-SHA production lexer differs from the frozen checker")
        return {"input_hashes": inputs.input_hashes(root), "ranking": inputs.ranked_crates(root)}
