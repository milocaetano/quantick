#!/bin/sh
# The `ai-review` findings that live on a pull request, as resolvable review
# threads. See .claude/hooks/README.md and .claude/skills/ai-review/SKILL.md.
#
# The review chain used to carry its findings in a session's context. That made
# them perishable — a restart or a compaction lost them — and uncountable, so
# "are we converging?" was a feeling rather than a number. PR #306 ran 28
# commits and 17 self-declared rounds against a three-round budget nothing
# enforced, and still returned 5 of 6 dimensions WEAK.
#
# A GitHub review thread is the opposite of all of that: durable, addressable
# by id, anchored at file:line, countable, resolvable, and visible to the
# trader without an agent in the room. So the findings move there, and the
# count of open ones is the convergence trend, recorded for free.
#
#   post <pr> <path> <line>   create one thread, body on stdin, marker
#                             prepended. Prints the thread's comment id.
#   list <pr>                 every open ai-review thread: id, anchor, first
#                             line of the finding. What phase two works from,
#                             one thread at a time.
#   count <pr>                how many are open. What the merge gate reads.
#   resolve <id>              close one thread, by the fix or by an acceptance
#                             the trader recorded on it.
#
# `count` is the only subcommand with a machine contract, because a gate reads
# it: a number on stdout and exit 0, or nothing on stdout, a reason on stderr
# and exit 2. Exit 2 means "could not be determined", which is not the same
# answer as zero and must never be mistaken for it.
#
# POSIX sh. `gh` is the one dependency, and its built-in `--jq` does the JSON:
# the surrounding hooks parse JSON with `sed` because they must run before
# anything is installed, but nothing here can run without `gh` anyway.

set -u

# The marker that makes a thread this repository's rather than a human's. It
# leads the comment body, before the severity, and it is an HTML comment so it
# renders as nothing at all.
#
# The gate counts *marked* threads, not every unresolved one. Without that, a
# reviewer's question or a trader's note would block a merge, and the first
# thing anybody would do about it is stop leaving comments — which costs more
# than the gate is worth.
MARKER="<!-- ai-review -->"

usage() {
    cat >&2 <<'USAGE'
usage: ai_review_threads.sh post <pr> <path> <line>   # finding body on stdin
       ai_review_threads.sh list <pr>
       ai_review_threads.sh count <pr>
       ai_review_threads.sh resolve <thread-id>

`count` prints a number and exits 0, or prints a reason on stderr and exits 2
when the count cannot be taken. Every other subcommand exits non-zero on
failure in the ordinary way.
USAGE
    exit 64
}

# The repository the current directory belongs to, as owner/name. Read from
# `gh` rather than from the git remote so a fork, an SSH remote and an HTTPS
# remote all resolve the same way.
repository() {
    gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null
}

# Every review thread on a PR that carries the marker and is not resolved, one
# per line: `<thread id><TAB><path>:<line><TAB><first line of the finding>`.
#
# `first: 100` is the page and there is no second one. A branch with more than
# a hundred open findings has a problem no pagination fixes, and the stall rule
# in CLAUDE.md is the thing that should have caught it long before.
open_threads() {
    pr=$1
    repo=$(repository) || return 1
    [ -n "$repo" ] || return 1
    owner=${repo%%/*}
    name=${repo#*/}

    gh api graphql \
        -f owner="$owner" -f name="$name" -F pr="$pr" \
        -f query='
          query($owner: String!, $name: String!, $pr: Int!) {
            repository(owner: $owner, name: $name) {
              pullRequest(number: $pr) {
                reviewThreads(first: 100) {
                  nodes {
                    id
                    isResolved
                    path
                    line
                    comments(first: 1) { nodes { body } }
                  }
                }
              }
            }
          }' \
        --jq '
          .data.repository.pullRequest.reviewThreads.nodes[]
          | select(.isResolved | not)
          | select((.comments.nodes[0].body // "") | startswith("'"$MARKER"'"))
          | [
              .id,
              ((.path // "?") + ":" + ((.line // 0) | tostring)),
              ((.comments.nodes[0].body // "") | split("\n") | map(select(length > 0)) | .[1] // "")
            ]
          | @tsv' 2>/dev/null
}

# Create one thread. The body arrives on stdin so a finding may be as long as
# it needs to be without passing through a shell argument.
post_thread() {
    pr=$1
    path=$2
    line=$3

    repo=$(repository) || {
        echo "cannot resolve the repository — is gh authenticated here?" >&2
        return 1
    }
    head=$(gh pr view "$pr" --json headRefOid --jq .headRefOid 2>/dev/null) || head=""
    if [ -z "$head" ]; then
        echo "cannot read the head commit of PR #$pr" >&2
        return 1
    fi

    body=$(printf '%s\n%s' "$MARKER" "$(cat)")

    # Anchored at the line first. A finding that names a line the diff does not
    # contain cannot be anchored there — GitHub refuses it — so it falls back
    # to the file, which is still a resolvable thread and still says where the
    # problem is. Falling back to a plain PR comment would not: that is not a
    # thread, nothing can resolve it, and the gate would never see it close.
    if gh api "repos/$repo/pulls/$pr/comments" \
        -f body="$body" -f commit_id="$head" -f path="$path" \
        -F line="$line" -f side=RIGHT --jq .id 2>/dev/null; then
        return 0
    fi

    echo "line $line of $path is not in the diff; anchoring the thread at the file" >&2
    gh api "repos/$repo/pulls/$pr/comments" \
        -f body="$body" -f commit_id="$head" -f path="$path" \
        -f subject_type=file --jq .id
}

case "${1:-}" in
    post)
        [ $# -eq 4 ] || usage
        post_thread "$2" "$3" "$4"
        ;;
    list)
        [ $# -eq 2 ] || usage
        open_threads "$2"
        ;;
    count)
        [ $# -eq 2 ] || usage
        # Two failures are told apart deliberately. `gh` absent, unauthenticated
        # or unreachable is "could not be determined" — exit 2, and the gate
        # that reads this says so out loud instead of waving the merge through
        # in silence. A PR with no findings is a real zero.
        command -v gh >/dev/null 2>&1 || {
            echo "gh is not on PATH, so open ai-review threads cannot be counted" >&2
            exit 2
        }
        threads=$(open_threads "$2") || {
            echo "the GitHub API did not answer, so open ai-review threads cannot be counted" >&2
            exit 2
        }
        # `printf` rather than `echo "$threads" | wc -l`: the latter counts one
        # line for the empty string and reports a clean PR as having a finding.
        if [ -z "$threads" ]; then
            echo 0
        else
            printf '%s\n' "$threads" | wc -l | tr -d ' '
        fi
        ;;
    resolve)
        [ $# -eq 2 ] || usage
        gh api graphql -f threadId="$2" -f query='
          mutation($threadId: ID!) {
            resolveReviewThread(input: { threadId: $threadId }) {
              thread { isResolved }
            }
          }' --jq .data.resolveReviewThread.thread.isResolved
        ;;
    *)
        usage
        ;;
esac
