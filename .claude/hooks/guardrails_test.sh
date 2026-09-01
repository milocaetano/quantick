#!/bin/sh
# Tests for .claude/hooks/guardrails.sh.
#
# The guard-rails are the only thing standing between a long agent session and
# a commit on the wrong checkout, so they need the same bar as the rest of the
# repo: a test that fails without the behaviour. Run from anywhere:
#
#   sh .claude/hooks/guardrails_test.sh
#
# Hermetic, with one stated exception: the hook cases build throwaway git repos
# under a temp dir and remove them, but the final block reads this repository's
# own instruction files, because the agreement between the script's marker
# names and the prose that tells an agent to write them is itself under test.
# That block is marked where it starts.
#
# POSIX sh, no jq, so it runs under Git Bash on Windows and dash in CI.

set -u

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
GUARDRAILS="$script_dir/guardrails.sh"

passed=0
failed=0

# The tier vocabulary, read out of guardrails.sh rather than written here.
# Restating the file name or the ceiling would make this suite a second source
# of truth for both, and a test that agrees with itself about a renamed file
# proves nothing. Empty means the constant moved, which is a failure loud
# enough to stop: every tier case below would otherwise pass vacuously.
tier_file_name=$(sed -n 's/^TIER_FILE_NAME="\([^"]*\)".*/\1/p' "$GUARDRAILS")
tiers=$(sed -n 's/^TIERS="\([^"]*\)".*/\1/p' "$GUARDRAILS")
small_ceiling=$(sed -n 's/^SMALL_TIER_MAX_CHANGED_LINES=\([0-9][0-9]*\).*/\1/p' "$GUARDRAILS")

# Loud, and deliberately not fatal. Stopping here would hide whatever else a
# rename broke behind one line of output, and the whole suite reporting a
# single failure reads like a small problem. The substitutes below are chosen
# to keep every later case running and failing honestly: a file name nothing
# will ever match, one tier, and a ceiling of 1. `set_tier` must never be
# handed an empty name - it would build a path ending in a bare slash.
#
# The ceiling substitute is deliberately not the real default. Writing 300 here
# would put a second copy of a value this file exists to read from one place,
# and it would go stale the first time the real one moved - while looking, to
# the next reader, like the authority. All this path needs is a number the
# fixture can build a branch around.
if [ -z "$tier_file_name" ] || [ -z "$tiers" ] || [ -z "$small_ceiling" ]; then
    printf 'FAIL the tier constants are not readable from guardrails.sh\n'
    printf '  file=%s tiers=%s ceiling=%s\n' "$tier_file_name" "$tiers" "$small_ceiling"
    failed=$((failed + 1))
    tier_file_name="mission-tier-unreadable"
    tiers="small"
    small_ceiling=1
fi

# --- fixture ----------------------------------------------------------------

root=$(mktemp -d)
trap 'git -C "$root/mainco" worktree remove --force "$root/wt" >/dev/null 2>&1; git -C "$root/mainco" worktree remove --force "$root/big" >/dev/null 2>&1; git -C "$root/mainco" worktree remove --force "$root/binary" >/dev/null 2>&1; git -C "$root/mainco" worktree remove --force "$root/paperwork" >/dev/null 2>&1; rm -rf "$root"' EXIT

git init -b main -q "$root/mainco"
git -C "$root/mainco" config user.email t@t
git -C "$root/mainco" config user.name t
# The fixture, not the harness, is where the noise belongs: without this,
# git's "LF will be replaced by CRLF" warnings on Windows reach the
# captured output and fail correct cases. Discarding stderr in `run`
# instead would hide every diagnostic guardrails.sh ever emits.
git -C "$root/mainco" config core.autocrlf false
git -C "$root/mainco" config core.safecrlf false
mkdir -p "$root/mainco/src" "$root/mainco/.claude"
echo one > "$root/mainco/src/a.txt"
git -C "$root/mainco" add -A
git -C "$root/mainco" commit -qm "first"
# A remote-tracking ref without a remote: commit-reminder measures against it.
git -C "$root/mainco" update-ref refs/remotes/origin/main HEAD

git -C "$root/mainco" worktree add -q -b feat/x "$root/wt" >/dev/null 2>&1
echo two > "$root/wt/src/a.txt"
git -C "$root/wt" add -A
git -C "$root/wt" commit -qm "second"

head_sha=$(git -C "$root/wt" rev-parse HEAD)
wt_git_dir=$(git -C "$root/wt" rev-parse --absolute-git-dir)

# A second branch, one line fatter than the `small` tier's ceiling, so the
# bound on the exemption is tested against a diff git actually measures rather
# than a number the suite asserts about itself. Sized from the script's own
# constant: a hardcoded 400 here would go quietly vacuous the day the ceiling
# moved past it, and a ceiling case that cannot fail is worse than none.
git -C "$root/mainco" worktree add -q -b feat/big "$root/big"
big_line=0
: > "$root/big/src/big.txt"
while [ "$big_line" -le "$small_ceiling" ]; do
    echo "line $big_line" >> "$root/big/src/big.txt"
    big_line=$((big_line + 1))
done
git -C "$root/big" add -A
git -C "$root/big" commit -qm "over the ceiling"
big_sha=$(git -C "$root/big" rev-parse HEAD)
big_git_dir=$(git -C "$root/big" rev-parse --absolute-git-dir)
big_changed=$((small_ceiling + 1))

