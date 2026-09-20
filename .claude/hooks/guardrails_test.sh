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

# Which copy of the script `run` invokes. The real one, except in the block
# that has to control what `ai_review_threads.sh` answers: the gate finds that
# sibling beside itself, so the only way to stub it without teaching the script
# an env override -- a switch whose whole purpose would be to turn the gate off
# -- is to run a copy from a directory the fixture owns. The copy is `cp`ed from
# the real file, so nothing here tests a script that is not the shipped one.
GUARDRAILS_UNDER_TEST="$GUARDRAILS"

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
trap 'git -C "$root/mainco" worktree remove --force "$root/wt" >/dev/null 2>&1; git -C "$root/mainco" worktree remove --force "$root/big" >/dev/null 2>&1; git -C "$root/mainco" worktree remove --force "$root/binary" >/dev/null 2>&1; rm -rf "$root"' EXIT

git init -b main -q "$root/mainco"
git -C "$root/mainco" config user.email t@t
git -C "$root/mainco" config user.name t
# The fixture, not the harness, is where the noise belongs: without this,
# git's "LF will be replaced by CRLF" warnings on Windows reach the
# captured output and fail correct cases. Discarding stderr in `run`
# instead would hide every diagnostic guardrails.sh ever emits.
git -C "$root/mainco" config core.autocrlf false
git -C "$root/mainco" config core.safecrlf false
mkdir -p "$root/mainco/src" "$root/mainco/.claude/skills/mission"
cp "$script_dir/../skills/mission/SKILL.md" "$root/mainco/.claude/skills/mission/SKILL.md"
echo one > "$root/mainco/src/a.txt"
git -C "$root/mainco" add -A
git -C "$root/mainco" commit -qm "first"
# A remote-tracking ref without a remote: commit-reminder measures against it.
git -C "$root/mainco" update-ref refs/remotes/origin/main HEAD

git -C "$root/mainco" worktree add -q -b feat/x "$root/wt" >/dev/null 2>&1
echo two > "$root/wt/src/a.txt"
mkdir -p "$root/wt/.claude"
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
    out=$(printf '%s' "$payload" | sh "$GUARDRAILS_UNDER_TEST" "$mode" 2>&1)
    status=$?

    actual=silent
    case "$out" in
        *'"permissionDecision":"deny"'*) actual=deny ;;
        # Neither yes nor no: the gate could not determine something and said
        # so. Distinguished from `silent` on purpose -- the whole point of the
        # decision is that a count nobody could take does not read as zero.
        *'"permissionDecision":"ask"'*) actual=ask ;;
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
json_bash() { printf '{"tool_name":"%s","cwd":"%s","tool_input":{"command":"%s"}}' "${3:-Bash}" "$1" "$2"; }
json_patch() {
    printf '%s%s%s%s%s' \
        '{"tool_name":"apply_patch","cwd":"' "$1" \
        '","tool_input":{"command":"*** Begin Patch\n*** Update File: ' "$2" \
        '\n*** End Patch"}}'
}
json_patch_two() {
    printf '%s%s%s%s%s%s%s' \
        '{"tool_name":"apply_patch","cwd":"' "$1" \
        '","tool_input":{"command":"*** Begin Patch\n*** Update File: ' "$2" \
        '\n*** Update File: ' "$3" '\n*** End Patch"}}'
}
json_patch_move() {
    printf '%s%s%s%s%s' \
        '{"tool_name":"apply_patch","cwd":"' "$1" \
        '","tool_input":{"command":"*** Begin Patch\n*** Update File: ' "$2" \
        '\n*** Move to: ' "$3" '\n*** End Patch"}}'
}

# --- worktree-guard ---------------------------------------------------------

run "write into the main checkout on main is denied" \
    worktree-guard "$(json_path "$root/mainco/src/a.txt")" deny

run "a file that does not exist yet is still denied" \
    worktree-guard "$(json_path "$root/mainco/src/deep/new.txt")" deny

run "agent working files under .claude are allowed" \
    worktree-guard "$(json_path "$root/mainco/.claude/GOAL.md")" silent

run "write into a linked worktree is allowed" \
    worktree-guard "$(json_path "$root/wt/src/a.txt")" silent

run "a Codex patch into the main checkout is denied" \
    worktree-guard "$(json_patch "$root/mainco" "src/a.txt")" deny

run "a Codex patch into a linked worktree is allowed" \
    worktree-guard "$(json_patch "$root/wt" "src/a.txt")" silent

run "every file in a multi-file Codex patch is guarded" \
    worktree-guard "$(json_patch_two "$root/wt" "$root/wt/src/a.txt" "$root/mainco/src/a.txt")" deny

run "a Codex patch move destination is guarded" \
    worktree-guard "$(json_patch_move "$root/wt" "$root/wt/src/a.txt" "$root/mainco/src/a.txt")" deny

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

# --- pr-gate: the draft PR and the merge gate -------------------------------
#
# Phase one ends at a draft PR and phase two ends at a merge, so the gate moved
# with the work: a draft opens ungated, and `gh pr ready` and `gh pr merge`
# want both markers *and* zero open ai-review threads.
#
# These cases run a copy of the script from a fixture directory, because the
# gate resolves `ai_review_threads.sh` beside itself and the count has to be
# controlled without reaching GitHub. `threads` in that directory is what the
# stub answers: a number, or `unavailable` for the case where no count can be
# taken at all.
mkdir -p "$root/hooks"
cp "$GUARDRAILS" "$root/hooks/guardrails.sh"
cat > "$root/hooks/ai_review_threads.sh" <<'STUB'
#!/bin/sh
# Fixture stub for the real ai_review_threads.sh. Honours only the one
# definition's two read operations and refuses everything else loudly.
stub_answer=$(cat "$(dirname "$0")/threads" 2>/dev/null)
case "$stub_answer" in
    unavailable)
        echo "the fixture says gh cannot answer" >&2
        exit 2
        ;;
esac
case "${1:-}" in
    count)
        [ "$stub_answer" = late5 ] && stub_answer=0
        printf '%s\n' "$stub_answer"
        ;;
    list)
        if [ "$stub_answer" = late5 ]; then
            late_file=${QUANTICK_COMPLETION_FIXTURE:?}/listed-once
            if [ -f "$late_file" ]; then stub_answer=5; else : > "$late_file"; stub_answer=0; fi
        fi
        stub_line=0
        while [ "$stub_line" -lt "$stub_answer" ]; do
            stub_line=$((stub_line + 1))
            printf 'thread-%s\tsrc/a.txt:1\tfixture finding %s\n' "$stub_line" "$stub_line"
        done
        ;;
    *) exit 64 ;;
esac
STUB

cat > "$root/hooks/review_report.sh" <<'STUB'
#!/bin/sh
stub_dir=$(dirname "$0")
operation=${1:-}
kind=${2:-}
report_state=$(cat "$stub_dir/reports" 2>/dev/null || printf 'current\n')
case "$operation" in
    verify)
        [ "$report_state" != unavailable ] || exit 2
        [ "$report_state" != "missing-$kind" ] || exit 1
        printf 'https://example.test/%s-report\n' "$kind"
        ;;
    publish)
        [ "$kind" = mission-completion ] || exit 64
        grep -q '^MISSION-COMPLETION: PASS$' "$4" || exit 1
        printf 'https://example.test/mission-completion-report\n'
        ;;
    *) exit 64 ;;
esac
STUB
chmod +x "$root/hooks/review_report.sh"

set_threads() { printf '%s\n' "$1" > "$root/hooks/threads"; }

# The full-CI verdict, stubbed the same way: `fullci` in the fixture directory
# is the answer (`green` when absent), and the real script's own cases run
# further down against a fake `gh`.
cat > "$root/hooks/full_ci.sh" <<'STUB'
#!/bin/sh
case "$(cat "$(dirname "$0")/fullci" 2>/dev/null || printf 'green\n')" in
    green) exit 0 ;;
    unavailable) echo 'GitHub could not list the ci check runs.' >&2; exit 2 ;;
    *) echo 'No ci run has a verdict at the fixture head.' >&2; exit 1 ;;
esac
STUB
chmod +x "$root/hooks/full_ci.sh"
set_full_ci() { printf '%s\n' "$1" > "$root/hooks/fullci"; }

