#!/bin/sh
# Run one crate's already-built test binary as many concurrent copies pinned to
# fewer cores than copies, and fail if any copy fails.
#
#   sh tools/ci/contention.sh [package]        # default: quantick-app
#
# Why: a test that assumes a worker thread keeps up with the thread asserting
# on it passes on a quiet machine and fails on a loaded one. CI's `Test` step
# runs every binary once, on an idle runner, so such a test ships green and
# goes red later on an unrelated pull request. Every one found so far (#361,
# #364, #385, #403) reproduced the same way: many copies of the binary sharing
# one or two cores. This script is that harness, run where the test is added.
#
# Linux only: pinning uses `taskset`. Knobs, all optional; the defaults are
# what CI runs, and `docs/quality/app-tests-under-contention.md` says why:
#
#   CONTENTION_COPIES   concurrent copies               (default 24)
#   CONTENTION_CORES    cores they share, from core 0   (default 4)
#   CONTENTION_TIMEOUT  seconds before a copy is killed (default 360)
#   CONTENTION_LOGS     where each copy's output goes   (default target/contention)
#   CONTENTION_KNOWN_ISSUES  the skip list              (default: beside this script)
#
# The skip list, `contention-known-issues.txt`, names tests already known to
# fail under contention: one exact test name per line, then the issue that
# tracks its fix (`app::tests::x::y #123`). The copies skip them, and only the
# copies: the `Test` step still runs every one. A line without an issue, or a
# name the binary no longer has, stops the harness, so the list can neither
# grow silently nor outlive the test it excuses. A fix removes its line.
#
# Each copy keeps libtest's default `--test-threads`, which is the size of the
# affinity mask, so every copy runs CONTENTION_CORES tests at once and all of
# their worker threads compete for the same cores.
#
# The binary is located with the exact `cargo test --workspace` invocation the
# CI `Test` step runs, plus `--no-run`: after that step it compiles nothing, so
# the copies run the very binary the step just tested. A single-package
# selection could resolve different features and rebuild.
#
# Exit status: 0 when every copy passed, 1 when any copy failed or timed out,
# 2 when the harness itself could not run.

set -u

package=${1:-quantick-app}
copies=${CONTENTION_COPIES:-24}
cores=${CONTENTION_CORES:-4}
limit=${CONTENTION_TIMEOUT:-360}
logs=${CONTENTION_LOGS:-target/contention}
known=${CONTENTION_KNOWN_ISSUES:-$(dirname "$0")/contention-known-issues.txt}

die() {
    echo "contention: $*" >&2
    exit 2
}

command -v taskset >/dev/null 2>&1 || die "taskset not found (util-linux); this harness is Linux only"
command -v timeout >/dev/null 2>&1 || die "timeout not found (coreutils)"
[ "$copies" -gt "$cores" ] 2>/dev/null || die "CONTENTION_COPIES ($copies) must exceed CONTENTION_CORES ($cores), or nothing contends"
[ "$cores" -ge 1 ] || die "CONTENTION_CORES must be at least 1"
online=$(nproc)
[ "$cores" -le "$online" ] || die "CONTENTION_CORES ($cores) exceeds the $online online cores"

# One JSON object per line. The package's test executables are the artifacts
# whose package id names it — `.../dir#<package>@<version>`, or
# `.../<package>#<version>` when the directory already carries the name — and
# whose `executable` is a path rather than null.
artifacts=$(cargo test --workspace --no-run --message-format=json) || die "cargo test --no-run failed"
matches=$(printf '%s\n' "$artifacts" | grep "\"reason\":\"compiler-artifact\"" | grep -E "(#$package@|/$package#)" | grep '"executable":"')
count=$(printf '%s\n' "$matches" | grep -c '"executable":"')
[ "$count" -eq 1 ] || die "expected exactly one test executable for $package, found $count"
binary=$(printf '%s\n' "$matches" | sed 's/.*"executable":"\([^"]*\)".*/\1/')
manifest=$(printf '%s\n' "$matches" | sed 's/.*"manifest_path":"\([^"]*\)".*/\1/')
[ -x "$binary" ] || die "not executable: $binary"
# cargo runs a test binary from its package root; tests that read fixtures by
# relative path depend on that, so the copies do too.
workdir=$(dirname "$manifest")

