#!/bin/sh
# Agent guardrails for quantick. See .claude/hooks/README.md.
#
# Three CLAUDE.md rules were enforceable only by an agent remembering them.
# These make the harness enforce them instead:
#
#   worktree-guard    PreToolUse on Edit|Write|NotebookEdit. Denies a write
#                     that lands in the main checkout while it sits on the
#                     main branch ("one goal, one worktree").
#   pr-gate           PreToolUse on Bash|PowerShell. Denies `gh pr create`
#                     until BOTH reviews have been recorded for the exact
#                     commit being shipped: arch-review ("no branch ships
#                     un-reviewed") and delivery-review ("no branch ships
#                     ungraded against what was asked for").
#   commit-reminder   PostToolUse on Bash|PowerShell. Cannot block (the commit
#                     already landed); says the gate is coming and how to
#                     satisfy it.
#
# What `runs_command` can and cannot see is a known, bounded limitation, and it
# is deliberately left as it was rather than deepened. It splits on `&&`, `||`
# and `;` and anchors the match, so spellings that put the command elsewhere —
# a pipe, a newline, an env prefix, a wrapper — are not detected. An attempt to
# close that by parsing harder ran to five review rounds without converging:
# every round shut some spellings and opened others, twice producing a denial
# whose own remedy would have disabled the gate permanently. Widening this is
# its own change, with its own review; a half-parser that looks airtight is
# worse than a narrow one that is documented. See README.md.
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
# run. Each lives in the worktree's own git dir, so it is per-branch and never
# committed, and each holds a sha rather than a timestamp: commit again after a
# review and its marker no longer matches. They are separate files because they
# answer separate questions — arch-review whether the branch is well built,
# delivery-review whether it is what was asked for — and a branch that has
# passed one has not passed both.
ARCH_MARKER_NAME="arch-review-ok"
DELIVERY_MARKER_NAME="delivery-review-ok"
# The visible opt-out from `pr-gate`, deliberately a file beside the markers
# rather than an environment variable, so the session that hits a false
# positive can create it. Named in the denial itself.
SKIP_NAME="skip-pr-gate"

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

# Deny unless marker `$3` in worktree `$1` records exactly the commit `$2`
# being shipped. `$4` states the rule in CLAUDE.md's own words, `$5` says how
# to satisfy it, and the recording line is derived from the marker name — so
# the instruction and the file the gate reads cannot drift apart.
#
# The marker is meant to hold a sha and nothing enforces that: a mistyped
# redirect, an editor appending a line, a half-written file. Its contents are
# reduced to hex and lower-cased before they reach `deny`, which interpolates
# them into a JSON string — an unescaped quote or a raw newline there emits a
# payload the harness cannot parse, and a *lost* deny decision turns the gate
# off rather than tripping it.
require_marker() {
    require_dir=$1
    require_head=$2
    require_name=$3
    require_rule=$4
    require_how=$5

    require_file=$(marker_path "$require_dir" "$require_name")

    # `git -C "$require_dir"`, never `git -C .`. The remedy is pasted into a
    # shell whose cwd is the session's — the main checkout — not the worktree
    # being shipped. With `.` the marker lands in the *shared* git dir holding
    # main's HEAD, and a later `gh pr create` with no leading `cd` (the shape
    # the PowerShell tool's own contract mandates) falls back to that same
    # directory, matches, and is allowed. One paste of the gate's own
    # instruction switches it off for every later branch. Naming the resolved
    # worktree is what stops the remedy from being the bypass.
    require_record="git -C \\\"$require_dir\\\" rev-parse HEAD > \\\"$require_file\\\""

    if [ ! -f "$require_file" ]; then
        deny "\"CLAUDE.md: $require_rule. \`$require_name\` has not been recorded for this branch. $require_how, then record it:\n\n  $require_record\n\nIf this command only *mentions* the gated command rather than running it, that is a known false positive of the matcher; the visible way past is a file the gate names:\n\n  : > \\\"$(marker_path "$require_dir" "$SKIP_NAME")\\\"\""
    fi

    require_reviewed=$(head -n 1 "$require_file" 2>/dev/null | tr -cd '0-9a-fA-F' | tr 'A-F' 'a-f')
    [ ${#require_reviewed} -eq 40 ] || require_reviewed="(not a commit id)"

    if [ "$require_reviewed" != "$require_head" ]; then
        deny "\"CLAUDE.md: $require_rule. \`$require_name\` was recorded for $require_reviewed but HEAD is now $require_head, so the newest commits are ungraded. $require_how, then record it again:\n\n  $require_record\""
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
    # its archives, the skills. Blocking them would break the very workflow
    # this guard protects.
    #
    # The hooks are the exception to the exception. `settings.json` arms every
    # session from `${CLAUDE_PROJECT_DIR}/.claude/hooks/`, so an edit there
    # while the main checkout sits on `main` disarms both gates for every
    # session and every branch at once, with no record and no override. That is
    # a code change like any other and belongs on a branch — where this guard
    # allows it, and where the reviews can see it.
    case "$path" in
        */.claude/hooks/*) ;;
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

    git_dir=$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null) || exit 0
    common_dir=$(git -C "$dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || exit 0

    # A PR ships from a linked worktree — CLAUDE.md's "one goal, one worktree".
    # Landing on the main checkout means either that rule is being broken or
    # the `cd` was spelled in a way this script cannot read, and judging it
    # there is how the gate gets switched off: main's HEAD rarely moves, so
    # markers written into the shared git dir keep matching for every later
    # branch. Deny instead, and name no path — a remedy pointing at the main
    # checkout is the bypass, not the fix.
    if [ "$git_dir" = "$common_dir" ]; then
        deny "\"CLAUDE.md: no branch ships un-reviewed. This resolves to the main checkout ($dir) rather than a linked worktree, so the gate cannot tell which branch is being shipped and will not guess. Run it from the worktree, with an explicit leading \`cd\` so the gate can follow:\n\n  cd <worktree>\n  <the command>\""
    fi

    # The deliberate way past a false positive, and it must be reachable by the
    # session that hits one. `runs_command` matches the gated command at the
    # start of any `&&`/`||`/`;` segment, so a shell command that merely
    # *quotes* the workflow — a doc line, a commit message, a heredoc PR body
    # describing this very gate — is denied along with the real thing. An
    # environment variable cannot help there: the hook inherits the environment
    # of whatever launched Claude, so an inline `VAR=1 <command>` never reaches
    # it. A file in the worktree's git dir can be created by the session that
    # was just denied, is plainly visible, and goes away with the worktree.
    #
    # Checked *after* the main-checkout refusal above, deliberately: a
    # skip file in the shared git dir would otherwise switch the gate off for
    # every branch at once, which is the failure that refusal exists to stop.
    [ -f "$(marker_path "$dir" "$SKIP_NAME")" ] && exit 0

    # Checked in the order the reviews run. arch-review first: a delivery
    # review of a branch the shape review is about to change is wasted work,
    # and naming the earlier gap first is the shorter path back to green.
    require_marker "$dir" "$head" "$ARCH_MARKER_NAME" \
        "no branch ships un-reviewed" \
        "Run the arch-review skill over \`git diff origin/$MAIN_BRANCH...HEAD\` and resolve every Blocker and Should-fix (or note the deferral in the PR body)"

    require_marker "$dir" "$head" "$DELIVERY_MARKER_NAME" \
        "no branch ships ungraded against what was asked for" \
        "Run the delivery-review skill: it grades every ask in the branch's goal file and every acceptance criterion, and passes only when none is MISSING, PARTIAL or UNPROVEN"

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