GUARDRAILS_UNDER_TEST="$root/hooks/guardrails.sh"

# A draft PR is where phase one ends and where the findings get posted, so it
# opens with no marker at all. This is the case the whole two-phase split rests
# on: gating it would require the reviews before the review that informs them.
set_marker arch-review-ok ""
set_marker delivery-review-ok ""
set_threads 0
run "a draft PR opens with no marker recorded" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill")" silent

run "the short spelling of draft is a draft too" \
    pr-gate "$(json_bash "$root/wt" "gh pr create -d --fill")" silent

# The one spelling that contains the flag and means the opposite of it. Read as
# a draft, it would open an ungated real PR.
run "--draft=false is not a draft" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft=false --fill")" deny "arch-review-ok"

# The flag is read from what the command *does*, never from what it says. The
# first version globbed the whole statement, so this exact spelling opened a
# real, un-reviewed PR -- and the PR that introduced the draft exemption is
# itself the PR whose title is about `--draft`.
run "a --draft inside a quoted title is not a draft flag" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --title \"Make --draft PRs ungated\" --fill")" deny "arch-review-ok"

run "a -d inside a quoted body is not a draft flag either" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --body \"pass -d to open a draft\" --fill")" deny "arch-review-ok"

# `--draft=f` is what gh's own flag parser calls false, and so are `F`,
# `False` and `FALSE`. Excluding only `false` and `0` left four spellings of
# "not a draft" opening a real PR with no review.
for draft_false in f F False FALSE 0 false; do
    run "--draft=$draft_false opens a real PR and is gated" \
        pr-gate "$(json_bash "$root/wt" "gh pr create --draft=$draft_false --fill")" deny "arch-review-ok"
done

for draft_true in 1 t T true TRUE True; do
    run "--draft=$draft_true is a draft" \
        pr-gate "$(json_bash "$root/wt" "gh pr create --draft=$draft_true --fill")" silent
done

# A quote this cannot pair off means the blanking cannot be trusted, so the
# exemption is refused rather than guessed at.
run "an unbalanced quote is never a draft" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --title \"unclosed --draft --fill")" deny "arch-review-ok"

# The flag has to come from the statement the gate matched. A `--draft`
# anywhere else on the line is somebody else's argument.
run "a draft flag in a neighbouring statement is not this PR's" \
    pr-gate "$(json_bash "$root/wt" "echo --draft && gh pr create --fill")" deny "arch-review-ok"

# --- pr-gate: the read-cost advisory ----------------------------------------
#
# The one thing `pr-gate` says that is not a decision. It has to reach a draft
# creation, which is the point where detaching a reference is still cheap, and
# it must never become a refusal, nor noise on a branch the ceiling was never
# about.
#
# Its own repository, because the fixture has to hold a file the branch
# *references without touching* — that is the whole number the warning offers
# to lower, and a crate added wholesale on the branch has none. The real
# `tools/read_cost/` and the frozen production lexer it loads are copied in,
# so nothing here tests a stub of the thing under test. Only the ledger is
# fixture-written: the ceiling is what each case varies.
advisory_python=
for advisory_candidate in python3 python; do
    if command -v "$advisory_candidate" >/dev/null 2>&1; then
        advisory_python=$advisory_candidate
        break
    fi
done

if [ -z "$advisory_python" ]; then
    # Honest silence rather than a vacuous pass: the advisory cannot run
    # without an interpreter, and a case that asserts nothing is worse than a
    # line saying so.
    printf 'SKIP the read-cost advisory cases: no python3 or python on PATH\n'
else
    git init -b main -q "$root/readcost"
    git -C "$root/readcost" config user.email t@t
    git -C "$root/readcost" config user.name t
    git -C "$root/readcost" config core.autocrlf false
    git -C "$root/readcost" config core.safecrlf false
    mkdir -p "$root/readcost/crates/core/src" \
        "$root/readcost/docs/quality/read-cost" \
        "$root/readcost/tools/read_cost" \
        "$root/readcost/tools/outside_score"
    # Every module, not the three the advisory needed on the day this was
    # written: a fourth one arriving makes the copied calculator fail to
    # import, the advisory fall silent, and these cases fail for a reason
    # that has nothing to do with the hook.
    cp "$script_dir/../../tools/read_cost/"*.py "$root/readcost/tools/read_cost/"
    cp "$script_dir/../../tools/outside_score/measure.py" \
        "$root/readcost/tools/outside_score/"
    printf '[package]\nname = "core"\n' > "$root/readcost/crates/core/Cargo.toml"
    printf 'pub fn helper() {}\n' > "$root/readcost/crates/core/src/helper.rs"
    printf 'mod helper;\npub fn before() {}\n' > "$root/readcost/crates/core/src/lib.rs"
    git -C "$root/readcost" add -A
    git -C "$root/readcost" commit -qm "a crate to measure"
    git -C "$root/readcost" update-ref refs/remotes/origin/main HEAD
    git -C "$root/readcost" checkout -q -b feat/readcost
    printf 'mod helper;\npub fn before() {}\npub fn after() {}\n' \
        > "$root/readcost/crates/core/src/lib.rs"
    git -C "$root/readcost" add -A
    git -C "$root/readcost" commit -qm "touch one file that reads another"

    # set_ceiling <number> — the whole ledger the advisory reads.
    set_ceiling() {
        printf '<!-- read-cost-ceiling:v1 %s -->\n' "$1" \
            > "$root/readcost/docs/quality/read-cost/ledger.md"
    }

    set_ceiling 1
    run "a feature branch over the ceiling is told, on a draft, what it pulls in" \
        pr-gate "$(json_bash "$root/readcost" "gh pr create --draft --fill")" \
        context "crates/core/src/helper.rs"

    # The same case, one assertion deeper: it names the ceiling it is measured
    # against, and it arrives as news rather than as a refusal.
    run "the advisory names the ceiling it measured against" \
        pr-gate "$(json_bash "$root/readcost" "gh pr create --draft --fill")" \
        context "over the 1 ceiling"

    set_ceiling 100000
    run "a feature branch under the ceiling hears nothing" \
        pr-gate "$(json_bash "$root/readcost" "gh pr create --draft --fill")" silent

    set_ceiling 1
    git -C "$root/readcost" checkout -q -b docs/readcost
    run "a branch the ceiling was never about hears nothing" \
        pr-gate "$(json_bash "$root/readcost" "gh pr create --draft --fill")" silent
    git -C "$root/readcost" checkout -q feat/readcost

    rm -f "$root/readcost/docs/quality/read-cost/ledger.md"
    run "no recorded ceiling is silence, not a finding" \
        pr-gate "$(json_bash "$root/readcost" "gh pr create --draft --fill")" silent

    set_ceiling 1
    # The gate's own verdict is never softened by the advisory: a real PR on a
    # branch with no review is still denied, and denied about the review.
    run "an over-ceiling branch is still denied for the review it skipped" \
        pr-gate "$(json_bash "$root/readcost" "gh pr create --fill")" deny "arch-review-ok"
fi

# Order matters: an unreviewed branch is told about the review it skipped, not
# about threads. The markers are the older rule and the cheaper check.
run "gh pr ready wants the reviews before it wants a thread count" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" deny "arch-review-ok"

run "gh pr merge wants the reviews too" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 42 --squash")" deny "arch-review-ok"

set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok "$(marker_key "$root/wt")"