# A failed `worktree add` leaves this empty, and an empty git dir turns every
# helper below into a write at the filesystem root - `> "/mission-tier"`,
# `rm -f "/arch-review-ok"`. The helpers refuse an empty argument as well; this
# is the louder half, because it names which fixture went missing.
if [ -z "$big_git_dir" ]; then
    printf 'FAIL the over-ceiling fixture worktree was not created\n'
    failed=$((failed + 1))
fi

# A small branch carrying a binary file. numstat reports `-` for both counts,
# and reading that as "unmeasurable" made a single icon void the exemption and
# blame a broken git for it. The text edit stays well under the ceiling.
git -C "$root/mainco" worktree add -q -b feat/binary "$root/binary"
printf 'changed\n' > "$root/binary/src/a.txt"
printf 'PNG\000\001\002binary\000payload\n' > "$root/binary/src/logo.png"
git -C "$root/binary" add -A
git -C "$root/binary" commit -qm "a fix and an icon"
binary_sha=$(git -C "$root/binary" rev-parse HEAD)
binary_git_dir=$(git -C "$root/binary" rev-parse --absolute-git-dir)
if [ -z "$binary_git_dir" ]; then
    printf 'FAIL the binary-asset fixture worktree was not created\n'
    failed=$((failed + 1))
fi

# A two-line change carrying a goal archive larger than the whole ceiling.
# `mission` requires that archive as the branch's *last* commit, and this repo
# has produced one bigger than the ceiling itself - so counting it would push a
# genuinely small mission out of its own tier by the paperwork the tier obliged
# it to write, with a denial telling it to make an escalation the skill calls
# irreversible.
git -C "$root/mainco" worktree add -q -b feat/paperwork "$root/paperwork"
mkdir -p "$root/paperwork/.claude"
printf 'changed\n' > "$root/paperwork/src/a.txt"
paper_line=0
: > "$root/paperwork/.claude/GOAL-archive-paperwork.md"
while [ "$paper_line" -le "$small_ceiling" ]; do
    echo "line $paper_line" >> "$root/paperwork/.claude/GOAL-archive-paperwork.md"
    paper_line=$((paper_line + 1))
done
git -C "$root/paperwork" add -A
git -C "$root/paperwork" commit -qm "a two-line fix and its goal archive"
paperwork_git_dir=$(git -C "$root/paperwork" rev-parse --absolute-git-dir)
if [ -z "$paperwork_git_dir" ]; then
    printf 'FAIL the goal-archive fixture worktree was not created\n'
    failed=$((failed + 1))
fi

# A repository with no `origin/main` at all, for the fail-closed path: the one
# place guardrails.sh deliberately breaks its own fail-open rule, and until now
# the only branch of `changed_lines` no case reached.
git init -b main -q "$root/noremote"
git -C "$root/noremote" config user.email t@t
git -C "$root/noremote" config user.name t
git -C "$root/noremote" config core.autocrlf false
git -C "$root/noremote" config core.safecrlf false
echo one > "$root/noremote/a.txt"
git -C "$root/noremote" add -A
git -C "$root/noremote" commit -qm "first"
git -C "$root/noremote" checkout -q -b feat/unmeasurable
echo two > "$root/noremote/a.txt"
git -C "$root/noremote" add -A
git -C "$root/noremote" commit -qm "second"
noremote_sha=$(git -C "$root/noremote" rev-parse HEAD)
noremote_git_dir=$(git -C "$root/noremote" rev-parse --absolute-git-dir)

# marker_key <worktree> — the value a marker must hold for that branch: the
# hash of its own diff against origin/main, falling back to the commit when
# that cannot be computed. This restates guardrails.sh's `review_key`, and is
# called out as a second copy: the alternative is sourcing a script whose first
# act is to read stdin and whose last is to dispatch on $1. Every case below
# fails loudly if the two ever disagree, which is the property that matters.
marker_key() {
    # The preconditions come first, exactly as `review_key` checks them. A bare
    # pipe would hand hash-object an empty stream when `git diff` fails and
    # yield the empty-blob hash - a well-formed value that is not the fallback
    # the hook uses, so every no-remote case would fail for the wrong reason.
    if git -C "$1" rev-parse --verify --quiet origin/main >/dev/null 2>&1 &&
        git -C "$1" merge-base origin/main HEAD >/dev/null 2>&1; then
        git -C "$1" diff "origin/main...HEAD" 2>/dev/null |
            git -C "$1" hash-object --stdin 2>/dev/null
    else
        git -C "$1" rev-parse HEAD 2>/dev/null
    fi
}

# --- harness ----------------------------------------------------------------

