#!/bin/sh
# Agent guardrails for quantick. See .claude/hooks/README.md.
#
# Three CLAUDE.md rules were enforceable only by an agent remembering them.
# These make the harness enforce them instead:
#
#   worktree-guard    PreToolUse on Edit|Write|NotebookEdit. Denies a write
#                     that lands in the main checkout while it sits on the
#                     main branch ("one goal, one worktree").
#   pr-gate           PreToolUse on Bash. Denies `gh pr create`
#                     until BOTH reviews have been recorded for the exact
#                     commit being shipped: arch-review ("no branch ships
#                     un-reviewed") and delivery-review ("no branch ships
#                     ungraded against what was asked for"). A mission
#                     that declared the `small` tier is exempt from the
#                     second one, but only while the branch stays small
#                     enough to have earned the word: `declared_tier` reads
#                     the declaration, `changed_lines` and
#                     `SMALL_TIER_MAX_CHANGED_LINES` are the bound on it.
#   commit-reminder   PostToolUse on Bash. Cannot block (the commit
#                     already landed); says the gate is coming and how to
#                     satisfy it.
#
# What `runs_command` can and cannot see is a known, bounded limitation, and it
# is deliberately left as it was rather than deepened. It splits on `&&`, `||`
# and `;` and anchors the match, so spellings that put the command elsewhere —
# a pipe, a newline, an env prefix, a wrapper — are not detected. An attempt to
# close that by parsing harder ran to eight review rounds without converging:
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

# What ceremony a mission declared for this branch, written by the `mission`
# skill into the same per-branch git dir as the markers above and never
# committed. Only `small` changes anything here: it exempts the branch from
# delivery-review, which is the whole reason a tier is worth declaring.
#
# The exemption is bounded, and the bound is the design rather than a
# refinement of it. A way past this gate was tried once before, as a skip file,
# and reverted: the denial that taught an agent to create it was the same
# denial an agent sees when it has simply not run the review, which handed the
# kill switch to exactly the caller with a motive to pull it. Two things keep
# this one honest.
#
#   1. The denial says nothing about tiers unless the branch already declared
#      one. An agent that never asked for the cheap path is never told there
#      is one, so the gate cannot teach its own way around itself.
#   2. The word has to be true. The exemption holds only while the diff stays
#      under the ceiling below, so declaring `small` dishonestly at PR time
#      buys it only on branches where declaring it honestly would have been
#      allowed anyway. That is the argument the skip file could not make.
#
# The file names the branch it was declared for, `<branch> <tier>`, and a
# declaration for any other branch is no declaration at all. That is not
# decoration: the two markers above hold a sha, so they go stale the moment the
# branch moves, while a bare tier word would outlive the mission that wrote it.
# A worktree reused for a second branch then inherits an exemption it never
# asked for and ships with no delivery-review — reproduced against the first
# version of this feature, which stored the word alone.
TIER_FILE_NAME="mission-tier"
# Every tier `mission` may declare, in the order they cost. A file holding
# anything else is treated as no declaration at all: an unrecognised word must
# never be the difference between a graded branch and an ungraded one.
TIERS="small medium high max"
# Changed lines — insertions plus deletions against origin/main — a `small`
# branch may carry and keep its exemption. A fix, a tweak or a paragraph of
# prose sits well under it; past it a branch carries enough separate asks that
# a ledger is worth grading, which is exactly when delivery-review earns its
# cost. One constant with one comment, so reversing the judgement is one edit.
SMALL_TIER_MAX_CHANGED_LINES=300

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

# Empty when the git dir cannot be resolved, so the caller can fail open
# rather than build a path rooted at `/` and hand back a remedy telling an
# agent to write the marker at the filesystem root.
marker_path() {
    marker_git_dir=$(git -C "$1" rev-parse --absolute-git-dir 2>/dev/null) || return 1
    [ -n "$marker_git_dir" ] || return 1
    printf '%s/%s' "$marker_git_dir" "$2"
}