set_threads 0
# AI completion is not inferred from zero findings. Pin each failure with all
# preceding evidence current, through both hosts' shared hook entry point.
for client in Bash exec_command; do
    for tier in $tiers; do
        set_tier "$root/wt" "$tier"
        set_marker ai-review-complete ""
        if [ "$tier" = small ]; then set_marker delivery-review-ok ""; fi
        for action in ready merge; do
            run "$client $tier $action requires AI completion with zero threads" \
                pr-gate "$(json_bash "$root/wt" "gh pr $action 42" "$client")" deny "ai-review-complete"
        done
        set_marker ai-review-complete "feat/x $(marker_key "$root/wt")"
        run "$client $tier current clean AI completion allows ready" \
            pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" silent
        set_threads 1
        run "$client $tier completed review with findings still denies ready" \
            pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny 'PR #42 has 1'
        set_threads 0
        set_marker delivery-review-ok "$(marker_key "$root/wt")"
    done
    set_tier "$root/wt" ""
    set_marker ai-review-complete "feat/x $stale_sha"
    run "$client stale AI key denies ready" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny 'ai-review-complete'
    for malformed in "$(marker_key "$root/wt")" 'he said "hi"' \
        "feat/x $(marker_key "$root/wt") extra" "feat/x $(marker_key "$root/wt") "; do
        set_marker ai-review-complete "$malformed"
        run "$client malformed AI record denies with safe JSON" \
            pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny '(not a commit id)'
    done
    : > "$wt_git_dir/ai-review-complete"
    run "$client empty AI record denies" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny 'ai-review-complete'
    printf 'feat/x %s\nextra' "$(marker_key "$root/wt")" > "$wt_git_dir/ai-review-complete"
    run "$client unterminated appended AI record denies" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny '(not a commit id)'
    printf 'feat/x %s\n\n' "$(marker_key "$root/wt")" > "$wt_git_dir/ai-review-complete"
    run "$client multiline AI record denies" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny '(not a commit id)'

    set_marker ai-review-complete ""
    set_marker_in "$big_git_dir" ai-review-complete "feat/x $(marker_key "$root/wt")"
    run "$client another worktree cannot supply the identical AI record" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny 'ai-review-complete'
    set_marker ai-review-complete "feat/x $(marker_key "$root/wt")"
    ai_original_key=$(marker_key "$root/wt")
    git -C "$root/wt" checkout -qb "feat/ai-$client"
    if [ "$(marker_key "$root/wt")" = "$ai_original_key" ]; then
        passed=$((passed + 1))
    else
        printf 'FAIL branch-only AI fixture changed the raw key\n'
        failed=$((failed + 1))
    fi
    run "$client branch-only change invalidates AI completion" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny 'ai-review-complete'
    set_marker ai-review-complete "feat/ai-$client $ai_original_key"
    git -C "$root/wt" commit -q --amend -m "same diff, $client reword"
    run "$client same-branch reword preserves AI completion" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" silent
    printf 'AI source change\n' >> "$root/wt/src/a.txt"
    git -C "$root/wt" commit -qam 'change after AI review'
    set_marker arch-review-ok "$(marker_key "$root/wt")"
    set_marker delivery-review-ok "$(marker_key "$root/wt")"
    run "$client source change stales AI after other reviews refresh" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny 'ai-review-complete'
    git -C "$root/wt" checkout -q feat/x
    set_marker arch-review-ok "$(marker_key "$root/wt")"
    set_marker delivery-review-ok "$(marker_key "$root/wt")"
    set_marker ai-review-complete "feat/x $(marker_key "$root/wt")"
    set_marker arch-review-ok ""
    run "$client AI completion cannot replace architecture review" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny 'arch-review-ok'
    set_marker delivery-review-ok ""
    set_marker ai-review-complete ""
    run "$client draft creation needs no review markers" \
        pr-gate "$(json_bash "$root/wt" 'gh pr create --draft --fill' "$client")" silent
    set_marker arch-review-ok "$(marker_key "$root/wt")"
    set_marker ai-review-complete "feat/x $(marker_key "$root/wt")"
    run "$client AI completion cannot replace delivery review" \
        pr-gate "$(json_bash "$root/wt" 'gh pr ready 42' "$client")" deny 'delivery-review-ok'
    set_marker delivery-review-ok "$(marker_key "$root/wt")"
done

run "all required reviews and no open thread makes the branch ready" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" silent

# The advisory rides a pass rather than replacing a decision, and the pull
# request it asks about is the one the command named. A stub calculator here
# on purpose: what is under test is the wiring — that `gh pr ready 42` arrives
# as `--pr 42` and a creation arrives without one — and the real calculator
# answers about a diff instead of about its arguments. The cases above cover
# what it says.
mkdir -p "$root/wt/tools/read_cost"
cat > "$root/wt/tools/read_cost/report.py" <<'STUB'
import json
import sys

print(json.dumps("read-cost stub saw " + " ".join(sys.argv[1:])))
STUB
run "the advisory rides a passing gate and names the pull request" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" context "--pr 42"
# No `--pr` at all, not an empty one: the argument ends the stub's line.
run "a creation names no pull request to the advisory" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill")" context '--branch feat/x"'
rm -rf "$root/wt/tools"

# Draft pushes run only the fast job, so readiness is where full CI is owed.
# Every review is recorded here; only the full-CI verdict moves.
set_full_ci missing
run "gh pr ready is denied without full CI at the exact head" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" deny "Full final-head CI gates"
run "the full-CI denial names the label that runs it" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" deny "gh pr edit 42 --add-label full-ci"
run "gh pr merge is denied without full CI at the exact head" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 42 --squash")" deny "Full final-head CI gates"
set_full_ci unavailable
run "an unreadable full-CI verdict asks rather than passes" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" ask "unknown is not green"
mv "$root/hooks/full_ci.sh" "$root/hooks/full_ci.sh.away"
run "a gate with no full_ci.sh beside it asks rather than passes" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" ask "full_ci.sh is not beside the gate"
mv "$root/hooks/full_ci.sh.away" "$root/hooks/full_ci.sh"
set_full_ci green

printf 'missing-arch-review\n' > "$root/hooks/reports"
run "a hand-written architecture marker cannot replace its durable report" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" deny "durable PR report"
printf 'current\n' > "$root/hooks/reports"

run "even reviewed main merges remain exclusively human" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 42 --squash")" deny "reserved exclusively"

# The `cd <worktree> &&` prefix an agent needs to reach its worktree is read for
# the campaign form pins only. On a main-based branch it changes nothing: the
# ready case still passes on its markers, and the merge is still the user's.
run "a cd prefix leaves a main-based ready where it was" \
    pr-gate "$(json_bash "$root/wt" "cd $root/wt && gh pr ready 42")" silent

run "a cd prefix does not make a main merge the agent's" \
    pr-gate "$(json_bash "$root/wt" "cd $root/wt && gh pr merge 42 --squash")" deny "reserved exclusively"

set_threads 2
run "gh pr ready is denied while an ai-review thread is open" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" deny "PR #42 has 2"

run "gh pr merge is denied while an ai-review thread is open" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 42 --squash")" deny "PR #42 has 2"

# The denial has to name the way out, and the way out is the list of threads.
run "the denial names the command that lists the open threads" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 42")" deny "ai_review_threads.sh list 42"

# A count nobody could take is not a count of zero, and the gate says which of
# the two it is holding. It asks rather than denies: the file's rule is
# fail-open, and a human can still answer for it.
set_threads unavailable
run "a count that cannot be taken asks rather than passing in silence" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 42 --squash")" ask "could not be counted"

set_threads 0
run "a command naming no PR asks rather than guessing which PR to count" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready")" ask "names no PR number"

# Two bare numbers and the operand cannot be told from a flag's value. Guessing
# would count another PR's threads and report a clean number for a branch
# nobody reviewed, so the gate asks instead.
run "two bare numbers are an ambiguous PR, not a guess" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 306 42 --squash")" ask "names no PR number"

# The hook always runs from the main checkout, so a branch that has not merged
# the counting script yet cannot be counted from there. That is a different
# failure from `gh` refusing to answer, and it gets a different message: an
# agent told to check its GitHub authentication over a missing file goes
# looking in the wrong place.
mkdir -p "$root/lonely"
cp "$GUARDRAILS" "$root/lonely/guardrails.sh"
cp "$root/hooks/review_report.sh" "$root/lonely/review_report.sh"
GUARDRAILS_UNDER_TEST="$root/lonely/guardrails.sh"
run "a missing counting script is named as such, not blamed on gh" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 42 --squash")" ask "is not beside the hook"
GUARDRAILS_UNDER_TEST="$root/hooks/guardrails.sh"

# `gh pr create` is never held on the thread count, draft or not: the PR is
# where the findings live, so they cannot be a precondition for opening it.
set_threads 9
run "an open thread never blocks opening the PR that carries it" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" silent

