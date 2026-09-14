#!/usr/bin/env python3
"""Print the measured rows the outside-score rubric grades, for one tree.

Usage:
    python tools/outside_score/measure.py <tree> [--ui-crate app] [--top 10]

<tree> is a checkout or an exported revision (`git archive <sha>`). Output is
`metric<TAB>value` rows in a fixed order, so two runs over one tree are
byte-identical and two revisions diff row by row.

This is a lexer, not a parser. It blanks comments and literal contents before
matching, so a brace or an `fn` inside a string never counts, and it strips
`#[cfg(test)]` items and test files the way the size guard does. Its numbers
can differ from `quantick-guards --report` by a few lines per file; where a
`--report` row exists for the same quantity, that row is authoritative.
"""

import os
import re
import sys
from collections import defaultdict

VERSION = 1
UI_TOOLKIT = re.compile(r"\b(?:egui|eframe)\b")
FN_ITEM = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")
IMPL = re.compile(r"\bimpl\b")
TEST_ATTR = re.compile(r"#\[cfg\((?:all\()?\s*test\b")
CRATE_VIS = re.compile(r"\bpub\s*\(\s*(?:crate|super)\s*\)")
UNWRAP = re.compile(r"\.unwrap\(\s*\)")
PANIC = re.compile(r"\b(?:panic|unreachable|todo|unimplemented)!")
EXPECT = re.compile(r"\.expect\(")
HOOK = re.compile(r"QUANTICK_[A-Z0-9_]+")
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
QUOTE = '"'


def is_test_file(rel):
    """A file only tests compile: under a `tests` directory, or a sidecar."""
    parts = rel.split("/")
    name = parts[-1]
    return (
        "tests" in parts[:-1]
        or name == "tests.rs"
        or name.endswith("_tests.rs")
        or any(p.endswith("_tests") for p in parts[:-1])
    )


def is_ident_char(c):
    return c.isalnum() or c == "_"


def raw_start(src, i):
    """For a raw string opening at i (`r"`, `r#"`, `br#"`, `cr"`), return
    (content start, hash count); otherwise None."""
    j = i
    if src[j] in "bc":
        j += 1
    if j >= len(src) or src[j] != "r":
        return None
    j += 1
    hashes = 0
    while j < len(src) and src[j] == "#":
        hashes, j = hashes + 1, j + 1
    if j < len(src) and src[j] == QUOTE:
        return j + 1, hashes
    return None


def mask(src):
    """Return (code, literals): comments and literal contents blanked to
    spaces with newlines kept, and every string literal as (offset, text)."""
    out = list(src)
    literals = []
    i, n = 0, len(src)

    def blank(a, b):
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        prev = src[i - 1] if i else ""
        starts_word = not is_ident_char(prev)
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif c in "rbc" and starts_word and raw_start(src, i):
            start, hashes = raw_start(src, i)
            end = src.find(QUOTE + "#" * hashes, start)
            end = n if end < 0 else end
            literals.append((i, src[start:end]))
            blank(start, end)
            i = end + 1 + hashes
        elif c == QUOTE or (c in "bc" and starts_word and src.startswith(QUOTE, i + 1)):
            j = i + 1 if c == QUOTE else i + 2
            start = j
            while j < n and src[j] != QUOTE:
                j += 2 if src[j] == "\\" else 1
            literals.append((i, src[start:j]))
            blank(start, j)
            i = j + 1
        elif c == "'" or (c == "b" and starts_word and src.startswith("'", i + 1)):
            j = i + 1 if c == "'" else i + 2
            if j < n and src[j] == "\\":
                k = src.find("'", j + 2)
                k = n if k < 0 else k
                blank(j, k)
                i = k + 1
            elif j + 1 < n and src[j + 1] == "'":
                blank(j, j + 1)
                i = j + 2
            else:
                i = j  # a lifetime or a loop label
        else:
            i += 1
    return "".join(out), literals


def match_brace(code, open_at):
    """Offset just past the brace that closes the one at open_at."""
    depth = 0
    for k in range(open_at, len(code)):
        if code[k] == "{":
            depth += 1
        elif code[k] == "}":
            depth -= 1
            if depth == 0:
                return k + 1
    return len(code)


