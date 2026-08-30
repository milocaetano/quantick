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
# The visible opt-out from `pr-gate`, deliberately a file rather than an
# environment variable so the session that hits a false positive can create it.
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

# Does this command run the gated program? A substring test, deliberately.
#
# The precise version of this question — split the command into statements,
# strip the dressings, anchor the match to the command word — was tried, and it
# took five review rounds without converging. Each round closed the last
# round's holes and opened new ones: a pipe, a newline, an env prefix, `bash
# -c`, a brace group, two spaces, a trailing redirect, then a CRLF separator
# and PowerShell's `&` call operator introduced by the fix for the round
# before. `/usr/bin/gh`, `'gh'`, `xargs gh`, a tab — each a different spelling
# of the same command, each invisible to an anchored match.
#
# A gate whose correctness depends on out-parsing every way two shells can
# spell a command is a gate that is wrong and does not know it. So this one
# does not parse. If the command *mentions* the gated program, the gate fires.
#
# What that buys: no bypass by spelling, one `case` instead of four `sed`
# stages on every shell call, and an error that only happens in the visible
# direction — a commit message or a doc edit naming the command is denied, its
# author sees exactly why, and `<git-dir>/skip-pr-gate` is the way past. The
# other error, a PR opening unreviewed because of how a command was written,
# is silent, and this script must not make silent errors.
#
# Quotes are stripped and JSON's `\n`, `\r` and `\t` become spaces first, so
# `'gh' pr create` and a CRLF-separated command read the same as the plain one.
mentions_command() {
    # Cheap reject first. This runs on every `Bash` and `PowerShell` call in a
    # session — twice, since `commit-reminder` calls it too — and answers "no"
    # for almost all of them. A `case` on the program's first word costs
    # nothing and spares those calls the three processes below; the parser this
    # replaced spent four `sed` stages before it could say no, about 34 ms a
    # call on this machine.
    case "$1" in
        *"${2%% *}"*) ;;
        *) return 1 ;;
    esac

    mentions_text=$(printf '%s' "$1" |
        sed 's|\\\\|/|g; s|\\"|"|g; s/\\[nrt]/ /g' |
        tr -d "\"'" |
        tr -s '[:space:]' ' ')
    case "$mentions_text" in
        *"$2"*) return 0 ;;
        *) return 1 ;;
    esac
}

# The directory the command operates on. The payload's `cwd` is the session's,
# which resets between calls, while an agent command reaches its worktree with
# a leading `cd <dir>`. Trusting `cwd` alone made the gate judge the main
# checkout no matter which branch was being shipped.
#
# This is a heuristic and is treated as one: `pr_gate` refuses to name a
# directory it is not sure of, rather than printing a remedy that would send
# the agent to stamp markers in the wrong repository. Both quoting styles and
# PowerShell's `Set-Location` are recognised; a CRLF or a relative path is not,
# and that is what the refusal is for.
effective_dir() {
    dir_text=$(printf '%s' "$1" | sed 's|\\\\|/|g; s|\\"|"|g; s/\\[nrt]/\
/g')
    d=$(printf '%s' "$dir_text" |
        sed 's/&&/\
/g; s/;/\
/g' |
        sed -n "s#^[[:space:]]*\(cd\|sl\|Set-Location\|pushd\)[[:space:]][[:space:]]*[\"']\{0,1\}\([^\"';&|]*[^\"';&| ]\)[\"']\{0,1\}[[:space:]]*.*#\2#p" |
        head -n 1)
    d=$(normalize_path "$d")
    if [ -n "$d" ] && [ -d "$d" ]; then
        printf '%s' "$d"
        return 0
    fi
    printf '%s' "$2"
}

