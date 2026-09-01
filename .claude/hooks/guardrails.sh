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
#   pr-gate           PreToolUse on Bash. Denies `gh pr create`
#                     until BOTH reviews have been recorded for the exact
#                     commit being shipped: arch-review ("no branch ships
#                     un-reviewed") and delivery-review ("no branch ships
#                     ungraded against what was asked for").
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
    require_marker "$dir" "$head" "$ARCH_MARKER_NAME" \
        "no branch ships un-reviewed" \
        "Run the arch-review skill over \`git diff origin/$MAIN_BRANCH...HEAD\` and resolve every Blocker and Should-fix (or note the deferral in the PR body)"

    require_marker "$dir" "$head" "$DELIVERY_MARKER_NAME" \
        "no branch ships ungraded against what was asked for" \
        "Run the delivery-review skill: it grades every ask in the branch's goal file and every acceptance criterion, and passes only when none is MISSING, PARTIAL or UNPROVEN"

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

    context "\"Branch \`$branch\` is $ahead commit(s) ahead of origin/$MAIN_BRANCH. \`gh pr create\` is gated on both \`$ARCH_MARKER_NAME\` and \`$DELIVERY_MARKER_NAME\` recording the exact HEAD being shipped, so run arch-review and then delivery-review once the branch is final — a commit after either one makes its marker stale.\""
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
guard_watch() {
    file=$(normalize_path "$(json_string_field file_path)")
    [ -n "$file" ] || exit 0

    # No extension filter here on purpose. One lived here and was a third
    # hand-kept copy of a list the two Rust guards already own; adding an
    # extension there while forgetting it here would have left the suite
    # seeing a file the edit-time hook silently did not — an all-clear that
    # reads exactly like a clean file. Each guard's `check_file` already
    # returns nothing for a path it does not read, so the filter bought a
    # process spawn and cost a drift.
    dir=$(dirname "$file")
    [ -d "$dir" ] || exit 0
    root=$(git -C "$dir" rev-parse --show-toplevel 2>/dev/null) || exit 0
    root=$(normalize_path "$root")

    # The binary the branch already built, under either name this platform
    # gives it. Absent is the ordinary case on a fresh worktree, and silence
    # is the correct answer to it.
    binary="$root/target/debug/quantick-guards"
    [ -x "$binary" ] || binary="$binary.exe"
    [ -x "$binary" ] || exit 0

    # Workspace-relative, forward slashes: the spelling the baseline uses.
    # Computed by git rather than by subtracting the root from the absolute
    # path, because the two are not spelled alike here — the payload carries
    # the path the way the host writes it and `--show-toplevel` answers the
    # way git does, so under Git Bash `/tmp/x/src/a.rs` and
    # `C:/Users/.../Temp/x` describe the same tree and share no prefix. The
    # subtraction silently produced no match, which reads exactly like a
    # clean file.
    prefix=$(git -C "$dir" rev-parse --show-prefix 2>/dev/null) || exit 0
    relative="$prefix$(basename "$file")"

    # The binary is told which root to read, because the one compiled into it
    # is whichever worktree happened to build it.
    findings=$(QUANTICK_GUARDS_ROOT="$root" "$binary" --file "$relative" 2>&1) && exit 0
    [ -n "$findings" ] || exit 0

    # JSON-escape by hand, because there is no jq here. Separate `-e` scripts
    # and a `|` delimiter: a `;`-joined script with a `/` delimiter is what
    # this environment's sed rejects, and it rejects it to stderr while the
    # pipeline still exits 0 — which produced an empty message that read as a
    # clean file.
    escaped=$(printf '%s' "$findings" |
        sed -e 's|\\|\\\\|g' -e 's|"|\\"|g' |
        awk 'BEGIN { ORS = "" } { if (NR > 1) printf "\\n"; print }')
    context "\"Repository guards on \`$relative\`:\n$escaped\n\nThis is advisory and blocks nothing; \`cargo test --workspace\` is still the gate. Fix it now while the edit is in hand, or run \`cargo run -p quantick-guards -- --tighten\` if a size entry only needs lowering.\""
}

case "$mode" in
    worktree-guard) worktree_guard ;;
    pr-gate) pr_gate ;;
    commit-reminder) commit_reminder ;;
    guard-watch) guard_watch ;;
    *) exit 0 ;;
esac