# The skip list becomes libtest arguments: `--exact` makes every `--skip`
# match one whole test name rather than any name containing it.
[ -r "$known" ] || die "cannot read the skip list $known"
# Two steps, so a binary that cannot list is reported as that, rather than as
# a skip-list line naming a test that no longer exists.
listing=$(cd "$workdir" && "$binary" --list 2>/dev/null) || die "cannot list the tests in $binary"
listed=$(printf '%s\n' "$listing" | sed -n 's/: test$//p')
[ -n "$listed" ] || die "$binary --list named no tests"
set -- --exact
skipped=0
cr=$(printf '\r')
while IFS= read -r line || [ -n "$line" ]; do
    line=${line%"$cr"}
    case "$line" in
        '' | '#'*) continue ;;
    esac
    printf '%s\n' "$line" | grep -Eq '^[A-Za-z0-9_:]+[[:space:]]+#[0-9]+$' ||
        die "$known: every line is '<exact test name> #<issue>': $line"
    name=${line%%[[:space:]]*}
    printf '%s\n' "$listed" | grep -Fxq "$name" ||
        die "$known: no test named $name in $binary; remove its line"
    set -- "$@" --skip "$name"
    skipped=$((skipped + 1))
    echo "contention: skipping $line"
done <"$known"

rm -rf "$logs"
mkdir -p "$logs" || die "cannot create $logs"
logs=$(cd "$logs" && pwd)

last=$((cores - 1))
echo "contention: $copies copies of $binary on cores 0-$last of $online, ${limit}s limit each, $skipped known issue(s) skipped"
echo "contention: logs in $logs"

started=$(date +%s)
i=1
while [ "$i" -le "$copies" ]; do
    (
        cd "$workdir" &&
            exec timeout --kill-after=10 "$limit" taskset -c "0-$last" "$binary" "$@"
    ) >"$logs/copy-$i.log" 2>&1 &
    eval "pid_$i=$!"
    i=$((i + 1))
done

failed=0
i=1
while [ "$i" -le "$copies" ]; do
    eval "pid=\$pid_$i"
    wait "$pid"
    status=$?
    log="$logs/copy-$i.log"
    if [ "$status" -eq 0 ] && grep -q '^test result: ok\.' "$log"; then
        i=$((i + 1))
        continue
    fi
    failed=$((failed + 1))
    echo
    case "$status" in
        124 | 137) echo "== copy $i of $copies: TIMED OUT after ${limit}s (exit $status)" ;;
        0) echo "== copy $i of $copies: exited 0 without a passing test summary" ;;
        *) echo "== copy $i of $copies: FAILED (exit $status)" ;;
    esac
    # The names, one per line, as libtest prints them while running.
    grep -E '^test .* \.\.\. FAILED$' "$log" | sed 's/^test \(.*\) \.\.\. FAILED$/   failed: \1/'
    # A hung test is announced but never finishes.
    grep -E 'has been running for over' "$log" | sed 's/^/   /'
    # libtest's own failure report: every failing test's captured output and
    # panic message, then the list of names.
    sed -n '/^failures:$/,/^test result:/p' "$log"
    if [ "$status" -eq 124 ] || [ "$status" -eq 137 ]; then
        tail -n 20 "$log"
    fi
    i=$((i + 1))
done

elapsed=$(($(date +%s) - started))
echo
if [ "$failed" -eq 0 ]; then
    echo "contention: all $copies copies passed on $cores core(s) in ${elapsed}s"
    exit 0
fi

echo "contention: $failed of $copies copies failed on $cores core(s) in ${elapsed}s"
names=$(cat "$logs"/copy-*.log | grep -E '^test .* \.\.\. FAILED$' |
    sed 's/^test \(.*\) \.\.\. FAILED$/\1/')
if [ -z "$names" ]; then
    echo "contention: no test reported FAILED; see the copies above that timed out or ended early"
    exit 1
fi
echo "contention: failing tests, by the number of copies each failed in:"
printf '%s\n' "$names" | sort | uniq -c | sort -rn
if [ -n "${GITHUB_ACTIONS:-}" ]; then
    # One annotation per distinct test, on the run's summary page.
    printf '%s\n' "$names" | sort -u | sed 's/^/::error title=Failed under CPU contention::/'
fi
exit 1
