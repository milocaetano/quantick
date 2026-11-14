"""Draw and score a reproducible sample of crates/app test functions.

Selection is systematic PPS (probability proportional to size) over every
`#[test]` fn in crates/app/src, ordered by file then line. A test's size is
its file's mean test body length, so each file receives a share of the draws
proportional to its total test body lines (within one draw of
n * L_file / L_all), and tests inside a file are equally likely.

    python -I tools/test_diet/select_sample.py crates/app/src --n 60 --seed 20261114
    python -I tools/test_diet/select_sample.py crates/app/src --n 60 --seed 20261114 \
        --estimate docs/test-diet/sample.tsv

The first form prints the sample as TSV. The second reads a classified sample
(columns: id, file, test, lines, class, ...) and prints Horvitz-Thompson
estimates for the whole suite with a with-replacement 95% interval.
"""
import argparse
import math
import random
import re
import sys
from collections import defaultdict
from pathlib import Path

FN_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+([a-z_][a-z0-9_]*)", re.M)
TEST_ATTR = re.compile(r"#\[(?:test|tokio::test)[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*$")


def test_fns(root: Path):
    """Return (file, name, line, body_lines) for every test fn under root."""
    out = []
    for p in sorted(root.rglob("*.rs")):
        text = p.read_text(encoding="utf-8", errors="replace")
        rel = p.relative_to(root).as_posix()
        for m in FN_RE.finditer(text):
            brace = text.find("{", m.end())
            semi = text.find(";", m.end())
            if brace < 0 or 0 <= semi < brace:
                continue
            if not TEST_ATTR.search(text[max(0, m.start() - 200):m.start()]):
                continue
            depth, j = 0, brace
            while j < len(text):
                if text[j] == "{":
                    depth += 1
                elif text[j] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
            line = text.count("\n", 0, m.start()) + 1
            out.append((rel, m.group(1), line, text.count("\n", m.start(), j) + 1))
    return out


def population(root: Path):
    tests = test_fns(root)
    per_file = defaultdict(lambda: [0, 0])
    for f, _, _, n in tests:
        per_file[f][0] += 1
        per_file[f][1] += n
    total = sum(n for *_, n in tests)
    return tests, per_file, total


def draw(tests, per_file, total, n, seed):
    """Systematic PPS: one random start, step total/n over cumulative size."""
    step = total / n
    start = random.Random(seed).uniform(0, step)
    points = [start + k * step for k in range(n)]
    picked, cum, k = [], 0.0, 0
    for t in tests:
        count, lines = per_file[t[0]]
        size = lines / count
        while k < n and points[k] < cum + size:
            picked.append(t)
            k += 1
        cum += size
    return picked


def ratio(rows, per_file, total, hit, weight):
    """Ratio estimate of the share of `weight` that `hit` rows hold, scaled by
    the known population total, with a linearised 95% interval."""
    n = len(rows)
    num, den = [], []
    for r in rows:
        count, lines = per_file[r[1]]
        p = (lines / count) / total  # one-draw selection probability
        w = weight(r)
        num.append((w if hit(r) else 0.0) / p)
        den.append(w / p)
    share = sum(num) / sum(den)
    resid = [a - share * b for a, b in zip(num, den)]
    mean_r = sum(resid) / n
    se = math.sqrt(sum((x - mean_r) ** 2 for x in resid) / (n - 1) / n) / (sum(den) / n)
    return share, max(0.0, share - 1.96 * se), min(1.0, share + 1.96 * se)


def estimate(tsv: Path, per_file, total, n_tests):
    rows = [r.split("\t") for r in tsv.read_text(encoding="utf-8").splitlines()[1:] if r.strip()]
    n = len(rows)
    classes = sorted({r[4] for r in rows})
    flags = sorted({f for r in rows for f in r[6].split(",") if f})
    print(f"population: {n_tests} tests, {total} body lines; sample n={n}")
    print("group\tsample_tests\tsample_lines\test_tests (95% CI)\test_lines (95% CI)")
    groups = [(c, lambda r, c=c: r[4] == c) for c in classes]
    groups.append(("a+d", lambda r: r[4] in ("a", "d")))
    groups.append(("mismatch=yes", lambda r: r[5] == "yes"))
    groups += [(f"flag:{f}", lambda r, f=f: f in r[6].split(",")) for f in flags]
    for name, hit in groups:
        out = []
        for weight, size in ((lambda r: 1.0, n_tests), (lambda r: float(r[3]), total)):
            share, lo, hi = ratio(rows, per_file, total, hit, weight)
            out.append(f"{share * size:.0f} ({lo * size:.0f}-{hi * size:.0f})")
        st = sum(1 for r in rows if hit(r))
        sl = sum(int(r[3]) for r in rows if hit(r))
        print(f"{name}\t{st}\t{sl}\t{out[0]}\t{out[1]}")
    def per_file_row(f):
        count, lines = per_file[f]
        mine = [r for r in rows if r[1] == f]
        sampled = sum(int(r[3]) for r in mine)
        ad = sum(int(r[3]) for r in mine if r[4] in ("a", "d"))
        est = lines * ad / sampled if sampled else float("nan")
        return f"{f}\t{lines}\t{len(mine)}\t{ad}/{sampled}\t{est:.0f}", est

    head = "file\tfile_lines\tsampled_tests\tsampled_ad_lines/sampled_lines\test_ad_lines"
    print("\ntop 10 files by test body lines")
    print(head)
    for f, _ in sorted(per_file.items(), key=lambda kv: -kv[1][1])[:10]:
        print(per_file_row(f)[0])
    print("\nevery sampled file holding (a)+(d), by estimated (a)+(d) lines")
    print(head)
    hits = {r[1] for r in rows if r[4] in ("a", "d")}
    for line, _ in sorted((per_file_row(f) for f in hits), key=lambda x: -x[1]):
        print(line)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("root", type=Path, help="path to crates/app/src")
    ap.add_argument("--n", type=int, default=60)
    ap.add_argument("--seed", type=int, default=20261114)
    ap.add_argument("--estimate", type=Path, help="classified sample TSV")
    args = ap.parse_args()
    tests, per_file, total = population(args.root)
    if args.estimate:
        estimate(args.estimate, per_file, total, len(tests))
        return
    print("id\tfile\ttest\tline\tbody_lines\tfile_tests\tfile_lines")
    for i, (f, name, line, lines) in enumerate(draw(tests, per_file, total, args.n, args.seed), 1):
        print(f"{i}\t{f}\t{name}\t{line}\t{lines}\t{per_file[f][0]}\t{per_file[f][1]}")


if __name__ == "__main__":
    sys.exit(main())
