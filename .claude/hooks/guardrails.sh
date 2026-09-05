#!/bin/sh
# Agent guardrails for quantick. See .claude/hooks/README.md.
#
# Three CLAUDE.md rules were enforceable only by an agent remembering them.
# The first three modes make the harness enforce them instead. The fourth
# enforces nothing and is not a gate: it carries news from a check that
# already exists, and says so where it is defined.
#
#   worktree-guard    PreToolUse on Edit|Write|NotebookEdit. Denies a write
#                     that lands in the main checkout while it sits on the
#                     main branch ("one goal, one worktree").
#   pr-gate           PreToolUse on Bash. Denies `gh pr create`, `gh pr
#                     ready` and `gh pr merge` until BOTH reviews have been
#                     recorded for the exact commit being shipped:
#                     arch-review ("no branch ships un-reviewed") and
#                     delivery-review ("no branch ships ungraded against
#                     what was asked for"). A mission that declared the
#                     `small` tier is exempt from the second one, but only
#                     while the branch stays small enough to have earned
#                     the word: `declared_tier` reads the declaration,
#                     `changed_lines` and `SMALL_TIER_MAX_CHANGED_LINES`
#                     are the bound on it.
#
#                     Two things are new with the two-phase chain. A
#                     *draft* `gh pr create` passes ungated: a draft PR is
#                     where phase one ends and where `ai-review` posts its
#                     findings, so gating it would demand the reviews
#                     before the findings that inform them. And `gh pr
#                     ready` and `gh pr merge` additionally want zero open
#                     `ai-review` threads, counted by the sibling script
#                     that posts them — so the reviewer and the gate share
#                     one definition of an open finding.
#   commit-reminder   PostToolUse on Bash. Cannot block (the commit
#                     already landed); says the gate is coming and how to
#                     satisfy it.
#   guard-watch       PostToolUse on Edit|Write. Not a gate: runs the
#                     repository guards over the file just written, using
#                     the already-built binary, and reports. It never
#                     denies and never blocks; `cargo test --workspace`
#                     remains the thing that enforces those guards.
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

# Paths whose churn is mission bookkeeping rather than the change under review.
# The goal file and its archive are written by `mission` itself, and the archive
# is required to be the branch's *last* commit - so counting it against the
# ceiling lets a small branch be pushed out of its own tier by the paperwork the
# tier obliged it to file. Measured: one real archive in this repo is larger
# than the entire ceiling. Excluded from the *size* only. The review key below
# still covers it, because both reviews read that file and a change to it is a
# change they should see again.
SIZE_EXCLUDES=":(exclude).claude/GOAL.md :(exclude).claude/GOAL-archive-*.md"

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

# Every file an edit tool targets. Claude's Write/Edit payloads carry one
# `file_path`; Codex's apply_patch payload carries the patch in `command`.
# Keep the translation here so both hosts reach the same policy functions.
edit_paths() {
    edit_file=$(json_string_field file_path)
    if [ -n "$edit_file" ]; then
        printf '%s\n' "$edit_file"
        return 0
    fi

    edit_patch=$(json_string_field command)
    [ -n "$edit_patch" ] || return 0
    printf '%s' "$edit_patch" |
        sed 's/\\r\\n/\
/g; s/\\n/\
/g' |
        sed -n 's/^\*\*\* \(Add\|Update\|Delete\) File: //p; s/^\*\*\* Move to: //p'
}