# A compound line runs two of the gated commands. The draft exemption belongs
# to the `create` half only, and taking it for the whole line would let the
# `gh pr ready` beside it ship with no review -- the natural spelling for
# ending phase one and starting phase two, and the one that must not slip.
set_marker arch-review-ok ""
set_marker delivery-review-ok ""
run "a draft create beside a ready is judged as the ready" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill && gh pr ready")" deny "arch-review-ok"

# The PR number is a whole word. Reading a digit run out of a filename made
# `--body-file notes2.md 42` count PR 2's threads, which reports a clean number
# for a branch nobody reviewed -- worse than reporting no number at all.
set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok "$(marker_key "$root/wt")"
set_threads 7
GUARDRAILS_UNDER_TEST="$root/hooks/guardrails.sh"
run "a digit inside a filename is not the PR number" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge --body-file notes2.md 42")" deny "PR #42 has 7"
GUARDRAILS_UNDER_TEST="$GUARDRAILS"

# --- ai_review_threads.sh: the contract the gate reads ----------------------
#
# The gate treats "no count" and "a count of zero" as different answers, and
# the whole of that distinction rests on this script's exit status. Neither
# case here reaches GitHub: one asks for nothing, the other runs with a PATH
# that has no `gh` on it.
threads_script="$script_dir/ai_review_threads.sh"

threads_out=$(sh "$threads_script" 2>&1)
threads_status=$?
if [ "$threads_status" -eq 64 ]; then
    passed=$((passed + 1))
else
    printf 'FAIL ai_review_threads.sh with no subcommand: exited %s, wanted 64\n  output: %s\n' \
        "$threads_status" "$threads_out"
    failed=$((failed + 1))
fi

# An absolute interpreter, because the emptied PATH would otherwise stop the
# shell being found before the script ever runs -- 127, not the 2 under test.
# Every command the script reaches on this path is a builtin.
threads_sh=$(command -v sh)
threads_out=$(PATH=/nonexistent "$threads_sh" "$threads_script" count 1 2>/dev/null)
threads_status=$?
if [ "$threads_status" -eq 2 ] && [ -z "$threads_out" ]; then
    passed=$((passed + 1))
else
    printf 'FAIL ai_review_threads.sh count without gh: exited %s printing "%s", wanted 2 and nothing\n' \
        "$threads_status" "$threads_out"
    failed=$((failed + 1))
fi

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

# --- mission/ship final completion -----------------------------------------
#
# PR #306 did not cross `gh pr ready` during the faulty completion attempt: it
# was already open and non-draft. These cases therefore drive the explicit
# final verifier used by both workflow skills. GitHub's optimistic signals are
# present on purpose; they cannot replace any review gate.
MISSION_SHIP_GATE="$script_dir/mission_ship_gate.sh"
cp "$MISSION_SHIP_GATE" "$root/hooks/mission_ship_gate.sh"
cp "$script_dir/campaign_context.sh" "$root/hooks/campaign_context.sh"
MISSION_SHIP_GATE="$root/hooks/mission_ship_gate.sh"
mkdir -p "$root/bin"
cat > "$root/bin/gh" <<'STUB'
#!/bin/sh
stub_root=${QUANTICK_COMPLETION_FIXTURE:?}
if [ "${1:-} ${2:-}" = "pr view" ]; then
    case " $* " in
        *' --json body '*) cat "$stub_root/pr-body" ;;
        *' --json baseRefName,'*) cat "$stub_root/pr-context" ;;
        *) cat "$stub_root/pr-identity" ;;
    esac
    exit 0
fi
if [ "${1:-} ${2:-}" = "issue view" ]; then
    cat "$stub_root/issue-body"
    exit 0
fi
if [ "${1:-} ${2:-}" = "pr checks" ]; then
    cat "$stub_root/checks"
    exit 0
fi
if [ "${1:-} ${2:-}" = "pr edit" ]; then
    while [ $# -gt 0 ]; do
        if [ "$1" = --body-file ]; then
            cp "$2" "$stub_root/pr-body"
            exit 0
        fi
        shift
    done
fi
exit 64
STUB
chmod +x "$root/bin/gh"

cat > "$root/hooks/review_report.sh" <<'STUB'
#!/bin/sh
stub_root=${QUANTICK_COMPLETION_FIXTURE:?}
operation=${1:-}
kind=${2:-}
case "$operation" in
    verify)
        report_state=$(cat "$stub_root/reports")
        [ "$report_state" != unavailable ] || exit 2
        [ "$report_state" != "missing-$kind" ] || exit 1
        printf 'https://example.test/%s-report\n' "$kind"
        ;;
    publish)
        [ "$kind" = mission-completion ] || exit 64
        grep -q '^MISSION-COMPLETION: PASS$' "$4" || exit 1
        cp "$4" "$stub_root/published-report"
        printf 'https://example.test/mission-completion-report\n'
        ;;
    *) exit 64 ;;
esac
STUB
chmod +x "$root/hooks/review_report.sh"

run_completion() {
    completion_name=$1
    completion_mode=$2
    completion_expect=$3
    completion_want=${4:-}
    completion_out=$(QUANTICK_COMPLETION_FIXTURE="$root/completion" \
        PATH="$root/bin:$PATH" \
        sh "$MISSION_SHIP_GATE" "$completion_mode" 42 "${completion_wt:-$root/wt}" 2>&1)
    completion_status=$?

    if [ "$completion_expect" = pass ] && [ "$completion_status" -eq 0 ]; then
        case "$completion_out" in
            *MISSION-COMPLETION:PASS*) passed=$((passed + 1)); return ;;
        esac
    elif [ "$completion_expect" = fail ] && [ "$completion_status" -ne 0 ]; then
        case "$completion_out" in
            *"$completion_want"*) passed=$((passed + 1)); return ;;
        esac
    fi

    printf 'FAIL %s: expected %s containing "%s", status=%s\n  output: %s\n' \
        "$completion_name" "$completion_expect" "$completion_want" \
        "$completion_status" "$completion_out"
    failed=$((failed + 1))
}

mkdir -p "$root/completion"
printf '<!-- quantick-mission-summary:v1 -->\nObjective: fixture\nTier: high\nSource: fixture\nCriteria: A1 delivered\nValidation: fixture pass\n<!-- end quantick-mission-summary:v1 -->\n' \
    > "$root/completion/pr-body"
printf 'pass\n' > "$root/completion/checks"
printf 'current\n' > "$root/completion/reports"
printf 'feat/x %s main %s OPEN false MERGEABLE CLEAN https://example.test/pr/42 false\n' \
    "$head_sha" "$(git -C "$root/wt" rev-parse origin/main)" > "$root/completion/pr-identity"

GUARDRAILS_UNDER_TEST="$root/hooks/guardrails.sh"
set_tier "$root/wt" high
set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok "$(marker_key "$root/wt")"
set_threads 0
set_marker ai-review-complete ""
for completion_mode in mission ship; do
    run_completion "PR 306 $completion_mode refuses an already-ready green mergeable PR without AI completion" \
        "$completion_mode" fail ai-review-complete
done

set_marker ai-review-complete "feat/x $(marker_key "$root/wt")"
set_threads 5
for completion_mode in mission ship; do
    run_completion "PR 306 $completion_mode refuses five open AI-review threads" \
        "$completion_mode" fail '5 unresolved AI-review threads'
done

set_threads 0
printf 'missing-ai-review\n' > "$root/completion/reports"
for completion_mode in mission ship; do
    run_completion "PR 306 $completion_mode refuses AI completion without its durable report" \
        "$completion_mode" fail 'ai-review` marker has no matching durable PR report'
done

printf 'missing-arch-review\n' > "$root/completion/reports"
run_completion "a hand-written architecture marker has no durable skill report" \
    mission fail 'arch-review` marker has no matching durable PR report'

printf 'missing-delivery-review\n' > "$root/completion/reports"
run_completion "high cannot complete with a hand-written delivery marker" \
    ship fail 'delivery-review` marker has no matching durable PR report'

printf 'current\n' > "$root/completion/reports"
for completion_mode in mission ship; do
    run_completion "a fully evidenced already-ready PR completes through $completion_mode" \
        "$completion_mode" pass
done