def item_end(code, start):
    """End of the item beginning at start: past its closing brace or `;`."""
    parens = 0
    for k in range(start, len(code)):
        ch = code[k]
        if ch in "([":
            parens += 1
        elif ch in ")]":
            parens -= 1
        elif parens == 0 and ch == ";":
            return k + 1
        elif parens == 0 and ch == "{":
            return match_brace(code, k)
    return len(code)


def test_spans(code):
    """Offset ranges of every `#[cfg(test)]` item, attribute included."""
    spans = []
    for m in TEST_ATTR.finditer(code):
        close = code.find("]", m.end())
        start = close + 1 if close >= 0 else m.end()
        while True:
            rest = code[start:]
            stripped = rest.lstrip()
            start += len(rest) - len(stripped)
            if not stripped.startswith("#["):
                break
            start = code.find("]", start) + 1
        spans.append((m.start(), item_end(code, start)))
    return spans


def in_spans(pos, spans):
    return any(a <= pos < b for a, b in spans)


def blank_spans(code, spans):
    out = list(code)
    for a, b in spans:
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "
    return "".join(out)


def code_lines(text):
    return sum(1 for line in text.split("\n") if line.strip())


def functions(prod):
    """(name, lines) for every fn item that has a body."""
    found = []
    for m in FN_ITEM.finditer(prod):
        end = item_end(prod, m.end())
        if prod[end - 1] != "}":
            continue  # a declaration: a trait method or an extern fn
        first = prod.count("\n", 0, m.start())
        last = prod.count("\n", 0, end - 1)
        found.append((m.group(1), last - first + 1))
    return found


def strip_leading_generics(header):
    s = header.lstrip()
    if not s.startswith("<"):
        return header
    depth = 0
    for k, ch in enumerate(s):
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth -= 1
            if depth == 0:
                return s[k + 1:]
    return s


def inherent_impls(prod):
    """The type name of every inherent (non-trait) impl block."""
    names = []
    for m in IMPL.finditer(prod):
        line_start = prod.rfind("\n", 0, m.start()) + 1
        if prod[line_start:m.start()].strip() not in ("", "unsafe"):
            continue  # `impl Trait` in a type position, not an impl item
        brace = prod.find("{", m.end())
        semi = prod.find(";", m.end())
        if brace < 0 or 0 <= semi < brace:
            continue
        header = strip_leading_generics(prod[m.end():brace])
        header = re.split(r"\bwhere\b", header)[0]
        if re.search(r"\bfor\b", header):
            continue
        head = header.strip().lstrip("&").replace("mut ", "").split("<")[0]
        segments = [s.strip() for s in head.split("::") if s.strip()]
        name = IDENT.match(segments[-1]) if segments else None
        if name:
            names.append(name.group(0))
    return names


def measure(tree, ui_crate):
    crates = defaultdict(lambda: defaultdict(int))
    files = []  # (production lines, rel)
    fns = []  # (lines, rel, name)
    impls = defaultdict(lambda: defaultdict(set))
    ui_free_lines = 0
    hooks = set()
    crates_dir = os.path.join(tree, "crates")
    crate_of = {}  # directory -> crate: the nearest Cargo.toml, else crates/<x>
    for dirpath, dirnames, filenames in os.walk(crates_dir):
        dirnames[:] = sorted(d for d in dirnames if d != "target")
        parent = crate_of.get(os.path.dirname(dirpath))
        if "Cargo.toml" in filenames or (parent is None and os.path.dirname(dirpath) == crates_dir):
            crate_of[dirpath] = os.path.basename(dirpath)
        else:
            crate_of[dirpath] = parent
        for filename in sorted(filenames):
            if not filename.endswith(".rs") or crate_of[dirpath] is None:
                continue
            path = os.path.join(dirpath, filename)
            rel = os.path.relpath(path, tree).replace(os.sep, "/")
            crate = crate_of[dirpath]
            with open(path, encoding="utf-8", errors="replace") as f:
                src = f.read().replace("\r\n", "\n")
            code, literals = mask(src)
            stats = crates[crate]
            if is_test_file(rel):
                stats["test"] += code_lines(code)
                continue
            spans = test_spans(code)
            prod = blank_spans(code, spans)
            lines = code_lines(prod)
            stats["prod"] += lines
            stats["test"] += code_lines(code) - lines
            stats["crate_vis"] += len(CRATE_VIS.findall(prod))
            stats["panic"] += len(UNWRAP.findall(prod)) + len(PANIC.findall(prod))
            stats["expect"] += len(EXPECT.findall(prod))
            files.append((lines, rel))
            fns.extend((n, rel, name) for name, n in functions(prod))
            for name in inherent_impls(prod):
                impls[crate][name].add(rel)
            if crate == ui_crate:
                if not UI_TOOLKIT.search(prod):
                    ui_free_lines += lines
                hooks.update(
                    text
                    for offset, text in literals
                    if HOOK.fullmatch(text) and not in_spans(offset, spans)
                )
    return crates, files, fns, impls, ui_free_lines, hooks