# Codex patch headers may be relative to the hook's cwd; Claude normally sends
# absolute file paths. Resolve both into the spelling the existing checks use.
absolute_edit_path() {
    edit_path=$(normalize_path "$1")
    case "$edit_path" in
        /* | ?:/*) printf '%s' "$edit_path" ;;
        *)
            edit_cwd=$(normalize_path "$(json_string_field cwd)")
            [ -d "$edit_cwd" ] || edit_cwd=$(normalize_path "$(pwd -P)")
            printf '%s/%s' "${edit_cwd%/}" "$edit_path"
            ;;
    esac
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

# Neither a denial nor a silent pass: the gate could not determine something it
# is supposed to check, and says so where a human will read it.
#
# The file's fail-open rule is kept — `ask` blocks nothing a human does not
# block. What it refuses to do is fail open *silently*. An unreachable GitHub
# is the difference between "this branch has no open findings" and "nobody
# knows whether it has any", and a gate that prints the same nothing for both
# has taught its reader that silence means clean.
ask() {
    printf '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","permissionDecisionReason":%s}}\n' "$1"
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

    # `rev-parse --abbrev-ref` prints the literal string `HEAD` when the head is
    # detached, and the snippet that *writes* this file uses that same command -
    # so a declaration made while detached records `HEAD`, and comparing it here
    # would then match every future detached checkout in this worktree. That is
    # the inheritance bug the branch field exists to close, wearing a different
    # hat, and it was reproduced against the version of this function that
    # merely claimed detached heads "grant nothing". A detached head names no
    # branch, so it declares nothing.
    [ "$tier_branch" != "HEAD" ] || return 1

    # A declaration belongs to one branch.
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
    # `-- .` then the exclusions: the goal file and its archive are the
    # mission's own bookkeeping, and `mission` requires that archive as the
    # branch's last commit. Counting it lets a branch be pushed out of a tier it
    # legitimately claimed by the very artifact the tier obliged it to write -
    # and the denial's remedy is "raise the tier", an escalation the skill makes
    # irreversible. The ceiling proxies how many asks a branch carries; a goal
    # file carries none, it *describes* them.
    # shellcheck disable=SC2086
    lines_raw=$(LC_ALL=C git -C "$1" diff --numstat "origin/$MAIN_BRANCH...HEAD" -- . $SIZE_EXCLUDES 2>/dev/null) || return 1

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

# The identity of what a review covered: a hash of this branch's own diff
# against origin/<main>, not the sha of whichever commit happened to carry it.
# Empty when it cannot be computed, and the caller then fails open exactly as it
# did when a `rev-parse` failed.
#
# The change is strictly tightening one case while relaxing another that never
# mattered:
#
#   rebase, amend, reword         same change -> holds  (it used to break)
#   origin/main moves, no rebase  same change -> holds  (the merge base is put)
#   any edit to a tracked file    new change  -> stales, as before
#   upstream edits a file this    new change  -> stales, which the sha form
#     branch also touches                        did NOT catch
#
# The last row is the point. A sha-keyed marker survives a rebase that lands the
# branch on top of someone else's edits to the very files it changes, which is
# the case most deserving of a second look. A diff-keyed one does not.
review_key() {
    # The preconditions are checked first, and the diff is then piped *raw*.
    # Capturing it in $( ) first would strip the trailing newline, so this
    # function and the recording command the denial prints - a plain
    # `git diff ... | git hash-object --stdin` - would hash different bytes and
    # the gate could never be satisfied. Same pipeline on both sides, always.
    git -C "$1" rev-parse --verify --quiet "origin/$MAIN_BRANCH" >/dev/null 2>&1 || return 1
    git -C "$1" merge-base "origin/$MAIN_BRANCH" HEAD >/dev/null 2>&1 || return 1
    git -C "$1" diff "origin/$MAIN_BRANCH...HEAD" 2>/dev/null |
        git -C "$1" hash-object --stdin 2>/dev/null
}

# Deny unless marker `$3` in worktree `$1` records exactly the change `$2`
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
    require_key=$2
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
    require_record="git -C \\\"$require_dir\\\" diff origin/$MAIN_BRANCH...HEAD | git hash-object --stdin > \\\"$require_file\\\""

    if [ ! -f "$require_file" ]; then
        deny "\"CLAUDE.md: $require_rule. \`$require_name\` has not been recorded for this change. $require_how, then record it:\n\n  $require_record\""
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
            [ ${#require_reviewed} -eq ${#require_key} ] ||
                require_reviewed="(not a commit id)"
            ;;
    esac
    require_reviewed=$(printf '%s' "$require_reviewed" | tr 'A-F' 'a-f')

    if [ "$require_reviewed" != "$require_key" ]; then
        deny "\"CLAUDE.md: $require_rule. \`$require_name\` was recorded for change $require_reviewed but this branch's diff is now $require_key, so the newest work is ungraded. A rebase, an amend or a reword does not move this value; an edit to a tracked file does. $require_how, then record it again:\n\n  $require_record\""
    fi
}

# --- worktree-guard ---------------------------------------------------------

worktree_guard() {
    # An explicit opt-out for the rare deliberate edit on the main checkout: a
    # hotfix, a repo-hygiene sweep. Set it in the shell before launching.
    if [ "${QUANTICK_ALLOW_MAIN_WRITES:-}" = "1" ]; then
        exit 0
    fi

    paths=$(edit_paths)
    [ -n "$paths" ] || exit 0
    old_ifs=$IFS
    IFS='
'
    for raw in $paths; do
        path=$(absolute_edit_path "$raw")

        # The live goal is agent working state and is deliberately written
        # before the branch artifact exists. Tracked skills and hooks are not
        # exempt: they belong in the worktree like every other repository edit.
        case "$path" in
            */.claude/GOAL.md) continue ;;
        esac

        dir=$(nearest_existing_dir "$path") || continue

        git_dir=$(git -C "$dir" rev-parse --absolute-git-dir 2>/dev/null) || continue
        common_dir=$(git -C "$dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || continue

        # In a linked worktree these differ (<common>/worktrees/<name> vs
        # <common>). Equal means the write lands in the main checkout.
        [ "$git_dir" = "$common_dir" ] || continue

        branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null) || continue
        [ "$branch" = "$MAIN_BRANCH" ] || continue

        deny "\"CLAUDE.md: one goal, one worktree. This write lands in the main checkout while it is on \`$MAIN_BRANCH\`. Create the worktree first:\n\n  git fetch origin\n  git worktree add -b <prefix>/<slug> ../quantick-worktrees/<prefix>-<slug> origin/$MAIN_BRANCH\n\nthen work inside that directory. Set QUANTICK_ALLOW_MAIN_WRITES=1 to override deliberately.\""
    done
    IFS=$old_ifs
    exit 0
}