# run <name> <mode> <stdin-json> <expect: deny|context|silent> [substring]
#
# The optional fifth argument asserts the output carries a given string —
# which marker the denial names, say. It is a parameter rather than a second
# helper because a second helper skipped the exit-status and payload-shape
# checks, and six of the eight pr-gate cases ran through it, including the
# corrupt-marker case whose whole point is that the payload stays parseable.
run() {
    name=$1
    mode=$2
    payload=$3
    expect=$4
    want=${5:-}

    # stderr is captured on purpose. It is the only channel guardrails.sh
    # has for explaining itself, and a diagnostic it prints while still
    # emitting a well-formed decision is a defect the suite should see.
    # The fixture silences git's own CRLF warnings above so this stays
    # signal rather than noise.
    out=$(printf '%s' "$payload" | sh "$GUARDRAILS" "$mode" 2>&1)
    status=$?

    actual=silent
    case "$out" in
        *'"permissionDecision":"deny"'*) actual=deny ;;
        *'"additionalContext"'*) actual=context ;;
    esac

    if [ "$status" -ne 0 ]; then
        printf 'FAIL %s: exited %s (guard-rails must always exit 0)\n' "$name" "$status"
        failed=$((failed + 1))
        return
    fi
    if [ "$actual" != "$expect" ]; then
        printf 'FAIL %s: expected %s, got %s\n  output: %s\n' "$name" "$expect" "$actual" "$out"
        failed=$((failed + 1))
        return
    fi

    # The payload has to be JSON the harness can parse, and every mode emits it
    # on one line. A raw newline inside a JSON string value is an invalid
    # control character: the decision is discarded and the normal permission
    # flow takes over, which for `pr-gate` means the PR opens. Checking only
    # for the substring `"deny"` cannot see that — a gate that had silently
    # stopped denying still reported every case green.
    if [ -n "$out" ]; then
        if [ "$(printf '%s' "$out" | wc -l)" -ne 0 ]; then
            printf 'FAIL %s: payload spans multiple lines, so it is not parseable JSON\n  output: %s\n' "$name" "$out"
            failed=$((failed + 1))
            return
        fi
        case "$out" in
            '{'*'}') ;;
            *)
                printf 'FAIL %s: payload is not a JSON object\n  output: %s\n' "$name" "$out"
                failed=$((failed + 1))
                return
                ;;
        esac
    fi

    if [ -n "$want" ]; then
        case "$out" in
            *"$want"*) ;;
            *)
                printf 'FAIL %s: output did not carry "%s"\n  output: %s\n' "$name" "$want" "$out"
                failed=$((failed + 1))
                return
                ;;
        esac
    fi

    passed=$((passed + 1))
}

# set_marker_in <git-dir> <marker-name> <sha, or empty to remove it>
set_marker_in() {
    if [ -z "$1" ]; then
        printf 'FAIL set_marker_in called with no git dir, which would write at the filesystem root\n'
        failed=$((failed + 1))
        return
    fi
    if [ -z "$3" ]; then
        rm -f "$1/$2"
    else
        printf '%s\n' "$3" > "$1/$2"
    fi
}

# set_marker <marker-name> <sha, or empty to remove it>, on the default
# worktree. Every pre-existing case speaks through this one.
set_marker() { set_marker_in "$wt_git_dir" "$1" "$2"; }

# set_tier <worktree> <tier, or empty to remove the declaration>. Takes the
# worktree rather than its git dir, because the declaration now names the
# branch it belongs to and only the worktree knows which that is. The file name
# comes from the script, so a rename there fails the suite here rather than
# leaving it testing a file nothing reads.
set_tier() {
    set_tier_git_dir=$(git -C "$1" rev-parse --absolute-git-dir 2>/dev/null)
    if [ -z "$set_tier_git_dir" ]; then
        printf 'FAIL set_tier could not resolve a git dir for %s\n' "$1"
        failed=$((failed + 1))
        return
    fi

    if [ -z "$2" ]; then
        rm -f "$set_tier_git_dir/$tier_file_name"
    else
        printf '%s %s\n' "$(git -C "$1" rev-parse --abbrev-ref HEAD)" "$2" \
            > "$set_tier_git_dir/$tier_file_name"
    fi
}

json_path() { printf '{"tool_name":"Write","tool_input":{"file_path":"%s"}}' "$1"; }
json_bash() { printf '{"tool_name":"Bash","cwd":"%s","tool_input":{"command":"%s"}}' "$1" "$2"; }

# --- worktree-guard ---------------------------------------------------------

run "write into the main checkout on main is denied" \
    worktree-guard "$(json_path "$root/mainco/src/a.txt")" deny

run "a file that does not exist yet is still denied" \
    worktree-guard "$(json_path "$root/mainco/src/deep/new.txt")" deny

run "agent working files under .claude are allowed" \
    worktree-guard "$(json_path "$root/mainco/.claude/GOAL.md")" silent

run "write into a linked worktree is allowed" \
    worktree-guard "$(json_path "$root/wt/src/a.txt")" silent

run "a payload without file_path fails open" \
    worktree-guard '{"tool_name":"Bash","tool_input":{"command":"ls"}}' silent

run "a path outside any repo fails open" \
    worktree-guard "$(json_path "$root/loose.txt")" silent

out=$(printf '%s' "$(json_path "$root/mainco/src/a.txt")" |
    QUANTICK_ALLOW_MAIN_WRITES=1 sh "$GUARDRAILS" worktree-guard)
if [ -z "$out" ]; then
    passed=$((passed + 1))
else
    printf 'FAIL the env override is honoured\n  output: %s\n' "$out"
    failed=$((failed + 1))
fi

git -C "$root/mainco" checkout -q -b fix/y
run "the main checkout on another branch is allowed" \
    worktree-guard "$(json_path "$root/mainco/src/a.txt")" silent
git -C "$root/mainco" checkout -q main

# --- pr-gate ----------------------------------------------------------------

