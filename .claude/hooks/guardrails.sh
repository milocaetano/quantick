#!/bin/sh
# Agent guardrails for quantick. See .claude/hooks/README.md.
#
# Two CLAUDE.md rules were enforceable only by an agent remembering them.
# These make the harness enforce them instead:
#
#   worktree-guard    PreToolUse on Edit|Write|NotebookEdit. Denies a write
#                     that lands in the main checkout while it sits on the
#                     main branch ("one goal, one worktree").
#   pr-gate           PreToolUse on Bash. Denies `gh pr create` until both
#                     reviews have been recorded for the exact commit being
#                     shipped: arch-review ("no branch ships un-reviewed") and
#                     delivery-review ("no branch ships ungraded against what
#                     was asked for").
#   commit-reminder   PostToolUse on Bash. Cannot block (the commit already
#                     landed); says the gate is coming and how to satisfy it.
#
# Every mode filters the tool payload itself rather than relying on the hook
# config's `if` matcher: a matcher that silently fails to match would leave a
# gate that looks armed and is not, and the filtering is covered by
# guardrails_test.sh either way.
#
# Fail-open by design: anything this script cannot determine exits 0 and the
# normal permission flow applies. A guardrail that blocks the session over its
# own bugs is worse than no guardrail, and the rules are also written down in
# CLAUDE.md.
#
# POSIX sh, no jq: it runs under Git Bash on Windows, under dash in CI.

set -u

# The branch that is never worked on directly, and the base every branch is
# cut from and measured against.
MAIN_BRANCH="main"
# Where the two pre-PR reviews record what they approved, in the order they
# run. Each marker lives in the worktree's own git dir, so it is per-branch and
# never committed, and each holds a sha rather than a timestamp: commit again
# after a review and the marker no longer matches, so the gate denies and names
# both shas. They are separate files because they answer separate questions —
# arch-review asks whether the branch is well built, delivery-review whether it
# is what was asked for — and a branch that has passed one has not passed both.
ARCH_MARKER_NAME="arch-review-ok"
DELIVERY_MARKER_NAME="delivery-review-ok"

mode="${1:-}"
input=$(cat)

# --- shared helpers ---------------------------------------------------------