# The draft-only fast job is skipped on every ready head. Skipped is no verdict,
# so it neither passes nor blocks; full CI is what completion requires.
printf 'pass\nskipping\n' > "$root/completion/checks"
run_completion "a skipped fast job does not block completion" mission pass
set_full_ci missing
for completion_mode in mission ship; do
    run_completion "$completion_mode refuses a green PR whose full CI never ran at the head" \
        "$completion_mode" fail 'Full final-head CI gates'
done
set_full_ci green
printf 'pass\nfail\n' > "$root/completion/checks"
run_completion "a failed check still blocks completion beside green full CI" \
    mission fail 'not green'
printf 'pass\n' > "$root/completion/checks"

rm -f "$root/completion/listed-once" "$root/completion/published-report"
set_threads late5
run_completion "threads opened during reconciliation block durable completion publication" \
    mission fail 'threads changed during final reconciliation'
if [ ! -f "$root/completion/published-report" ]; then
    passed=$((passed + 1))
else
    printf 'FAIL late threads left a durable positive completion report\n'
    failed=$((failed + 1))
fi
set_threads 0

cp "$root/completion/pr-body" "$root/completion/mission-summary"
: > "$root/completion/pr-body"
run_completion "completion refuses a mission PR without its concise summary" \
    mission fail 'exactly one quantick-mission-summary:v1 block'
cp "$root/completion/mission-summary" "$root/completion/pr-body"
cat "$root/completion/mission-summary" >> "$root/completion/pr-body"
run_completion "completion refuses duplicate mission summaries" \
    mission fail 'exactly one quantick-mission-summary:v1 block'
sed 's/^Criteria: .*/Criteria: <IDs and disposition>/' "$root/completion/mission-summary" \
    > "$root/completion/pr-body"
run_completion "completion refuses a summary field left as the template placeholder" \
    mission fail 'no filled Criteria: line'
sed '/^Validation:/d' "$root/completion/mission-summary" > "$root/completion/pr-body"
run_completion "completion refuses a summary missing a field" \
    mission fail 'no filled Validation: line'
sed 's/$/\r/' "$root/completion/mission-summary" > "$root/completion/pr-body"
rm -f "$root/completion/published-report"
run_completion "completion accepts a summary saved with CRLF line endings" mission pass
cp "$root/completion/mission-summary" "$root/completion/pr-body"

printf 'feat/x %s main %s OPEN true MERGEABLE CLEAN https://example.test/pr/42 false\n' \
    "$head_sha" "$(git -C "$root/wt" rev-parse origin/main)" > "$root/completion/pr-identity"
run_completion "final completion refuses a PR that is still draft" \
    ship fail 'still a draft'
printf 'feat/x %s main %s OPEN false MERGEABLE CLEAN https://example.test/pr/42 false\n' \
    "$head_sha" "$(git -C "$root/wt" rev-parse origin/main)" > "$root/completion/pr-identity"

set_tier "$root/wt" small
set_marker delivery-review-ok ""
printf 'missing-delivery-review\n' > "$root/completion/reports"
run_completion "small keeps only its bounded delivery-review exemption" \
    mission pass

printf 'current\n' > "$root/completion/reports"
set_marker delivery-review-ok "$(marker_key "$root/wt")"
for tier in medium high max; do
    set_tier "$root/wt" "$tier"
    run_completion "$tier completes through the same final gate" mission pass
done

cp "$root/wt/.claude/skills/mission/SKILL.md" "$root/completion/mission-skill"
sed -i '/<!-- end what-done-means:v1 -->/i\- **D9** -- An unmapped completion clause.' \
    "$root/wt/.claude/skills/mission/SKILL.md"
git -C "$root/wt" add .claude/skills/mission/SKILL.md
git -C "$root/wt" commit -qm 'mutate completion contract'
head_sha=$(git -C "$root/wt" rev-parse HEAD)
set_marker arch-review-ok "$(marker_key "$root/wt")"
set_marker delivery-review-ok "$(marker_key "$root/wt")"
set_marker ai-review-complete "feat/x $(marker_key "$root/wt")"
printf 'feat/x %s main %s OPEN false MERGEABLE CLEAN https://example.test/pr/42 false\n' \
    "$head_sha" "$(git -C "$root/wt" rev-parse origin/main)" > "$root/completion/pr-identity"
run_completion "an unknown What done means clause blocks completion" \
    mission fail 'Unknown What done means clause'
cp "$root/completion/mission-skill" "$root/wt/.claude/skills/mission/SKILL.md"
git -C "$root/wt" add .claude/skills/mission/SKILL.md
git -C "$root/wt" commit -qm 'restore completion contract'
head_sha=$(git -C "$root/wt" rev-parse HEAD)

set_tier "$root/wt" ""
set_marker delivery-review-ok "$(marker_key "$root/wt")"
printf 'current\n' > "$root/completion/reports"

# --- completion by PR kind (#439) -------------------------------------------
# The gate distinguishes mission, synchronization and consolidated campaign
# PRs from verified head/base facts. Mission and sync PRs use a concise body
# summary; the consolidated campaign uses its parent charter.
main_first=$(git -C "$root/mainco" rev-parse origin/main)
kind_commit() { git -C "$1" add -A && git -C "$1" commit -qm "$2"; }

git -C "$root/mainco" worktree add -q -b campaign/demo "$root/camp" "$main_first" >/dev/null 2>&1
echo campaign > "$root/camp/src/a.txt"
kind_commit "$root/camp" 'integrated campaign change'
git -C "$root/mainco" update-ref refs/remotes/origin/campaign/demo "$(git -C "$root/camp" rev-parse HEAD)"

git -C "$root/mainco" worktree add -q -b main-next "$root/mainnext" "$main_first" >/dev/null 2>&1
echo main-next > "$root/mainnext/src/main-next.txt"
kind_commit "$root/mainnext" 'main advances'
git -C "$root/mainco" update-ref refs/remotes/origin/main "$(git -C "$root/mainnext" rev-parse HEAD)"

# kind_pr <worktree> <PR base ref> [campaign parent URL] - reviewed markers at
# the worktree's current key and a PR identity matching its head and base.
kind_pr() {
    kind_git=$(git -C "$1" rev-parse --absolute-git-dir)
    kind_branch=$(git -C "$1" symbolic-ref --short HEAD)
    if [ -n "${3:-}" ]; then
        printf '%s origin/%s %s none\n' "$kind_branch" "$2" "$3" > "$kind_git/mission-base"
    fi
    kind_key=$(sh "$root/hooks/campaign_context.sh" key "$1")
    kind_head=$(git -C "$1" rev-parse HEAD)
    kind_tip=$(git -C "$1" rev-parse "origin/$2")
    set_marker_in "$kind_git" arch-review-ok "$kind_key"
    set_marker_in "$kind_git" delivery-review-ok "$kind_key"
    set_marker_in "$kind_git" ai-review-complete "$kind_branch $kind_key"
    printf '%s %s %s %s OPEN false MERGEABLE CLEAN https://github.com/owner/repo/pull/42 false\n' \
        "$kind_branch" "$kind_head" "$2" "$kind_tip" > "$root/completion/pr-identity"
    printf '%s %s %s OPEN false false %s CLEAN\n' \
        "$2" "$kind_branch" "$kind_head" "$kind_tip" > "$root/completion/pr-context"
}
# kind_report <case> <text> - the published reconciliation names the verified
# PR kind and durable source.
kind_report() {
    if grep -qF -- "$2" "$root/completion/published-report" 2>/dev/null; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s: the published report does not say "%s"\n' "$1" "$2"
        failed=$((failed + 1))
    fi
}
parent=https://github.com/owner/repo/issues/7
cp "$root/completion/mission-summary" "$root/completion/pr-body"

# A mission child of the campaign stays a mission after main moves on.
git -C "$root/mainco" worktree add -q -b feat/child "$root/child" origin/campaign/demo >/dev/null 2>&1
completion_wt=$root/child
echo child > "$root/child/src/a.txt"
kind_commit "$root/child" 'child code'
kind_pr "$root/child" campaign/demo "$parent"
rm -f "$root/completion/published-report"
run_completion "a campaign mission child with one concise summary completes" ship pass
kind_report "the mission child's report" 'Mission source stayed local; the PR carries its concise durable summary.'

