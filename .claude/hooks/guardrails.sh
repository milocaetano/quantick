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
#   pr-gate           PreToolUse on Bash. Denies a non-draft `gh pr
#                     create`, `gh pr ready` and `gh pr merge` until a
#                     current `code-review-ok` matches the exact change;
#                     an `implement` branch owes it before any PR, draft
#                     included. Ready and merge also want full CI (every
#                     job in FULL_CI_CHECKS) green at the exact HEAD, read
#                     by the sibling `full_ci.sh`: draft pushes run only
#                     the fast job, so readiness is where it is owed.
#   commit-reminder   PostToolUse on Bash. Cannot block (the commit
#                     already landed); says the gate is coming and how to
#                     satisfy it.
#   guard-watch       PostToolUse on Edit|Write. Not a gate: runs the
#                     repository guards over the file just written, using
#                     the already-built binary, and reports. It never
#                     denies and never blocks; `cargo test --workspace`
#                     remains the thing that enforces those guards.
#   review-verdict    PostToolUse on SubagentHandback, and SubagentStop.
#                     Records `code-review-ok` from a review subagent's
#                     verdict line, so the agent never writes it.
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
# The one review an `implement` branch owes, recorded after the code-review
# skill ran over the change and before any PR, draft included, is opened.
IMPLEMENT_MARKER="code-review-ok"

# What ceremony a branch declared, written by the `mission` or `implement`
# skill into the per-branch git dir as `<branch> <tier>` and never
# committed. A declaration naming another branch is no declaration, so a
# reused worktree cannot inherit one. Only `implement` changes the gate.
TIER_FILE_NAME="mission-tier"
# How long the read-cost advisory may delay a command before it is dropped.
# The calculator is fast on an ordinary branch - a third of a second - but it
# walks every changed file's references, and the largest branch in the ledger,
# 408 changed files, takes 24 seconds. That is a stall in front of `gh pr
# create` for a line the pull request comment prints anyway, so past this
# budget the advice is dropped like anything else the advisory cannot
# determine in time. The budget is the whole advisory's, not each
# interpreter's: see `read_cost_advisory`.
READ_COST_ADVISORY_SECONDS=10
# Every tier `mission` may declare, in the order they cost. A file holding
# anything else is treated as no declaration at all: an unrecognised word must
# never be the difference between a graded branch and an ungraded one.
TIERS="small medium high max"
# Not a mission tier: the `implement` skill's goal declares it in the same file,
# and it replaces the mission's reviews with a single code-review.
IMPLEMENT_TIER="implement"

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

