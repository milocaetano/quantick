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
# summary of it. Linux runs as four parallel jobs so the app's harness feature
# builds overlap instead of queueing, and Windows as three so its workspace
# test run does; each one is named here, because a job nothing requires is a
# job that can be dropped without anyone noticing. `guardrails_test.sh` fails
# when a name here has no job in ci.yml, and
# `tools/ci/windows_test_coverage.py` fails when the Windows test jobs stop
# being complements.
#
# Draft pushes run only the `fast` job, so these are *skipped* on
# them. A skipped run carries no verdict and is ignored rather than counted as
# passing; a commit with nothing but skipped runs has no full CI at all. The
# check runs are read per commit rather than through `gh pr checks`, because
# a later skipped run for the same head (adding an unrelated label posts one)
# must not hide the green one.
# Only runs posted by GitHub Actions count: a check name is not an identity.

set -u

FULL_CI_CHECKS='ci harness-app harness-combined harness-scenario windows windows-tests windows-app-tests'

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

errors=$(mktemp) || exit 2
trap 'rm -f "$errors"' EXIT HUP INT TERM

for check in $FULL_CI_CHECKS; do
    verdict=$(cd "$worktree" && gh api \
        "repos/{owner}/{repo}/commits/$sha/check-runs?check_name=$check&per_page=100" \
        --jq '[.check_runs[] | select(.app.slug == "github-actions" and .conclusion != "skipped")]
              | sort_by(.id) | last
              | if . == null then "missing" else "\(.status):\(.conclusion // "none")" end' 2>"$errors") || {
        # A commit GitHub has never seen was never pushed, so it has no CI:
        # that is a known answer, not an unreadable one.
        if grep -q 'No commit found' "$errors"; then
            echo "Commit $sha is not on GitHub; push it and run full CI." >&2
            exit 1
        fi
        echo "GitHub could not list the $check check runs at $sha." >&2
        exit 2
    }
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