#
# Two markers gate the PR and both must record the exact commit being shipped.
# The cases move one marker at a time, so a regression says which half broke
# rather than only that the gate stopped working.

# Derived, not literal: `require_marker` grades any marker whose length
# differs from HEAD's as "(not a commit id)", so a hardcoded 40 zeroes
# would take the corrupt path instead of the staleness path in a repo
# using a longer hash — still green, testing nothing it claims to.
stale_sha=$(printf '%*s' "${#head_sha}" '' | tr ' ' 0)

# What each fixture's markers must hold. A function of the diff, not of the
# commit that carries it.
wt_key=$(marker_key "$root/wt")
big_key=$(marker_key "$root/big")
binary_key=$(marker_key "$root/binary")
noremote_key=$(marker_key "$root/noremote")

if [ -z "$wt_key" ]; then
    printf 'FAIL the fixture worktree has no review key, so no marker case is meaningful\n'
    failed=$((failed + 1))
fi

# Rewording a commit must not move the key. That is the whole reason it is a
# diff hash rather than a commit id, and without this case the marker could
# regress to keying on the sha with every other case still green.
git -C "$root/wt" commit -q --amend -m "second, reworded"
if [ "$(marker_key "$root/wt")" = "$wt_key" ]; then
    passed=$((passed + 1))
else
    printf 'FAIL rewording a commit moved the review key, so the marker is not rebase-safe\n'
    failed=$((failed + 1))
fi
head_sha=$(git -C "$root/wt" rev-parse HEAD)

set_marker arch-review-ok ""
set_marker delivery-review-ok ""

run "a bash command that is not gh pr create is ignored" \
    pr-gate "$(json_bash "$root/wt" "cargo test --workspace")" silent

run "gh pr create without a recorded review is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny

# With neither recorded, the denial names arch-review — the review that runs
# first, because a delivery review of a branch the shape review is about to
# change is wasted work. The script states that order as a contract; without
# this assertion the two `require_marker` calls could be swapped with every
# case still green, and an agent starting a fresh branch would be sent to the
# conformance review first.
run "with neither review recorded the gate names arch-review first" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "arch-review-ok"

# Staleness, one marker at a time. Each keeps the *other* marker at HEAD, so
# the case can only pass through the staleness branch it aims at — with the
# other missing, every one of them would deny for the wrong reason and prove
# nothing about staleness at all.
set_marker arch-review-ok "$stale_sha"
set_marker delivery-review-ok "$(marker_key "$root/wt")"
run "an arch review recorded for an older change is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "arch-review-ok"

set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok "$stale_sha"
run "a delivery review recorded for an older change is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"

# A marker holding something other than a sha must trip the gate, not break it:
# `deny` interpolates the contents into JSON, and a payload the harness cannot
# parse loses the decision and lets the command through. The delivery marker is
# parked at HEAD so the arch marker is the only thing left to complain about.
set_marker delivery-review-ok "$(marker_key "$root/wt")"
printf 'he said "hi"\nsecond line\n' > "$wt_git_dir/arch-review-ok"
run "a corrupt marker is reported as not a commit id" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny '(not a commit id)'

# Absence, one marker at a time. Each pins that the *other* being satisfied
# does not carry the branch through.
set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok ""
run "arch-review alone does not open the PR" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"

set_marker arch-review-ok ""
set_marker delivery-review-ok "$(marker_key "$root/wt")"
run "delivery-review alone does not open the PR" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "arch-review-ok"

set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok "$(marker_key "$root/wt")"
run "both reviews recorded for the exact change is allowed" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" silent

# The script is tool-agnostic: it reads the payload's `command` field and does
# not care which tool produced it. What decides coverage is the matcher in
# `.claude/settings.json`, and that names `Bash` only.
#
# PowerShell is deliberately NOT gated, though it is this machine's primary
# shell. Adding it to the matcher was tried on this branch and reverted: that
# tool's commands carry no leading `cd` (its own contract forbids one), so
# `effective_dir` fell back to the session cwd and the gate judged the main
# checkout — and the remedy it then printed pointed at the shared git dir,
# which would have opened the gate for every branch. Gating a shell means
# teaching the parser that shell; until then this case pins only that the
# script itself is indifferent to the tool name.
set_marker arch-review-ok ""
set_marker delivery-review-ok ""
run "the script judges the payload, not the tool that produced it" \
    pr-gate "$(printf '{"tool_name":"PowerShell","cwd":"%s","tool_input":{"command":"gh pr create --fill"}}' "$root/wt")" deny

# --- pr-gate: the small tier ------------------------------------------------
#
# `small` is the only tier that changes what the gate requires, so it is the
# only tier that needs cases. They fix both directions - the exemption is
# granted where it was earned and refused everywhere else - and then the
# property the whole design rests on: a branch that never asked for the
# exemption is never told that one exists.

set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok ""

set_tier "$root/wt" small
run "a small mission opens its PR on arch-review alone" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" silent

# The tier buys a shorter review, never no review. Without this case the
# exemption could be widened to cover both markers with every other case still
# green, and the bug pass is the last thing a branch should buy its way out of.
set_marker arch-review-ok ""
run "a small mission still cannot skip arch-review" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "arch-review-ok"
set_marker arch-review-ok "$(marker_key "$root/wt")"