# Pull one string field out of the hook's stdin JSON. Good enough for the flat
# fields we need (`file_path`, `command`, `cwd`); not a JSON parser.
json_string_field() {
    printf '%s' "$input" |
        tr -d '\n' |
        sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\(\([^\"\\\\]\|\\\\.\)*\)\".*/\1/p" |
        head -n 1
}

# JSON escapes a Windows path as C:\\src\\x. Undo that and speak one slash.
normalize_path() {
    printf '%s' "$1" | sed 's|\\\\|/|g; s|\\|/|g'
}

# Walk up to the first directory that exists: a Write targets a file that does
# not exist yet, and may create its parents too.
nearest_existing_dir() {
    d=$(dirname "$1")
    while [ ! -d "$d" ]; do
        p=$(dirname "$d")
        [ "$p" != "$d" ] || return 1
        d="$p"
    done
    printf '%s' "$d"
}

deny() {
    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":%s}}\n' "$1"
    exit 0
}

context() {
    printf '{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":%s}}\n' "$1"
    exit 0
}

# True when the command actually runs `$2` as a statement, rather than merely
# mentioning it. A commit message body reaches the hook inside the command
# string, so a free substring match blocks `git commit -F -` whenever the
# message happens to name the gated command.
runs_command() {
    printf '%s' "$1" |
        sed 's/&&/\n/g; s/||/\n/g; s/;/\n/g' |
        grep -qE "^[[:space:]]*$2([[:space:]]|\$)"
}

# The directory the command operates on. The payload's `cwd` is the session's,
# which resets between calls, while every agent command reaches its worktree
# with a leading `cd <dir> &&`. Trusting `cwd` alone made the gate judge the
# main checkout no matter which branch was being shipped.
effective_dir() {
    d=$(printf '%s' "$1" | sed -n 's|^[[:space:]]*cd[[:space:]][[:space:]]*"\{0,1\}\([^"[:space:];&|]*\)"\{0,1\}.*|\1|p' | head -n 1)
    d=$(normalize_path "$d")
    if [ -n "$d" ] && [ -d "$d" ]; then
        printf '%s' "$d"
        return 0
    fi
    printf '%s' "$2"
}

marker_path() {
    printf '%s/%s' "$(git -C "$1" rev-parse --absolute-git-dir 2>/dev/null)" "$2"
}

# Deny unless marker `$3` in worktree `$1` records exactly the commit `$2` being
# shipped. `$4` states the rule in CLAUDE.md's own words and `$5` says how to
# satisfy it; the recording line is derived from the marker name, so the
# instruction and the file the gate reads can never drift apart.
require_marker() {
    require_dir=$1
    require_head=$2
    require_name=$3
    require_rule=$4
    require_how=$5

    require_marker_file=$(marker_path "$require_dir" "$require_name")
    require_record="git rev-parse HEAD > \\\"\$(git rev-parse --absolute-git-dir)/$require_name\\\""

    if [ ! -f "$require_marker_file" ]; then
        deny "\"CLAUDE.md: $require_rule. \`$require_name\` has not been recorded for this branch. $require_how, then record it:\n\n  $require_record\""
    fi

    require_reviewed=$(cat "$require_marker_file" 2>/dev/null)
    if [ "$require_reviewed" != "$require_head" ]; then
        deny "\"CLAUDE.md: $require_rule. \`$require_name\` was recorded for $require_reviewed but HEAD is now $require_head, so the newest commits are ungraded. Run it again over the final branch and record it again:\n\n  $require_record\""
    fi
}

# --- worktree-guard ---------------------------------------------------------

worktree_guard() {
    # An explicit opt-out for the rare deliberate edit on the main checkout: a
    # hotfix, a repo-hygiene sweep. Set it in the shell before launching.
    if [ "${QUANTICK_ALLOW_MAIN_WRITES:-}" = "1" ]; then
        exit 0
    fi

    raw=$(json_string_field file_path)
    [ -n "$raw" ] || exit 0
    path=$(normalize_path "$raw")

    # Agent working files live in the main checkout by design: the goal file,
    # its archives, the skills, these hooks. Blocking them would break the
    # very workflow this guard protects.
    case "$path" in
        */.claude/*) exit 0 ;;
    esac

    dir=$(nearest_existing_dir "$path") || exit 0

    git_dir=$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null) || exit 0
    common_dir=$(git -C "$dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || exit 0

    # In a linked worktree these differ (<common>/worktrees/<name> vs
    # <common>). Equal means the write lands in the main checkout.
    [ "$git_dir" = "$common_dir" ] || exit 0

    branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null) || exit 0
    [ "$branch" = "$MAIN_BRANCH" ] || exit 0

    deny "\"CLAUDE.md: one goal, one worktree. This write lands in the main checkout while it is on \`$MAIN_BRANCH\`. Create the worktree first:\n\n  git fetch origin\n  git worktree add -b <prefix>/<slug> ../quantick-worktrees/<prefix>-<slug> origin/$MAIN_BRANCH\n\nthen work inside that directory. Set QUANTICK_ALLOW_MAIN_WRITES=1 to override deliberately.\""
}

# --- pr-gate ----------------------------------------------------------------

pr_gate() {
    command=$(json_string_field command)
    runs_command "$command" "gh pr create" || exit 0

    dir=$(effective_dir "$command" "$(normalize_path "$(json_string_field cwd)")")
    [ -d "$dir" ] || exit 0

    head=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || exit 0

    # Checked in the order the reviews run. arch-review first: a delivery
    # review of a branch the shape review is about to change is wasted work,
    # and naming the earlier gap first is the shorter path back to green.
    require_marker "$dir" "$head" "$ARCH_MARKER_NAME" \
        "no branch ships un-reviewed" \
        "Run the arch-review skill over \`git diff $MAIN_BRANCH...HEAD\` and resolve every Blocker and Should-fix (or note the deferral in the PR body)"

    require_marker "$dir" "$head" "$DELIVERY_MARKER_NAME" \
        "no branch ships ungraded against what was asked for" \
        "Run the delivery-review skill: it grades every ask in the request ledger and every acceptance criterion in \`.claude/GOAL.md\`, and passes only when none is MISSING, PARTIAL or UNPROVEN"

    exit 0
}

# --- commit-reminder --------------------------------------------------------

commit_reminder() {
    command=$(json_string_field command)
    runs_command "$command" "git commit" || exit 0

    dir=$(effective_dir "$command" "$(normalize_path "$(json_string_field cwd)")")
    [ -d "$dir" ] || exit 0

    branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null) || exit 0
    [ "$branch" != "$MAIN_BRANCH" ] || exit 0

    ahead=$(git -C "$dir" rev-list --count "origin/$MAIN_BRANCH..HEAD" 2>/dev/null) || exit 0
    [ "${ahead:-0}" -gt 0 ] || exit 0

    context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH. \`gh pr create\` is gated on both \`$ARCH_MARKER_NAME\` and \`$DELIVERY_MARKER_NAME\` recording the exact HEAD being shipped, so run arch-review and then delivery-review once the branch is final — a commit after either one makes its marker stale.\""
}

case "$mode" in
    worktree-guard) worktree_guard ;;
    pr-gate) pr_gate ;;
    commit-reminder) commit_reminder ;;
    *) exit 0 ;;
esac
