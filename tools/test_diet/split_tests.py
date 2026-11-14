"""Move large inline `#[cfg(test)] mod <name> { ... }` blocks out of
`crates/app/src` production files into sibling module files.

    python tools/test_diet/split_tests.py [--check] [--min-lines N] [--root DIR]

For `foo.rs` the body goes to `foo/<name>.rs`; for `mod.rs`, `main.rs` and
`lib.rs` it goes to `<name>.rs` beside the file. The parent keeps its
attributes and a `mod <name>;` declaration, so module paths and test names
do not change. Bodies are written byte for byte at their original
indentation, then the new files go through `rustfmt` (never a hand
de-indent, which would rewrite multi-line string fixtures). The one edit made
inside a body: a relative `include_str!`/`include_bytes!` path gains a
`../` when the module file lands one directory deeper.

Only modules named `tests` or `*_tests` move, because those are the file
names `quantick-guards` treats as test code (`size::tracked`). The script is idempotent: a
second run finds nothing to move. It refuses to overwrite an existing file.
With `--check` it reports what it would move and exits 1 if anything is left.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

SOURCE = Path("crates/app/src")
MIN_LINES = 40
EDITION = "2024"
ROOT_FILES = {"mod.rs", "main.rs", "lib.rs"}
MOD_LINE = re.compile(rb"^((?:pub(?:\([^)]*\))? )?)mod ([A-Za-z_][A-Za-z0-9_]*) \{$")
DECL_LINE = re.compile(rb"^(?:pub(?:\([^)]*\))? )?mod ([A-Za-z_][A-Za-z0-9_]*);$")
INCLUDE = re.compile(rb'(include_(?:str|bytes)!\(\s*)"([^"\\]*)"')


@dataclass
class Block:
    name: str
    start: int  # index of the `#[cfg(test)]` line
    mod_line: int  # index of the `mod name {` line
    end: int  # index of the closing `}` line
    visibility: bytes


def lines_of(data: bytes) -> list[bytes]:
    return data.split(b"\n")


def skip_code(data: bytes, i: int) -> int:
    """Index just past the token starting at `i` when it is a comment,
    string or char literal; `i` itself otherwise."""
    n = len(data)
    c = data[i : i + 1]
    if data.startswith(b"//", i):
        j = data.find(b"\n", i)
        return n if j < 0 else j
    if data.startswith(b"/*", i):
        depth, j = 1, i + 2
        while j < n and depth:
            if data.startswith(b"/*", j):
                depth, j = depth + 1, j + 2
            elif data.startswith(b"*/", j):
                depth, j = depth - 1, j + 2
            else:
                j += 1
        return j
    raw = re.match(rb'[bc]?r(#*)"', data[i : i + 300])
    if raw and (i == 0 or not (data[i - 1 : i].isalnum() or data[i - 1 : i] == b"_")):
        close = b'"' + raw.group(1)
        j = data.find(close, i + raw.end())
        if j < 0:
            raise ValueError(f"unterminated raw string at byte {i}")
        return j + len(close)
    if c == b'"' or (data.startswith(b'b"', i) and not data[i - 1 : i].isalnum()):
        j = i + (2 if c == b"b" else 1)
        while j < n:
            if data[j : j + 1] == b"\\":
                j += 2
            elif data[j : j + 1] == b'"':
                return j + 1
            else:
                j += 1
        raise ValueError(f"unterminated string at byte {i}")
    if c == b"'":
        # A char literal, or a lifetime / label (`'a`), which has no close.
        m = re.match(rb"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)|[^\\'\n])'", data[i : i + 16], re.S)
        if m is None:
            # Multi-byte UTF-8 char literal.
            m = re.match(rb"'[\xc0-\xff][\x80-\xbf]{1,3}'", data[i : i + 6])
        return i + m.end() if m else i + 1
    return i


def matching_close(data: bytes, open_at: int) -> int:
    """Byte index of the `}` that closes the `{` at `open_at`."""
    depth, i = 0, open_at
    while i < len(data):
        j = skip_code(data, i)
        if j != i:
            i = j
            continue
        ch = data[i : i + 1]
        if ch == b"{":
            depth += 1
        elif ch == b"}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise ValueError("unbalanced braces")


def find_blocks(data: bytes, min_lines: int) -> list[Block]:
    lines = lines_of(data)
    offsets, pos = [], 0
    for line in lines:
        offsets.append(pos)
        pos += len(line) + 1
    blocks: list[Block] = []
    i = 0
    while i < len(lines):
        if lines[i].rstrip(b"\r") != b"#[cfg(test)]":
            i += 1
            continue
        j = i + 1
        while j < len(lines) and lines[j].startswith(b"#["):
            j += 1
        m = MOD_LINE.match(lines[j].rstrip(b"\r")) if j < len(lines) else None
        if m is None:
            i += 1
            continue
        open_at = offsets[j] + lines[j].rstrip(b"\r").rindex(b"{")
        close_at = matching_close(data, open_at)
        end = data.count(b"\n", 0, close_at)
        if offsets[end] != close_at or lines[end].rstrip(b"\r") != b"}":
            raise ValueError(f"block `{m.group(2).decode()}` does not close on a column-0 `}}` line")
        name = m.group(2).decode()
        if end - i + 1 >= min_lines and (name == "tests" or name.endswith("_tests")):
            blocks.append(Block(name, i, j, end, m.group(1)))
        i = end + 1
    return blocks


def target_for(path: Path, name: str) -> tuple[Path, bool]:
    """The module file for `name` declared in `path`, and whether it sits one
    directory deeper than `path`."""
    if path.name in ROOT_FILES:
        return path.parent / f"{name}.rs", False
    return path.parent / path.stem / f"{name}.rs", True


def deepen_includes(body: bytes) -> tuple[bytes, int]:
    count = 0

    def fix(m: re.Match[bytes]) -> bytes:
        nonlocal count
        literal = m.group(2)
        if literal.startswith(b"/") or re.match(rb"^[A-Za-z]:", literal):
            return m.group(0)
        count += 1
        return m.group(1) + b'"../' + literal + b'"'

    return INCLUDE.sub(fix, body), count


def is_test_path(relative: str) -> bool:
    """The guards' reading of a test file (`size::tracked`)."""
    parts = relative.split("/")
    return "tests" in parts or parts[-1] == "tests.rs" or parts[-1].endswith("_tests.rs")