# The command with its single leading `cd <dir> &&` removed, trimmed at both
# ends. Everything else is left exactly as written.
#
# The campaign statements below are pinned by string equality, and the function
# above says why every agent command carries that prefix: the payload's `cwd`
# is the session's and resets between calls, so the bare form the pin asks for
# would be judged against the main checkout and denied as a merge to main. The
# pinned form was therefore unreachable from the Bash tool in both directions.
# The prefix is read with the pattern `effective_dir` uses, so the directory
# the gate judges and the directory it strips can never disagree.
#
# Exactly one prefix, and only across `&&`. A second `cd`, a `;`, a `||` or any
# trailing statement survives the strip and fails the equality check that
# follows, so nothing beside the first statement can share its authorization.
bare_statement() {
    bare_command=$(printf '%s' "$1" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
    bare_rest=$(printf '%s' "$bare_command" |
        sed -n 's|^cd[[:space:]][[:space:]]*"\{0,1\}[^"[:space:];&|]\{1,\}"\{0,1\}[[:space:]]*&&[[:space:]]*||p')
    if [ -n "$bare_rest" ]; then
        printf '%s' "$bare_rest"
        return 0
    fi
    printf '%s' "$bare_command"
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
    for tier_known in $TIERS $IMPLEMENT_TIER; do
        if [ "$tier" = "$tier_known" ]; then
            printf '%s' "$tier"
            return 0
        fi
    done
    return 1
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
review_base() {
    base_record=$(marker_path "$1" mission-base) || return 1
    if [ -e "$base_record" ]; then
        sh "$(dirname "$0")/campaign_context.sh" base "$1"
    else
        printf 'origin/%s\n' "$MAIN_BRANCH"
    fi
}

review_key() {
    key_record=$(marker_path "$1" mission-base) || return 1
    if [ -e "$key_record" ]; then
        sh "$(dirname "$0")/campaign_context.sh" key "$1"
        return $?
    fi
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
# Optional `$6` requires a branch-bound, single-line `<branch> <key>` record;
# the existing key validation/comparison below remains the sole comparator.
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
    require_branch=${6:-}

    require_file=$(marker_path "$require_dir" "$require_name") || exit 0
    # `git -C "$require_dir"`, never `git -C .`. The remedy is pasted into a
    # shell whose cwd is the session's — the main checkout — not the worktree
    # being shipped. With `.` the marker lands in the *shared* git dir holding
    # main's HEAD, where a later command resolving to the main checkout would
    # match it, so one paste of the gate's own instruction could switch it off.
    # Every doc on this branch spells the command with the `cd`; the message
    # must not be the one place that drops it.
    require_record="git -C \\\"$require_dir\\\" diff origin/$MAIN_BRANCH...HEAD | git hash-object --stdin > \\\"$require_file\\\""
    if [ -e "$(marker_path "$require_dir" mission-base)" ]; then
        require_record="sh .claude/hooks/campaign_context.sh key \\\"$require_dir\\\" > \\\"$require_file\\\""
    fi
    if [ "$require_name" = "$IMPLEMENT_MARKER" ]; then
        # Claude Code's classifier denies the agent this write; the hook does it.
        require_record="end the review with \`REVIEW-VERDICT: pass $require_dir <HEAD>\` and the review-verdict hook records it. Codex only: $require_record"
    fi
    if [ -n "$require_branch" ]; then
        # Do not offer a bare restamp as the remedy for a review that never ran.
        require_record="Follow the ai-review skill's completion procedure after publishing its PR report and checking review identity stability."
    fi

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
    if [ -n "$require_branch" ]; then
        # Read the entire record: head alone would ignore an appended second
        # record. Branch text never reaches the denial's JSON interpolation.
        require_reviewed=$(cat "$require_file" 2>/dev/null)
        if [ "$(wc -l < "$require_file" | tr -d ' ')" != 1 ]; then
            require_reviewed=""
        fi
        case "$require_reviewed" in
            "$require_branch "*) require_reviewed=${require_reviewed#"$require_branch "} ;;
            *) require_reviewed="" ;;
        esac
    fi
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

# The statement with its quoted spans blanked out, so a flag can be read from
# what the command *does* rather than from what a title or a body says.
#
# This exists because the first version of the draft exemption globbed the
# whole statement, and `gh pr create --title "Make --draft PRs ungated"` then
# opened a real, un-reviewed PR. The file already carries that lesson one
# function up: `runs_command` splits into statements precisely so a commit
# message naming a gated command cannot trip the gate. Reading a *flag* needs
# the same care, and more of it — there the mistake blocks something that
# should pass, here it passes something that should be blocked.
#
# A title with a space in it has to be quoted, so blanking quoted spans covers
# the reachable spellings. It is deliberately not a shell parser: what it
# cannot understand it leaves in place, and anything left in place is read as
# an argument, which errs towards gating.
unquoted_arguments() {
    printf '%s' "$1" | sed 's/"[^"]*"/ /g; s/'"'"'[^'"'"']*'"'"'/ /g'
}

# True when the statement asks for a draft PR, as a whole argument word.
#
# Every uncertainty here resolves to *not a draft*, because the two mistakes
# are not the same size: reading a draft as a real PR costs a permission
# prompt, while reading a real PR as a draft opens it with no review at all.
draft_flag() {
    # An odd count of either quote character means a span this cannot pair
    # off, so `unquoted_arguments` cannot be trusted to have blanked the right
    # text. Refuse rather than guess.
    for draft_quote in '"' "'"; do
        draft_quotes=$(printf '%s' "$1" | tr -cd "$draft_quote" | wc -c)
        [ $((draft_quotes % 2)) -eq 0 ] || return 1
    done

    draft_words=$(unquoted_arguments "$1")

    # `--draft=<value>` is a draft only when gh would read the value as true.
    # gh parses it with Go's ParseBool, whose true spellings are exactly these
    # six — so `--draft=f`, `--draft=F` and `--draft=False` are all *false*,
    # and a gate that reads them as a draft opens a real, un-reviewed PR. Only
    # `false` and `0` were excluded before, which left the other four spellings
    # of the same instruction wide open.
    case " $draft_words " in
        *' --draft=1 '* | *' --draft=t '* | *' --draft=T '* | \
        *' --draft=true '* | *' --draft=TRUE '* | *' --draft=True '*) return 0 ;;
        # Any other explicit value: not a draft, whatever it says.
        *' --draft='*) return 1 ;;
    esac

    case " $draft_words " in
        *' --draft '* | *' -d '*) return 0 ;;
    esac
    return 1
}

