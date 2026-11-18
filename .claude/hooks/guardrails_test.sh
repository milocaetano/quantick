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

# Which copy of the script `run` invokes. The real one, except in the blocks
# that have to control what `full_ci.sh` answers: the gate finds that sibling
# beside itself, so the only way to stub it without teaching the script an env
# override -- a switch whose whole purpose would be to turn the gate off -- is
# to run a copy from a directory the fixture owns. The copy is `cp`ed from
# the real file, so nothing here tests a script that is not the shipped one.
GUARDRAILS_UNDER_TEST="$GUARDRAILS"

passed=0
failed=0

# The tier file name, read out of guardrails.sh rather than written here.
# Restating it would make this suite a second source of truth for it, and a
# test that agrees with itself about a renamed file proves nothing. Empty means
# the constant moved, which is a failure loud enough to stop: every tier case
# below would otherwise pass vacuously.
tier_file_name=$(sed -n 's/^TIER_FILE_NAME="\([^"]*\)".*/\1/p' "$GUARDRAILS")

# Loud, and deliberately not fatal. Stopping here would hide whatever else a
# rename broke behind one line of output. The substitutes keep every later case
# running and failing honestly: a file name nothing will ever match. `set_tier`
# must never be handed an empty name - it would build a path ending in a bare
# slash.
if [ -z "$tier_file_name" ]; then
    printf 'FAIL the tier file name is not readable from guardrails.sh\n'
    failed=$((failed + 1))
    tier_file_name="mission-tier-unreadable"
fi

# --- fixture ----------------------------------------------------------------

root=$(mktemp -d)
trap 'git -C "$root/mainco" worktree remove --force "$root/wt" >/dev/null 2>&1; git -C "$root/mainco" worktree remove --force "$root/rv" >/dev/null 2>&1; rm -rf "$root"' EXIT

git init -b main -q "$root/mainco"
git -C "$root/mainco" config user.email t@t
git -C "$root/mainco" config user.name t
# The fixture, not the harness, is where the noise belongs: without this,
# git's "LF will be replaced by CRLF" warnings on Windows reach the
# captured output and fail correct cases. Discarding stderr in `run`
# instead would hide every diagnostic guardrails.sh ever emits.
git -C "$root/mainco" config core.autocrlf false
git -C "$root/mainco" config core.safecrlf false
mkdir -p "$root/mainco/src"
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

# A branch for review-verdict, whose cases commit on it.
git -C "$root/mainco" worktree add -q -b feat/verdict "$root/rv"
echo verdict > "$root/rv/src/a.txt"
git -C "$root/rv" add -A
git -C "$root/rv" commit -qm "reviewed change"
rv_head=$(git -C "$root/rv" rev-parse HEAD)
rv_git_dir=$(git -C "$root/rv" rev-parse --absolute-git-dir)
# A failed `worktree add` leaves this empty, and an empty git dir turns every
# helper below into a write at the filesystem root. The helpers refuse an empty
# argument as well; this is the louder half, because it names the fixture.
if [ -z "$rv_git_dir" ]; then
    printf 'FAIL the review-verdict fixture worktree was not created\n'
    failed=$((failed + 1))
fi

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
# One marker gates a PR that leaves draft: `code-review-ok`, recording the exact
# change being shipped. A draft opens with no marker on every branch but an
# `implement` one, which owes the review before any PR (its section is below).

# Derived, not literal: `require_marker` grades any marker whose length
# differs from HEAD's as "(not a commit id)", so a hardcoded 40 zeroes
# would take the corrupt path instead of the staleness path in a repo
# using a longer hash — still green, testing nothing it claims to.
stale_sha=$(printf '%*s' "${#head_sha}" '' | tr ' ' 0)

# What the marker must hold. A function of the diff, not of the commit that
# carries it.
wt_key=$(marker_key "$root/wt")

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

set_marker code-review-ok ""

