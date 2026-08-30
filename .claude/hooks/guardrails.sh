#!/bin/sh
# Agent guardrails for quantick. See .claude/hooks/README.md.
#
# Three CLAUDE.md rules were enforceable only by an agent remembering them.
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

# The command as a shell would see it: JSON's doubled backslashes collapsed to
# one. Order matters for what comes next — a Windows path arrives as
# `C:\\new\\x`, and its `\n` must not be mistaken for JSON's two-character line
# break, so the doubling is resolved before anything looks for `\n`.
unescaped_command() {
    printf '%s' "$1" | sed 's|\\\\|/|g; s|\\"|"|g'
}

# True when the command actually runs `$2` as a statement, rather than merely
# mentioning it. A commit message body reaches the hook inside the command
# string, so a free substring match blocks `git commit -F -` whenever the
# message happens to name the gated command.
#
# Every statement separator has to be here, and the list was once short enough
# to be a hole rather than a gap: splitting only on `&&`, `||` and `;` let
# `cat body.md | gh pr create --body-file -` through ungated — the spelling
# `ship` itself recommends — along with a newline in place of `&&`. A gate the
# documented way of doing the thing walks around is not a gate.
#
# Beyond separators, a statement can be dressed up before its command word.
# `gh  pr create` with two spaces, `env`/`time`/`sudo` in front, a `VAR=value`
# prefix, `bash -c '…'`, an opening brace, a `then` — each of these ran the
# gated command while the match, anchored to the literal single-spaced string,
# saw nothing. Whitespace is squeezed and the known dressings are stripped.
#
# The prefix strips run twice: `env VAR=x gh` needs the wrapper gone before
# the assignment, `VAR=x env gh` needs the reverse, and one pass can only
# serve one of those orders.
#
# Two honest limits, stated rather than papered over:
#
#   * A heredoc carries prose, not statements, so when the command opens one
#     (`<<`) newlines stop being separators. Without that, `cat > notes.md
#     <<'EOF' … EOF` denies an agent for *writing documentation* — and this
#     repo's own docs contain a line beginning `gh pr create`. The cost is that
#     `sh <<EOF` with the gated command inside is not caught.
#   * A determined wrapper — `xargs`, a shell function, a script — still gets
#     past. This gate exists against forgetting, not against someone working to
#     defeat it, and the README says the same about what the marker proves.
#
# Where it must not err is the other way. A misread commit message produces a
# denial its author can see and argue with; a PR opening unreviewed because of
# how the command was spelled is silent.
runs_command() {
    runs_text=$(unescaped_command "$1")

    case "$runs_text" in
        *'<<'*) ;;
        *) runs_text=$(printf '%s' "$runs_text" | sed 's/\\n/\
/g') ;;
    esac

    printf '%s' "$runs_text" |
        sed 's/&&/\
/g; s/||/\
/g; s/;/\
/g; s/|/\
/g' |
        sed -E 's/^[[:space:]]*[{(][[:space:]]*/ /' |
        sed -E 's/^[[:space:]]*(then|do|else|!)[[:space:]]+/ /' |
        sed -E "s/^[[:space:]]*(sh|bash|zsh|pwsh|powershell)[[:space:]]+-[Cc][[:space:]]+['\"]?/ /" |
        sed -E 's/^[[:space:]]*([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]*[[:space:]]+)+/ /' |
        sed -E 's/^[[:space:]]*(env|time|sudo|nohup|command|builtin|exec)[[:space:]]+/ /' |
        sed -E 's/^[[:space:]]*([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]*[[:space:]]+)+/ /' |
        sed -E 's/^[[:space:]]*(env|time|sudo|nohup|command|builtin|exec)[[:space:]]+/ /' |
        sed -E 's/[[:space:]]+/ /g' |
        sed -E "s/['\"]+ ?$//" |
        grep -qE "^ ?$2( |\$)"
}