# --- pr-gate ----------------------------------------------------------------

# The statement inside `$1` that runs `$2`, or nothing. Split exactly as
# `runs_command` splits, because a flag the gate reads must come from the
# statement it matched rather than from anywhere else on the line: a `--draft`
# in a neighbouring `echo` is not a draft PR.
gh_statement() {
    printf '%s' "$1" |
        sed 's/&&/\n/g; s/||/\n/g; s/;/\n/g' |
        grep -E "^[[:space:]]*$2([[:space:]]|$)" |
        head -n 1
}

# The PR number the statement names, or nothing.
#
# A *whole word* of digits, never a digit run pulled out of one. Splitting on
# every non-digit read `--body-file notes2.md 42` as PR 2, and a gate that
# counts the wrong PR's threads is worse than one that counts none: it reports
# a clean number for a branch nobody reviewed.
#
# Nothing is a legitimate answer — `gh pr merge` with no number means "the one
# for this branch" — and the caller treats it as undetermined rather than
# guessing.
pr_number() {
    printf '%s' "$1" |
        tr -s ' \t' '\n\n' |
        grep -E '^[0-9]+$' |
        head -n 1
}

# How many ai-review threads are open on PR `$2`, printed on stdout. Empty when
# the count could not be taken, which is not the same answer as zero and is why
# the caller tells the two apart.
#
# The sibling script is the single owner of what an ai-review thread is: it
# writes the marker when it posts one and reads the same marker when it counts.
# A second definition here would drift, and the first symptom would be a merge
# gate that either ignores real findings or blocks on a human's question.
open_ai_review_threads() {
    threads_script="$1/ai_review_threads.sh"
    [ -f "$threads_script" ] || return 1
    threads_count=$(sh "$threads_script" count "$2" 2>/dev/null) || return 1
    case "$threads_count" in
        '' | *[!0-9]*) return 1 ;;
    esac
    printf '%s' "$threads_count"
}