run "a bash command that is not gh pr create is ignored" \
    pr-gate "$(json_bash "$root/wt" "cargo test --workspace")" silent

run "gh pr create without a recorded review is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "code-review-ok"

set_marker code-review-ok "$stale_sha"
run "a code-review recorded for an older change is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "was recorded for change"

# A marker holding something other than a sha must trip the gate, not break it:
# `deny` interpolates the contents into JSON, and a payload the harness cannot
# parse loses the decision and lets the command through.
printf 'he said "hi"\nsecond line\n' > "$wt_git_dir/code-review-ok"
run "a corrupt marker is reported as not a commit id" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny '(not a commit id)'

: > "$wt_git_dir/code-review-ok"
run "an empty marker is reported as not a commit id" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny '(not a commit id)'

set_marker code-review-ok "$wt_key extra"
run "a key with a word beside it is not a commit id" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny '(not a commit id)'

# The marker lives in the worktree's own git dir. The identical key recorded in
# another worktree's is a review of that worktree, not of this one.
set_marker code-review-ok ""
set_marker_in "$rv_git_dir" code-review-ok "$wt_key"
run "another worktree cannot supply the marker" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "has not been recorded"
set_marker_in "$rv_git_dir" code-review-ok ""

set_marker code-review-ok "$wt_key"
run "a code-review of the exact change opens a non-draft PR" \
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
set_marker code-review-ok ""
run "the script judges the payload, not the tool that produced it" \
    pr-gate "$(printf '{"tool_name":"PowerShell","cwd":"%s","tool_input":{"command":"gh pr create --fill"}}' "$root/wt")" deny

# --- pr-gate: the draft PR and the merge gate -------------------------------
#
# A draft ships nothing, so it opens with no marker. A non-draft creation,
# `gh pr ready` and `gh pr merge` want the code-review; ready and merge also
# want full CI at the exact head.
#
# These cases run a copy of the script from a fixture directory, because the
# gate resolves `full_ci.sh` beside itself and its verdict has to be controlled
# without reaching GitHub. `fullci` in that directory is what the stub answers
# (`green` when absent); the real script's own cases run further down against a
# fake `gh`.
mkdir -p "$root/hooks"
cp "$GUARDRAILS" "$root/hooks/guardrails.sh"
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

# A draft PR opens with no marker at all. Gating it would ask for the review
# before the PR its findings are discussed on exists.
set_marker code-review-ok ""
run "a draft PR opens with no marker recorded" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill")" silent

run "the short spelling of draft is a draft too" \
    pr-gate "$(json_bash "$root/wt" "gh pr create -d --fill")" silent

# The one spelling that contains the flag and means the opposite of it. Read as
# a draft, it would open an ungated real PR.
run "--draft=false is not a draft" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft=false --fill")" deny "code-review-ok"

# The flag is read from what the command *does*, never from what it says. The
# first version globbed the whole statement, so this exact spelling opened a
# real, un-reviewed PR -- and the PR that introduced the draft exemption is
# itself the PR whose title is about `--draft`.
run "a --draft inside a quoted title is not a draft flag" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --title \"Make --draft PRs ungated\" --fill")" deny "code-review-ok"

run "a -d inside a quoted body is not a draft flag either" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --body \"pass -d to open a draft\" --fill")" deny "code-review-ok"

# `--draft=f` is what gh's own flag parser calls false, and so are `F`,
# `False` and `FALSE`. Excluding only `false` and `0` left four spellings of
# "not a draft" opening a real PR with no review.
for draft_false in f F False FALSE 0 false; do
    run "--draft=$draft_false opens a real PR and is gated" \
        pr-gate "$(json_bash "$root/wt" "gh pr create --draft=$draft_false --fill")" deny "code-review-ok"
done

for draft_true in 1 t T true TRUE True; do
    run "--draft=$draft_true is a draft" \
        pr-gate "$(json_bash "$root/wt" "gh pr create --draft=$draft_true --fill")" silent
