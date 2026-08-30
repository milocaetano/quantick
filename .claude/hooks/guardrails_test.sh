#!/bin/sh
# Tests for .claude/hooks/guardrails.sh.
#
# The guard-rails are the only thing standing between a long agent session and
# a commit on the wrong checkout, so they need the same bar as the rest of the
# repo: a test that fails without the behaviour. Run from anywhere:
#
#   sh .claude/hooks/guardrails_test.sh
#
# Hermetic: builds throwaway git repos under a temp dir and removes them.
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
run_deny_naming() {
    dn_name=$1
    dn_out=$(printf '%s' "$3" | sh "$GUARDRAILS" "$2" 2>&1)
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

run "gh pr create without a recorded review is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny

set_marker arch-review-ok "$stale_sha"
run "a review recorded for an older commit is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny

# The delivery gate, one marker at a time. Each case pins that the *other*
# marker being satisfied does not carry the branch through.

set_marker arch-review-ok "$head_sha"
set_marker delivery-review-ok ""
run_deny_naming "arch-review alone does not open the PR" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" "delivery-review-ok"

set_marker delivery-review-ok "$stale_sha"
run_deny_naming "a delivery review recorded for an older commit is denied" \
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
# handing back a recording line that does not fix it. The repo's rule for a
# value that cannot be imported is a test pinning the two sides together, so
# the names come out of the script itself rather than being repeated here — a
# third copy in the test would be the same bug wearing a different hat.

repo_root=$(CDPATH='' cd -- "$script_dir/../.." && pwd)
markers=$(sed -n 's/^[A-Z_]*MARKER_NAME="\([^"]*\)".*/\1/p' "$GUARDRAILS")

if [ -z "$markers" ]; then
    printf 'FAIL no MARKER_NAME constants found in guardrails.sh\n'
    failed=$((failed + 1))
fi

for marker in $markers; do
    for doc in .claude/hooks/README.md .claude/skills/mission/SKILL.md .claude/skills/ship/SKILL.md; do
        if grep -q -- "$marker" "$repo_root/$doc"; then
            passed=$((passed + 1))
        else
            printf 'FAIL %s never names %s, which the gate requires\n' "$doc" "$marker"
            failed=$((failed + 1))
        fi
    done
done

# --- report -----------------------------------------------------------------

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
