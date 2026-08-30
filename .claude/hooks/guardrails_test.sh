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

# run <name> <mode> <stdin-json> <expect: deny|context|silent> [substring]
#
# The optional fifth argument asserts that the emitted reason carries a given
# string — which marker the denial names, say. It is a parameter rather than a
# second helper because the second helper skipped the exit-status and
# payload-shape checks, and six of the eight pr-gate cases ran through it.
run() {
    name=$1
    mode=$2
    payload=$3
    expect=$4
    want=${5:-}

    # stdout only: `run` shape-checks this text, and folding stderr in lets a
    # stray git warning ("LF will be replaced by CRLF", which this fixture
    # emits on Windows) fail a correct deny as "spans multiple lines". That
    # was an intermittent red on a required CI step.
    out=$(printf '%s' "$payload" | sh "$GUARDRAILS" "$mode" 2>/dev/null)
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
                printf 'FAIL %s: output did not carry "%s"
  output: %s
' "$name" "$want" "$out"
                failed=$((failed + 1))
                return
                ;;
        esac
    fi

    passed=$((passed + 1))
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

# ...but not the hooks themselves. `settings.json` arms every session from the
# main checkout's copy, so an edit there while it sits on `main` disarms both
# gates for every session and every branch at once, with no record and no
# override. That is a code change like any other and belongs on a branch.
run "editing the hooks in the main checkout on main is denied" \
    worktree-guard "$(json_path "$root/mainco/.claude/hooks/guardrails.sh")" deny

run "editing the hooks from a linked worktree is allowed" \
    worktree-guard "$(json_path "$root/wt/.claude/hooks/guardrails.sh")" silent

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

stale_sha="0000000000000000000000000000000000000000"

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
set_marker delivery-review-ok "$head_sha"
run "an arch review recorded for an older commit is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "arch-review-ok"

set_marker arch-review-ok "$head_sha"
set_marker delivery-review-ok "$stale_sha"
run "a delivery review recorded for an older commit is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"

# A marker holding something other than a sha must trip the gate, not break it:
# `deny` interpolates the contents into JSON, and a payload the harness cannot
# parse loses the decision and lets the command through. The delivery marker is
# parked at HEAD so the arch marker is the only thing left to complain about.
set_marker delivery-review-ok "$head_sha"
printf 'he said "hi"\nsecond line\n' > "$wt_git_dir/arch-review-ok"
run "a corrupt marker is reported as not a commit id" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny '(not a commit id)'

# Absence, one marker at a time. Each pins that the *other* being satisfied
# does not carry the branch through.
set_marker arch-review-ok "$head_sha"
set_marker delivery-review-ok ""
run "arch-review alone does not open the PR" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "delivery-review-ok"

set_marker arch-review-ok ""
set_marker delivery-review-ok "$head_sha"
run "delivery-review alone does not open the PR" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "arch-review-ok"

set_marker arch-review-ok "$head_sha"
set_marker delivery-review-ok "$head_sha"
run "both reviews recorded for the exact HEAD is allowed" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" silent

# The two halves of the gate's worst failure mode, pinned.
#
# A command that resolves to the main checkout must deny and must NOT name a
# directory: main's HEAD rarely moves, so markers written into the shared git
# dir keep matching for every later branch. And the remedy must name the
# worktree rather than `.`, because it is pasted into a shell whose cwd is the
# session's. Together those two lines were the bypass: follow the denial's own
# instruction from the session cwd, then issue the command with no leading
# `cd` — the shape the PowerShell tool's contract mandates — and the gate is
# off, for good.
set_marker arch-review-ok ""
set_marker delivery-review-ok ""
run "a command resolving to the main checkout is denied"     pr-gate "$(json_bash "$root/mainco" "gh pr create --fill")" deny "main checkout"

mc_out=$(printf '%s' "$(json_bash "$root/mainco" "gh pr create --fill")" |
    sh "$GUARDRAILS" pr-gate 2>/dev/null)
case "$mc_out" in
    *"$root/mainco/.git"*|*"rev-parse HEAD >"*)
        printf 'FAIL the main-checkout denial hands back a recording path
  output: %s
