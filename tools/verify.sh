#!/bin/sh
# Verify the committed branch in the foreground and print one short verdict.
#
#   sh tools/verify.sh            guards, affected tests, then the PR's CI
#   sh tools/verify.sh --no-ci    guards and affected tests only
#   sh tools/verify.sh --ci-only  the PR's CI only
#
# Stages: `quantick-guards`; `cargo test` on the crates the diff from the
# merge base with origin/main can affect (tools/ci/affected_crates.py, the
# selection the draft CI job uses); then `gh pr checks --watch` on the
# current branch's PR, once its remote head is this HEAD. CI runs only when
# the local stages pass. Full logs stay in a temporary directory whose path
# the verdict prints; the verdict names every failing test or check, so the
# caller never reads a log to learn what failed. Exits non-zero on any
# failure. VERIFY_CI_TIMEOUT (seconds, default 3600) bounds the CI watch,
# because a hung CI job has no timeout of its own.
set -u

cd "$(git rev-parse --show-toplevel)"

local_stages=1
ci_stage=1
case "${1:-}" in
"") ;;
--no-ci) ci_stage=0 ;;
--ci-only) local_stages=0 ;;
*)
    echo "usage: sh tools/verify.sh [--no-ci | --ci-only]" >&2
    exit 2
    ;;
esac

logs=$(mktemp -d "${TMPDIR:-/tmp}/verify.XXXXXX")
verdict=""
failed=0

report() {
    verdict="$verdict
$1"
}

if [ "$local_stages" = 1 ]; then
    if env -u QUANTICK_BUBBLES cargo run -q -p quantick-guards >"$logs/guards.log" 2>&1; then
        report "guards: pass"
    else
        failed=1
        report "guards: FAIL"
        # Findings are the lines that name a file; fall back to the tail.
        findings=$(grep -E '^[^ ]+:[0-9]+|^(error|FAIL)' "$logs/guards.log" || tail -n 5 "$logs/guards.log")
        report "$findings"
    fi

    python=python
    command -v python >/dev/null 2>&1 || python=python3
    git fetch -q origin main
    base=$(git merge-base HEAD origin/main)
    if packages=$("$python" tools/ci/affected_crates.py --base "$base" --cargo-args 2>"$logs/affected.log"); then
        # shellcheck disable=SC2086 # one -p flag per word
        if env -u QUANTICK_BUBBLES cargo test $packages >"$logs/tests.log" 2>&1; then
            passed=$(grep -cE '^test result: ok' "$logs/tests.log")
            report "tests: pass ($passed test binaries; $packages)"
        else
            failed=1
            report "tests: FAIL ($packages)"
            failing=$(grep -E '^test .* \.\.\. FAILED$|^error(\[E[0-9]+\])?:' "$logs/tests.log" | sort -u)
            [ -n "$failing" ] || failing=$(tail -n 5 "$logs/tests.log")
            report "$failing"
        fi
    else
        failed=1
        report "tests: FAIL (crate selection)"
        report "$(tail -n 5 "$logs/affected.log")"
    fi
fi

if [ "$ci_stage" = 1 ] && [ "$failed" = 0 ]; then
    head=$(git rev-parse HEAD)
    remote=$(gh pr view --json headRefOid --jq .headRefOid 2>"$logs/ci.log")
    if [ -z "$remote" ]; then
        failed=1
        report "ci: FAIL (no PR for this branch)"
    elif [ "$remote" != "$head" ]; then
        failed=1
        report "ci: FAIL (PR head $remote is not HEAD $head; push first)"
    else
        # Checks appear a few seconds after a push; give them two minutes.
        tries=0
        until gh pr checks >/dev/null 2>&1 || [ "$tries" -ge 12 ]; do
            tries=$((tries + 1))
            sleep 10
        done
        timeout "${VERIFY_CI_TIMEOUT:-3600}" gh pr checks --watch --interval 30 >>"$logs/ci.log" 2>&1
        watched=$?
        states=$(gh pr checks --json name,bucket --jq '.[] | "\(.bucket) \(.name)"' 2>>"$logs/ci.log")
        bad=$(printf '%s\n' "$states" | grep -E '^(fail|cancel|pending) ' || true)
        skipped=$(printf '%s\n' "$states" | grep -E '^skipping ' | cut -d' ' -f2- | paste -sd, - || true)
        total=$(printf '%s\n' "$states" | grep -c . || true)
        if [ "$watched" = 124 ]; then
            failed=1
            report "ci: FAIL (still running after ${VERIFY_CI_TIMEOUT:-3600}s: a hung job?)"
            report "$bad"
        elif [ -n "$bad" ] || [ "$total" = 0 ]; then
            failed=1
            report "ci: FAIL at $head"
            report "${bad:-no checks reported}"
        else
            report "ci: pass at $head ($total checks${skipped:+; skipped: $skipped})"
        fi
    fi
fi

if [ "$failed" = 0 ]; then
    echo "VERIFY: pass$verdict"
else
    echo "VERIFY: FAIL$verdict"
fi
echo "logs: $logs"
exit "$failed"