# A sync/* branch carries the same concise summary contract.
git -C "$root/mainco" worktree add -q -b sync/main-demo "$root/sync" origin/campaign/demo >/dev/null 2>&1
completion_wt=$root/sync
git -C "$root/sync" merge -q --no-ff --no-edit origin/main
kind_pr "$root/sync" campaign/demo "$parent"
rm -f "$root/completion/published-report"
run_completion "a synchronization with one concise summary completes" ship pass
kind_report "the sync's report" 'Main synchronization: the PR carries its concise durable summary.'

# Not the name: a branch carrying main commits its campaign base lacks is a sync.
git -C "$root/mainco" worktree add -q -b feat/main-into-demo "$root/merged" origin/campaign/demo >/dev/null 2>&1
completion_wt=$root/merged
git -C "$root/merged" merge -q --no-ff --no-edit origin/main
kind_pr "$root/merged" campaign/demo "$parent"
rm -f "$root/completion/published-report"
run_completion "a sync detected by its main merge, not its name, completes" ship pass
kind_report "the unnamed sync's report" 'Main synchronization: the PR carries its concise durable summary.'

# The consolidated campaign PR is verified against its parent charter instead
# of a mission summary.
completion_wt=$root/camp
kind_pr "$root/camp" main
printf '<!-- quantick-campaign:v1 -->\nExact branch: `campaign/demo`.\n' > "$root/completion/issue-body"
run_completion "a consolidated campaign PR without its parent fails" \
    ship fail 'Campaign-parent: <issue URL in this repository>'
printf 'Campaign-parent: https://github.com/other/repo/issues/7\n' > "$root/completion/pr-body"
run_completion "a consolidated campaign PR naming another repository's parent fails" \
    ship fail 'Campaign-parent: <issue URL in this repository>'
# A dot in the repository name is a literal dot, not a wildcard for a look-alike.
sed -i 's|owner/repo/pull|owner/re.po/pull|' "$root/completion/pr-identity"
printf 'Campaign-parent: https://github.com/owner/reXpo/issues/7\n' > "$root/completion/pr-body"
run_completion "a consolidated campaign PR naming a look-alike repository's parent fails" \
    ship fail 'Campaign-parent: <issue URL in this repository>'
sed -i 's|owner/re.po/pull|owner/repo/pull|' "$root/completion/pr-identity"
printf 'Campaign-parent: %s/7\n' "$parent" > "$root/completion/pr-body"
run_completion "a consolidated campaign PR whose parent is not an issue number fails" \
    ship fail 'Campaign-parent: <issue URL in this repository>'
printf 'Campaign #7.\nCampaign-parent: %s\n' "$parent" > "$root/completion/pr-body"
printf 'Exact branch: `campaign/demo`.\n' > "$root/completion/issue-body"
run_completion "a consolidated campaign PR whose parent is no charter fails" \
    ship fail 'not a campaign charter'
printf '<!-- quantick-campaign:v1 -->\nExact branch: `campaign/demo-2`.\n' > "$root/completion/issue-body"
run_completion "a consolidated campaign PR whose charter names another branch fails" \
    ship fail 'does not name `campaign/demo`'
printf '<!-- quantick-campaign:v1 -->\nBranches: campaign/demo-2, campaign/demo/next.\n' > "$root/completion/issue-body"
run_completion "a charter naming only plain-text look-alike branches fails" \
    ship fail 'does not name `campaign/demo`'
# The state contract fixes no format for the branch, so plain text counts too.
printf '<!-- quantick-campaign:v1 -->\nExact branch: campaign/demo.\n' > "$root/completion/issue-body"
run_completion "a charter naming the branch in plain text completes" ship pass
printf '<!-- quantick-campaign:v1 -->\nExact branch: `campaign/demo`.\n' > "$root/completion/issue-body"
rm -f "$root/completion/published-report"
run_completion "a consolidated campaign PR with its verified charter completes" ship pass
kind_report "the consolidated report" "Consolidated campaign: charter $parent names \`campaign/demo\`"
set_threads 3
run_completion "a consolidated campaign PR still needs zero AI-review threads" \
    ship fail '3 unresolved AI-review threads'
set_threads 0
printf 'fail\n' > "$root/completion/checks"
run_completion "a consolidated campaign PR still needs green exact-head CI" \
    ship fail 'not green'
printf 'pass\n' > "$root/completion/checks"

completion_wt=
: > "$root/completion/pr-body"
git -C "$root/mainco" update-ref refs/remotes/origin/main "$main_first"

# --- commit-reminder --------------------------------------------------------

# At the `small` tier the reminder has to name the gate that branch actually
# faces. Sending it to run the review its own tier exempts it from would spend
# exactly the saving the tier exists to buy.
set_tier "$root/wt" small
run "the reminder at the small tier names the exemption it runs under" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context "exemption"
run "the reminder at the small tier still requires AI evidence" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context "ai-review-complete"

# The other direction. Only `small` changes what the reminder says, and without
# this a widened condition (`small*`, or a prefix match) would start telling
# `medium` branches they are exempt with every case still green. The reminder is
# the surface an agent reads most often, so a wrong one there shapes behaviour
# more than a wrong denial does.
for tier in $tiers; do
    [ "$tier" != "small" ] || continue
    set_tier "$root/wt" "$tier"
    run "the reminder at the $tier tier still names all reviews" \
        commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context "architecture, delivery and AI"
done

# Cleared before anything else runs: every case from here on predates tiers and
# assumes no declaration, and a leftover file would quietly change what they
# test rather than fail them.
set_tier "$root/wt" ""

run "the untiered reminder still names all reviews" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context "architecture, delivery and AI"

run "a bash command that is not git commit is ignored" \
    commit-reminder "$(json_bash "$root/wt" "git status")" silent

run "a commit on main says nothing" \
    commit-reminder "$(json_bash "$root/mainco" "git commit -m x")" silent

run "a commit on a branch ahead of origin/main reminds" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context

# --- guard-watch ------------------------------------------------------------

# The ordinary case on a worktree nobody has built yet: there is no binary to
# run, so the mode says nothing. This is the property that keeps the hook off
# the critical path — it must never reach for cargo and start a compile of its
# own behind an edit.
run "guard-watch: silent when the binary has not been built" \
    guard-watch "$(json_path "$root/wt/src/a.rs")" silent

# A stub standing in for the built binary. What is under test here is the
# hook's plumbing — finding the repository, deriving the path the baseline is
# keyed on, and escaping a multi-line report into one line of JSON — not the
# guards themselves, which have their own tests in `crates/guards`.
mkdir -p "$root/wt/target/debug"
stub="$root/wt/target/debug/quantick-guards"
{
    echo '#!/bin/sh'
    echo 'echo "size: 1 finding(s)"'
    echo 'echo "  $2: 9999 production lines, ceiling 10 (+9989)"'
    echo 'echo "a \"quoted\" remedy"'
    echo 'exit 1'
} > "$stub"
chmod +x "$stub"

run "guard-watch: reports what the binary found" \
    guard-watch "$(json_path "$root/wt/src/a.rs")" context "9989"

# The path handed to the binary is workspace-relative. An absolute one would
# match no baseline entry, so every file would come back clean and the hook
# would look like it was working while reporting nothing.
run "guard-watch: passes a workspace-relative path" \
    guard-watch "$(json_path "$root/wt/src/a.rs")" context "src/a.rs"

run "guard-watch: reads the edited path from a Codex patch" \
    guard-watch "$(json_patch "$root/wt" "src/a.rs")" context "src/a.rs"

# A quotation mark in the report has to survive into the JSON string. The
# harness already rejects a payload that is not a parseable one-line object,
# so this case fails loudly if the escaping regresses — which it did once,
# when a `;`-joined sed script this environment rejects left the message
# empty and the hook looked like it had found nothing.
run "guard-watch: escapes quotes in the report" \
    guard-watch "$(json_path "$root/wt/src/a.rs")" context 'quoted'

