#!/bin/sh
# Tests for tools/ci/contention.sh: every exit path, driven without cargo, a
# real test binary or `taskset`. PATH shims stand in for `cargo` (which prints
# canned artifact JSON) and `taskset` (which drops its `-c` mask and runs the
# command), and a fake libtest binary prints libtest's output shapes.
#
#   sh tools/ci/contention_test.sh
#
# Exit 0 when every case passed, 1 otherwise. POSIX sh; runs in a second or
# two, so CI runs it before the slow steps.

set -u

here=$(cd "$(dirname "$0")" && pwd)
harness="$here/contention.sh"
work=$(mktemp -d) || exit 1
trap 'rm -rf "$work"' EXIT INT TERM
mkdir -p "$work/bin" "$work/pkg/app" "$work/pkg/multi"

cat >"$work/bin/taskset" <<'EOF'
#!/bin/sh
shift 2
exec "$@"
EOF

# Two packages: `quantick-app` with one test executable, `multi` with a unit
# test binary and an integration test binary. A library artifact with a null
# executable, and another package's executable, must both be ignored.
cat >"$work/bin/cargo" <<EOF
#!/bin/sh
echo '{"reason":"compiler-artifact","package_id":"path+file:///w/crates/app#quantick-app@0.1.0","manifest_path":"$work/pkg/app/Cargo.toml","target":{"kind":["lib"],"name":"quantick_app_lib"},"executable":null}'
echo '{"reason":"compiler-artifact","package_id":"path+file:///w/crates/app#quantick-app@0.1.0","manifest_path":"$work/pkg/app/Cargo.toml","target":{"kind":["bin"],"name":"quantick-app"},"executable":"$work/fake"}'
echo '{"reason":"compiler-artifact","package_id":"path+file:///w/crates/multi#0.1.0","manifest_path":"$work/pkg/multi/Cargo.toml","target":{"kind":["lib"],"name":"multi"},"executable":"$work/fake"}'
echo '{"reason":"compiler-artifact","package_id":"path+file:///w/crates/multi#0.1.0","manifest_path":"$work/pkg/multi/Cargo.toml","target":{"kind":["test"],"name":"golden"},"executable":"$work/fake"}'
echo '{"reason":"compiler-artifact","package_id":"path+file:///w/crates/engine#quantick-engine@0.1.0","manifest_path":"/w/crates/engine/Cargo.toml","target":{"kind":["lib"],"name":"quantick_engine"},"executable":"/w/engine"}'
echo '{"reason":"build-finished","success":true}'
EOF

# The fake binary. FAKE_MODE: pass | fail_once | hang_once | list_fails.
# `*_once` modes act in exactly one copy, claimed with an atomic mkdir.
cat >"$work/fake" <<EOF
#!/bin/sh
if [ "\${1:-}" = "--list" ]; then
    [ "\${FAKE_MODE:-pass}" = list_fails ] && exit 3
    printf 'a::ok: test\na::flaky: test\na::known: test\n\n3 tests, 0 benchmarks\n'
    exit 0
fi
echo "cwd=\$(pwd) args=\$*"
echo "running 3 tests"
echo "test a::ok ... ok"
case "\${FAKE_MODE:-pass}" in
    fail_once)
        if mkdir "$work/claimed" 2>/dev/null; then
            printf 'test a::flaky ... FAILED\n\nfailures:\n\n---- a::flaky stdout ----\nboom (309 vs 224)\n\nfailures:\n    a::flaky\n\n'
            echo "test result: FAILED. 2 passed; 1 failed; 0 ignored; finished in 0.01s"
            exit 101
        fi ;;
    hang_once)
        if mkdir "$work/claimed" 2>/dev/null; then
            echo "test a::flaky has been running for over 60 seconds"
            sleep 30
        fi ;;
esac
echo "test a::flaky ... ok"
echo "test result: ok. 3 passed; 0 failed; 0 ignored; finished in 0.01s"
EOF
chmod +x "$work/bin/taskset" "$work/bin/cargo" "$work/fake"

