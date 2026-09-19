#!/usr/bin/env python3
"""Measure direct Rust source read cost between two explicit Git revisions.

The report counts production lines in changed Rust files under ``crates/`` and
in the source files containing modules those changed files directly reference.
It deliberately does not compute a transitive closure. Present files use their
head image; deleted files use the merge-base image the diff actually removed.
Output is canonical JSON, so the same repository objects produce
byte-identical evidence.

This is a lexical module resolver, not rustc. It supports ordinary external
``mod`` declarations and explicit/local ``use`` paths, including grouped uses,
aliases and re-exports. Ambiguous or unsupported local syntax is reported
rather than guessed. Run ``python tools/read_cost/measure.py --help`` for the
CLI.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys
from collections import defaultdict

VERSION = 1
QUALIFIED = re.compile(
    r"\b(?P<path>[A-Za-z_][A-Za-z0-9_]*"
    r"(?:\s*::\s*[A-Za-z_][A-Za-z0-9_]*)+)"
)
USE = re.compile(r"\buse\b")
USE_TOKEN = re.compile(r"::|[{},;*]|[A-Za-z_][A-Za-z0-9_]*")
MOD_DECL = re.compile(r"\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")
INLINE_MOD = re.compile(r"\bmod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{")
PATH_MOD = re.compile(
    r"#\s*\[\s*path\s*=\s*\"[^\"\n]*\"\s*\]\s*"
    r"(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*;"
)

LIMITATIONS = [
    "Direct references only; referenced modules are not traversed transitively.",
    "Lexical resolution is not rustc name or type resolution.",
    "Aliases, re-exports, and grouped imports are expanded lexically; later alias uses are not type-resolved.",
    "References inside inline module scopes are reported unresolved.",
    "Path-attributed, include!-provided, and macro-generated modules are not resolved.",
    "Custom Cargo target source paths outside conventional crate layouts are reported unresolved.",
]


class ReadCostError(RuntimeError):
    """The requested Git evidence could not be measured."""


def load_production_lexer():
    here = os.path.dirname(os.path.abspath(__file__))
    path = os.path.join(here, "..", "outside_score", "measure.py")
    spec = importlib.util.spec_from_file_location("outside_score_measure_frozen", path)
    if spec is None or spec.loader is None:
        raise ReadCostError(f"cannot load production-line lexer at {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


PRODUCTION = load_production_lexer()


def git(repo, *args, binary=False):
    command = ["git", "-C", repo, *args]
    try:
        result = subprocess.run(
            command,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except (OSError, subprocess.CalledProcessError) as problem:
        detail = getattr(problem, "stderr", b"")
        if isinstance(detail, bytes):
            detail = detail.decode("utf-8", errors="replace")
        raise ReadCostError(f"{' '.join(command)} failed: {detail.strip()}") from problem
    if binary:
        return result.stdout
    return result.stdout.decode("utf-8", errors="strict").strip()


def commit_sha(repo, revision):
    return git(
        repo,
        "rev-parse",
        "--verify",
        "--end-of-options",
        f"{revision}^{{commit}}",
    )


def file_sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(65536), b""):
            digest.update(block)
    return digest.hexdigest()


def production_text(source):
    code, _ = PRODUCTION.mask(source.replace("\r\n", "\n"))
    return PRODUCTION.blank_spans(code, PRODUCTION.test_spans(code))


def production_lines(source):
    return PRODUCTION.code_lines(production_text(source))


def parse_changes(repo, merge_base, head):
    raw = git(
        repo,
        "diff",
        "--name-status",
        "-z",
        "--find-renames",
        merge_base,
        head,
        "--",
        binary=True,
    )
    fields = raw.decode("utf-8", errors="strict").split("\0")
    if fields and fields[-1] == "":
        fields.pop()
    changes = []
    index = 0
    while index < len(fields):
        status = fields[index]
        index += 1
        if status.startswith(("R", "C")):
            if index + 1 >= len(fields):
                raise ReadCostError("truncated rename/copy record in Git diff")
            old_path, new_path = fields[index : index + 2]
            index += 2
        else:
            if index >= len(fields):
                raise ReadCostError("truncated path record in Git diff")
            path = fields[index]
            index += 1
            old_path = path if status.startswith("D") else None
            new_path = None if status.startswith("D") else path
        changes.append(
            {"new_path": new_path, "old_path": old_path, "status": status}
        )
    return sorted(
        changes,
        key=lambda item: (
            item["new_path"] or item["old_path"] or "",
            item["status"],
        ),
    )


def nearest_manifest(path, manifests):
    directory = path.rsplit("/", 1)[0] if "/" in path else ""
    candidates = [
        manifest
        for manifest in manifests
        if directory == manifest or directory.startswith(manifest + "/")
    ]
    return max(candidates, key=len) if candidates else None


def module_context(path, crate_dir):
    local = path[len(crate_dir) + 1 :]
    if local == "build.rs":
        return "build", ()
    if not local.startswith("src/"):
        return None
    parts = local.split("/")[1:]
    if parts in (["lib.rs"], ["main.rs"]):
        return "primary", ()
    if parts[0] == "bin":
        if len(parts) == 2 and parts[1].endswith(".rs"):
            return f"bin:{parts[1][:-3]}", ()
        if len(parts) >= 2:
            target = f"bin:{parts[1].removesuffix('.rs')}"
            module_parts = parts[2:]
        else:
            return None
    else:
        target = "primary"
        module_parts = parts
    if not module_parts:
        return target, ()
    filename = module_parts[-1]
    directories = module_parts[:-1]
    if filename == "mod.rs":
        module = tuple(directories)
    elif filename.endswith(".rs"):
        module = tuple(directories + [filename[:-3]])
    else:
        return None
    return target, module


class Snapshot:
    def __init__(self, repo, sha, label):
        self.repo = repo
        self.sha = sha
        self.label = label
        listed = git(repo, "ls-tree", "-r", "-z", "--name-only", sha, "--", "crates", binary=True)
        self.paths = set(
            path for path in listed.decode("utf-8", errors="strict").split("\0") if path
        )
        self.manifests = sorted(
            path[: -len("/Cargo.toml")]
            for path in self.paths
            if path.endswith("/Cargo.toml")
        )
        self.sources = {}
        self.production = {}
        self.info = {}
        self.catalog = defaultdict(list)
        for path in sorted(self.paths):
            if not path.endswith(".rs") or PRODUCTION.is_test_file(path):
                continue
            crate_dir = nearest_manifest(path, self.manifests)
            if crate_dir is None:
                continue
            context = module_context(path, crate_dir)
            self.info[path] = {"crate": crate_dir, "context": context}
            if context is not None:
                target, module = context
                self.catalog[(crate_dir, target, module)].append(path)

    def source(self, path):
        if path not in self.sources:
            data = git(self.repo, "show", f"{self.sha}:{path}", binary=True)
            self.sources[path] = data.decode("utf-8", errors="strict")
        return self.sources[path]

    def line_count(self, path):
        if path not in self.production:
            self.production[path] = production_lines(self.source(path))
        return self.production[path]

    def is_production_source(self, path):
        return path in self.info and self.line_count(path) > 0


def inline_spans(prod):
    spans = []
    for match in INLINE_MOD.finditer(prod):
        opening = prod.find("{", match.start(), match.end())
        spans.append((match.start(), PRODUCTION.match_brace(prod, opening)))
    return spans


def containing_span(offset, spans):
    return any(start < offset < end for start, end in spans)


def use_statements(prod):
    for match in USE.finditer(prod):
        end = prod.find(";", match.end())
        if end < 0:
            continue
        yield match.start(), prod[match.end() : end]


def expand_use_tokens(tokens):
    """Expand a Rust use tree into lexical path segment tuples."""
    found = []
    position = 0

    def group(prefix):
        nonlocal position
        if position >= len(tokens) or tokens[position] != "{":
            return
        position += 1
        while position < len(tokens) and tokens[position] != "}":
            node(prefix)
            if position < len(tokens) and tokens[position] == ",":
                position += 1
        if position < len(tokens) and tokens[position] == "}":
            position += 1

    def node(prefix):
        nonlocal position
        path = list(prefix)
        if position < len(tokens) and tokens[position] == "{":
            group(path)
            return
        while position < len(tokens):
            token = tokens[position]
            if token in (",", "}", ";"):
                break
            if token == "as":
                position += 1
                if position < len(tokens):
                    position += 1
                break
            if token == "*":
                position += 1
                found.append(tuple(path))
                break
            if token == "{":
                group(path)
                break
            if token == "::":
                position += 1
                continue
            position += 1
            if token == "self" and path:
                found.append(tuple(path))
                continue
            path.append(token)
            if position >= len(tokens) or tokens[position] != "::":
                found.append(tuple(path))
                break

    if tokens and tokens[0] == "{":
        group([])
    else:
        node([])
    return [path for path in found if path]


def reference_paths(prod):
    references = []
    for match in QUALIFIED.finditer(prod):
        path = re.sub(r"\s+", "", match.group("path")).split("::")
        references.append((match.start(), tuple(path), "qualified"))
    for offset, statement in use_statements(prod):
        tokens = USE_TOKEN.findall(statement)
        for path in expand_use_tokens(tokens):
            references.append((offset, path, "use"))
    unique = {(offset, path, kind) for offset, path, kind in references}
    return sorted(unique, key=lambda item: (item[0], item[1], item[2]))


def resolve_path(snapshot, source_path, segments):
    info = snapshot.info.get(source_path)
    if info is None or info["context"] is None:
        return [], "unsupported_source_layout"
    crate_dir = info["crate"]
    target, current = info["context"]
    first = segments[0]
    rest = list(segments[1:])
    explicit = first in ("crate", "self", "super")
    if first == "crate":
        base = ()
    elif first == "self":
        base = current
    elif first == "super":
        levels = 1
        while rest and rest[0] == "super":
            levels += 1
            rest.pop(0)
        if levels > len(current):
            return [], "super_outside_crate"
        base = current[: len(current) - levels]
    else:
        base = ()
        rest = list(segments)

    if not explicit:
        local = []
        for local_base in dict.fromkeys((current, ())):
            for length in range(len(rest), 0, -1):
                key = (crate_dir, target, tuple(local_base) + tuple(rest[:length]))
                candidates = snapshot.catalog.get(key, [])
                if candidates:
                    local.extend(candidates)
                    break
        local = sorted(set(local))
        if len(local) > 1:
            return local, "ambiguous_bare_module"
        if local:
            return local, None
        return [], "external_or_unresolved_bare_path"

    for length in range(len(rest), -1, -1):
        key = (crate_dir, target, tuple(base) + tuple(rest[:length]))
        candidates = snapshot.catalog.get(key, [])
        if candidates:
            if len(candidates) > 1:
                return sorted(candidates), "ambiguous_module"
            return list(candidates), None
    return [], "module_not_found"


def resolve_mod(snapshot, source_path, name):
    if source_path.endswith(("/lib.rs", "/main.rs", "/mod.rs")):
        directory = source_path.rsplit("/", 1)[0]
    elif source_path.endswith(".rs"):
        directory = source_path[:-3]
    else:
        return [], "unsupported_source_layout"
    candidates = [
        candidate
        for candidate in (f"{directory}/{name}.rs", f"{directory}/{name}/mod.rs")
        if candidate in snapshot.info
    ]
    if len(candidates) > 1:
        return sorted(candidates), "ambiguous_module"
    if candidates:
        return candidates, None
    return [], "module_not_found"


def line_at(text, offset):
    return text.count("\n", 0, offset) + 1


def direct_references(snapshot, source_path):
    prod = production_text(snapshot.source(source_path))
    inline = inline_spans(prod)
    unresolved = []
    resolved = []
    path_mod_offsets = set()

    for match in PATH_MOD.finditer(prod):
        mod = MOD_DECL.search(prod, match.start(), match.end())
        if mod is None:
            continue
        path_mod_offsets.add(mod.start())
        unresolved.append(
            {
                "expression": f"mod {mod.group(1)}",
                "from": source_path,
                "line": line_at(prod, mod.start()),
                "reason": "path_attribute",
            }
        )

    for match in MOD_DECL.finditer(prod):
        if match.start() in path_mod_offsets:
            continue
        expression = f"mod {match.group(1)}"
        if containing_span(match.start(), inline):
            unresolved.append(
                {
                    "expression": expression,
                    "from": source_path,
                    "line": line_at(prod, match.start()),
                    "reason": "inline_module_scope",
                }
            )
            continue
        candidates, reason = resolve_mod(snapshot, source_path, match.group(1))
        if reason:
            unresolved.append(
                {
                    "candidates": candidates,
                    "expression": expression,
                    "from": source_path,
                    "line": line_at(prod, match.start()),
                    "reason": reason,
                }
            )
        else:
            resolved.append((candidates[0], expression, "mod"))

    for offset, segments, kind in reference_paths(prod):
        expression = "::".join(segments)
        if containing_span(offset, inline):
            unresolved.append(
                {
                    "expression": expression,
                    "from": source_path,
                    "line": line_at(prod, offset),
                    "reason": "inline_module_scope",
                }
            )
            continue
        candidates, reason = resolve_path(snapshot, source_path, segments)
        if reason == "external_or_unresolved_bare_path":
            continue
        if reason:
            entry = {
                "expression": expression,
                "from": source_path,
                "line": line_at(prod, offset),
                "reason": reason,
            }
            if candidates:
                entry["candidates"] = candidates
            unresolved.append(entry)
        else:
            resolved.append((candidates[0], expression, kind))

    return sorted(set(resolved)), sorted(
        {json.dumps(item, sort_keys=True): item for item in unresolved}.values(),
        key=lambda item: (
            item["from"],
            item["line"],
            item["expression"],
            item["reason"],
        ),
    )


def render(report):
    return json.dumps(report, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def measure(repo, base_revision, head_revision):
    repo = os.path.abspath(repo)
    base = commit_sha(repo, base_revision)
    head = commit_sha(repo, head_revision)
    merge_base = git(repo, "merge-base", base, head)
    changes = parse_changes(repo, merge_base, head)
    merge_base_tree = Snapshot(repo, merge_base, "merge_base")
    head_tree = Snapshot(repo, head, "head")
    snapshots = {"merge_base": merge_base_tree, "head": head_tree}
    rename_to_head = {
        change["old_path"]: change["new_path"]
        for change in changes
        if change["status"].startswith(("R", "C"))
    }
    entries = {}
    roots = []

    def add(path, revision, role):
        snapshot = snapshots[revision]
        existing = entries.get(path)
        if existing is None or (
            existing["revision"] == "merge_base" and revision == "head"
        ):
            info = snapshot.info[path]
            entries[path] = {
                "crate": info["crate"],
                "lines": snapshot.line_count(path),
                "path": path,
                "revision": revision,
                "roles": set() if existing is None else existing["roles"],
            }
        entries[path]["roles"].add(role)

    for change in changes:
        if change["status"].startswith("D"):
            path, revision = change["old_path"], "merge_base"
        else:
            path, revision = change["new_path"], "head"
        snapshot = snapshots[revision]
        if path is None or not snapshot.is_production_source(path):
            continue
        add(path, revision, "touched")
        roots.append((path, revision))

    edges = []
    unresolved = []
    for source_path, revision in sorted(set(roots)):
        snapshot = snapshots[revision]
        resolved, missing = direct_references(snapshot, source_path)
        unresolved.extend({**item, "revision": revision} for item in missing)
        for target_path, expression, kind in resolved:
            target_revision = revision
            canonical = target_path
            if revision == "merge_base" and target_path in rename_to_head:
                renamed = rename_to_head[target_path]
                if renamed in head_tree.info:
                    canonical, target_revision = renamed, "head"
            elif revision == "merge_base" and target_path in head_tree.info:
                target_revision = "head"
            target_snapshot = snapshots[target_revision]
            if canonical not in target_snapshot.info:
                unresolved.append(
                    {
                        "expression": expression,
                        "from": source_path,
                        "line": None,
                        "reason": "target_not_production_source",
                        "revision": revision,
                    }
                )
                continue
            add(canonical, target_revision, "referenced")
            edges.append(
                {
                    "expression": expression,
                    "from": source_path,
                    "kind": kind,
                    "revision": revision,
                    "to": canonical,
                }
            )

    files = []
    for path in sorted(entries):
        entry = entries[path]
        files.append({**entry, "roles": sorted(entry["roles"])})
    edge_rows = [
        dict(row)
        for _, row in sorted(
            {json.dumps(row, sort_keys=True): row for row in edges}.items()
        )
    ]
    unresolved_rows = [
        dict(row)
        for _, row in sorted(
            {json.dumps(row, sort_keys=True): row for row in unresolved}.items()
        )
    ]
    here = os.path.dirname(os.path.abspath(__file__))
    return {
        "base": base,
        "changes": changes,
        "files": files,
        "head": head,
        "limitations": LIMITATIONS,
        "merge_base": merge_base,
        "measurement_inputs": {
            "calculator_sha256": file_sha256(os.path.join(here, "measure.py")),
            "production_lexer": "tools/outside_score/measure.py",
            "production_lexer_sha256": file_sha256(
                os.path.join(here, "..", "outside_score", "measure.py")
            ),
        },
        "production_change": "measured" if roots else "not_applicable",
        "production_lines": sum(entry["lines"] for entry in files),
        "references": edge_rows,
        "schema": VERSION,
        "scope": "direct_in_crate_modules",
        "unresolved": unresolved_rows,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, help="path to the Git repository")
    parser.add_argument("--base", required=True, help="explicit pull-request base revision")
    parser.add_argument("--head", required=True, help="explicit pull-request head revision")
    args = parser.parse_args(argv)
    try:
        report = measure(args.repo, args.base, args.head)
    except ReadCostError as problem:
        parser.exit(2, f"error: {problem}\n")
    sys.stdout.write(render(report))
    return 0


if __name__ == "__main__":
    sys.exit(main())