# Every other tier pays in full. Looped over the tiers the script itself
# declares, minus the exempt one, so a tier added later is covered on the day
# it is added rather than the day someone remembers this file exists.
for tier in $tiers; do
    [ "$tier" != "small" ] || continue
    set_tier "$root/wt" "$tier"
    run "the $tier tier still requires delivery-review" \
        pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"
done

# A word the script does not know is not a tier. Read as `small` instead, the
# gate would open for anything at all written into that file.
set_tier "$root/wt" "smallish"
run "an unrecognised tier grants nothing" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"

# The bound, on a branch that really is too big. Two cases rather than one: the
# first pins that the exemption lapses, the second that it lapsed because the
# size was *measured*. Without the second, a `declared_tier` that had stopped
# recognising `small` at all would pass the first and look correct.
set_marker_in "$big_git_dir" arch-review-ok "$(marker_key "$root/big")"
set_marker_in "$big_git_dir" delivery-review-ok ""
set_tier "$root/big" small

run "a small mission that outgrew the ceiling pays in full" \
    pr-gate "$(json_bash "$root/big" "gh pr create --fill")" deny "delivery-review-ok"

run "and is told the measured size that cost it the exemption" \
    pr-gate "$(json_bash "$root/big" "gh pr create --fill")" deny "carries $big_changed"

# A binary file has no lines, which is not the same as a diff that cannot be
# read. Without this, every `small` mission shipping an icon, a font or a
# captured screenshot paid the full delivery-review and was told its size could
# not be measured - pointing at an absent remote it does have.
set_tier "$root/binary" small
set_marker_in "$binary_git_dir" arch-review-ok "$(marker_key "$root/binary")"
set_marker_in "$binary_git_dir" delivery-review-ok ""

# Without this the case is vacuous: `silent` is also what any within-ceiling
# branch produces, so if `logo.png` ever stops being seen as binary - a
# `printf` that truncates at the NUL, a git that calls 21 bytes text - the case
# passes while proving nothing about the `'-') continue` branch it exists to
# pin. Assert the premise, not only the conclusion.
if LC_ALL=C git -C "$root/binary" diff --numstat origin/main...HEAD |
        grep -qP '^-\t-\t' 2>/dev/null ||
    LC_ALL=C git -C "$root/binary" diff --numstat origin/main...HEAD |
        grep -q "^-$(printf '\t')-$(printf '\t')"; then
    passed=$((passed + 1))
else
    printf 'FAIL the binary fixture is not seen as binary by git, so its case proves nothing\n'
    printf '  numstat: %s\n' "$(LC_ALL=C git -C "$root/binary" diff --numstat origin/main...HEAD | tr '\n' ' ')"
    failed=$((failed + 1))
fi

run "a binary file does not cost a small mission its exemption" \
    pr-gate "$(json_bash "$root/binary" "gh pr create --fill")" silent

# Asserted on the phrase only the *within-ceiling* reminder carries. "the
# exemption from" appears in the unmeasurable message too, so it passed under
# the mutation this case exists to catch - a case that cannot fail is worse
# than no case, because the suite then reports the behaviour as proven.
run "and the reminder does not tell it to raise the tier" \
    commit-reminder "$(json_bash "$root/binary" "git commit -m x")" \
    context "of the $small_ceiling changed lines"

# The exclusion the size measurement applies, which had no case at all: delete
# `SIZE_EXCLUDES` from the hook and every other case here still passes, while
# every small mission is thrown out of its tier the moment it files its own
# goal file. The branch below is over the ceiling by paperwork and under it by
# work, so it can only pass through the exclusion.
set_tier "$root/paperwork" small
set_marker_in "$paperwork_git_dir" arch-review-ok "$(marker_key "$root/paperwork")"
set_marker_in "$paperwork_git_dir" delivery-review-ok ""

if [ "$(LC_ALL=C git -C "$root/paperwork" diff --numstat origin/main...HEAD |
        cut -f1 | paste -sd+ - | sed 's/^/0+/' | xargs -I{} sh -c 'echo $(({}))')" \
        -gt "$small_ceiling" ]; then
    passed=$((passed + 1))
else
    printf 'FAIL the goal-archive fixture is not actually over the ceiling, so its case proves nothing\n'
    failed=$((failed + 1))
fi

run "a goal archive does not push a small mission over the ceiling" \
    pr-gate "$(json_bash "$root/paperwork" "gh pr create --fill")" silent

# Fail-closed, where the size cannot be measured at all. Without this the
# git-error branch of `changed_lines` could be 'simplified' to return 0 - which
# looks harmless next to the empty-diff case that legitimately yields 0 - and
# every small branch in a checkout without origin/main would ship ungraded at
# any size, with all other cases still green.
set_tier "$root/noremote" small
set_marker_in "$noremote_git_dir" arch-review-ok "$(marker_key "$root/noremote")"
set_marker_in "$noremote_git_dir" delivery-review-ok ""

run "a small mission whose size cannot be measured pays in full" \
    pr-gate "$(json_bash "$root/noremote" "gh pr create --fill")" deny "delivery-review-ok"

run "and is told the size is what it could not measure" \
    pr-gate "$(json_bash "$root/noremote" "gh pr create --fill")" deny "can be measured"