def ratio(num, den, scale=1.0, places=2):
    return f"{scale * num / den:.{places}f}" if den else "n/a"


def render(tree, ui_crate="app", top=10):
    crates, files, fns, impls, ui_free, hooks = measure(tree, ui_crate)
    prod = sum(s["prod"] for s in crates.values())
    test = sum(s["test"] for s in crates.values())
    panic = sum(s["panic"] for s in crates.values())
    ui = crates[ui_crate]["prod"] if ui_crate in crates else 0
    lengths = sorted(n for n, _, _ in fns)
    rows = [
        ("measure.version", VERSION),
        ("lines.production", prod),
        ("lines.test", test),
        ("tests.per_production_line", ratio(test, prod)),
        ("ui_crate", ui_crate),
        ("ui_crate.lines.production", ui),
        ("ui_crate.share_of_production_percent", ratio(ui, prod, 100, 1)),
        ("ui_crate.ui_free_lines", ui_free),
        ("ui_crate.ui_free_share_percent", ratio(ui_free, ui, 100, 1)),
        ("ui_crate.harness_hooks", len(hooks)),
        ("files.production", len(files)),
        ("files.over_1000", sum(1 for n, _ in files if n > 1000)),
        ("files.over_1500", sum(1 for n, _ in files if n > 1500)),
        ("files.largest", max((n for n, _ in files), default=0)),
        ("fns.production", len(lengths)),
        ("fns.median", lengths[len(lengths) // 2] if lengths else 0),
        ("fns.p99", lengths[int(len(lengths) * 0.99)] if lengths else 0),
        ("fns.over_100", sum(1 for n in lengths if n > 100)),
        ("fns.over_200", sum(1 for n in lengths if n > 200)),
        ("fns.over_200.per_100k", ratio(sum(1 for n in lengths if n > 200), prod, 100000, 1)),
        ("panic_sites", panic),
        ("panic_sites.per_kloc", ratio(panic, prod, 1000)),
        ("expect_sites", sum(s["expect"] for s in crates.values())),
    ]
    for name in sorted(crates):
        s = crates[name]
        rows.append((f"crate.{name}.lines.production", s["prod"]))
        rows.append((f"crate.{name}.crate_visibility_per_kloc", ratio(s["crate_vis"], s["prod"], 1000)))
        spread = sorted((-len(v), t) for t, v in impls[name].items())
        if spread:
            rows.append((f"crate.{name}.impl_spread.{spread[0][1]}", -spread[0][0]))
    for n, rel, fname in sorted(fns, key=lambda x: (-x[0], x[1], x[2]))[:top]:
        rows.append((f"fn.longest.{rel}::{fname}", n))
    for n, rel in sorted(files, key=lambda x: (-x[0], x[1]))[:top]:
        rows.append((f"file.largest.{rel}", n))
    return "".join(f"{k}\t{v}\n" for k, v in rows)


def main(argv):
    args = list(argv[1:])
    options = {"--ui-crate": "app", "--top": "10"}
    for flag in options:
        if flag in args:
            k = args.index(flag)
            options[flag] = args[k + 1]
            del args[k:k + 2]
    if len(args) != 1 or not os.path.isdir(os.path.join(args[0], "crates")):
        sys.stderr.write(__doc__)
        return 2
    sys.stdout.write(render(args[0], options["--ui-crate"], int(options["--top"])))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
