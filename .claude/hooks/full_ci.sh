#!/bin/sh
# The full-CI verdict at one exact commit. See README.md, "Full CI at the
# exact head".
#
#   full_ci.sh verify <worktree> [<sha>]
#
# Exit 0: the latest non-skipped check run at <sha> (default: the worktree's
# HEAD) concluded success for every job in FULL_CI_CHECKS. Exit 1: one did not;
# the reason is on stderr. Exit 2: GitHub could not answer, which is not the
# same answer as "red".
#
# The list is every job that carries part of the full verification, not a
# summary of it. `gate` runs first and every other job waits on it. Linux runs
# as five parallel jobs so the app's harness feature builds and its workspace
# test run overlap instead of queueing, and Windows as four so its workspace
# test run is dealt across two; each one is named here, because a job nothing
# requires is a job that can be dropped without anyone noticing.
# `guardrails_test.sh` fails when a name here has no job in ci.yml, and
# `tools/ci/windows_test_coverage.py` fails when the three Windows test
# selectors stop being exactly the workspace.
#
# Draft pushes run `gate` and `fast`, so the other jobs are *skipped* on them.
# A skipped run carries no verdict and is ignored rather than counted as
# passing; a commit with nothing but skipped runs has no full CI at all. The
# check runs are read per commit rather than through `gh pr checks`, because
# a later skipped run for the same head (adding an unrelated label posts one)
# must not hide the green one.
# Only runs posted by GitHub Actions count: a check name is not an identity.
#
# One exception, and it is the gate's own: when `gate` decided the head needs
# no Rust build (a prose-only diff, or full CI already green at this head), it
# runs the `rust-skip` job. A successful `rust-skip` from the same workflow run
# (check suite) as the latest `gate` lets a job after `gate` have no verdict at
# all, or a verdict of `cancelled` from an older run that a newer one
# superseded at this head (a prose run's own jobs are skipped, so an older
# cancelled run would otherwise stay the latest forever). It never excuses a
# red or running job, a job cancelled in the gate's own run, a missing `gate`,
# or a job missing after a newer `gate` that built Rust; an unknown run id
# never matches. Full jobs are still read as the newest non-skipped run per
# name.
#
# QUANTICK_FULL_CI_FOR_REUSE=1 is the gate asking whether an earlier run already
# finished the full verification at this head. It reads completed runs only, so
# its own run's pending jobs count as missing rather than as the latest, and it
# takes no `rust-skip` excuse: only real green runs are reused.

set -u

FULL_CI_CHECKS='gate ci linux-tests harness-app harness-combined harness-scenario windows windows-tests windows-heavy-tests windows-app-tests'
RUST_SKIP_CHECK='rust-skip'

operation=${1:-}
worktree=${2:-}
[ "$operation" = verify ] && [ -n "$worktree" ] || {
    echo 'Usage: full_ci.sh verify <worktree> [<sha>]' >&2
    exit 64
}
sha=${3:-$(git -C "$worktree" rev-parse HEAD 2>/dev/null)} || {
    echo 'The worktree has no readable HEAD.' >&2
    exit 2
}

if [ "${QUANTICK_FULL_CI_FOR_REUSE:-}" = 1 ]; then
    finished='and .status == "completed"'
    excuse=''
else
    finished=''
    excuse=yes
fi

errors=$(mktemp) || exit 2
trap 'rm -f "$errors"' EXIT HUP INT TERM

# Sets `verdict` (status:conclusion) and `suite` (the run it belongs to) for
# one check name, or exits when GitHub cannot answer.
read_verdict() {
    answer=$(cd "$worktree" && gh api \
        "repos/{owner}/{repo}/commits/$sha/check-runs?check_name=$1&per_page=100" \
        --jq "[.check_runs[] | select(.app.slug == \"github-actions\" and .conclusion != \"skipped\" $finished)]
              | sort_by(.id) | last
              | if . == null then \"missing\" else \"\(.status):\(.conclusion // \"none\") \(.check_suite.id // \"\")\" end" 2>"$errors") || {
        # A commit GitHub has never seen was never pushed, so it has no CI:
        # that is a known answer, not an unreadable one.
        if grep -q 'No commit found' "$errors"; then
            echo "Commit $sha is not on GitHub; push it and run full CI." >&2
            exit 1
        fi
        echo "GitHub could not list the $1 check runs at $sha." >&2
        exit 2
    }
    verdict=${answer%% *}
    case "$answer" in
        *' '*) suite=${answer#* } ;;
        *) suite='' ;;
    esac
}

# Whether a green `rust-skip` from the latest `gate`'s own run excuses the
# full jobs. Read once; an unknown run id is never a match.
gate_suite=''
rust_skipped=''
rust_skip_excuses() {
    if [ -z "$rust_skipped" ]; then
        rust_skipped=no
        if [ -n "$excuse" ]; then
            read_verdict "$RUST_SKIP_CHECK"
            if [ "$verdict" = completed:success ] && [ -n "$suite" ] && [ "$suite" = "$gate_suite" ]; then
                rust_skipped=yes
            fi
        fi
    fi
    [ "$rust_skipped" = yes ]
}

for check in $FULL_CI_CHECKS; do
    read_verdict "$check"
    if [ "$check" = gate ]; then
        gate_suite=$suite
    else
        # Missing, or cancelled by a run that superseded it at this head: no
        # verdict either way, so a valid `rust-skip` excuses both. A run
        # cancelled in the gate's own run is not excused.
        check_verdict=$verdict
        case "$check_verdict" in
            missing) rust_skip_excuses && continue ;;
            completed:cancelled) [ "$suite" != "$gate_suite" ] && rust_skip_excuses && continue ;;
        esac
        # rust_skip_excuses reads another check; keep this one's answer.
        verdict=$check_verdict
    fi
    case "$verdict" in
        completed:success) ;;
        missing)
            echo "No $check run has a verdict at $sha." >&2
            exit 1
            ;;
        completed:*)
            echo "The latest $check run at $sha concluded ${verdict#completed:}." >&2
            exit 1
            ;;
        *:*)
            echo "The latest $check run at $sha is still ${verdict%%:*}." >&2
            exit 1
            ;;
        *)
            echo "GitHub returned an unreadable $check verdict at $sha." >&2
            exit 2
            ;;
    esac
done
