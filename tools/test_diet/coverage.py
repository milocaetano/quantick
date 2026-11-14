"""Per-file line coverage of crates/app tests against the domain crates' own tests.

Run from the worktree root after `source tools/test_diet/env.sh` and the
instrumented `cargo test --no-run ... --message-format=json` build (see
docs/test-diet/coverage.md). Subcommands, in order:

  units  BUILD_JSON          resolve modules, list tests, assign them to units
  run    [RUN..]             run the domain baselines and every unit (or RUN..)
  table                      fold the lcov exports into the report table

Everything it writes lives under target/test-diet/.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path.cwd()
APP_SRC = ROOT / "crates" / "app" / "src"
OUT = ROOT / "target" / "test-diet"
DOMAIN = {
    "quantick-control-handlers": "control-handlers",
    "quantick-drawings": "drawings",
    "quantick-paper": "paper",
    "quantick-sim": "sim",
    "quantick-chart": "chart",
    "quantick-orderflow": "orderflow",
    "quantick-sources": "sources",
    "quantick-session": "session",
    "quantick-workspace": "workspace",
    "quantick-engine": "engine",
    "quantick-ui-model": "ui-model",
}
EXTRA_DIRS = ["orderflow_render/tests", "pane/tests", "paper_trading/tests", "orderflow_view/tests"]
TOP_N = 25
SYSROOT = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
LLVM_BIN = Path(SYSROOT) / "lib" / "rustlib" / "x86_64-pc-windows-msvc" / "bin"


# --- module resolution -------------------------------------------------------

def blank_literals(src):
    """Same-length copy of `src` with comments and string/char contents blanked."""
    out = list(src)
    i, n = 0, len(src)

    def blank(a, b):
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
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
        elif (m := re.match(r'b?r(#*)"', src[i:i + 40])) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            close = '"' + m.group(1)
            j = src.find(close, i + m.end())
            j = n if j < 0 else j + len(close)
            blank(i + m.end(), j - len(close))
            i = j
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(i + 1, j)
            i = j + 1
        elif c == "'":
            m = re.match(r"'(\\[^']{1,10}|[^\\'])'", src[i:i + 12])
            if m:
                blank(i + 1, i + m.end() - 1)
                i += m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


MOD_RE = re.compile(r"((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?\bmod\s+(\w+)\s*([;{])")
PATH_RE = re.compile(r'#\[path\s*=\s*"([^"]+)"\]')


def resolve_modules(entry):
    """Map every reachable file to its module path, walking `mod` items from `entry`."""
    files = {}

    def walk(path, modpath, owns_dir):
        path = path.resolve()
        if path in files or not path.exists():
            return
        files[path] = modpath
        src = path.read_text(encoding="utf-8")
        code = blank_literals(src)
        child_dir = path.parent if owns_dir else path.parent / path.stem
        events = [(m.start(2), "mod", m) for m in MOD_RE.finditer(code)]
        events += [(i, ch, None) for i, ch in enumerate(code) if ch in "{}"]
        events.sort(key=lambda e: e[0])
        stack, depth = [], 0
        for pos, kind, m in events:
            if kind == "mod":
                name, term = m.group(2), m.group(3)
                attr = PATH_RE.search(src[m.start(1):m.end(1)])
                inner = [s for s, _ in stack]
                if term == "{":
                    stack.append((name, depth + 1))
                    continue
                sub = modpath + inner + [name]
                if attr:
                    base = path.parent if not inner else child_dir.joinpath(*inner)
                    walk(base / attr.group(1), sub, True)
                else:
                    base = child_dir.joinpath(*inner)
                    if (base / f"{name}.rs").exists():
                        walk(base / f"{name}.rs", sub, False)
                    else:
                        walk(base / name / "mod.rs", sub, True)
            elif kind == "{":
                depth += 1
            else:
                if stack and stack[-1][1] == depth:
                    stack.pop()
                depth -= 1

    walk(APP_SRC / "main.rs", [], True)
    return {p: "::".join(mp) for p, mp in files.items()}


def rel(p):
    return Path(os.path.relpath(p, ROOT)).as_posix()


def is_test_file(relpath):
    parts = relpath.split("/")
    name = parts[-1]
    return (
        "tests" in parts[:-1]
        or name in ("tests.rs",)
        or name.endswith("_tests.rs")
        or (len(parts) > 2 and parts[2] == "tests")
    )


# --- build artifacts -----------------------------------------------------------

def test_binaries(build_json):
    bins = {}
    for line in Path(build_json).read_text(encoding="utf-8").splitlines():
        if not line.startswith("{"):
            continue
        msg = json.loads(line)
        if msg.get("reason") != "compiler-artifact" or not msg.get("executable"):
            continue
        if not msg["profile"]["test"]:
            continue
        pkg = re.search(r"#?(quantick-[\w-]+)[@#]", msg["package_id"])
        pkg = pkg.group(1) if pkg else msg["target"]["name"]
        manifest_dir = Path(msg["manifest_path"]).parent
        bins.setdefault(pkg, []).append((msg["executable"], str(manifest_dir)))
    return bins


def list_tests(exe, cwd):
    out = subprocess.run([exe, "--list", "--format", "terse"], cwd=cwd, capture_output=True, text=True)
    return [l[:-len(": test")] for l in out.stdout.splitlines() if l.endswith(": test")]


# --- units ---------------------------------------------------------------------

def unit_of_file(relpath, units_by_top):
    app_tests = "crates/app/src/app/tests/"
    if relpath.startswith(app_tests):
        rest = relpath[len(app_tests):]
        top = rest.split("/")[0]
        top = top if top.endswith(".rs") else top + ".rs"
        key = "app/tests/" + top
        return key if key in units_by_top else None
    for d in EXTRA_DIRS:
        if relpath.startswith("crates/app/src/" + d + "/"):
            return d
    return None


def cmd_units(build_json):
    OUT.mkdir(parents=True, exist_ok=True)
    files = resolve_modules(APP_SRC)
    bins = test_binaries(build_json)
    (app_exe, app_cwd), = bins["quantick-app"]
    names = list_tests(app_exe, app_cwd)

    # Size of each top-level app/tests file, its own subdirectory counted with it.
    tests_dir = APP_SRC / "app" / "tests"
    sizes = {}
    for f in tests_dir.glob("*.rs"):
        if f.name == "mod.rs":
            continue
        owned = [f] + list((tests_dir / f.stem).rglob("*.rs"))
        sizes["app/tests/" + f.name] = sum(len(p.read_text(encoding="utf-8").splitlines()) for p in owned)
    top = dict(sorted(sizes.items(), key=lambda kv: -kv[1])[:TOP_N])
    for d in EXTRA_DIRS:
        top[d] = sum(len(p.read_text(encoding="utf-8").splitlines()) for p in (APP_SRC / d).rglob("*.rs"))

    by_mod = {mp: rel(p) for p, mp in files.items()}
    assigned, unresolved = {u: [] for u in top}, []
    assigned["rest"] = []
    for name in names:
        parts = name.split("::")
        owner = None
        for k in range(len(parts) - 1, -1, -1):
            if "::".join(parts[:k]) in by_mod:
                owner = by_mod["::".join(parts[:k])]
                break
        if owner is None:
            unresolved.append(name)
            owner = ""
        assigned[unit_of_file(owner, top) or "rest"].append(name)
    for u in top:
        if not assigned[u]:
            print(f"warning: unit {u} owns no tests", file=sys.stderr)
    plan = {
        "app_exe": app_exe,
        "app_cwd": app_cwd,
        "test_lines": top,
        "tests": assigned,
        "domain_bins": {p: bins.get(p, []) for p in DOMAIN},
        "app_test_files": sorted(rel(p) for p in files if is_test_file(rel(p))),
    }
    (OUT / "plan.json").write_text(json.dumps(plan, indent=1), encoding="utf-8")
    print(f"{len(names)} app tests, {len(unresolved)} unresolved")
    for u in list(top) + ["rest"]:
        print(f"{u:60} {top.get(u, 0):6} lines {len(assigned[u]):5} tests")


# --- running -------------------------------------------------------------------

def slug(name):
    return re.sub(r"[^A-Za-z0-9]+", "_", name).strip("_")


def run_profiled(run, invocations, objects):
    """Run each (exe, cwd, args) with its own profile, then export one lcov."""
    raw = OUT / "raw" / run
    raw.mkdir(parents=True, exist_ok=True)
    for f in raw.glob("*.profraw"):
        f.unlink()
    log = open(OUT / f"{run}.log", "w", encoding="utf-8")
    env = dict(os.environ, LLVM_PROFILE_FILE=str(raw / "%p-%m.profraw"))
    for exe, cwd, args in invocations:
        subprocess.run([exe, *args], cwd=cwd, env=env, stdout=log, stderr=subprocess.STDOUT)
    log.close()
    profdata = OUT / f"{run}.profdata"
    subprocess.run([str(LLVM_BIN / "llvm-profdata"), "merge", "-sparse", "-o", str(profdata),
                    *map(str, raw.glob("*.profraw"))], check=True)
    obj_args = [objects[0]] + [a for o in objects[1:] for a in ("-object", o)]
    with open(OUT / f"{run}.lcov", "w", encoding="utf-8") as out:
        subprocess.run([str(LLVM_BIN / "llvm-cov"), "export", "-format=lcov", "-skip-functions",
                        f"-instr-profile={profdata}", *obj_args,
                        r"-ignore-filename-regex=(\.cargo|\.rustup|rustc)[\\/]"],
                       stdout=out, check=True)
    for f in raw.glob("*.profraw"):
        f.unlink()
    profdata.unlink()


def batches(names, limit=24000):
    batch, size = [], 0
    for n in names:
        if batch and size + len(n) + 1 > limit:
            yield batch
            batch, size = [], 0
        batch.append(n)
        size += len(n) + 1
    if batch:
        yield batch


def cmd_run(only):
    plan = json.loads((OUT / "plan.json").read_text(encoding="utf-8"))
    for pkg, bins in plan["domain_bins"].items():
        run = "domain_" + slug(pkg)
        if only and run not in only:
            continue
        print("run", run, flush=True)
        run_profiled(run, [(exe, cwd, []) for exe, cwd in bins], [exe for exe, _ in bins])
    for unit, names in plan["tests"].items():
        run = "app_" + slug(unit)
        if only and run not in only:
            continue
        print("run", run, len(names), "tests", flush=True)
        inv = [(plan["app_exe"], plan["app_cwd"], ["--exact", *b]) for b in batches(names)]
        run_profiled(run, inv, [plan["app_exe"]])


# --- table ---------------------------------------------------------------------

def read_lcov(lcov):
    """(instrumented, covered) sets of (path, line)."""
    every, hit, path = set(), set(), None
    for raw in open(lcov, encoding="utf-8"):
        if raw.startswith("SF:"):
            path = rel(raw[3:].strip())
        elif raw.startswith("DA:"):
            ln, count = raw[3:].split(",")[:2]
            every.add((path, int(ln)))
            if int(count) > 0:
                hit.add((path, int(ln)))
    return every, hit


def area(path):
    """'app', a domain crate dir, 'other' (another workspace crate), or None for test code."""
    if not path.startswith("crates/") or is_test_file(path):
        return None
    crate = path.split("/")[1]
    if crate == "app":
        return "app"
    if crate in DOMAIN.values():
        return crate
    return "other"


def verdict(unique, test_lines):
    """Unique (app + domain) lines bought per test line: <2% delete, <10% slim."""
    y = unique / test_lines
    return "delete-candidate" if y < 0.02 else ("slim" if y < 0.10 else "keep")


def cmd_table():
    plan = json.loads((OUT / "plan.json").read_text(encoding="utf-8"))
    own, base = set(), []
    for pkg in plan["domain_bins"]:
        crate = DOMAIN[pkg]
        every, hit = read_lcov(OUT / f"domain_{slug(pkg)}.lcov")
        mine = {l for l in hit if area(l[0]) == crate}
        total = {l for l in every if area(l[0]) == crate}
        own |= mine
        base.append((pkg, len(total), len(mine)))
    cov = {u: {l for l in read_lcov(OUT / f"app_{slug(u)}.lcov")[1] if area(l[0])} for u in plan["tests"]}
    suite = set().union(*cov.values())
    print("| crate | instrumented lines | covered by own tests | own % | covered by app suite | app-only (not own) |")
    print("|---|---:|---:|---:|---:|---:|")
    for pkg, total, mine in base:
        crate = DOMAIN[pkg]
        by_app = {l for l in suite if area(l[0]) == crate}
        print(f"| `{pkg}` | {total} | {mine} | {mine / total:.0%} | {len(by_app)} | {len(by_app - own)} |")
    print()
    rows = []
    for unit, lines in cov.items():
        if unit == "rest":
            continue
        others = set().union(*(v for k, v in cov.items() if k != unit))
        measured = set().union(*(v for k, v in cov.items() if k not in (unit, "rest")))
        app = {l for l in lines if area(l[0]) == "app"}
        dom = {l for l in lines if area(l[0]) not in ("app", "other")}
        scope = app | dom
        unique = scope - others - own
        where = {}
        for path, _ in unique:
            where[path] = where.get(path, 0) + 1
        top = sorted(where.items(), key=lambda kv: -kv[1])[:2]
        rows.append(dict(
            unit=unit, test_lines=plan["test_lines"][unit], tests=len(plan["tests"][unit]),
            app=len(app), domain=len(dom), overlap=len(dom & own) / len(dom) if dom else 0.0,
            unique=len(unique), unique_measured=len(scope - measured - own),
            share=len(unique) / len(scope) if scope else 0.0,
            other_unique=len({l for l in lines if area(l[0]) == "other"} - others),
            verdict=verdict(len(unique), plan["test_lines"][unit]),
            where=", ".join(f"{p.removeprefix('crates/')} {n}" for p, n in top)))
    rows.sort(key=lambda r: -r["test_lines"])
    (OUT / "table.json").write_text(json.dumps(rows, indent=1), encoding="utf-8")
    print("| file | tests | test lines | app lines | domain lines | domain overlap | unique | unique vs measured only "
          "| unique / covered | unique / test lines | other-crate unique | verdict | most unique lines in |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---|")
    for r in rows:
        print(f"| `{r['unit']}` | {r['tests']} | {r['test_lines']} | {r['app']} | {r['domain']} | "
              f"{r['overlap']:.0%} | {r['unique']} | {r['unique_measured']} | {r['share']:.1%} | "
              f"{r['unique'] / r['test_lines']:.1%} | {r['other_unique']} | {r['verdict']} | {r['where']} |")
    print()
    for v in ("delete-candidate", "slim", "keep"):
        picked = [r for r in rows if r["verdict"] == v]
        print(f"{v}: {len(picked)} files, {sum(r['test_lines'] for r in picked)} test lines, "
              f"{sum(r['unique'] for r in picked)} unique lines")


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "units":
        cmd_units(sys.argv[2])
    elif cmd == "run":
        cmd_run(set(sys.argv[2:]))
    elif cmd == "table":
        cmd_table()