# The PR number the statement names, or nothing.
#
# A *whole word* of digits, never a digit run pulled out of one: splitting on
# every non-digit read `--body-file notes2.md 42` as PR 2. And exactly one such
# word, or none — two bare numbers mean the operand cannot be told from a
# flag's value, and a gate that counts the wrong PR's threads is worse than one
# that counts none, because it reports a clean number for a branch nobody
# reviewed. The caller turns "none" into a question rather than a pass.
pr_number() {
    number_words=$(unquoted_arguments "$1" | tr -s ' \t' '\n\n' | grep -E '^[0-9]+$')
    [ "$(printf '%s\n' "$number_words" | grep -c '[0-9]')" -eq 1 ] || return 0
    printf '%s' "$number_words"
}

# --- the read-cost advisory -------------------------------------------------

# Not a gate, and it lives inside `pr-gate` only because this is the moment a
# branch is finished enough for the number to mean something.
#
# `docs/quality/read-cost/ledger.md` records what each merged pull request
# asked a reader to hold, and carries the ceiling those rows produced: the
# median of the feature branches, rounded to the thousand. A feature branch
# over it hears about it once, here, while detaching a reference is still
# cheap, and hears it as news. Nothing on this path can refuse anything.
#
# It carries the ledger's other half too. A pull request records its own row,
# because `main` takes pull requests only and no bot can push to it, so a
# `gh pr ready` or `gh pr merge` naming a pull request with no row is told
# which command writes it. That half is not about the ceiling and needs no
# measurement, so it reaches every branch prefix.
#
# Silence answers everything it cannot determine - no interpreter, no
# calculator in the worktree, no recorded ceiling, a measurement that fails, a
# branch the ceiling was never about. That is not the file's fail-open rule
# being stretched: advice that reports its own plumbing teaches its reader to
# skip the line that matters.
#
# Each interpreter is tried until one *runs*, rather than until one exists.
# On the Windows checkout this hook runs on, `python3` is on PATH as the
# Microsoft Store alias: it resolves, prints "Python was not found" and exits
# 49. Stopping at the first name found meant the trader's own machine — the
# one machine this advice is for — silently never saw it.
read_cost_advisory() {
    advisory_script="$1/tools/read_cost/report.py"
    [ -f "$advisory_script" ] || return 0
    advisory_branch=$(git -C "$1" symbolic-ref --quiet --short HEAD 2>/dev/null) ||
        return 0
    # The pull request, when the command named exactly one. A creation names
    # none, which is the point: there is no row to ask for before the pull
    # request exists.
    advisory_pr=${3:-}

    # The interpreter is chosen before the measurement, not by attempting it.
    # Choosing by attempt gave every interpreter its own budget: a branch that
    # exhausted the first one was killed, read as "that interpreter did not
    # run", and measured again under the second, so a 20-second cap delayed a
    # command by 40.
    advisory_python=
    for advisory_candidate in python3 python; do
        command -v "$advisory_candidate" >/dev/null 2>&1 || continue
        "$advisory_candidate" -c "" >/dev/null 2>&1 || continue
        advisory_python=$advisory_candidate
        break
    done
    [ -n "$advisory_python" ] || return 0

    advisory_limit=
    command -v timeout >/dev/null 2>&1 &&
        advisory_limit="timeout $READ_COST_ADVISORY_SECONDS"

    if [ -n "$advisory_pr" ]; then
        $advisory_limit "$advisory_python" "$advisory_script" warn \
            --repo "$1" --base "$2" --head HEAD \
            --branch "$advisory_branch" --pr "$advisory_pr" 2>/dev/null
    else
        $advisory_limit "$advisory_python" "$advisory_script" warn \
            --repo "$1" --base "$2" --head HEAD \
            --branch "$advisory_branch" 2>/dev/null
    fi
    return 0
}