# A declaration belongs to the branch that made it. The first version of this
# feature stored the tier word alone, and a worktree reused for a second branch
# inherited the exemption: a live run opened a PR for an undeclared branch with
# no delivery-review at all. Both halves are pinned - the stale declaration
# grants nothing, and the one-field format that caused it is refused outright.
set_tier "$root/wt" small
git -C "$root/wt" checkout -q -b feat/inherits
echo three > "$root/wt/src/a.txt"
git -C "$root/wt" add -A
git -C "$root/wt" commit -qm "a different mission, same worktree"
set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok ""

run "a second branch in the same worktree does not inherit the tier" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"

git -C "$root/wt" checkout -q feat/x

# A detached head names no branch. `rev-parse --abbrev-ref` prints the literal
# `HEAD` there, and the snippet that writes this file uses that same command -
# so without the guard, a declaration made while detached matches every future
# detached checkout in this worktree: the inheritance bug in a different hat.
git -C "$root/wt" checkout -q --detach
printf 'HEAD small\n' > "$wt_git_dir/$tier_file_name"
set_marker arch-review-ok "$(marker_key "$root/wt")"
run "a tier declared while detached grants nothing" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"
git -C "$root/wt" checkout -q feat/x
set_marker arch-review-ok "$(marker_key "$root/wt")"

printf 'small\n' > "$wt_git_dir/$tier_file_name"
set_marker arch-review-ok "$(marker_key "$root/wt")"
run "a tier file naming no branch grants nothing" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"

# The property the exemption rests on, and the reason the earlier skip file was
# reverted: an agent that has merely forgotten the review must not learn from
# the denial that a way around it exists. `run` can only assert a string is
# present, so absence is checked here.
set_tier "$root/wt" ""
set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok ""

# The denial itself first, through `run`, so the absence check below cannot
# pass on an empty string. A regression that stops the gate denying at all
# produces no output, contains none of the words, and would otherwise report
# this case green at exactly the moment the gate is off.
run "an untiered branch is still denied for delivery-review" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"

untiered=$(printf '%s' "$(json_bash "$root/wt" "gh pr create --fill")" | sh "$GUARDRAILS" pr-gate 2>&1)

# The denial quotes the recording command, which contains `$root` - a mktemp
# path whose random suffix can itself contain `max`, `high`, `tier` or `small`.
# Scanning it made a correct hook fail at random, with a message that sends the
# next reader hunting a leak which is not there.
untiered=$(printf '%s' "$untiered" | sed "s|$root||g")
teaches=""
case "$untiered" in
    *'"permissionDecision":"deny"'*) ;;
    *) teaches=" (no denial to inspect)" ;;
esac
# Every word that would give the mechanism away, not just two. A denial reading
# "see the reduced-ceremony path", or naming `medium`, or saying `exemption`,
# contains neither `mission-tier` nor `small` and would have reported green
# while the gate did the one thing the README says it must never do.
if [ -z "$teaches" ]; then
    for secret in $tier_file_name $tiers tier exemption; do
        case "$untiered" in
            *"$secret"*) teaches="$teaches $secret" ;;
        esac
    done
fi
if [ -z "$teaches" ]; then
    passed=$((passed + 1))
else
    printf 'FAIL the untiered denial advertises the exemption:%s\n  output: %s\n' \
        "$teaches" "$untiered"
    failed=$((failed + 1))
fi

# --- commit-reminder --------------------------------------------------------

# At the `small` tier the reminder has to name the gate that branch actually
# faces. Sending it to run the review its own tier exempts it from would spend
# exactly the saving the tier exists to buy.
set_tier "$root/wt" small
run "the reminder at the small tier names the exemption it runs under" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context "exemption"

# The other direction. Only `small` changes what the reminder says, and without
# this a widened condition (`small*`, or a prefix match) would start telling
# `medium` branches they are exempt with every case still green. The reminder is
# the surface an agent reads most often, so a wrong one there shapes behaviour
# more than a wrong denial does.
for tier in $tiers; do
    [ "$tier" != "small" ] || continue
    set_tier "$root/wt" "$tier"
    run "the reminder at the $tier tier still names both markers" \
        commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context "delivery-review-ok"
done

# Cleared before anything else runs: every case from here on predates tiers and
# assumes no declaration, and a leftover file would quietly change what they
# test rather than fail them.
set_tier "$root/wt" ""

run "the untiered reminder still names both markers" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context "delivery-review-ok"

run "a bash command that is not git commit is ignored" \
    commit-reminder "$(json_bash "$root/wt" "git status")" silent

run "a commit on main says nothing" \
    commit-reminder "$(json_bash "$root/mainco" "git commit -m x")" silent

run "a commit on a branch ahead of origin/main reminds" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context

# --- the gate must judge the command, not the prose around it ---------------
#
# Both cases below blocked or misjudged real work the first time these hooks
# ran for real, which is why they are pinned here.

set_marker arch-review-ok ""
set_marker delivery-review-ok ""

run "a commit message that merely names the gated command is ignored" \
    pr-gate "$(json_bash "$root/wt" "git commit -m 'records the marker the gate checks before gh pr create'")" silent

run "a commit message naming the gated command still reminds, not blocks" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m 'note about gh pr create'")" context

# --- the gate must follow the command into its worktree ---------------------
#
# The payload's cwd is the session's and resets between calls; every agent
# command reaches its worktree with a leading `cd`. Judging cwd alone made the
# gate read the main checkout no matter which branch was being shipped.