done

# A quote this cannot pair off means the blanking cannot be trusted, so the
# exemption is refused rather than guessed at.
run "an unbalanced quote is never a draft" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --title \"unclosed --draft --fill")" deny "code-review-ok"

# The flag has to come from the statement the gate matched. A `--draft`
# anywhere else on the line is somebody else's argument.
run "a draft flag in a neighbouring statement is not this PR's" \
    pr-gate "$(json_bash "$root/wt" "echo --draft && gh pr create --fill")" deny "code-review-ok"

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
        pr-gate "$(json_bash "$root/readcost" "gh pr create --fill")" deny "code-review-ok"
fi

run "gh pr ready without a code-review is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 42")" deny "code-review-ok"

run "gh pr merge without a code-review is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 42 --squash")" deny "code-review-ok"

# An untiered branch, through both hosts' shared hook entry point: the draft
# is free, and leaving draft wants the code-review and full CI.
for client in Bash exec_command; do
    set_tier "$root/wt" ""
    set_marker code-review-ok ""
    run "$client untiered draft opens with no marker" \
        pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill" "$client")" silent
    run "$client untiered non-draft creation wants the code-review" \
        pr-gate "$(json_bash "$root/wt" "gh pr create --fill" "$client")" deny "code-review-ok"
    run "$client untiered ready wants the code-review" \
        pr-gate "$(json_bash "$root/wt" "gh pr ready 42" "$client")" deny "code-review-ok"
    set_marker code-review-ok "$wt_key"
    run "$client untiered non-draft creation opens on the code-review" \
        pr-gate "$(json_bash "$root/wt" "gh pr create --fill" "$client")" silent
    run "$client untiered ready passes on the code-review and full CI" \
        pr-gate "$(json_bash "$root/wt" "gh pr ready 42" "$client")" silent
done
set_tier "$root/wt" ""

run "a code-review and full CI make the branch ready" \
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
# The code-review is recorded here; only the full-CI verdict moves.
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

run "even reviewed main merges remain exclusively human" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 42 --squash")" deny "reserved exclusively"

# The `cd <worktree> &&` prefix an agent needs to reach its worktree is read for
# the integration form pins only. On a main-based branch it changes nothing: the
# ready case still passes on its markers, and the merge is still the user's.
run "a cd prefix leaves a main-based ready where it was" \
    pr-gate "$(json_bash "$root/wt" "cd $root/wt && gh pr ready 42")" silent

run "a cd prefix does not make a main merge the agent's" \
    pr-gate "$(json_bash "$root/wt" "cd $root/wt && gh pr merge 42 --squash")" deny "reserved exclusively"

# The PR number is named, never guessed: full CI and the integration checks are
# read for one PR, and a guess would read another's.
run "gh pr ready naming no PR is denied" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready")" deny "Name the PR number explicitly"

# Two bare numbers and the operand cannot be told from a flag's value.
run "two bare numbers are an ambiguous PR, not a guess" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 306 42 --squash")" deny "Name the PR number explicitly"

# A compound line runs two of the gated commands. The draft exemption belongs
# to the `create` half only, and taking it for the whole line would let the
# `gh pr ready` beside it through with no review.
set_marker code-review-ok ""
run "a draft create beside a ready is judged as the ready" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill && gh pr ready")" deny "code-review-ok"

# The PR number is a whole word. Reading a digit run out of a filename made
# `--body-file notes2.md 42` name PR 2, and the full-CI denial names the PR it
# read.
set_marker code-review-ok "$wt_key"
set_full_ci missing
run "a digit inside a filename is not the PR number" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge --body-file notes2.md 42")" deny "gh pr edit 42 --add-label full-ci"
set_full_ci green
GUARDRAILS_UNDER_TEST="$GUARDRAILS"