# Leave the gate without a decision, carrying whatever the advisory has to
# say. `systemMessage` is what a person sees and `additionalContext` is what
# the session reads; both hold the same sentence, and neither is a permission
# decision, so the normal flow continues exactly as it did before.
#
# The measurement happens here, on the way past, rather than when the gate
# starts. A denial says its own thing and the advice returns on the attempt
# that passes, so measuring earlier would only put the calculator's seconds in
# front of a command that was about to be refused anyway.
#
# An invalid review base is the gate's business, not the advisory's: with
# nothing to measure against it simply says nothing.
pass_pr_gate() {
    if pass_base=$(review_base "$1" 2>/dev/null); then
        pass_note=$(read_cost_advisory "$1" "$pass_base" "${2:-}")
        if [ -n "$pass_note" ]; then
            printf '{"systemMessage":%s,"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":%s}}\n' \
                "$pass_note" "$pass_note"
        fi
    fi
    exit 0
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
    # `--draft=false` is spelled out rather than left to the match. It is the
    # one spelling that contains the flag and means the opposite of it, and a
    # gate that reads it as a draft opens an ungated real PR.
    dir=$(effective_dir "$command" "$(normalize_path "$(json_string_field cwd)")")
    [ -d "$dir" ] || exit 0

    # An `implement` branch trades the mission's reviews for one code-review,
    # owed before any PR opens, draft included. Green CI and the user's own
    # merge are what protect `main` past it. Against `main`, or a feat/* or
    # feature/* branch recorded with no parent issue; a campaign or /feature
    # base keeps its own authorization and identity checks in the full gate.
    implement_base=$(review_base "$dir" 2>/dev/null) || implement_base=
    if [ "$(declared_tier "$dir")" = "$IMPLEMENT_TIER" ] &&
        { [ "$implement_base" = "origin/$MAIN_BRANCH" ] ||
            sh "$(dirname "$0")/campaign_context.sh" implement-target "$dir" >/dev/null 2>&1; }; then
        implement_target=${implement_base#origin/}
        # `gh pr create` defaults to `main`, which would carry the target's
        # whole stack into a PR nobody meant to open. Read from the unquoted
        # words, one value exactly: `gh` takes the last of several, and a title
        # may name the flag. Whichever verb ranks highest on the line.
        if [ "$implement_target" != "$MAIN_BRANCH" ] &&
            runs_command "$(unquoted_arguments "$command")" "gh pr create"; then
            implement_named=$(unquoted_arguments "$(gh_statement "$command" "gh pr create")" |
                sed 's/\\[nrt]/ /g' |
                tr -s ' \t\r\n\\' '\n' |
                awk 'named { print; named = 0; next }
                    /^(--base|-B)$/ { named = 1; next }
                    /^--base=/ { sub(/^--base=/, ""); print; next }
                    /^-B./ { sub(/^-B/, ""); print }')
            [ "$implement_named" = "$implement_target" ] ||
                deny "\"This branch targets $implement_target: run \`gh pr create --base $implement_target\`, once and unquoted, with the text in \`--body-file\`.\""
        fi
        implement_key=$(review_key "$dir")
        if [ -z "$implement_key" ]; then
            [ "$implement_target" = "$MAIN_BRANCH" ] ||
                deny '"Review identity against the target cannot be computed; no fallback approval is available."'
            implement_key=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || exit 0
        fi
        require_marker "$dir" "$implement_key" "$IMPLEMENT_MARKER" \
            "an implement branch opens its PR only after code-review" \
            "Run the code-review skill over \`git diff $implement_base...HEAD\` and fix its valid findings"
        if [ "$gate_action" = merge ]; then
            # The trader may grant agent merges into a named non-main target by
            # listing it, one branch per line, in the common git dir's
            # `agent-merge-grant`. `main` is never grantable. A granted merge
            # still needs every PR check passing or skipped.
            implement_grant="$(git -C "$dir" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)/agent-merge-grant"
            if [ "$implement_target" != "$MAIN_BRANCH" ] && [ -f "$implement_grant" ] &&
                grep -qxF "$implement_target" "$implement_grant"; then
                implement_pr=$(pr_number "$(gh_statement "$command" "gh pr merge")")
                implement_checks=$(cd "$dir" && gh pr checks "$implement_pr" --json bucket --jq '.[].bucket' 2>/dev/null) ||
                    deny '"Cannot read the PR checks for the granted merge."'
                [ -n "$implement_checks" ] && [ -z "$(printf '%s\n' "$implement_checks" | grep -v -e '^pass$' -e '^skipping$')" ] ||
                    deny '"Granted merge waits for every PR check to pass."'
                exit 0
            fi
            deny "\"Merge to $implement_target is reserved exclusively for the user; do not enable auto-merge or enqueue it.\""
        fi
        # A reused worktree may hold a PR into another base, and this review
        # covered only the diff against the target.
        if [ "$gate_action" = ready ] && [ "$implement_target" != "$MAIN_BRANCH" ]; then
            implement_pr=$(pr_number "$(gh_statement "$command" "gh pr ready")")
            sh "$(dirname "$0")/campaign_context.sh" check-pr "$dir" "$implement_pr" ready >/dev/null 2>&1 ||
                deny "\"Name one PR number whose base is $implement_target and whose head is this reviewed branch, after a fetch.\""
        fi
        pass_pr_gate "$dir"
    fi

    if [ "$gate_action" = create ]; then
        gate_statement=$(gh_statement "$command" "gh pr create")
        if draft_flag "$gate_statement"; then
            pass_pr_gate "$dir"
        fi
    fi

    gate_base=$(review_base "$dir") || deny '"Campaign review base is invalid or unavailable; reconcile the branch-bound mission-base record."'
    key=$(review_key "$dir")
    if [ -z "$key" ]; then
        [ "$gate_base" = "origin/$MAIN_BRANCH" ] || deny '"Campaign review identity cannot be computed; no fallback approval is available."'
        # The branch's own change cannot be identified - no origin/<main>, an
        # unrelated history. Fall back to the commit, which is what this gate
        # keyed on before diffs. That is strictly stricter than failing open,
        # and a checkout without origin/<main> is outside this workflow anyway:
        # every review in it measures against that ref.
        key=$(git -C "$dir" rev-parse HEAD 2>/dev/null) || exit 0
    fi
    [ -n "$key" ] || exit 0

    # Every branch owes what an implement branch owes: the code-review at this
    # exact change. The arch-review, delivery-review and ai-review markers are
    # retired; full CI and the merge rules below still apply. A /feature child
    # integrates under its persisted grant; campaign/* keeps its identity check.
    require_marker "$dir" "$key" "$IMPLEMENT_MARKER" \
        "no branch ships without a code-review" \
        "Run the code-review skill over \`git diff $gate_base...HEAD\` and fix its valid findings"
    [ "$gate_action" = create ] && pass_pr_gate "$dir"
    gate_pr=$(pr_number "$(gh_statement "$command" "gh pr $gate_action")")
    [ -n "$gate_pr" ] || deny '"Name the PR number explicitly."'

    # Draft pushes run only the fast job, so readiness is where full CI is
    # owed: every FULL_CI_CHECKS verdict at the exact reviewed HEAD. The
    # sibling script is the one definition the final verifier reads too.
    full_ci_script="$(dirname "$0")/full_ci.sh"
    [ -f "$full_ci_script" ] ||
        ask '"Full CI at the exact head cannot be verified because full_ci.sh is not beside the gate; the fast draft job is not readiness evidence."'
    full_ci_reason=$(sh "$full_ci_script" verify "$dir" 2>&1 >/dev/null)
    full_ci_status=$?
    if [ "$full_ci_status" -eq 2 ]; then
        ask "\"Full CI at the exact head could not be read from GitHub; unknown is not green. $full_ci_reason\""
    elif [ "$full_ci_status" -ne 0 ]; then
        deny "\"Full final-head CI gates \`gh pr $gate_action\`: $full_ci_reason The draft's fast job is a signal, not the verdict. Run full CI on this head with \`gh pr edit $gate_pr --add-label full-ci\`, wait for every full-CI job to pass, then retry.\""
    fi

    if [ "$gate_action" = merge ]; then
        [ "$gate_base" != "origin/$MAIN_BRANCH" ] || deny '"Merge to main is reserved exclusively for the user; do not enable auto-merge or enqueue it."'
        # Only this explicit, head-pinned form is supported, bare or behind the
        # single `cd <worktree> &&` that reaches the task worktree. This also
        # rejects --admin, --auto, alternate repositories and merge-queue
        # shortcuts.
        gate_merge=$(gh_statement "$command" "gh pr merge" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
        gate_whole=$(bare_statement "$command")
        [ "$gate_merge" = "$gate_whole" ] || deny '"Run one campaign merge command from the task worktree; compound commands cannot share an authorization check."'
        gate_head=$(git -C "$dir" rev-parse HEAD)
        case "$gate_merge" in
            "gh pr merge $gate_pr --merge --match-head-commit $gate_head"|"gh pr merge $gate_pr --squash --match-head-commit $gate_head") ;;
            *) deny '"Campaign merges require an explicit PR, --merge or --squash, and --match-head-commit with the reviewed HEAD; no auto-merge, admin override or alternate repository."' ;;
        esac
    fi
    if [ "$gate_base" != "origin/$MAIN_BRANCH" ]; then
        if [ "$gate_action" = ready ]; then
            gate_ready=$(bare_statement "$command")
            [ "$gate_ready" = "gh pr ready $gate_pr" ] || deny '"Run one explicit campaign ready command in the task worktree, without repository overrides or compound commands."'
        fi
        if ! sh "$(dirname "$0")/campaign_context.sh" check-pr "$dir" "$gate_pr" "$gate_action" >/dev/null 2>&1; then
            deny '"Campaign PR identity, authorization or CI could not be verified. Reconcile its exact base/head, persisted merge grant and green checks before continuing."'
        fi
    fi

    pass_pr_gate "$dir" "$gate_pr"
}

# --- commit-reminder --------------------------------------------------------

commit_reminder() {
    command=$(json_string_field command)
    runs_command "$command" "git commit" || exit 0

    dir=$(effective_dir "$command" "$(normalize_path "$(json_string_field cwd)")")
    [ -d "$dir" ] || exit 0

    reminder_base=$(review_base "$dir") || exit 0
    branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null) || exit 0
    [ "$branch" != "$MAIN_BRANCH" ] || exit 0

    ahead=$(git -C "$dir" rev-list --count "$reminder_base..HEAD" 2>/dev/null) || exit 0
    [ "${ahead:-0}" -gt 0 ] || exit 0

    if [ "$(declared_tier "$dir")" = "$IMPLEMENT_TIER" ]; then
        context "\"Branch \`$branch\` is $ahead commit(s) ahead of $reminder_base at the \`implement\` tier, so opening its PR needs a current \`$IMPLEMENT_MARKER\` from the code-review skill.\""
    fi

    context "\"Branch \`$branch\` is $ahead commit(s) ahead of $reminder_base. A draft PR opens freely; a non-draft PR, \`gh pr ready\` and \`gh pr merge\` need a current \`$IMPLEMENT_MARKER\` from the code-review skill.\""
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

# --- review-verdict ---------------------------------------------------------

# A background code-review hands its report back through SubagentHandback; a
# foreground fork ends on it as the subagent's last message. The main agent's
# own text is never read; a subagent could still write the line, so this is
# workflow evidence like every marker, not a boundary. The report's last
# `REVIEW-VERDICT: pass|fail <worktree> <reviewed HEAD>` counts only while that
# worktree sits at that HEAD, whatever its tier, since pr-gate asks every
# branch for the marker: `pass` writes
# `review_key`, the value pr-gate compares, so a later commit leaves it stale;
# `fail` removes the record. Anything else changes nothing.
review_verdict() {
    verdict_flat=$(printf '%s' "$input" | tr -d '\r\n')
    if [ "$(json_string_field hook_event_name)" = SubagentStop ]; then
        verdict_report=${verdict_flat#*\"last_assistant_message\"}
    elif [ "$(json_string_field tool_name)" = SubagentHandback ]; then
        verdict_report=${verdict_flat#*\"tool_input\"}
    else
        exit 0
    fi
    [ "$verdict_report" != "$verdict_flat" ] || exit 0

    # JSON spells C:\src\x as C:\\src\\x: one slash first, so a path segment
    # starting with n never reads as a \n line break.
    verdict=$(printf '%s' "$verdict_report" |
        sed 's/\\\\/\//g; s/\\[nrt]/\n/g' |
        sed -n 's/^.*REVIEW-VERDICT:[[:space:]*_]*\(pass\|fail\)[*_]*[[:space:]]\{1,\}["`]*\([^"`[:space:]]\{1,\}\)["`]*[[:space:]]\{1,\}\([0-9a-fA-F]\{7,40\}\).*$/\1 \2 \3/p' |
        tail -n 1)
    [ -n "$verdict" ] || exit 0
    set -f
    # shellcheck disable=SC2086 # three words, none of them globbed
    set -- $verdict
    set +f
    verdict_reviewed=$(printf '%s' "$3" | tr 'A-F' 'a-f')
    dir=$2
    [ -d "$dir" ] || exit 0
    case "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" in
        "$verdict_reviewed"*) ;;
        *) exit 0 ;;
    esac
    verdict_file=$(marker_path "$dir" "$IMPLEMENT_MARKER") || exit 0
    if [ "$1" = fail ]; then
        rm -f "$verdict_file"
        exit 0
    fi
    verdict_key=$(review_key "$dir")
    [ -n "$verdict_key" ] || exit 0
    printf '%s\n' "$verdict_key" > "$verdict_file"
    exit 0
}

case "$mode" in
    worktree-guard) worktree_guard ;;
    pr-gate) pr_gate ;;
    commit-reminder) commit_reminder ;;
    guard-watch) guard_watch ;;
    review-verdict) review_verdict ;;
    *) exit 0 ;;
esac
