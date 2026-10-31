"""Same-VM A/B of the quantick-drawings edit loop: one worktree, one target, toggled manifest."""
import csv
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

worktree, target, out = map(Path, sys.argv[1:4])
rounds = int(sys.argv[4])
out.mkdir(parents=True, exist_ok=True)
pkg = "quantick-drawings"
manifest = worktree / "crates/drawings/Cargo.toml"
source = worktree / "crates/drawings/src/gesture/projection.rs"
base_manifest = manifest.read_bytes()
variants = {
    "A": base_manifest,
    "B": base_manifest.replace(b"\n[dependencies]\n", b"\n[lib]\ndoctest = false\n\n[dependencies]\n", 1),
}
assert variants["A"] != variants["B"]
env = dict(os.environ, CARGO_TERM_COLOR="never", CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS="4",
           CARGO_PROFILE_DEV_DEBUG="line-tables-only",
           QUANTICK_BUBBLES=str(worktree / "crates/app/config/bubbles.toml"))


def cargo(args, log=None, extra=None):
    started = time.perf_counter()
    run = subprocess.run(["cargo", *args], cwd=worktree, env=dict(env, **(extra or {})),
                         capture_output=True, text=True)
    elapsed = time.perf_counter() - started
    if log:
        (out / log).write_text(run.stdout + "\n--- stderr\n" + run.stderr, encoding="utf-8")
    if run.returncode:
        print(run.stdout[-3000:], run.stderr[-3000:])
        raise SystemExit(f"cargo {args} failed")
    return elapsed, run


def touch():
    time.sleep(2)
    now = time.time()
    os.utime(source, (now, now))


for package in ("quantick-app", "quantick-orderflow", pkg):
    elapsed, _ = cargo(["test", "--no-run", "-p", package], f"warmup-{package}.log")
    print("warmup", package, round(elapsed, 3), flush=True)

rows = []
for round_index in range(rounds):
    # ABBA keeps any drift across a run off one side.
    order = "AB" if round_index % 2 == 0 else "BA"
    for variant in order:
        manifest.write_bytes(variants[variant])
        settle, _ = cargo(["test", "--no-run", "-p", pkg])
        control, _ = cargo(["test", "--no-run", "-p", pkg])
        touch()
        build, built = cargo(["test", "--no-run", "-p", pkg])
        assert f"Compiling {pkg}" in built.stderr
        test, tested = cargo(["test", "-p", pkg])
        assert "310 passed; 0 failed" in tested.stdout or " passed; 0 failed" in tested.stdout
        rows.append(dict(round=round_index, variant=variant, control=control, build=build, test=test,
                         loop=build + test, doctests="Doc-tests" in tested.stderr))
        print(rows[-1], flush=True)

with open(out / "ab.csv", "w", newline="") as handle:
    writer = csv.DictWriter(handle, fieldnames=list(rows[0]))
    writer.writeheader()
    writer.writerows(rows)

(out / "invocations").mkdir(exist_ok=True)
for variant in "AB":
    manifest.write_bytes(variants[variant])
    cargo(["test", "--no-run", "-p", pkg])
    touch()
    cargo(["test", "--no-run", "-p", pkg, "-v", "--timings"], f"invocations/verbose-{variant}.log")
    timings = target / "cargo-timings" / "cargo-timing.html"
    if timings.exists():
        shutil.copy(timings, out / f"timing-{variant}.html")
    _, tested = cargo(["test", "-p", pkg, "-v"], f"invocations/test-verbose-{variant}.log")

wrapper = out / "rustc-passes.sh"
wrapper.write_text('#!/bin/sh\nexport RUSTC_BOOTSTRAP=1\nexec "$@" -Ztime-passes -Ztime-passes-format=text\n')
wrapper.chmod(0o755)
for variant in "AB":
    manifest.write_bytes(variants[variant])
    extra = {"RUSTC_WORKSPACE_WRAPPER": str(wrapper)}
    cargo(["test", "--no-run", "-p", pkg], None, extra)
    for touch_index in range(3):
        touch()
        cargo(["test", "--no-run", "-p", pkg], f"passes-{variant}-{touch_index}.log", extra)

manifest.write_bytes(base_manifest)
summary = {}
deps = target / "debug" / "deps"
summary["deps_entries"] = len(list(deps.iterdir()))
summary["sizes"] = {path.name: path.stat().st_size for path in deps.iterdir()
                    if path.is_file() and path.name.startswith(("libquantick", "quantick_drawings"))
                    and (path.suffix in (".rmeta", ".rlib") or path.suffix == "")}
(out / "summary.json").write_text(json.dumps(summary, indent=1), encoding="utf-8")


def stats(key, variant):
    values = sorted(row[key] for row in rows if row["variant"] == variant)
    return dict(min=round(values[0], 3), median=round(values[len(values) // 2], 3), max=round(values[-1], 3))


for key in ("control", "build", "test", "loop"):
    print(key, "A", stats(key, "A"), "B", stats(key, "B"))