# --- ai_review_threads.sh: the exit-status contract ------------------------
#
# "No list" and "an empty list" are different answers, and the whole of that
# distinction rests on this script's exit status. Neither
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

# --- pr-gate: the implement tier --------------------------------------------
#
# One review, owed before any PR, draft included.
set_tier "$root/wt" implement
set_marker code-review-ok ""
run "an implement branch cannot open a draft before code-review" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill")" deny "code-review-ok"
set_marker code-review-ok "$(marker_key "$root/wt")"
run "an implement branch opens its PR on code-review alone" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" silent
run "an implement branch goes ready on code-review alone" \
    pr-gate "$(json_bash "$root/wt" "gh pr ready 7")" silent
run "an implement branch still leaves the merge to main to the user" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 7 --squash")" deny "reserved exclusively for the user"
# The merge grant names non-main targets only; a `main` line in it buys nothing.
merge_grant="$(git -C "$root/wt" rev-parse --path-format=absolute --git-common-dir)/agent-merge-grant"
printf 'main\n' > "$merge_grant"
run "a merge grant cannot name main" \
    pr-gate "$(json_bash "$root/wt" "gh pr merge 7 --squash")" deny "reserved exclusively for the user"
rm -f "$merge_grant"
set_marker code-review-ok "stale"
run "a code-review of another change does not count" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --fill")" deny "code-review-ok"
set_marker code-review-ok ""

# A declaration belongs to the branch that made it. The first version of the
# tier file stored the word alone, and a worktree reused for a second branch
# inherited a tier it never declared. With no code-review recorded, a draft
# tells the two apart: an `implement` branch is denied it, any other opens it.
git -C "$root/wt" checkout -q -b feat/inherits
echo three > "$root/wt/src/a.txt"
git -C "$root/wt" add -A
git -C "$root/wt" commit -qm "a different goal, same worktree"
run "a second branch in the same worktree does not inherit the tier" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill")" silent
git -C "$root/wt" checkout -q feat/x

# A detached head names no branch. `rev-parse --abbrev-ref` prints the literal
# `HEAD` there, and the snippet that writes this file uses that same command -
# so without the guard, a declaration made while detached matches every future
# detached checkout in this worktree: the inheritance bug in a different hat.
git -C "$root/wt" checkout -q --detach
printf 'HEAD implement\n' > "$wt_git_dir/$tier_file_name"
run "a tier declared while detached declares nothing" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill")" silent
git -C "$root/wt" checkout -q feat/x

# One field is the format that caused the inheritance, so it is refused.
printf 'implement\n' > "$wt_git_dir/$tier_file_name"
run "a tier file naming no branch declares nothing" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill")" silent

set_tier "$root/wt" "implementish"
run "an unrecognised tier is no tier" \
    pr-gate "$(json_bash "$root/wt" "gh pr create --draft --fill")" silent
set_tier "$root/wt" ""

# --- review-verdict: the hook that records code-review-ok -------------------
#
# The reviewer's last verdict line records the marker; the implementer never
# writes it. On the `rv` fixture, so its commits move no other case.
set_tier "$root/rv" implement

# verdict_case <name> <stdin-json> <expected marker contents, empty for none>
verdict_case() {
    rm -f "$rv_git_dir/code-review-ok"
    printf '%s' "$2" | sh "$GUARDRAILS" review-verdict >/dev/null 2>&1
    verdict_status=$?
    verdict_got=$(cat "$rv_git_dir/code-review-ok" 2>/dev/null)
    if [ "$verdict_status" -eq 0 ] && [ "$verdict_got" = "$3" ]; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s: exited %s, marker "%s", wanted "%s"\n' "$1" "$verdict_status" "$verdict_got" "$3"
        failed=$((failed + 1))
    fi
}
stop_json() { printf '{"hook_event_name":"SubagentStop","agent_id":"a1","last_assistant_message":"Findings: none.\\n%s"}' "$1"; }
handback_json() { printf '{"hook_event_name":"PostToolUse","agent_id":"a1","tool_name":"SubagentHandback","tool_input":{"message":"Reviewed.\\n%s"},"tool_response":{"success":true}}' "$1"; }

