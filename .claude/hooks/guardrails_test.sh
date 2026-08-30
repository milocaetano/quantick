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

# --- fixture ----------------------------------------------------------------

root=$(mktemp -d)
trap 'git -C "$root/mainco" worktree remove --force "$root/wt" >/dev/null 2>&1; rm -rf "$root"' EXIT

git init -b main -q "$root/mainco"
git -C "$root/mainco" config user.email t@t
git -C "$root/mainco" config user.name t
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

# --- harness ----------------------------------------------------------------

# run <name> <mode> <stdin-json> <expect: deny|context|silent>
run() {
    name=$1
    mode=$2
    payload=$3
    expect=$4

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
    passed=$((passed + 1))
}

# run_deny_naming <name> <mode> <stdin-json> <substring the denial must carry>
#
# `run` above sees three states and cannot tell a denial for the right reason
# from a denial for the other one. With two markers gating the PR, that
# difference is the whole value of the message: an agent told "a review is
# missing" has to guess which, and guessing wrong costs a full review.
# stdout only, deliberately: the contract under test is the JSON decision, and
# folding stderr in would let a diagnostic satisfy the substring assertion. Drop
# one of the script's `2>/dev/null` guards and `cat: …/delivery-review-ok: No
# such file` on stderr would match a check for "delivery-review-ok" while the
# JSON named the other marker — the test passing as it sends the agent to the
# wrong review.
run_deny_naming() {
    dn_name=$1
    dn_out=$(printf '%s' "$3" | sh "$GUARDRAILS" "$2" 2>/dev/null)
    dn_status=$?

    if [ "$dn_status" -ne 0 ]; then
        printf 'FAIL %s: exited %s (guard-rails must always exit 0)\n' "$dn_name" "$dn_status"
        failed=$((failed + 1))
        return
    fi
    case "$dn_out" in
        *'"permissionDecision":"deny"'*) ;;
        *)
            printf 'FAIL %s: expected deny\n  output: %s\n' "$dn_name" "$dn_out"
            failed=$((failed + 1))
            return
            ;;
    esac
    case "$dn_out" in
        *"$4"*) passed=$((passed + 1)) ;;
        *)
            printf 'FAIL %s: denial did not name "%s"\n  output: %s\n' "$dn_name" "$4" "$dn_out"
            failed=$((failed + 1))
            ;;
    esac
}

# set_marker <marker-name> <sha, or empty to remove it>
set_marker() {
    if [ -z "$2" ]; then
        rm -f "$wt_git_dir/$1"
    else
        printf '%s\n' "$2" > "$wt_git_dir/$1"
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
# Two markers gate the PR and both must record the exact commit being shipped:
# arch-review (is it well built) and delivery-review (is it what was asked
# for). The cases below move one marker at a time, so a regression says which
# half broke instead of only that the gate stopped working.

stale_sha="0000000000000000000000000000000000000000"

set_marker arch-review-ok ""
set_marker delivery-review-ok ""

run "a bash command that is not gh pr create is ignored" \
    pr-gate "$(json_bash "$root/wt" "cargo test --workspace")" silent

# Every shape that runs `gh pr create` has to reach the gate. Each of these
# walked straight past it while only `&&`, `||` and `;` counted as separators,
# and the pipe is the one that matters most: `ship` step 6 tells the agent to
# `use gh pr create --body-file -`, and piping the body in is how that is
# spelled. A gate the documented spelling avoids is not a gate.
run "a piped gh pr create is gated" \
    pr-gate "$(json_bash "$root/wt" "cd $root/wt && cat body.md | gh pr create --body-file -")" deny

run "a newline-separated gh pr create is gated" \
    pr-gate "$(json_bash "$root/wt" "cd $root/wt\\ngh pr create --fill")" deny

run "an env-prefixed gh pr create is gated" \
    pr-gate "$(json_bash "$root/wt" "cd $root/wt && GH_TOKEN=x gh pr create --fill")" deny

run "a heredoc body piped into gh pr create is gated" \
    pr-gate "$(json_bash "$root/wt" "cd $root/wt && printf 'body' | gh pr create --body-file - --title x")" deny

# With neither recorded, the message must name arch-review: guardrails.sh
# states that order as a contract ("a delivery review of a branch the shape
# review is about to change is wasted work"), and without this assertion the
# two `require_marker` calls could be swapped with the suite still green.
run_deny_naming "with neither review recorded the gate names arch-review first" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" "arch-review-ok"

# Staleness, one marker at a time. Each of these keeps the *other* marker at
# HEAD, so the case can only pass through the staleness branch it is aiming
# at — with the other marker missing, every one of them would deny for the
# wrong reason and prove nothing about staleness at all.
set_marker arch-review-ok "$stale_sha"
set_marker delivery-review-ok "$head_sha"
run_deny_naming "an arch review recorded for an older commit is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" "arch-review-ok"

set_marker arch-review-ok "$head_sha"
set_marker delivery-review-ok "$stale_sha"
run_deny_naming "a delivery review recorded for an older commit is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" "delivery-review-ok"

# A marker holding something other than a sha must trip the gate, not break
# it: `deny` interpolates the contents into JSON, and a payload the harness
# cannot parse loses the decision and lets `gh pr create` through.
printf 'he said "hi"\nsecond line\n' > "$wt_git_dir/arch-review-ok"
run_deny_naming "a corrupt marker denies rather than emitting broken JSON" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" '"permissionDecisionReason"'

corrupt_out=$(printf '%s' "$(json_bash "$root/wt" "gh pr create --fill")" |
    sh "$GUARDRAILS" pr-gate 2>/dev/null)
case "$corrupt_out" in
    *'
'*) printf 'FAIL a corrupt marker leaks a raw newline into the JSON payload\n  output: %s\n' "$corrupt_out"
        failed=$((failed + 1)) ;;
    *'he said'*) printf 'FAIL a corrupt marker leaks its unescaped contents into the JSON payload\n  output: %s\n' "$corrupt_out"
        failed=$((failed + 1)) ;;
    *) passed=$((passed + 1)) ;;