' "$mc_out"
        failed=$((failed + 1)) ;;
    *) passed=$((passed + 1)) ;;
esac

wt_out=$(printf '%s' "$(json_bash "$root/wt" "gh pr create --fill")" |
    sh "$GUARDRAILS" pr-gate 2>/dev/null)
case "$wt_out" in
    *'git -C .'*)
        printf 'FAIL the remedy uses `git -C .`, which resolves against the pasting shell
  output: %s
' "$wt_out"
        failed=$((failed + 1)) ;;
    *"$wt_git_dir"*) passed=$((passed + 1)) ;;
    *)
        printf 'FAIL the remedy does not name the worktree marker path
  output: %s
' "$wt_out"
        failed=$((failed + 1)) ;;
esac

# The escape hatch, and the ordering that keeps it from being a bypass.
#
# `runs_command` matches the gated command at the start of any segment, so a
# shell command that merely quotes the workflow — a doc line, a commit message,
# a heredoc PR body describing this gate — is denied along with the real thing.
# The skip file is the visible way past; the denial names it. It lives in the
# worktree's git dir and is checked *after* the main-checkout refusal, so one
# in the shared git dir cannot switch the gate off for every branch at once.
set_marker arch-review-ok ""
set_marker delivery-review-ok ""
run "the denial names the skip file, so the way out is discoverable"     pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "skip-pr-gate"

: > "$wt_git_dir/skip-pr-gate"
run "the skip file releases the command it was created for"     pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" silent
rm -f "$wt_git_dir/skip-pr-gate"
run "removing the skip file restores the gate"     pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny

# A skip file in the *shared* git dir must not disable the gate: the
# main-checkout refusal runs first, so it is never reached from there.
main_git_dir=$(git -C "$root/mainco" rev-parse --absolute-git-dir)
: > "$main_git_dir/skip-pr-gate"
run "a skip file in the shared git dir does not disable the gate"     pr-gate "$(json_bash "$root/mainco" "gh pr create --fill")" deny "main checkout"
run "and it does not leak into a linked worktree either"     pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny
rm -f "$main_git_dir/skip-pr-gate"

# The gate must reach commands run through the PowerShell tool too: on Windows
# that is the primary shell, and its payload carries the same `command` field.
# Only the matcher in `.claude/settings.json` decides whether the hook fires,
# which is why that matcher now names both tools.
set_marker arch-review-ok ""
set_marker delivery-review-ok ""
run "a PowerShell payload is gated like a Bash one" \
    pr-gate "$(printf '{"tool_name":"PowerShell","cwd":"%s","tool_input":{"command":"gh pr create --fill"}}' "$root/wt")" deny

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
markers_padded=" $(echo $markers) "

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
    written=$(grep -o -- 'absolute-git-dir)/[A-Za-z0-9._-]*' "$repo_root/$doc" | sed 's|.*/||' | sort -u)
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

# Every marker name the prose tells an agent to *write* is one the gate reads.
# Anchored on the recording command's shape, not on the names — grepping for
# the current names is what let a renamed doc escape the set. `grep -o`, not
# `sed`, because a leading `.*` is greedy and would see only the last recording
# command on a line.
for doc in $flow_docs $review_skills CLAUDE.md; do
    [ -f "$repo_root/$doc" ] || continue
    for written in $(grep -o -- 'absolute-git-dir)/[A-Za-z0-9._-]*' "$repo_root/$doc" | sed 's|.*/||' | sort -u); do
        case "$markers_padded" in
            *" $written "*) passed=$((passed + 1)) ;;
            *)
                printf 'FAIL %s tells an agent to write %s, which guardrails.sh never reads\n' "$doc" "$written"
                failed=$((failed + 1))
                ;;
        esac
    done
done

# --- report -----------------------------------------------------------------

printf '\n%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