verdict_case "a passing review records the exact key pr-gate checks" \
    "$(stop_json "REVIEW-VERDICT: pass $root/rv $rv_head")" "$(marker_key "$root/rv")"
verdict_case "a background review's handback records it too" \
    "$(handback_json "REVIEW-VERDICT: pass $root/rv $(printf '%.10s' "$rv_head")")" "$(marker_key "$root/rv")"
verdict_case "a failing review records nothing" \
    "$(stop_json "REVIEW-VERDICT: fail $root/rv $rv_head")" ""
verdict_case "a report with no verdict line records nothing" \
    "$(stop_json "Findings: none.")" ""
verdict_case "a verdict on another HEAD records nothing" \
    "$(stop_json "REVIEW-VERDICT: pass $root/rv 0123456789abcdef")" ""
verdict_case "a main-agent tool payload records nothing" \
    "$(printf '{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{"command":"echo REVIEW-VERDICT: pass %s %s"}}' "$root/rv" "$rv_head")" ""
verdict_case "the brief's template records nothing" \
    "$(stop_json "End with REVIEW-VERDICT: pass|fail $root/rv $rv_head")" ""
verdict_case "markdown around the verdict still records it" \
    "$(stop_json "**REVIEW-VERDICT: **pass** \`$root/rv\` $rv_head**")" "$(marker_key "$root/rv")"
# pr-gate asks every branch for the marker, so the hook records it whatever
# tier the worktree declared; were it to record only `implement`, a non-draft PR
# on any other branch would wait on a record nothing writes.
set_tier "$root/rv" ""
verdict_case "a worktree outside the implement tier records it too" \
    "$(stop_json "REVIEW-VERDICT: pass $root/rv $rv_head")" "$(marker_key "$root/rv")"
set_tier "$root/rv" implement

printf '%s' "$(stop_json "REVIEW-VERDICT: pass $root/rv $rv_head")" | sh "$GUARDRAILS" review-verdict
printf '%s' "$(stop_json "REVIEW-VERDICT: fail $root/rv $rv_head")" | sh "$GUARDRAILS" review-verdict
if [ -e "$rv_git_dir/code-review-ok" ]; then
    printf 'FAIL a failing review of the same HEAD removes an earlier pass\n'
    failed=$((failed + 1))
else
    passed=$((passed + 1))
fi

run "pr-gate's denial names the verdict line, not a hand-written marker" \
    pr-gate "$(json_bash "$root/rv" "gh pr create --fill")" deny "REVIEW-VERDICT: pass"
printf '%s' "$(stop_json "REVIEW-VERDICT: pass $root/rv $rv_head")" | sh "$GUARDRAILS" review-verdict
run "pr-gate accepts the hook's record" \
    pr-gate "$(json_bash "$root/rv" "gh pr create --fill")" silent
echo after > "$root/rv/src/a.txt"
git -C "$root/rv" commit -qam "unreviewed change"
run "a commit after the review leaves the record stale" \
    pr-gate "$(json_bash "$root/rv" "gh pr create --fill")" deny "code-review-ok"

# --- pr-gate: the implement merge grant -------------------------------------
#
# A plain /implement child of feature/demo may merge itself only when the
# trader listed that target in the grant file, and only on green PR checks.
# A copy of the gate with the base reader beside it, and a `gh` that answers
# `pr checks` from the fixture.
cp "$script_dir/mission_base.sh" "$root/hooks/mission_base.sh"
GUARDRAILS_UNDER_TEST="$root/hooks/guardrails.sh"
mkdir -p "$root/bin" "$root/grant"
cat > "$root/bin/gh" <<'STUB'
#!/bin/sh
[ "${1:-} ${2:-}" = "pr checks" ] || exit 64
cat "${QUANTICK_GRANT_FIXTURE:?}/checks"
STUB
chmod +x "$root/bin/gh"
git -C "$root/mainco" worktree add -q -b feature/demo "$root/feat" origin/main >/dev/null 2>&1
echo feature > "$root/feat/src/a.txt"
git -C "$root/feat" commit -qam 'integrated feature change'
git -C "$root/mainco" update-ref refs/remotes/origin/feature/demo "$(git -C "$root/feat" rev-parse HEAD)"
git -C "$root/mainco" worktree add -q -b feat/granted "$root/granted" origin/feature/demo >/dev/null 2>&1
echo granted > "$root/granted/src/granted.txt"
git -C "$root/granted" add -A && git -C "$root/granted" commit -qm 'granted implement change'
printf 'feat/granted origin/feature/demo none none\n' \
    > "$(git -C "$root/granted" rev-parse --absolute-git-dir)/mission-base"
set_tier "$root/granted" implement
set_marker_in "$(git -C "$root/granted" rev-parse --absolute-git-dir)" code-review-ok \
    "$(sh "$root/hooks/mission_base.sh" key "$root/granted")"
merge_grant="$(git -C "$root/granted" rev-parse --path-format=absolute --git-common-dir)/agent-merge-grant"
grant_path=$PATH
PATH="$root/bin:$PATH"
export QUANTICK_GRANT_FIXTURE="$root/grant"
run "an ungranted feature target leaves the merge to the user" \
    pr-gate "$(json_bash "$root/granted" "gh pr merge 42 --squash")" deny "reserved exclusively for the user"
printf 'feature/other\nfeature/demo\n' > "$merge_grant"
printf 'pass\nskipping\n' > "$root/grant/checks"
run "a granted feature target merges on passing checks" \
    pr-gate "$(json_bash "$root/granted" "gh pr merge 42 --squash")" silent
printf 'pass\nfail\n' > "$root/grant/checks"
run "a granted merge still waits for every check" \
    pr-gate "$(json_bash "$root/granted" "gh pr merge 42 --squash")" deny "waits for every PR check"
printf 'feature/demo-old\n' > "$merge_grant"
printf 'pass\n' > "$root/grant/checks"
run "a grant names its target exactly" \
    pr-gate "$(json_bash "$root/granted" "gh pr merge 42 --squash")" deny "reserved exclusively for the user"
rm -f "$merge_grant"
unset QUANTICK_GRANT_FIXTURE
PATH=$grant_path
GUARDRAILS_UNDER_TEST="$GUARDRAILS"

# --- commit-reminder --------------------------------------------------------

# Only `implement` changes what the reminder says. An untiered branch hears
# that the draft is free and leaving draft wants the review.
set_tier "$root/wt" ""
run "the untiered reminder says a draft opens freely" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context "A draft PR opens freely"

set_tier "$root/wt" implement
run "the reminder at the implement tier names the review owed before any PR" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context 'at the `implement` tier'

# Cleared before anything else runs: every case from here on assumes no
# declaration, and a leftover file would quietly change what they test.
set_tier "$root/wt" ""

run "the untiered reminder names the code-review the gate wants" \
    commit-reminder "$(json_bash "$root/wt" "git commit -m x")" context 'need a current `code-review-ok`'

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

set_marker code-review-ok ""

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

set_marker code-review-ok "$(marker_key "$root/wt")"
run "a leading cd finds the review recorded in that worktree" \
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
markers=$(sed -n 's/^[A-Z_]*MARKER[A-Z_]*="\([^"]*\)".*/\1/p' "$GUARDRAILS")

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

    # Codex already runs `commandWindows` in PowerShell. A nested
    # `powershell.exe -Command "..."` has its `$` variables expanded empty by
    # the outer shell, so every hook exits 0 and the gate silently fails open.
    # Each command must also hand the guardrail's exit code back.
    nested_shells=$(grep -c -- '"commandWindows": "powershell' "$codex_hooks")
    forwarded_exits=$(grep -c -- '"commandWindows": .*exit \$LASTEXITCODE",' "$codex_hooks")
    if [ "$nested_shells" -eq 0 ] && [ "$forwarded_exits" -eq 4 ]; then
        passed=$((passed + 1))
    else
        printf 'FAIL Codex Windows hooks nest PowerShell (%s) or drop the exit code (%s of 4 forward it)\n' "$nested_shells" "$forwarded_exits"
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
    printf 'FAIL no MARKER constants found in guardrails.sh\n'
    failed=$((failed + 1))
fi

# The files that describe the whole gate, and must name every marker it needs.
# Written out rather than discovered: building the set by grepping for the
# current names made the check self-selecting, so a doc that renamed its marker
# dropped out of the set and the rename went unnoticed. A stale list here fails
# loudly; a self-selecting one failed silently.
flow_docs=".claude/hooks/README.md"

# Per file, not "somewhere among them": checking the set would stay green while
# the instruction vanished from one of them.
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

# Every file name the prose tells an agent to *write* into a git dir is one the
# gate reads - the review marker and the tier file.
# Anchored on the recording command's shape, not on the names — grepping for
# the current names is what let a renamed doc escape the set. `grep -o`, not
# `sed`, because a leading `.*` is greedy and would see only the last recording
# command on a line.
for doc in $flow_docs CLAUDE.md; do
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

# --- the gate and the implement skill must write the same tier file --------
#
# The same boundary the markers cross, one layer along: guardrails.sh reads the
# tier file and the `implement` skill writes it, and the README documents it.
# Rename the file or reorder its fields on one side only and a goal declares a
# tier the gate never sees - and nothing says so. Pinned on the whole recording
# command: the branch first, then `implement`, into the file the gate reads.

tier_snippet='echo "$(git branch --show-current) implement" > "$(git rev-parse --absolute-git-dir)/'"$tier_file_name"'"'
for doc in .claude/hooks/README.md .claude/skills/implement/references/execute.md; do
    if [ ! -f "$repo_root/$doc" ]; then
        printf 'FAIL %s is checked for the tier-recording command but does not exist\n' "$doc"
        failed=$((failed + 1))
    elif grep -qF -- "$tier_snippet" "$repo_root/$doc"; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s does not write the tier file in the branch-first format the gate parses\n' "$doc"
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

# And no document that describes the gate may still say a marker holds a
# commit sha. Command drift and prose drift are different failures: the first
# hands an agent a marker the gate rejects, the second teaches the next reader
# a rule that stopped being true. Round 3 of this branch's own delivery review
# found the second one surviving in `docs/agentic-development.md` after all
# four command sites had been fixed - in the passage that introduces the
# marker as "the one design decision that makes the gate honest".
for doc in .claude/hooks/README.md CLAUDE.md docs/agentic-development.md; do
    if [ -f "$repo_root/$doc" ] &&
        grep -qiE 'marker[^.]*(holds|holding|stores|storing|records|recording)[^.]*(commit sha|sha of|exact commit)' "$repo_root/$doc"; then
        printf 'FAIL %s still describes a marker as holding a commit sha\n' "$doc"
        failed=$((failed + 1))
    else
        passed=$((passed + 1))
    fi
done

for doc in .claude/hooks/README.md; do
    if [ -f "$repo_root/$doc" ] &&
        grep -qF -- 'git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)' "$repo_root/$doc"; then
        printf 'FAIL %s still records a marker as a commit id, which the gate no longer accepts\n' "$doc"
        failed=$((failed + 1))
    else
        passed=$((passed + 1))
    fi
done

# --- report -----------------------------------------------------------------

if sh "$script_dir/mission_base_test.sh"; then
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