run "a leading cd sends the gate to the worktree, not the session cwd" \
    pr-gate "$(json_bash "$root/mainco" "cd $root/wt && gh pr create --fill")" deny

set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok "$(marker_key "$root/wt")"
run "a leading cd finds the reviews recorded in that worktree" \
    pr-gate "$(json_bash "$root/mainco" "cd $root/wt && gh pr create --fill")" silent

run "a leading cd sends the reminder to the worktree branch" \
    commit-reminder "$(json_bash "$root/mainco" "cd $root/wt && git commit -m x")" context

run "a cd to a path that does not exist falls back to the session cwd" \
    commit-reminder "$(json_bash "$root/mainco" "cd $root/nowhere && git commit -m x")" silent

# --- the gate and the instructions must name the same markers ---------------
#
# The marker names cross a boundary nothing type-checks: guardrails.sh reads
# those files, and the prose tells an agent to write them. Rename one side only
# and the gate denies a branch whose review actually ran, handing back a
# recording line that does not fix it.
#
# This block is the one part of the suite that is NOT hermetic, and the header
# says so. It reads the repository the script sits in, because the agreement
# between the script and the prose *is* the thing under test. Invoked by
# absolute path from another checkout it grades that checkout's files, and an
# unrelated doc edit can turn it red.

repo_root=$(CDPATH='' cd -- "$script_dir/../.." && pwd)
markers=$(sed -n 's/^[A-Z_]*MARKER_NAME="\([^"]*\)".*/\1/p' "$GUARDRAILS")

# Every file the gate reads out of a worktree's git dir, not only the review
# markers: the tier file is written by the same kind of prose snippet and read
# by the same script, so the drift check below has to know it is legitimate.
# Widened here rather than special-cased at the loop, so a third such file
# joins the set by being declared in guardrails.sh and nowhere else.
gate_files_padded=" $(echo $markers $tier_file_name) "

if [ -z "$markers" ]; then
    printf 'FAIL no MARKER_NAME constants found in guardrails.sh\n'
    failed=$((failed + 1))
fi

# The files that describe the whole gate, and must name every marker it needs.
# Written out rather than discovered: building the set by grepping for the
# current names made the check self-selecting, so a doc that renamed its marker
# dropped out of the set and the rename went unnoticed. A stale list here fails
# loudly; a self-selecting one failed silently.
flow_docs=".claude/hooks/README.md .claude/skills/mission/SKILL.md .claude/skills/ship/SKILL.md"

# Each review skill must carry its own recording command. Which marker belongs
# to which is derived from the skill's directory rather than listed, so this is
# not a third copy of the names.
review_skills=".claude/skills/arch-review/SKILL.md .claude/skills/delivery-review/SKILL.md"

# Per file, not "somewhere among them": checking the set would stay green while
# the instruction vanished from two of the three.
for marker in $markers; do
    for doc in $flow_docs; do
        if [ ! -f "$repo_root/$doc" ]; then
            printf 'FAIL %s is listed as an instruction file but does not exist\n' "$doc"
            failed=$((failed + 1))
        elif grep -qF -- "$marker" "$repo_root/$doc"; then
            passed=$((passed + 1))
        else
            printf 'FAIL %s never names %s, which the gate requires\n' "$doc" "$marker"
            failed=$((failed + 1))
        fi
    done
done

for doc in $review_skills; do
    skill=$(basename "$(dirname "$doc")")
    # The tier file is *read* by a review skill and never recorded by one, so
    # it is dropped from the set here rather than excused inside the loop.
    # Excusing it there left `written` non-empty on a skill that had lost its
    # own recording snippet, so the emptiness check below stopped firing and
    # the drift this block exists to catch went green.
    written=$(grep -o -- 'absolute-git-dir)/[A-Za-z0-9._-]*' "$repo_root/$doc" |
        sed 's|.*/||' |
        grep -vxF -- "$tier_file_name" |
        sort -u)
    if [ -z "$written" ]; then
        printf 'FAIL %s carries no marker-recording command of its own\n' "$doc"
        failed=$((failed + 1))
        continue
    fi
    for name in $written; do
        case "$name" in
            "$skill"-*) passed=$((passed + 1)) ;;
            *)
                printf 'FAIL %s records %s, which is not a marker named for %s\n' "$doc" "$name" "$skill"
                failed=$((failed + 1))
                ;;
        esac
    done
done

# Every file name the prose tells an agent to *write* into a git dir is one the
# gate reads - the two review markers and the tier file.
# Anchored on the recording command's shape, not on the names — grepping for
# the current names is what let a renamed doc escape the set. `grep -o`, not
# `sed`, because a leading `.*` is greedy and would see only the last recording
# command on a line.
for doc in $flow_docs $review_skills CLAUDE.md; do
    if [ ! -f "$repo_root/$doc" ]; then
        printf 'FAIL %s is checked for marker drift but does not exist\n' "$doc"
        failed=$((failed + 1))
        continue
    fi
    for written in $(grep -o -- 'absolute-git-dir)/[A-Za-z0-9._-]*' "$repo_root/$doc" | sed 's|.*/||' | sort -u); do
        case "$gate_files_padded" in
            *" $written "*) passed=$((passed + 1)) ;;
            *)
                printf 'FAIL %s tells an agent to write %s, which guardrails.sh never reads\n' "$doc" "$written"
                failed=$((failed + 1))
                ;;
        esac
    done