# The property the whole design rests on, and the one no other case pinned:
# a binary that exits 0 produces no output at all. Every other stubbed case
# uses a stub that exits 1, so dropping the `&& exit 0` on the findings line
# would make the hook emit a context block after every clean edit — the noise
# that gets a hook switched off — while the suite stayed green.
#
# The stub discriminates on the path it is handed rather than exiting 0 for
# everything. A stub that ignored its argument made the `.txt` case below
# vacuous: it would have stayed green through a re-grown extension filter, a
# mangled `$relative`, or an absolute path being passed — none of which is
# what it claims to pin.
{
    echo '#!/bin/sh'
    echo 'case "$2" in'
    echo '    *.rs) echo "size: 1 finding(s)"; echo "  $2: 9999 production lines"; exit 1 ;;'
    echo '    *) exit 0 ;;'
    echo 'esac'
} > "$stub"
chmod +x "$stub"

run "guard-watch: reports when the binary exits non-zero" \
    guard-watch "$(json_path "$root/wt/src/a.rs")" context "9999"

# A path the guards do not read reaches the binary now that the hook keeps no
# extension list of its own, and must come back silent because of the path —
# not because the stub is silent for everything.
run "guard-watch: silent for a file no guard reads" \
    guard-watch "$(json_path "$root/wt/src/a.txt")" silent

run "guard-watch: reaches a later file in a multi-file Codex patch" \
    guard-watch "$(json_patch_two "$root/wt" "src/a.txt" "src/a.rs")" context "src/a.rs"

# A relative `file_path` would make `dirname` answer `.`, and the git queries
# would then resolve against the hook's own working directory — reporting on
# a same-named file in a checkout the author is not editing.
run "guard-watch: silent for a relative path" \
    guard-watch '{"tool_name":"Write","tool_input":{"file_path":"src/a.rs"}}' silent

# A file outside any git repository cannot be made relative to a workspace
# root, and the mode fails open rather than guessing.
run "guard-watch: silent outside a repository" \
    guard-watch "$(json_path "$root/loose.rs")" silent

rm -rf "$root/wt/target"

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

# --- full_ci.sh and ci.yml must name the same jobs and label ----------------
#
# Another boundary nothing type-checks, read from this repository for the same
# reason as the markers above: full_ci.sh requires check runs by job name, and
# ci.yml gives those names and reads the `full-ci` label the gate's denial
# tells an agent to add. Rename a job on one side only and every ready flip is
# refused for a check that can never exist.
workflow="$repo_root/.github/workflows/ci.yml"
full_ci_checks=$(sed -n "s/^FULL_CI_CHECKS='\([^']*\)'.*/\1/p" "$script_dir/full_ci.sh")
if [ -z "$full_ci_checks" ]; then
    printf 'FAIL full_ci.sh declares no FULL_CI_CHECKS\n'
    failed=$((failed + 1))
fi
for check in $full_ci_checks; do
    if grep -q "^  $check:\$" "$workflow"; then
        passed=$((passed + 1))
    else
        printf 'FAIL full_ci.sh requires check %s, which ci.yml defines no job for\n' "$check"
        failed=$((failed + 1))
    fi
done
if grep -q "'full-ci'" "$workflow" && grep -q 'add-label full-ci' "$GUARDRAILS"; then
    passed=$((passed + 1))
else
    printf 'FAIL the full-ci label named in the gate denial is not the one ci.yml reads\n'
    failed=$((failed + 1))
fi

# --- Claude and Codex must expose the same repository workflows ------------

skill_parity() {
    parity_root=$1
    [ -d "$parity_root/.claude/skills" ] || return 1
    [ -d "$parity_root/.agents/skills" ] || return 1

    claude_skills=$(
        for skill_file in "$parity_root"/.claude/skills/*/SKILL.md; do
            [ -f "$skill_file" ] || continue
            basename "$(dirname "$skill_file")"
        done | sort
    )
    codex_skills=$(
        for skill_file in "$parity_root"/.agents/skills/*/SKILL.md; do
            [ -f "$skill_file" ] || continue
            basename "$(dirname "$skill_file")"
        done | sort
    )
    [ "$claude_skills" = "$codex_skills" ] || return 1

    for skill in $claude_skills; do
        adapter="$parity_root/.agents/skills/$skill/SKILL.md"
        grep -qF -- "../../../.claude/skills/$skill/SKILL.md" "$adapter" || return 1
        grep -qF -- "../../references/codex-compatibility.md" "$adapter" || return 1
        grep -qE -- "^name:[[:space:]]*$skill$" "$adapter" || return 1
    done
}

if skill_parity "$repo_root"; then
    passed=$((passed + 1))
else
    printf 'FAIL .claude/skills and .agents/skills do not expose the same canonical workflows\n'
    failed=$((failed + 1))
fi

# Pin the failure direction too. A comparison that accidentally ignored one
# tree would report the real repository green and this deliberately incomplete
# fixture green as well.
parity_fixture="$root/parity"
mkdir -p "$parity_fixture/.claude/skills/one" \
    "$parity_fixture/.agents/skills/one" \
    "$parity_fixture/.agents/references"
printf '%s\n' '---' 'name: one' 'description: fixture' '---' \
    > "$parity_fixture/.claude/skills/one/SKILL.md"
printf '%s\n' '---' 'name: one' 'description: fixture' '---' \
    '../../../.claude/skills/one/SKILL.md' \
    '../../references/codex-compatibility.md' \
    > "$parity_fixture/.agents/skills/one/SKILL.md"
if skill_parity "$parity_fixture"; then
    passed=$((passed + 1))
else
    printf 'FAIL the complete skill-parity fixture was rejected\n'
    failed=$((failed + 1))
fi
mkdir -p "$parity_fixture/.claude/skills/two"
printf '%s\n' '---' 'name: two' 'description: missing from Codex' '---' \
    > "$parity_fixture/.claude/skills/two/SKILL.md"
if skill_parity "$parity_fixture"; then
    printf 'FAIL skill parity accepted a Claude workflow with no Codex adapter\n'
    failed=$((failed + 1))
else
    passed=$((passed + 1))
fi

# The lifecycle configuration must keep delegating every mode to the one
# canonical script. The hook runner validates the JSON when the project layer
# is trusted; this test owns the repository-specific agreement.
codex_hooks="$repo_root/.codex/hooks.json"
if [ ! -f "$codex_hooks" ]; then
    printf 'FAIL .codex/hooks.json does not exist\n'
    failed=$((failed + 1))
else
    validate_json() {
        if command -v python3 >/dev/null 2>&1 && python3 --version >/dev/null 2>&1; then
            python3 -m json.tool "$1" >/dev/null
        elif command -v python >/dev/null 2>&1 && python --version >/dev/null 2>&1; then
            python -m json.tool "$1" >/dev/null
        elif command -v py >/dev/null 2>&1 && py -3 --version >/dev/null 2>&1; then
            py -3 -m json.tool "$1" >/dev/null
        else
            return 2
        fi
    }
    validate_json "$codex_hooks"
    json_status=$?
    case "$json_status" in
        0) passed=$((passed + 1)) ;;
        2) ;;
        *)
            printf 'FAIL .codex/hooks.json is not valid JSON\n'
            failed=$((failed + 1))
            ;;
    esac

    windows_commands=$(grep -c -- '"commandWindows"' "$codex_hooks")
    if [ "$windows_commands" -eq 4 ]; then
        passed=$((passed + 1))
    else
        printf 'FAIL Codex hooks define %s Windows commands, expected 4\n' "$windows_commands"
        failed=$((failed + 1))
    fi

    for mode in worktree-guard pr-gate commit-reminder guard-watch; do
        if grep -qF -- '.claude/hooks/guardrails.sh' "$codex_hooks" &&
            grep -qF -- "$mode" "$codex_hooks"; then
            passed=$((passed + 1))
        else
            printf 'FAIL Codex hooks do not delegate %s to guardrails.sh\n' "$mode"
            failed=$((failed + 1))
        fi
    done
fi

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