pr_gate() {
    command=$(json_string_field command)

    # Which of the three the command is, because they are gated differently:
    # `create` opens the PR that carries the findings, while `ready` and
    # `merge` are the two ways work leaves the branch.
    #
    # Tested most-restrictive first, and that order is the rule rather than a
    # preference. A line may run more than one of them — `gh pr create --draft
    # && gh pr ready` is the natural way to end phase one and open phase two —
    # and matching `create` first would hand that line the draft exemption and
    # let the `gh pr ready` beside it through with no review at all.
    if runs_command "$command" "gh pr merge"; then
        gate_action=merge
    elif runs_command "$command" "gh pr ready"; then
        gate_action=ready
    elif runs_command "$command" "gh pr create"; then
        gate_action=create
    else
        exit 0
    fi

    # Phase one ends at a draft PR, and a draft ships nothing: it cannot be
    # merged, and `gh pr ready` below is where the reviews are wanted. Gating
    # it would order the chain backwards — the reviews would have to run before
    # `ai-review` could post its findings onto a PR that does not exist yet.
    #
    # `--draft=false` is spelled out rather than left to the substring match.
    # It is the one spelling that contains the flag and means the opposite of
    # it, and a gate that reads it as a draft opens an ungated real PR.
    if [ "$gate_action" = create ]; then
        gate_statement=$(gh_statement "$command" "gh pr create")
        case "$gate_statement" in
            *--draft=false* | *--draft=0*) ;;
            *' --draft'* | *' -d '* | *' -d') exit 0 ;;
        esac
    fi

    dir=$(effective_dir "$command" "$(normalize_path "$(json_string_field cwd)")")
    [ -d "$dir" ] || exit 0

    key=$(review_key "$dir")
    if [ -z "$key" ]; then
        # The branch's own change cannot be identified - no origin/<main>, an
        # unrelated history. Fall back to the commit, which is what this gate
        # keyed on before diffs. That is strictly stricter than failing open,
        # and a checkout without origin/<main> is outside this workflow anyway:
        # every review in it measures against that ref.
        key=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || exit 0
    fi
    [ -n "$key" ] || exit 0

    # Checked in the order the reviews run. arch-review first: a delivery
    # review of a branch the shape review is about to change is wasted work,
    # and naming the earlier gap first is the shorter path back to green.
    #
    # Required at every tier, `small` included. A tier buys a shorter review,
    # never no review: the bug pass is the last thing a branch should be able
    # to buy its way out of, and a small diff is not the same as a safe one.
    require_marker "$dir" "$key" "$ARCH_MARKER_NAME" \
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

    require_marker "$dir" "$key" "$DELIVERY_MARKER_NAME" \
        "no branch ships ungraded against what was asked for" \
        "$delivery_how"

    # Both reviews are recorded. What is left is the branch's open findings,
    # and only the two commands that actually ship work are held on them: a PR
    # may be created, draft or not, while findings are still open — the PR is
    # where they live.
    [ "$gate_action" = create ] && exit 0

    gate_pr=$(pr_number "$(gh_statement "$command" "gh pr $gate_action")")
    if [ -z "$gate_pr" ]; then
        ask "\"CLAUDE.md: nothing merges with an \`ai-review\` thread open. This \`gh pr $gate_action\` names no PR number, so the open threads could not be counted — the gate is not saying there are none. Re-run it naming the PR, or read the PR's unresolved threads yourself before continuing.\""
    fi

    gate_open=$(open_ai_review_threads "$(dirname "$0")" "$gate_pr")
    if [ -z "$gate_open" ]; then
        ask "\"CLAUDE.md: nothing merges with an \`ai-review\` thread open. The open threads on PR #$gate_pr could not be counted here — \`gh\` missing, unauthenticated or unreachable — so this is not a clean count, it is no count at all. Read the PR's unresolved threads before continuing.\""
    fi

    if [ "$gate_open" -gt 0 ]; then
        deny "\"CLAUDE.md: nothing merges with an \`ai-review\` thread open. PR #$gate_pr has $gate_open. Phase two closes them one at a time, from fresh context and allowed to redesign; each closes by the fix, or by an acceptance the trader records on the thread. List them:\n\n  sh .claude/hooks/ai_review_threads.sh list $gate_pr\""
    fi

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
        # One chain, not three bare `if`s. Those were mutually exclusive only
        # because `context` ends in `exit`, so a fourth message added above - or
        # a `context` that ever printed without exiting - would run
        # `[ "" -le 300 ]`, a POSIX `[` error, and emit three contradictory
        # reminders on separate lines, which is not even parseable JSON.
        if [ -z "$small_size" ]; then
            context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH at the \`small\` tier, but its size against origin/$MAIN_BRANCH cannot be measured here — so the exemption from \`$DELIVERY_MARKER_NAME\` does not apply and \`gh pr create\` wants both markers. origin/$MAIN_BRANCH exists - this message could not print otherwise, since the commit count above was measured from it - so look instead for histories with no merge base, a shallow clone, or a file git cannot read. This is about the measurement, not the size of the work: do not raise the tier over it.\""
        elif [ "$small_size" -le "$SMALL_TIER_MAX_CHANGED_LINES" ]; then
            context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH at the \`small\` tier, so \`gh pr create\` wants \`$ARCH_MARKER_NAME\` alone — recorded for the exact change being shipped, which any later edit stales, though a rebase or an amend does not. It carries $small_size of the $SMALL_TIER_MAX_CHANGED_LINES changed lines the exemption from \`$DELIVERY_MARKER_NAME\` allows.\""
        else
            context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH and has outgrown its \`small\` tier: it carries $small_size changed lines against the $SMALL_TIER_MAX_CHANGED_LINES the exemption allows, so \`gh pr create\` now wants both \`$ARCH_MARKER_NAME\` and \`$DELIVERY_MARKER_NAME\` recorded for the exact change being shipped. Raise the tier in the goal file and run both reviews.\""
        fi
    fi

    context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH. \`gh pr create\` is gated on both \`$ARCH_MARKER_NAME\` and \`$DELIVERY_MARKER_NAME\` recording the exact change being shipped, so run arch-review and then delivery-review once the branch is final — an edit after either one makes its marker stale, though a rebase, an amend or a reword does not.\""
}

# PostToolUse on Edit|Write. Runs the repository guards over the file that was
# just written and reports what they found, so a crossed size ceiling, a
# non-English word or a codepage round-trip surfaces at the edit that caused
# it rather than at the end of a `cargo test --workspace` run minutes later.
#
# Three properties make this safe to run on every write:
#
#   It never blocks. PostToolUse cannot deny — the edit has already landed —
#   and that is the right shape here. The gate stays `cargo test --workspace`;
#   this only moves the *news* earlier.
#
#   It never invokes cargo. It runs the already-built binary straight out of
#   `target/`, so it cannot contend for the build lock with a `cargo build`
#   the agent is running, and cannot silently trigger a four-minute compile of
#   its own. No binary means no output: the guards still run in the suite.
#
#   It reads one file. `--file` checks that path against the baseline instead
#   of walking the repo, which is milliseconds rather than the seconds a full
#   scan costs.
guard_watch_one() {
    file=$(absolute_edit_path "$1")
    # No extension filter here on purpose. One lived here and was a third
    # hand-kept copy of a list the two Rust guards already own; adding an
    # extension there while forgetting it here would have left the suite
    # seeing a file the edit-time hook silently did not — an all-clear that
    # reads exactly like a clean file. Each guard's `check_file` already
    # returns nothing for a path it does not read, so the filter bought a
    # process spawn and cost a drift.
    # An absolute path or nothing. A relative `file_path` would make `dirname`
    # answer `.`, and both git queries below would then resolve against this
    # hook process's own working directory — the main checkout — so the guards
    # would run over a same-named file in a tree the author is not editing
    # while the file they did edit went unchecked.
    case "$file" in
        /* | ?:/*) ;;
        *) return 0 ;;
    esac

    dir=$(dirname "$file")
    [ -d "$dir" ] || return 0

    # Both answers from one process. `--show-toplevel` and `--show-prefix`
    # were two spawns for what one call prints on two lines, and on Windows a
    # spawn costs tens of milliseconds — against a binary that answers in 27.
    # This mode fires on every write in the session, so the shell plumbing had
    # become the dominant cost of the thing built to be cheap.
    location=$(git -C "$dir" rev-parse --show-toplevel --show-prefix 2>/dev/null) || return 0
    root=$(normalize_path "$(printf '%s\n' "$location" | sed -n '1p')")
    prefix=$(printf '%s\n' "$location" | sed -n '2p')

    # The binary the branch already built, under either name this platform
    # gives it. Absent is the ordinary case on a fresh worktree, and silence
    # is the correct answer to it.
    binary="$root/target/debug/quantick-guards"
    [ -x "$binary" ] || binary="$binary.exe"
    [ -x "$binary" ] || return 0

    # Workspace-relative, forward slashes: the spelling the baseline uses.
    # `$prefix` above comes from git rather than from subtracting the root out
    # of the absolute path, because the two are not spelled alike here — the
    # payload carries the path the way the host writes it and
    # `--show-toplevel` answers the way git does, so under Git Bash
    # `/tmp/x/src/a.rs` and `C:/Users/.../Temp/x` describe the same tree and
    # share no prefix. The subtraction silently produced no match, which reads
    # exactly like a clean file.
    relative="$prefix$(basename "$file")"

    # The binary is told which root to read, because the one compiled into it
    # is whichever worktree happened to build it.
    findings=$(QUANTICK_GUARDS_ROOT="$root" "$binary" --file "$relative" 2>&1) && return 0
    [ -n "$findings" ] || return 0

    # JSON-escape by hand, because there is no jq here. Separate `-e` scripts
    # and a `|` delimiter: a `;`-joined script with a `/` delimiter is what
    # this environment's sed rejects, and it rejects it to stderr while the
    # pipeline still exits 0 — which produced an empty message that read as a
    # clean file.
    escaped=$(printf '%s' "$findings" |
        sed -e 's|\\|\\\\|g' -e 's|"|\\"|g' |
        awk 'BEGIN { ORS = "" } { if (NR > 1) printf "\\n"; print }')
    # `$relative` gets the same treatment. It is derived from a filename, so a
    # quote in it would close the JSON string early and the harness would drop
    # the whole payload — leaving the author with the silence that reads as a
    # clean file. Escaping the findings and interpolating the path raw was the
    # same defect `require_marker` goes to some length to avoid on the deny
    # path.
    escaped_path=$(printf '%s' "$relative" | sed -e 's|\\|\\\\|g' -e 's|"|\\"|g')
    context "\"Repository guards on \`$escaped_path\`:\n$escaped\n\nThis is advisory and blocks nothing; \`cargo test --workspace\` is still the gate. Fix it now while the edit is in hand, or run \`cargo run -p quantick-guards -- --tighten\` if a baseline entry only needs lowering.\""
}

guard_watch() {
    paths=$(edit_paths)
    [ -n "$paths" ] || exit 0
    old_ifs=$IFS
    IFS='
'
    for file in $paths; do
        guard_watch_one "$file"
    done
    IFS=$old_ifs
    exit 0
}

case "$mode" in
    worktree-guard) worktree_guard ;;
    pr-gate) pr_gate ;;
    commit-reminder) commit_reminder ;;
    guard-watch) guard_watch ;;
    *) exit 0 ;;
esac