esac

# One marker at a time again, now for absence rather than staleness.
set_marker arch-review-ok "$head_sha"
set_marker delivery-review-ok ""
run_deny_naming "arch-review alone does not open the PR" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" "delivery-review-ok"

set_marker arch-review-ok ""
set_marker delivery-review-ok "$head_sha"
run_deny_naming "delivery-review alone does not open the PR" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" "arch-review-ok"

set_marker arch-review-ok "$head_sha"
set_marker delivery-review-ok "$head_sha"
run "both reviews recorded for the exact HEAD is allowed" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" silent

# --- commit-reminder --------------------------------------------------------

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

set_marker arch-review-ok "$head_sha"
set_marker delivery-review-ok "$head_sha"
run "a leading cd finds the reviews recorded in that worktree" \
    pr-gate "$(json_bash "$root/mainco" "cd $root/wt && gh pr create --fill")" silent

run "a leading cd sends the reminder to the worktree branch" \
    commit-reminder "$(json_bash "$root/mainco" "cd $root/wt && git commit -m x")" context

run "a cd to a path that does not exist falls back to the session cwd" \
    commit-reminder "$(json_bash "$root/mainco" "cd $root/nowhere && git commit -m x")" silent

# --- the gate and the instructions must name the same markers ---------------
#
# The marker names cross a boundary nothing in the repo type-checks:
# guardrails.sh reads those files, and the prose tells an agent to write them.
# Rename one side only and the gate denies a branch whose review actually ran,
# handing back a recording line that does not fix it.
#
# This block is the one part of the suite that is NOT hermetic, and the header
# says so. It has to read the repository the script sits in, because the
# agreement between the script and the prose *is* the thing under test. The
# consequence to know: invoked by absolute path from another checkout, it
# grades that checkout's instruction files rather than the branch's, and an
# unrelated doc edit can turn it red.

repo_root=$(CDPATH='' cd -- "$script_dir/../.." && pwd)
markers=$(sed -n 's/^[A-Z_]*MARKER_NAME="\([^"]*\)".*/\1/p' "$GUARDRAILS")
markers_padded=" $(echo $markers) "

if [ -z "$markers" ]; then
    printf 'FAIL no MARKER_NAME constants found in guardrails.sh
'
    failed=$((failed + 1))
fi

# The files that describe the whole gate, and must therefore name every marker
# it requires. This list is written out rather than discovered, and the first
# attempt at discovering it is why: building the set by grepping for the
# current marker names made the check self-selecting, so a doc that renamed its
# marker dropped out of the set and the one-sided rename went unnoticed —
# verified, the suite stayed green. A stale list here fails loudly (a named
# file that does not exist); a self-selecting one fails silently.
flow_docs=".claude/hooks/README.md .claude/skills/mission/SKILL.md .claude/skills/ship/SKILL.md"

# The two review skills, each of which must carry its own recording command.
# Which marker belongs to which is deliberately not asserted here — that would
# be a third copy of the names. Direction two below checks the name each one
# actually writes.
review_skills=".claude/skills/arch-review/SKILL.md .claude/skills/delivery-review/SKILL.md"

# Direction one, per file rather than across the set. Checking "some file names
# it" would pass while the recording instruction was deleted from both mission
# and ship, because the README alone still named the marker.
for marker in $markers; do
    for doc in $flow_docs; do
        if [ ! -f "$repo_root/$doc" ]; then
            printf 'FAIL %s is listed as an instruction file but does not exist
' "$doc"
            failed=$((failed + 1))
        elif grep -qF -- "$marker" "$repo_root/$doc"; then
            passed=$((passed + 1))
        else
            printf 'FAIL %s never names %s, which the gate requires
' "$doc" "$marker"
            failed=$((failed + 1))
        fi
    done
done

# Each review skill records its own marker, rather than leaving that to a
# caller. The asymmetry this pins was a real bug on this branch: delivery-review
# owned its marker and arch-review did not, so the recording lived only in
# `ship` and `mission` and a standalone run of one skill silently recorded
# nothing.
for doc in $review_skills; do
    if grep -q -- 'absolute-git-dir)/' "$repo_root/$doc"; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s carries no marker-recording command of its own
' "$doc"
        failed=$((failed + 1))
    fi
done

# Direction two, and the one that catches a one-sided rename: every marker name
# the instructions tell an agent to *write* is one the gate reads. Anchored on
# the recording command's shape, not on the marker names — grepping for the
# current names is what let a renamed doc escape the set last time. Whatever
# name follows `absolute-git-dir)/`, it has to be a marker the script defines.
for doc in $flow_docs $review_skills CLAUDE.md; do
    [ -f "$repo_root/$doc" ] || continue
    for written in $(sed -n 's|.*absolute-git-dir)/\([A-Za-z0-9._-]*\).*|\1|p' "$repo_root/$doc" | sort -u); do
        case "$markers_padded" in
            *" $written "*) passed=$((passed + 1)) ;;
            *)
                printf 'FAIL %s tells an agent to write %s, which guardrails.sh never reads
' "$doc" "$written"
                failed=$((failed + 1))
                ;;
        esac
    done
done

# --- report -----------------------------------------------------------------

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
