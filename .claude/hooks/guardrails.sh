#!/bin/sh
# Agent guardrails for quantick. See .claude/hooks/README.md.
#
# Two CLAUDE.md rules were enforceable only by an agent remembering them.
# These make the harness enforce them instead:
#
#   worktree-guard    PreToolUse on Edit|Write|NotebookEdit. Denies a write
#                     that lands in the main checkout while it sits on the
#                     main branch ("one goal, one worktree").
#   pr-gate           PreToolUse on Bash. Denies `gh pr create` until
#                     arch-review has been recorded for the exact commit
#                     being shipped ("no branch ships un-reviewed").
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
# Where arch-review records what it approved. Lives in the worktree's own git
# dir, so it is per-branch and never committed.
REVIEW_MARKER_NAME="arch-review-ok"

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

review_marker_path() {
    printf '%s/%s' "$(git -C "$1" rev-parse --absolute-git-dir 2>/dev/null)" "$REVIEW_MARKER_NAME"
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
    case "$command" in
        *"gh pr create"*) ;;
        *) exit 0 ;;
    esac

    dir=$(normalize_path "$(json_string_field cwd)")
    [ -d "$dir" ] || exit 0

    head=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || exit 0
    marker=$(review_marker_path "$dir")

    record="git -C . rev-parse HEAD > \\\"\$(git rev-parse --absolute-git-dir)/$REVIEW_MARKER_NAME\\\""

    if [ ! -f "$marker" ]; then
        deny "\"CLAUDE.md: no branch ships un-reviewed. arch-review has not been recorded for this branch. Run the arch-review skill over \`git diff $MAIN_BRANCH...HEAD\`, resolve every Blocker and Should-fix (or note the deferral in the PR body), then record it:\n\n  $record\""
    fi

    reviewed=$(cat "$marker" 2>/dev/null)
    if [ "$reviewed" != "$head" ]; then
        deny "\"CLAUDE.md: no branch ships un-reviewed. arch-review was recorded for $reviewed but HEAD is now $head, so the newest commits are unreviewed. Re-run arch-review over \`git diff $MAIN_BRANCH...HEAD\` and record it again:\n\n  $record\""
    fi

    exit 0
}

# --- commit-reminder --------------------------------------------------------

commit_reminder() {
    command=$(json_string_field command)
    case "$command" in
        *"git commit"*) ;;
        *) exit 0 ;;
    esac

    dir=$(normalize_path "$(json_string_field cwd)")
    [ -d "$dir" ] || exit 0

    branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null) || exit 0
    [ "$branch" != "$MAIN_BRANCH" ] || exit 0

    ahead=$(git -C "$dir" rev-list --count "origin/$MAIN_BRANCH..HEAD" 2>/dev/null) || exit 0
    [ "${ahead:-0}" -gt 0 ] || exit 0

    context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH. \`gh pr create\` is gated on arch-review having been recorded for the exact HEAD being shipped, so run it over \`git diff $MAIN_BRANCH...HEAD\` once the branch is final.\""
}

case "$mode" in
    worktree-guard) worktree_guard ;;
    pr-gate) pr_gate ;;
    commit-reminder) commit_reminder ;;
    *) exit 0 ;;
esac