def naturally_declared(source: Path, path: Path) -> bool:
    """Whether `path` is reached by a plain column-0 `mod <stem>;` in its
    natural parent. A module loaded through `#[path]` (written by hand or by a
    macro such as the surface catalog) resolves its own children beside
    itself, not in `<stem>/`, so moving its tests would point the
    declaration at the wrong file."""
    if path == source / "main.rs" or path == source / "lib.rs":
        return True
    if path.name == "mod.rs":
        stem, directory = path.parent.name, path.parent.parent
    else:
        stem, directory = path.stem, path.parent
    parents = [directory / "mod.rs"]
    if directory == source:
        parents += [source / "main.rs", source / "lib.rs"]
    else:
        parents.append(directory.with_suffix(".rs"))
    for parent in parents:
        if not parent.exists():
            continue
        lines = [line.rstrip(b"\r") for line in lines_of(parent.read_bytes())]
        for k, line in enumerate(lines):
            m = DECL_LINE.match(line)
            if not m or m.group(1).decode() != stem:
                continue
            j = k - 1
            attributes = []
            while j >= 0 and (lines[j].startswith(b"#[") or lines[j].startswith(b"//")):
                attributes.append(lines[j])
                j -= 1
            if not any(a.startswith(b"#[path") for a in attributes):
                return True
    return False