# The directory the command operates on. The payload's `cwd` is the session's,
# which resets between calls, while every agent command reaches its worktree
# with a leading `cd <dir> &&`. Trusting `cwd` alone made the gate judge the
# main checkout no matter which branch was being shipped.
#
# The `\n` is turned into a real line break here for the same reason
# `runs_command` does it: once a newline counts as a separator there, a command
# spelled `cd <wt>\ngh pr create` is *detected* — and was then judged against
# the wrong directory, because this function read the path as `<wt>\ngh`, found
# no such directory and fell back to the session's cwd. The denial that
# produced named the main checkout in its recording line, which is exactly the
# mistake `record_line` exists to prevent.
effective_dir() {
    dir_text=$(unescaped_command "$1" | sed 's/\\n/\
/g')
    d=$(printf '%s' "$dir_text" | sed -n 's|^[[:space:]]*cd[[:space:]][[:space:]]*"\{0,1\}\([^"[:space:];&|]*\)"\{0,1\}.*|\1|p' | head -n 1)
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

# What is wrong with marker `$3` in worktree `$1` for the commit `$2` being
# shipped: empty when nothing is. Reporting rather than denying is what lets
# the caller collect every unsatisfied marker and deny once — an agent that has
# done neither review should learn that in one denial, not discover the second
# review after re-running `gh pr create`, by which time the fix commits from
# the first may have moved HEAD again.
marker_problem() {
    problem_file=$(marker_path "$1" "$3")

    if [ ! -f "$problem_file" ]; then
        printf '`%s` has not been recorded for this branch' "$3"
        return 0
    fi

    # The marker is meant to hold a sha, and nothing enforces that: a mistyped
    # redirect (`git log >` in place of `git rev-parse HEAD >`), an editor
    # appending a line, a half-written file. Reduce it to the shape a sha has
    # before it reaches `deny`, which interpolates it into a JSON string — an
    # unescaped quote or a raw newline emits a payload the harness cannot
    # parse, and a *lost* deny decision turns the gate off instead of tripping
    # it. Fail-open is this script's rule for what it cannot determine; it is
    # not licence to fail open on a marker it can read and does not like.
    problem_reviewed=$(head -n 1 "$problem_file" 2>/dev/null | tr -cd '0-9a-fA-F')
    if [ ${#problem_reviewed} -ne 40 ]; then
        # Whatever is in there, it is not a commit id. Say that rather than
        # printing the hex residue, which would read like a truncated sha.
        problem_reviewed="(not a commit id)"
    fi

    if [ "$problem_reviewed" != "$2" ]; then
        printf '`%s` was recorded for %s but HEAD is now %s, so the newest commits are ungraded' "$3" "$problem_reviewed" "$2"
    fi
}

# The line that records marker `$2`, run from worktree `$1`.
#
# The `cd` is not decoration. Both `git` calls resolve against the shell's cwd,
# which for an agent session is the main checkout and not the worktree being
# shipped — this script's own `effective_dir` treats that as established fact.
# Without the prefix the pasted line writes the main checkout's git dir with
# main's HEAD, the worktree's marker still does not exist, and the next
# `gh pr create` denies with the identical message and no clue why.
record_line() {
    printf 'cd \\"%s\\" && git rev-parse HEAD > \\"$(git rev-parse --absolute-git-dir)/%s\\"' "$1" "$2"
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

    # Both are inspected before anything is said, so one denial can carry both
    # gaps. They are still *run* in this order — a delivery review of a branch
    # the shape review is about to change is wasted work — and the instructions
    # below say so; that is an ordering of work, not a reason to report half of
    # it and make the agent come back for the rest.
    arch_problem=$(marker_problem "$dir" "$head" "$ARCH_MARKER_NAME")
    delivery_problem=$(marker_problem "$dir" "$head" "$DELIVERY_MARKER_NAME")

    if [ -z "$arch_problem" ] && [ -z "$delivery_problem" ]; then
        exit 0
    fi

    reason="CLAUDE.md: no branch ships un-reviewed, or ungraded against what was asked for."
    [ -z "$arch_problem" ] || reason="$reason\n\n  * $arch_problem.\n    Run the arch-review skill over \`git diff origin/$MAIN_BRANCH...HEAD\`, resolve every Blocker and Should-fix (or note the deferral in the PR body), then:\n      $(record_line "$dir" "$ARCH_MARKER_NAME")"
    [ -z "$delivery_problem" ] || reason="$reason\n\n  * $delivery_problem.\n    Run the delivery-review skill: it grades every ask in the request ledger and every acceptance criterion in \`.claude/GOAL.md\`, and passes only when none is MISSING, PARTIAL or UNPROVEN. Then:\n      $(record_line "$dir" "$DELIVERY_MARKER_NAME")"

    if [ -n "$arch_problem" ] && [ -n "$delivery_problem" ]; then
        reason="$reason\n\nRun them in that order: the shape review is the one that sends you back to the code."
    fi

    deny "\"$reason\""
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