printf '# a comment\n\na::known #999\n' >"$work/good.txt"
printf 'a::known\n' >"$work/no-issue.txt"
printf 'a::gone #1\n' >"$work/stale.txt"

passed=0
failed=0

# check <name> <expected exit> <text the output must contain> -- env... -- args...
check() {
    name=$1 want=$2 text=$3
    shift 3
    rm -rf "$work/claimed" "$work/logs"
    out=$(env PATH="$work/bin:$PATH" CONTENTION_COPIES=3 CONTENTION_CORES=1 \
        CONTENTION_LOGS="$work/logs" CONTENTION_KNOWN_ISSUES="$work/good.txt" \
        "$@" 2>&1)
    got=$?
    if [ "$got" -eq "$want" ] && printf '%s\n' "$out" | grep -Fq -- "$text"; then
        passed=$((passed + 1))
    else
        failed=$((failed + 1))
        echo "FAIL: $name: exit $got (wanted $want), output lacks '$text':"
        printf '%s\n' "$out" | sed 's/^/    /'
    fi
}

check "every copy passes" 0 "all 3 copies passed" \
    sh "$harness"
check "the skip list reaches every copy as --exact --skip" 0 "args=--exact --skip a::known" \
    sh -c "sh '$harness' >/dev/null && cat '$work/logs/copy-2.log'"
check "copies run from the package root" 0 "cwd=$work/pkg/app" \
    sh -c "sh '$harness' >/dev/null && cat '$work/logs/copy-1.log'"
check "a failing copy is named" 1 "== copy" \
    env FAKE_MODE=fail_once sh "$harness"
check "the failing test is named" 1 "failed: a::flaky" \
    env FAKE_MODE=fail_once sh "$harness"
check "libtest's failure report is printed" 1 "boom (309 vs 224)" \
    env FAKE_MODE=fail_once sh "$harness"
check "failures are counted per test" 1 "1 a::flaky" \
    env FAKE_MODE=fail_once sh "$harness"
check "a hung copy is killed and reported" 1 "TIMED OUT after 2s" \
    env FAKE_MODE=hang_once CONTENTION_TIMEOUT=2 sh "$harness"
check "rounds run back to back when every one passes" 0 "3 round(s) in a row" \
    env CONTENTION_ROUNDS=3 sh "$harness"
check "a red round stops the rounds and says which" 1 "round 1 of 3: 1 of 3 copies failed" \
    env FAKE_MODE=fail_once CONTENTION_ROUNDS=3 sh "$harness"
check "zero rounds is refused" 2 "CONTENTION_ROUNDS must be at least 1" \
    env CONTENTION_ROUNDS=0 sh "$harness"
check "a skip-list line with no issue stops the harness" 2 "every line is" \
    env CONTENTION_KNOWN_ISSUES="$work/no-issue.txt" sh "$harness"
check "a skip-list name the binary lacks stops the harness" 2 "no test named a::gone" \
    env CONTENTION_KNOWN_ISSUES="$work/stale.txt" sh "$harness"
check "a missing skip list stops the harness" 2 "cannot read the skip list" \
    env CONTENTION_KNOWN_ISSUES="$work/absent.txt" sh "$harness"
check "a binary that cannot list is reported as that" 2 "cannot list the tests" \
    env FAKE_MODE=list_fails sh "$harness"
check "no more copies than cores is refused" 2 "must exceed" \
    env CONTENTION_COPIES=1 sh "$harness"
check "an unknown package is refused" 2 "found 0" \
    sh "$harness" quantick-nope
check "a package with two test targets needs one named" 2 "name one as the second argument" \
    sh "$harness" multi
check "a named target selects one executable" 0 "all 3 copies passed" \
    env CONTENTION_KNOWN_ISSUES="$work/good.txt" sh "$harness" multi golden

echo "$passed passed, $failed failed"
[ "$failed" -eq 0 ]