# The tier declared for worktree `$1`, or nothing when none is declared or the
# declaration is not one of `$TIERS`. The file's contents never leave this
# function: an unrecognised tier is reported as no tier at all, so a corrupt or
# hostile file can neither reach `deny`'s JSON nor widen anything.
declared_tier() {
    tier_file=$(marker_path "$1" "$TIER_FILE_NAME") || return 1
    [ -f "$tier_file" ] || return 1

    tier_branch=$(git -C "$1" rev-parse --abbrev-ref HEAD 2>/dev/null) || return 1
    [ -n "$tier_branch" ] || return 1

    tier_line=$(head -n 1 "$tier_file" 2>/dev/null |
        tr -d '\r' |
        tr '\t' ' ' |
        sed 's/^ *//; s/ *$//; s/  */ /g')

    # Exactly two fields, and the code says so rather than approximating it.
    # One field is the format this file used before it carried a branch, and
    # reading it as a tier would restore the inheritance bug the format exists
    # to close, so it is refused rather than migrated. Three fields is a typo,
    # and it is refused here rather than incidentally by the branch compare
    # below — a later edit that relaxes that compare must not quietly start
    # accepting a shape this comment promises is rejected.
    case "$tier_line" in
        *' '*' '*) return 1 ;;
        *' '*) ;;
        *) return 1 ;;
    esac

    tier_for=${tier_line% *}
    tier=$(printf '%s' "${tier_line##* }" | tr 'A-Z' 'a-z')

    # A declaration belongs to one branch. Detached HEAD reports `HEAD`, which
    # matches no branch name, so it grants nothing either.
    [ "$tier_for" = "$tier_branch" ] || return 1

    # Validated against every tier, though both callers currently compare only
    # against `small`, so returning `medium` and returning nothing behave
    # identically today. That is deliberate and worth stating, because the loop
    # looks inert: this function answers "what did the mission declare", not
    # "is this branch exempt", and `TIERS` is the single place the vocabulary
    # lives — the suite and the mission skill are both checked against it. A
    # second exempt tier should not have to reintroduce validation that was
    # deleted for looking unused.
    for tier_known in $TIERS; do
        if [ "$tier" = "$tier_known" ]; then
            printf '%s' "$tier"
            return 0
        fi
    done
    return 1
}

