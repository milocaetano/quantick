#!/bin/sh
# Verify the committed branch and print one short verdict. It blocks until
# done, so a caller runs it once (an agent, as one background command) and
# never polls.
#
#   sh tools/verify.sh            guards, affected tests, then the PR's CI
#   sh tools/verify.sh --no-ci    guards and affected tests only
#   sh tools/verify.sh --ci-only  the PR's CI only
#
# Stages: `quantick-guards`; `cargo test` on the crates the diff from the
# merge base with origin/main can affect (tools/ci/affected_crates.py, the
# selection the draft CI job uses); then `gh pr checks --watch` on the
# current branch's PR, once its remote head is this HEAD and it is not a
# draft (drafts skip the full CI jobs). CI runs only when the local stages
# pass; a skipped check is named in the verdict. Full logs stay in a
# temporary directory whose path is printed first; the verdict names every failing test or check, so the
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
echo "logs: $logs"
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

    # A Windows Store alias named python exists but cannot run scripts.
    python=python
    python -c '' >/dev/null 2>&1 || python=python3
    git fetch -q origin main
    base=$(git merge-base HEAD origin/main)
    if ! packages=$("$python" tools/ci/affected_crates.py --base "$base" --cargo-args 2>"$logs/affected.log"); then
        failed=1
        report "tests: FAIL (crate selection)"
        report "$(tail -n 5 "$logs/affected.log")"
    elif [ -z "$packages" ]; then
        # Plain `cargo test` would test the whole workspace.
        report "tests: none affected"
    else
        # shellcheck disable=SC2086 # one -p flag per word
        if env -u QUANTICK_BUBBLES cargo test --no-fail-fast $packages >"$logs/tests.log" 2>&1; then
            passed=$(grep -cE '^test result: ok' "$logs/tests.log")
            report "tests: pass ($passed test runs; $packages)"
        else
            failed=1
            report "tests: FAIL ($packages)"
            failing=$(grep -E '^test .* \.\.\. FAILED$|^error(\[E[0-9]+\])?:' "$logs/tests.log" | sort -u)
            [ -n "$failing" ] || failing=$(tail -n 5 "$logs/tests.log")
            report "$failing"
        fi
    fi
fi

# Jobs .github/workflows/ci.yml skips on a ready PR by design: `fast` runs on
# drafts, `read-cost-ledger` on pushes, `rust-skip` only when `gate` skips the
# build. Any other skipped check is not green, unless `rust-skip` passed:
# then `gate` found nothing to build and the build jobs skip by design.
ALTERNATE_JOBS="fast|read-cost-ledger|rust-skip"

check_states() {
    gh pr checks --json name,bucket --jq '.[] | "\(.bucket) \(.name)"' 2>>"$logs/ci.log"
}

if [ "$ci_stage" = 1 ] && [ "$failed" = 0 ]; then
    head=$(git rev-parse HEAD)
    # The second field is true for a draft without the `full-ci` label, whose
    # full CI jobs skip.
    pr=$(gh pr view --json headRefOid,isDraft,labels \
        --jq '"\(.headRefOid) \(.isDraft and ([.labels[].name] | index("full-ci") | not))"' 2>"$logs/ci.log")
    remote=${pr% *}
    if [ -z "$pr" ]; then
        failed=1
        report "ci: FAIL (no PR for this branch)"
    elif [ "${pr#* }" = true ]; then
        failed=1
        report "ci: FAIL (draft PR without the full-ci label: its full CI jobs skip)"
    elif [ "$remote" != "$head" ]; then
        failed=1
        report "ci: FAIL (PR head $remote is not HEAD $head; push first)"
    else
        # Checks appear a few seconds after a push; give them two minutes.
        # `gh pr checks` exits non-zero while any is pending, so count them.
        tries=0
        until [ "$(gh pr checks --json name --jq length 2>/dev/null || echo 0)" -gt 0 ] ||
            [ "$tries" -ge 12 ]; do
            tries=$((tries + 1))
            sleep 10
        done
        # A workflow can register its checks after the first ones finish, so
        # watch again until a finished watch sees no new check.
        watched=0
        seen=-1
        rounds=0
        total=$(check_states | grep -c . || true)
        while [ "$total" != "$seen" ] && [ "$watched" = 0 ] && [ "$rounds" -lt 5 ]; do
            seen=$total
            rounds=$((rounds + 1))
            timeout "${VERIFY_CI_TIMEOUT:-3600}" gh pr checks --watch --interval 30 >>"$logs/ci.log" 2>&1
            watched=$?
            [ "$watched" = 124 ] || sleep 20
            total=$(check_states | grep -c . || true)
        done
        states=$(check_states)
        bad=$(printf '%s\n' "$states" | grep -E '^(fail|cancel|pending) ' || true)
        skipped=$(printf '%s\n' "$states" | grep -E '^skipping ' | cut -d' ' -f2- || true)
        unexpected=""
        if ! printf '%s\n' "$states" | grep -qx 'pass rust-skip'; then
            unexpected=$(printf '%s\n' "$skipped" | grep -vxE "$ALTERNATE_JOBS" | grep . | paste -sd, - || true)
        fi
        skipped=$(printf '%s\n' "$skipped" | paste -sd, -)
        if [ "$watched" = 124 ]; then
            failed=1
            report "ci: FAIL (still running after ${VERIFY_CI_TIMEOUT:-3600}s: a hung job?)"
            report "${bad:-no check states read}"
        elif [ -n "$bad" ] || [ "$total" = 0 ]; then
            failed=1
            report "ci: FAIL at $head"
            report "${bad:-no checks reported}"
        elif [ -n "$unexpected" ]; then
            failed=1
            report "ci: FAIL at $head (skipped, not green: $unexpected)"
        else
            report "ci: pass at $head ($total checks${skipped:+; skipped by design: $skipped})"
        fi
    fi
fi

if [ "$failed" = 0 ]; then
    echo "VERIFY: pass$verdict"
else
    echo "VERIFY: FAIL$verdict"
fi
exit "$failed"