# Each review skill must call the shared producer under its own kind. The
# producer owns marker writes and durable receipts; a direct redirect in a
# skill would reopen the manual-marker path this suite guards.
review_skills=".claude/skills/arch-review/SKILL.md .claude/skills/delivery-review/SKILL.md .claude/skills/ai-review/SKILL.md"

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
    if grep -qF -- "review_report.sh publish $skill" "$repo_root/$doc"; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s does not publish through the shared %s producer\n' "$doc" "$skill"
        failed=$((failed + 1))
    fi
    if grep -q -- 'absolute-git-dir).*review-ok\|absolute-git-dir)/ai-review-complete' "$repo_root/$doc"; then
        printf 'FAIL %s still teaches a direct private marker write\n' "$doc"
        failed=$((failed + 1))
    else
        passed=$((passed + 1))
    fi
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

for marker in $markers; do
    if grep -qF -- "$marker" "$repo_root/.claude/hooks/review_report.sh"; then
        passed=$((passed + 1))
    else
        printf 'FAIL the shared report producer never owns %s\n' "$marker"
        failed=$((failed + 1))
    fi
done
if grep -qF -- 'campaign_context.sh" key' "$repo_root/.claude/hooks/review_report.sh"; then
    passed=$((passed + 1))
else
    printf 'FAIL the shared report producer does not use the canonical review key\n'
    failed=$((failed + 1))
fi

# AI completion uses the shared key producer and an explicit branch prefix.
# These fixed producer obligations must not vanish with a renamed marker.
for required in 'review_report.sh publish ai-review' 'AI-REVIEW: COMPLETE' \
    'ai_review_threads.sh list'; do
    if grep -qF -- "$required" "$repo_root/.claude/skills/ai-review/SKILL.md"; then
        passed=$((passed + 1))
    else
        printf 'FAIL AI completion producer lost required evidence command: %s\n' "$required"
        failed=$((failed + 1))
    fi
done

for required in 'At every tier copy the four reserved `G-AI` lines' \
    '**G-AI1**' '**G-AI2**' '**G-AI3**' '**G-AI4**'; do
    if grep -qF -- "$required" "$repo_root/.claude/skills/mission/SKILL.md"; then
        passed=$((passed + 1))
    else
        printf 'FAIL every mission tier no longer declares: %s\n' "$required"
        failed=$((failed + 1))
    fi
done

if grep -qF -- 'quantick-mission-summary:v1 block' \
    "$repo_root/.claude/hooks/mission_ship_gate.sh"; then
    passed=$((passed + 1))
else
    printf 'FAIL final completion no longer requires the concise mission summary\n'
    failed=$((failed + 1))
fi

for caller in mission ship; do
    if grep -qF -- "mission_ship_gate.sh $caller" "$repo_root/.claude/skills/$caller/SKILL.md"; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s does not invoke the shared final completion gate\n' "$caller"
        failed=$((failed + 1))
    fi
done

if [ "$(grep -cF -- 'require_green_checks' "$repo_root/.claude/hooks/mission_ship_gate.sh")" -ge 3 ] &&
    [ "$(grep -cF -- 'list_threads' "$repo_root/.claude/hooks/mission_ship_gate.sh")" -ge 3 ]; then
    passed=$((passed + 1))
else
    printf 'FAIL final completion no longer rechecks CI and AI threads after reconciliation\n'
    failed=$((failed + 1))
fi

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

if sh "$script_dir/campaign_context_test.sh"; then
    passed=$((passed + 1))
else
    failed=$((failed + 1))
fi

if sh "$script_dir/review_report_test.sh"; then
    passed=$((passed + 1))
else
    failed=$((failed + 1))
fi

if python3 -c 'import sys; assert sys.version_info.major == 3' >/dev/null 2>&1; then progress_python=python3;
elif python -c 'import sys; assert sys.version_info.major == 3' >/dev/null 2>&1; then progress_python=python;
else progress_python=python3; fi
if "$progress_python" "$script_dir/review_progress_test.py"; then
    passed=$((passed + 1))
else
    failed=$((failed + 1))
fi

for campaign_suite in "$repo_root/tools/campaign/test-architecture-a-coordinator-v2.py" \
    "$repo_root/tools/campaign/test_campaign_recovery.py"; do
    if "$progress_python" -B "$campaign_suite"; then
        passed=$((passed + 1))
    else
        failed=$((failed + 1))
    fi
done

# --- full_ci.sh: the verdict itself ------------------------------------------
#
# The gates above stub this script; these cases run the real one against a
# fake `gh` that answers the check-runs query with what the script's own jq
# filter would print for the check named in the URL. The filter itself was
# checked against GitHub when it was written: it keeps GitHub Actions runs,
# drops skipped ones and takes the newest.
full_ci_root="$root/full-ci"
mkdir -p "$full_ci_root/bin"
# The stub answers whatever check name the URL asks for, from a fixture file
# of that name. It used to answer `ci` and `windows` by name and exit 64 for
# anything else, which meant splitting the verification across more jobs turned
# every case here into "GitHub could not list" — a suite failing for a reason
# having nothing to do with what it grades. The names come from full_ci.sh now,
# the same place the script itself reads them.
cat > "$full_ci_root/bin/gh" <<'STUB'
#!/bin/sh
fixture=${QUANTICK_FULL_CI_FIXTURE:?}
[ "${1:-}" = api ] || exit 64
check=${2##*check_name=}
check=${check%%&*}
[ -f "$fixture/$check" ] || exit 64
answer=$(cat "$fixture/$check")
case "$answer" in
    unknown-commit) echo 'gh: No commit found for SHA: 0 (HTTP 422)' >&2; exit 1 ;;
    offline) echo 'gh: connection refused' >&2; exit 1 ;;
esac
printf '%s\n' "$answer"
STUB
chmod +x "$full_ci_root/bin/gh"

# Every job green unless a case says otherwise: the cases below are about the
# Linux verdict and the Windows verdict, and none of them should have to name
# each harness job to say "and the rest passed".
full_ci_case() {
    for full_ci_check in $full_ci_checks; do
        printf 'completed:success\n' > "$full_ci_root/$full_ci_check"
    done
    printf '%s\n' "$2" > "$full_ci_root/ci"
    printf '%s\n' "$3" > "$full_ci_root/windows"
    full_ci_out=$(QUANTICK_FULL_CI_FIXTURE="$full_ci_root" PATH="$full_ci_root/bin:$PATH" \
        sh "$script_dir/full_ci.sh" verify "$root/wt" 2>&1)
    full_ci_status=$?
    if [ "$full_ci_status" -eq "$4" ]; then
        case "$full_ci_out" in
            *"$5"*) passed=$((passed + 1)); return ;;
        esac
    fi
    printf 'FAIL full_ci.sh %s: expected exit %s with "%s", got %s: %s\n' \
        "$1" "$4" "$5" "$full_ci_status" "$full_ci_out"
    failed=$((failed + 1))
}

full_ci_case "both green" completed:success completed:success 0 ""
full_ci_case "only skipped runs are no full CI" missing completed:success 1 "No ci run has a verdict"
full_ci_case "a red windows fails" completed:success completed:failure 1 "windows run at"
full_ci_case "a running ci is not green" in_progress:none completed:success 1 "still in_progress"
full_ci_case "an unpushed head has no CI" unknown-commit completed:success 1 "is not on GitHub"
full_ci_case "an unreachable GitHub is unknown, not red" offline completed:success 2 "could not list"
full_ci_case "an unreadable answer is unknown" "" completed:success 2 "unreadable"

# Every name in FULL_CI_CHECKS is required, not only the two platforms the
# cases above vary. Redden each one in turn: a name that can go red without
# this loop going red is a name the gate does not really require.
for full_ci_check in $full_ci_checks; do
    for full_ci_other in $full_ci_checks; do
        printf 'completed:success\n' > "$full_ci_root/$full_ci_other"
    done
    printf 'completed:failure\n' > "$full_ci_root/$full_ci_check"
    full_ci_out=$(QUANTICK_FULL_CI_FIXTURE="$full_ci_root" PATH="$full_ci_root/bin:$PATH" \
        sh "$script_dir/full_ci.sh" verify "$root/wt" 2>&1)
    full_ci_status=$?
    case "$full_ci_status:$full_ci_out" in
        1:*"$full_ci_check run at"*) passed=$((passed + 1)) ;;
        *)
            printf 'FAIL full_ci.sh a red %s fails: got %s: %s\n' \
                "$full_ci_check" "$full_ci_status" "$full_ci_out"
            failed=$((failed + 1))
            ;;
    esac
done

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