def test_module_files(root: Path, files: list[Path]) -> set[Path]:
    """Files compiled only under `cfg(test)`: declared by a `#[cfg(test)]`
    `mod x;` line, and every file below such a module."""
    test_dirs: set[Path] = set()
    marked: set[Path] = set()
    for path in files:
        lines = [line.rstrip(b"\r") for line in lines_of(path.read_bytes())]
        for k, line in enumerate(lines):
            m = DECL_LINE.match(line)
            if not m:
                continue
            j = k - 1
            while j >= 0 and lines[j].startswith(b"#[") and lines[j] != b"#[cfg(test)]":
                j -= 1
            if j < 0 or lines[j] != b"#[cfg(test)]":
                continue
            name = m.group(1).decode()
            base = path.parent if path.name in ROOT_FILES else path.parent / path.stem
            marked.add(base / f"{name}.rs")
            marked.add(base / name / "mod.rs")
            test_dirs.add(base / name)
    result = set(marked)
    for path in files:
        if any(d in path.parents for d in test_dirs):
            result.add(path)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--min-lines", type=int, default=MIN_LINES)
    parser.add_argument("--check", action="store_true", help="report only; exit 1 if a block remains")
    parser.add_argument("--no-rustfmt", action="store_true", help="leave the moved bodies unformatted")
    args = parser.parse_args()

    root = args.root
    files = sorted((root / SOURCE).rglob("*.rs"))
    skip = test_module_files(root, files)
    source = root / SOURCE
    moved, total_lines, touched, adjusted, unresolved = 0, 0, [], [], []
    for path in files:
        rel = path.relative_to(root).as_posix()
        if path in skip or is_test_path(rel):
            continue
        data = path.read_bytes()
        if b"\r\n" in data:
            raise SystemExit(f"{path}: CRLF line endings; expected LF")
        blocks = find_blocks(data, args.min_lines)
        if not blocks:
            continue
        if not naturally_declared(source, path):
            lines_left = sum(b.end - b.start + 1 for b in blocks)
            unresolved.append(f"{rel} ({lines_left} lines)")
            continue
        lines = lines_of(data)
        for block in reversed(blocks):
            target, deeper = target_for(path, block.name)
            if target.exists() or (target.parent / block.name / "mod.rs").exists():
                raise SystemExit(f"{rel}: target for `{block.name}` already exists")
            body_lines = lines[block.mod_line + 1 : block.end]
            # A blank first line survives one rustfmt pass at file start and
            # then fails `--check`; drop it rather than format twice.
            while body_lines and not body_lines[0].strip():
                body_lines.pop(0)
            body = b"\n".join(body_lines) + b"\n"
            if deeper:
                body, n = deepen_includes(body)
                if n:
                    adjusted.append(f"{rel}::{block.name} ({n})")
            total_lines += block.end - block.start + 1
            moved += 1
            print(f"{rel}::{block.name}: {block.end - block.start + 1} lines -> {target.relative_to(root).as_posix()}")
            if args.check:
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(body)
            decl = block.visibility + b"mod " + block.name.encode() + b";"
            lines[block.mod_line : block.end + 1] = [decl]
            touched.append(target)
        if not args.check:
            path.write_bytes(b"\n".join(lines))
            touched.append(path)
    print(f"{moved} blocks, {total_lines} lines")
    if unresolved:
        print("left inline (module loaded through #[path]): " + ", ".join(unresolved))
    if adjusted:
        print("include paths deepened in: " + ", ".join(adjusted))
    if args.check:
        return 1 if moved else 0
    new_files = [p for p in touched if p not in files]
    if new_files and not args.no_rustfmt:
        # rustfmt, not `cargo fmt`: the cargo shim can be blocked, and only
        # the moved bodies need re-indenting. rustfmt never edits a string's
        # contents, so fixtures inside the bodies stay byte-identical.
        subprocess.run(
            ["rustfmt", "--edition", EDITION, *[str(p) for p in new_files]],
            cwd=root,
            check=True,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