# The line that records marker `$2`, run from worktree `$1`.
#
# Two lines rather than a `&&` chain: Windows PowerShell 5.1 is the primary
# shell on this machine and `&&` is a parser error there, so a chained remedy
# handed to an agent denied inside a PowerShell session cannot be run in the
# shell it was denied in. `cd` on its own line, then the redirect, is valid in
# both — as is `$(…)`, which PowerShell reads as a subexpression.
#
# The break between them is the *escaped* `\n`, not a real one. `deny` puts
# this string inside a JSON value, where a literal newline is an invalid
# control character: the payload stops parsing, the decision is lost, and the
# gate fails open on exactly the command it meant to stop. That is how the
# `&&` chain got here in the first place — it was the version that produced
# valid JSON, and the two-line form that replaced it broke the payload for one
# commit before this line was written.
#
# The `cd` is not decoration. Both `git` calls resolve against the shell's cwd,
# which for an agent session is the main checkout and not the worktree being
# shipped, so without it the pasted line writes the main checkout's git dir
# with main's HEAD and the next attempt denies identically.
record_line() {
    printf 'cd \\"%s\\"\\n      git rev-parse HEAD > \\"$(git rev-parse --absolute-git-dir)/%s\\"' "$1" "$2"
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
    # Lower-cased as well as filtered: `tr -cd` accepts uppercase hex on
    # purpose, and a marker written in uppercase would pass the shape test and
    # then never equal git's lowercase output — reported as "recorded for
    # ABC123 but HEAD is now abc123", two shas a reader cannot tell apart, and
    # unfixable by re-running the review.
    problem_reviewed=$(head -n 1 "$problem_file" 2>/dev/null | tr -cd '0-9a-fA-F' | tr 'A-F' 'a-f')
    if [ ${#problem_reviewed} -ne 40 ]; then
        # Whatever is in there, it is not a commit id. Say that rather than
        # printing the hex residue, which would read like a truncated sha.
        problem_reviewed="(not a commit id)"
    fi

    if [ "$problem_reviewed" != "$2" ]; then
        printf '`%s` was recorded for %s but HEAD is now %s, so the newest commits are ungraded' "$3" "$problem_reviewed" "$2"
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
    dir=$(nearest_existing_dir "$path") || exit 0

    # Agent working files live in the main checkout by design: the goal file,
    # its archives, the skills, these hooks. The exemption is matched against
    # the *resolved* path, not the raw string — `src/.claude/../evil.rs` has a
    # `.claude` component and resolves to an ordinary source file, while
    # `crates/app/../../.claude/GOAL.md` has two dots and is a legitimate
    # agent write. Rejecting every path containing `..` caught the first and
    # broke the second; resolving first separates them.
    resolved=$(cd "$dir" 2>/dev/null && pwd)
    resolved="${resolved:-$dir}/$(basename "$path")"
    case "$resolved" in
        */.claude/*) exit 0 ;;
    esac

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
    # The deliberate way past a false positive. This gate errs toward denying —
    # it splits on separators a quoted string can contain, so a commit message
    # or a heredoc that merely *names* `gh pr create` after a `&&` or a pipe is
    # denied along with the real thing. That trade is on purpose: the other
    # error is a PR opening unreviewed, and it is silent. This override is the
    # visible way out, and it mirrors QUANTICK_ALLOW_MAIN_WRITES above — it
    # shows up in the transcript, where a reviewer can see it was used, unlike
    # a spelling that quietly slips past.
    if [ "${QUANTICK_SKIP_PR_GATE:-}" = "1" ]; then
        exit 0
    fi

    command=$(json_string_field command)
    mentions_command "$command" "gh pr create" || exit 0

    dir=$(effective_dir "$command" "$(normalize_path "$(json_string_field cwd)")")
    [ -d "$dir" ] || exit 0

    # The deliberate way past a false positive, and it has to be reachable by
    # the session that hits one. An environment variable is not: the hook
    # inherits the environment of whatever launched Claude, so an inline
    # `VAR=1 <command>` never reaches it and a settings.json entry needs a
    # restart. A file in the worktree's git dir can be created by the session
    # that was just denied, is plainly visible to anyone looking, is named in
    # the denial itself, and goes away with the worktree.
    [ -f "$(marker_path "$dir" "$SKIP_NAME")" ] && exit 0

    git_dir=$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null) || exit 0
    common_dir=$(git -C "$dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || exit 0
    head=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || exit 0

    # A PR ships from a linked worktree — CLAUDE.md's "one goal, one worktree".
    # Landing on the main checkout means either that rule is being broken or
    # the `cd` was spelled in a way this script could not read (a CRLF, a
    # relative path, a `cd` that runs after the command). Both must deny, and
    # neither may be handed a concrete remedy: a recording line naming the main
    # checkout writes both markers into the shared git dir, where main's HEAD
    # rarely moves, so they would satisfy this gate for every later branch. A
    # gate that can be switched off by being obeyed is worse than no gate, so
    # when the directory is uncertain the remedy names no path at all.
    if [ "$git_dir" = "$common_dir" ]; then
        deny "\"CLAUDE.md: no branch ships un-reviewed. This resolves to the main checkout ($dir) rather than a linked worktree, so the gate cannot tell which branch is being shipped and will not guess. Run it from the worktree with an explicit leading \`cd\`:\n\n  cd <worktree>\n  <the command>\""
    fi

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
    [ -z "$delivery_problem" ] || reason="$reason\n\n  * $delivery_problem.\n    Run the delivery-review skill: it grades every ask in the request ledger and every acceptance criterion in the branch's goal file — \`.claude/GOAL.md\`, or \`.claude/GOAL-archive-<slug>.md\` once the mission has archived it, which the mandated order does before either review runs — and passes only when none is MISSING, PARTIAL or UNPROVEN. Then:\n      $(record_line "$dir" "$DELIVERY_MARKER_NAME")"

    if [ -n "$arch_problem" ] && [ -n "$delivery_problem" ]; then
        reason="$reason\n\nRun them in that order: the shape review is the one that sends you back to the code."
    fi

    deny "\"$reason\""
}

# --- commit-reminder --------------------------------------------------------

commit_reminder() {
    command=$(json_string_field command)
    mentions_command "$command" "git commit" || exit 0

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