# Insertions plus deletions on worktree `$1` against origin/<main>, or nothing
# when that cannot be measured — an absent remote ref, a git that errors.
#
# The caller reads "cannot measure" as "no exemption". This is the one place in
# the script that fails *closed*, and deliberately against the file's own
# fail-open rule: everywhere else an undetermined answer costs a permission
# prompt, while here it would cost an ungraded branch.
#
# Digits only, always. The result reaches `deny`'s JSON, where a stray quote
# emits a payload the harness cannot parse — and a lost deny decision switches
# the gate off rather than tripping it.
#
# `--numstat` under `LC_ALL=C`, never `--shortstat`. Shortstat is prose, and
# git ships translations of it: under a localised install the counts sit inside
# words no English pattern matches, so both parses come back empty, the sum is
# 0 — and 0 is indistinguishable from an empty diff, which grants the exemption
# to a branch of any size. That is failing open in the one function that must
# not. Numstat is `added<TAB>deleted<TAB>path` in every locale, which is the
# whole reason it is what gets read here.
changed_lines() {
    lines_raw=$(LC_ALL=C git -C "$1" diff --numstat "origin/$MAIN_BRANCH...HEAD" 2>/dev/null) || return 1

    lines_total=0
    for lines_n in $(printf '%s\n' "$lines_raw" | cut -f1,2); do
        case "$lines_n" in
            # `-` for both counts is how numstat reports a binary file. It
            # contributes nothing because it *has* no lines, and this metric
            # counts lines — an added icon, font or screenshot is not an
            # unreadable diff, and refusing the branch over one would blame a
            # broken git for a normal asset. The ceiling is a proxy for how
            # many separate asks a branch carries, and a binary carries none.
            '-') continue ;;
            # Anything else non-numeric means numstat itself was not
            # understood, which is the fail-closed case: no exemption.
            *[!0-9]*) return 1 ;;
            *) lines_total=$((lines_total + lines_n)) ;;
        esac
    done

    printf '%s' "$lines_total"
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

    require_file=$(marker_path "$require_dir" "$require_name") || exit 0
    # `git -C "$require_dir"`, never `git -C .`. The remedy is pasted into a
    # shell whose cwd is the session's — the main checkout — not the worktree
    # being shipped. With `.` the marker lands in the *shared* git dir holding
    # main's HEAD, where a later command resolving to the main checkout would
    # match it, so one paste of the gate's own instruction could switch it off.
    # Every doc on this branch spells the command with the `cd`; the message
    # must not be the one place that drops it.
    require_record="git -C \\\"$require_dir\\\" rev-parse HEAD > \\\"$require_file\\\""

    if [ ! -f "$require_file" ]; then
        deny "\"CLAUDE.md: $require_rule. \`$require_name\` has not been recorded for this branch. $require_how, then record it:\n\n  $require_record\""
    fi

    # Trim only what a file legitimately picks up — a trailing CR, a BOM,
    # surrounding blanks — then require the remainder to be exactly a sha.
    # Stripping every non-hex byte instead would silently accept `<sha> ok`
    # or a sha with a comment beside it, which a verbatim compare rejects.
    require_reviewed=$(head -n 1 "$require_file" 2>/dev/null |
        tr -d '\r' |
        sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
    case "$require_reviewed" in
        '' | *[!0-9a-fA-F]*) require_reviewed="(not a commit id)" ;;
        *)
            [ ${#require_reviewed} -eq ${#require_head} ] ||
                require_reviewed="(not a commit id)"
            ;;
    esac
    require_reviewed=$(printf '%s' "$require_reviewed" | tr 'A-F' 'a-f')

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
    #
    # Required at every tier, `small` included. A tier buys a shorter review,
    # never no review: the bug pass is the last thing a branch should be able
    # to buy its way out of, and a small diff is not the same as a safe one.
    require_marker "$dir" "$head" "$ARCH_MARKER_NAME" \
        "no branch ships un-reviewed" \
        "Run the arch-review skill over \`git diff origin/$MAIN_BRANCH...HEAD\` and resolve every Blocker and Should-fix (or note the deferral in the PR body)"

    delivery_how="Run the delivery-review skill: it grades every ask in the branch's goal file and every acceptance criterion, and passes only when none is MISSING, PARTIAL or UNPROVEN"

    # A branch that declared no tier takes the path it has always taken and is
    # told exactly what it was told before. That is deliberate, and it is the
    # property the whole exemption rests on: the denial an agent meets when it
    # has merely forgotten the review must never double as an advertisement for
    # the way around it. Only a branch that already asked for the cheap path
    # hears anything at all about the bound on it.
    if [ "$(declared_tier "$dir")" = "small" ]; then
        small_size=$(changed_lines "$dir")

        if [ -n "$small_size" ] && [ "$small_size" -le "$SMALL_TIER_MAX_CHANGED_LINES" ]; then
            exit 0
        fi

        if [ -z "$small_size" ]; then
            delivery_how="This branch declares the \`small\` tier, whose exemption from this review is granted only where its size against origin/$MAIN_BRANCH can be measured, and here it cannot — which is about the measurement, not the size of the work. $delivery_how"
        else
            delivery_how="This branch declares the \`small\` tier, whose exemption from this review stops at $SMALL_TIER_MAX_CHANGED_LINES changed lines against origin/$MAIN_BRANCH; it carries $small_size, so the work has outgrown the word. Raise the tier in the goal file — a tier goes up, never down. $delivery_how"
        fi
    fi

    require_marker "$dir" "$head" "$DELIVERY_MARKER_NAME" \
        "no branch ships ungraded against what was asked for" \
        "$delivery_how"

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

    # A `small` mission is reminded of the gate it actually faces. Repeating
    # the two-marker line at every tier would send it off to run the review its
    # own tier exempts it from, which is the saving the tier exists to buy. It
    # would also make this the place an agent first learns the exemption exists
    # the thing pr-gate's denial is careful never to be. Here that is safe:
    # the branch has already declared the tier, so nothing is being taught.
    if [ "$(declared_tier "$dir")" = "small" ]; then
        small_size=$(changed_lines "$dir")

        # Three situations, three messages, because the branch is already
        # measured here and a message that opens with the wrong one states a
        # falsehood in the clause an agent acts on. Folding "cannot measure"
        # into "outgrown" was the worst of them: it told a two-line branch it
        # had outgrown its tier and to raise it — a move the mission skill makes
        # deliberately irreversible.
        if [ -z "$small_size" ]; then
            context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH at the \`small\` tier, but its size against origin/$MAIN_BRANCH cannot be measured here — so the exemption from \`$DELIVERY_MARKER_NAME\` does not apply and \`gh pr create\` wants both markers. This is about the measurement, not the size of the work: check that origin/$MAIN_BRANCH exists in this checkout before raising the tier.\""
        fi

        if [ "$small_size" -le "$SMALL_TIER_MAX_CHANGED_LINES" ]; then
            context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH at the \`small\` tier, so \`gh pr create\` wants \`$ARCH_MARKER_NAME\` alone — recorded for the exact HEAD being shipped, which any later commit stales. It carries $small_size of the $SMALL_TIER_MAX_CHANGED_LINES changed lines the exemption from \`$DELIVERY_MARKER_NAME\` allows.\""
        fi

        context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH and has outgrown its \`small\` tier: it carries $small_size changed lines against the $SMALL_TIER_MAX_CHANGED_LINES the exemption allows, so \`gh pr create\` now wants both \`$ARCH_MARKER_NAME\` and \`$DELIVERY_MARKER_NAME\` recorded for the exact HEAD being shipped. Raise the tier in the goal file and run both reviews.\""
    fi

    context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH. \`gh pr create\` is gated on both \`$ARCH_MARKER_NAME\` and \`$DELIVERY_MARKER_NAME\` recording the exact HEAD being shipped, so run arch-review and then delivery-review once the branch is final — a commit after either one makes its marker stale.\""
}

case "$mode" in
    worktree-guard) worktree_guard ;;
    pr-gate) pr_gate ;;
    commit-reminder) commit_reminder ;;
    *) exit 0 ;;
esac