done

# --- the gate and the mission skill must name the same tiers ----------------
#
# The same boundary the markers cross, one layer along: guardrails.sh reads the
# tier file and the `mission` skill writes it. Rename the file on one side only
# and a mission declares a tier the gate never sees - which fails in the safe
# direction for `small`, silently costs the saving for the rest, and in neither
# case says anything.

tier_docs=".claude/hooks/README.md .claude/skills/mission/SKILL.md"
for doc in $tier_docs; do
    if [ ! -f "$repo_root/$doc" ]; then
        printf 'FAIL %s is checked for the tier file name but does not exist\n' "$doc"
        failed=$((failed + 1))
    elif grep -qF -- "$tier_file_name" "$repo_root/$doc"; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s never names %s, the file the gate reads\n' "$doc" "$tier_file_name"
        failed=$((failed + 1))
    fi
done

# Backticked, not bare. A plain substring match makes this check very nearly
# vacuous: `max` is in "maximum", `high` in "higher", `small` in "smallest",
# `medium` in "medium-effort" - so the whole tier table could be deleted and
# every tier would still be "named" by incidental prose. The skill writes each
# tier as code, which is a boundary a rename cannot fake.
for tier in $tiers; do
    if grep -qF -- "\`$tier\`" "$repo_root/.claude/skills/mission/SKILL.md"; then
        passed=$((passed + 1))
    else
        printf 'FAIL the mission skill never names the `%s` tier, which the gate accepts\n' "$tier"
        failed=$((failed + 1))
    fi
done

# --- the two copies of the tier-recording snippet must agree ----------------
#
# `mission/SKILL.md` writes the file and `README.md` documents it, and both
# carry the command because an agent executing the skill needs it inline. Two
# copies of a format the gate parses is the drift this suite exists to catch:
# change one to `<tier> <branch>`, or add a field, and the other keeps telling
# agents to write a shape `declared_tier` refuses - with every case green.
# Anchored on the branch-reading half, which is the part that was missing in
# the first version and the part a careless edit drops first.
snippet='printf '"'"'%s %s\n'"'"' "$(git rev-parse --abbrev-ref HEAD)"'
for doc in .claude/hooks/README.md .claude/skills/mission/SKILL.md; do
    if [ ! -f "$repo_root/$doc" ]; then
        printf 'FAIL %s is checked for the tier-recording snippet but does not exist\n' "$doc"
        failed=$((failed + 1))
    elif grep -qF -- "$snippet" "$repo_root/$doc"; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s does not write the tier file in the branch-pinned format the gate parses\n' "$doc"
        failed=$((failed + 1))
    fi
done

# --- the recording commands must write the key the gate actually reads -------
#
# `pr-gate` keys a marker on a hash of the branch's diff. Every document that
# tells an agent how to record one must therefore pipe that diff through
# `hash-object`, and a doc left on the old `rev-parse HEAD` form produces a
# marker the gate rejects - from following the repo's own instructions. That
# shipped once: the key changed in the hook and four documents kept writing a
# commit sha, with the whole suite green, because the checks above assert only
# that a doc *names* a marker file and never what it writes into it.

for doc in .claude/hooks/README.md .claude/skills/arch-review/SKILL.md \
    .claude/skills/delivery-review/SKILL.md; do
    if [ ! -f "$repo_root/$doc" ]; then
        printf 'FAIL %s is checked for its recording command but does not exist\n' "$doc"
        failed=$((failed + 1))
    elif grep -qF -- 'hash-object --stdin' "$repo_root/$doc"; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s records a marker without hashing the diff, so the gate would reject it\n' "$doc"
        failed=$((failed + 1))
    fi
done

# And no document that describes the gate may still say a marker holds a
# commit sha. Command drift and prose drift are different failures: the first
# hands an agent a marker the gate rejects, the second teaches the next reader
# a rule that stopped being true. Round 3 of this branch's own delivery review
# found the second one surviving in `docs/agentic-development.md` after all
# four command sites had been fixed - in the passage that introduces the
# marker as "the one design decision that makes the gate honest".
for doc in .claude/hooks/README.md .claude/skills/arch-review/SKILL.md \
    .claude/skills/delivery-review/SKILL.md CLAUDE.md docs/agentic-development.md; do
    if [ -f "$repo_root/$doc" ] &&
        grep -qiE 'marker[^.]*(holds|holding|stores|storing|records|recording)[^.]*(commit sha|sha of|exact commit)' "$repo_root/$doc"; then
        printf 'FAIL %s still describes a marker as holding a commit sha\n' "$doc"
        failed=$((failed + 1))
    else
        passed=$((passed + 1))
    fi
done

for doc in .claude/hooks/README.md .claude/skills/arch-review/SKILL.md \
    .claude/skills/delivery-review/SKILL.md; do
    if [ -f "$repo_root/$doc" ] &&
        grep -qF -- 'git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)' "$repo_root/$doc"; then
        printf 'FAIL %s still records a marker as a commit id, which the gate no longer accepts\n' "$doc"
        failed=$((failed + 1))
    else
        passed=$((passed + 1))
    fi
done

# --- report -----------------------------------------------------------------

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
