#!/bin/sh
# Final mission/ship completion gate. Unlike the PreToolUse hook, this command
# also runs when a PR is already non-draft, so readiness cannot be inferred
# from having crossed `gh pr ready` in an earlier session.

set -u

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)

fail() {
    printf 'MISSION-COMPLETION:FAIL %s\n' "$1" >&2
    exit 1
}

list_threads() {
    (cd "$worktree" && sh "$script_dir/ai_review_threads.sh" list "$pr")
}

# Two rules, both required. Every registered check that ran passed; a skipped
# one carries no verdict, and the draft-only fast job is skipped on every ready
# head. And full CI, the named `ci` and `windows` runs, passed at the exact
# head: without that second rule a PR whose full jobs never ran would pass on
# the fast job alone.
require_green_checks() {
    observed_checks=$(cd "$worktree" && gh pr checks "$pr" --json bucket --jq '.[].bucket') ||
        fail 'GitHub could not read the exact-head PR checks.'
    [ -n "$observed_checks" ] || fail 'No CI checks are registered for the current PR head.'
    observed_nonpassing=$(printf '%s\n' "$observed_checks" | grep -v -e '^pass$' -e '^skipping$')
    [ -z "$observed_nonpassing" ] || fail 'At least one exact-head CI check is not green.'
    full_ci_reason=$(sh "$script_dir/full_ci.sh" verify "$worktree" 2>&1 >/dev/null) ||
        fail "Full CI is not green at the exact head: $full_ci_reason"
}

mode=${1:-}
pr=${2:-}
worktree=${3:-.}
case "$mode" in mission|ship) ;; *) fail 'The caller must be mission or ship.' ;; esac
case "$pr" in ''|*[!0-9]*) fail 'Name one numeric PR.' ;; esac
worktree=$(CDPATH='' cd -- "$worktree" 2>/dev/null && pwd) || fail 'The task worktree does not exist.'

# Completion reads the literal list before the hook's count, preserving the
# unresolved set in the refusal rather than reducing it to a green-looking
# number from another path.
threads=$(list_threads) ||
    fail 'AI-review threads could not be listed; unknown is not zero.'
if [ -n "$threads" ]; then
    thread_count=$(printf '%s\n' "$threads" | wc -l | tr -d ' ')
    fail "PR #$pr has $thread_count unresolved AI-review threads returned by ai_review_threads.sh list."
fi

# Reuse the exact readiness policy rather than reimplementing markers, tier
# exemptions, campaign keys or thread counting here. A hook decision is a
# failure even though hooks themselves exit zero by design.
#
# A refusal is a `deny` decision, and only that. The same gate also carries
# advisories on stdout — the read-cost ceiling says "Nothing is blocked" in
# its own text — and reading any output as a refusal made every branch over
# that advisory ceiling unable to complete a mission, which is the opposite
# of what `docs/quality/read-cost/ledger.md` states the ceiling is for.
payload=$(printf '{"tool_name":"Bash","cwd":"%s","tool_input":{"command":"gh pr ready %s"}}' "$worktree" "$pr")
readiness=$(printf '%s' "$payload" | sh "$script_dir/guardrails.sh" pr-gate 2>&1) ||
    fail "The shared readiness gate could not be evaluated: $readiness"
case "$readiness" in
    '') ;;
    *'"permissionDecision":"deny"'*)
        fail "The shared readiness gate refused this PR: $readiness"
        ;;
    *'"hookEventName":"PreToolUse"'*) ;;
    *)
        # Neither a decision nor an advisory: the gate broke while exiting
        # zero, and unknown is not permission.
        fail "The shared readiness gate returned neither a decision nor an advisory: $readiness"
        ;;
esac

branch=$(git -C "$worktree" symbolic-ref --quiet --short HEAD) || fail 'A named task branch is required.'
head=$(git -C "$worktree" rev-parse HEAD) || fail 'Cannot read the current task head.'
base=$(sh "$script_dir/campaign_context.sh" base "$worktree") || fail 'Cannot read the review base.'
base_tip=$(git -C "$worktree" rev-parse "$base") || fail 'Cannot read the review base tip.'
key=$(sh "$script_dir/campaign_context.sh" key "$worktree") || fail 'Cannot compute the current review key.'
status=$(git -C "$worktree" status --porcelain=v1 --untracked-files=all) || fail 'Cannot read task status.'
[ -z "$status" ] || fail 'The task worktree is not clean.'

skill="$worktree/.claude/skills/mission/SKILL.md"
# Three PR kinds, told apart only by facts verified against the PR below (the
# branch is its head, the base its base), never by a flag the caller sets: a
# consolidated campaign PR (campaign/* into main), a main synchronization (into
# a campaign base, named sync/* or carrying main commits that base lacks), and
# every other PR, a mission.
kind=mission
case "$base:$branch" in
    origin/main:campaign/?*) kind=campaign ;;
    origin/campaign/?*:*)
        case "$branch" in sync/?*) kind=sync ;; esac
        main_fork=$(git -C "$worktree" merge-base origin/main HEAD 2>/dev/null) &&
            ! git -C "$worktree" merge-base --is-ancestor "$main_fork" "$base" && kind=sync
        ;;
esac
goal_note='Mission source stayed local; the PR carries its concise durable summary.'
[ "$kind" != sync ] || goal_note='Main synchronization: the PR carries its concise durable summary.'

remote=$(cd "$worktree" && gh pr view "$pr" \
    --json headRefName,headRefOid,baseRefName,baseRefOid,state,isDraft,mergeable,mergeStateStatus,url,isCrossRepository \
    --jq '[.headRefName,.headRefOid,.baseRefName,.baseRefOid,.state,(.isDraft|tostring),.mergeable,.mergeStateStatus,.url,(.isCrossRepository|tostring)] | join(" ")') ||
    fail 'GitHub could not provide the final PR identity.'
set -- $remote
[ "$#" -eq 10 ] || fail 'GitHub returned an incomplete final PR identity.'
[ "$1" = "$branch" ] && [ "$2" = "$head" ] && [ "$3" = "${base#origin/}" ] &&
    [ "$4" = "$base_tip" ] && [ "$5" = OPEN ] && [ "${10}" = false ] ||
    fail 'The open PR does not match the reviewed branch, head or base.'
[ "$6" = false ] || fail 'The PR is still a draft; run the gated ready transition before final completion.'
[ "$7" = MERGEABLE ] ||
    fail 'GitHub has not confirmed a mergeable PR; that signal is required but never sufficient.'
pr_url=$9

body=$(cd "$worktree" && gh pr view "$pr" --json body --jq .body) ||
    fail 'GitHub could not read the PR body evidence.'
if [ "$kind" = campaign ]; then
    # No single goal governs the consolidated PR: delivery-review grades the
    # parent charter's criteria, so the gate verifies that parent instead.
    parent=$(printf '%s\n' "$body" | sed -n 's/^Campaign-parent: *//p' | tr -d '\r')
    # The repository prefix is compared literally: a regex built from the URL
    # would let its dots match any character.
    parent_number=${parent#"${pr_url%/pull/*}/issues/"}
    [ "$(printf '%s\n' "$parent" | wc -l | tr -d ' ')" -eq 1 ] && [ "$parent_number" != "$parent" ] &&
        case "$parent_number" in ''|*[!0-9]*) false ;; esac ||
        fail 'A consolidated campaign PR names its charter once in its body: Campaign-parent: <issue URL in this repository>.'
    charter=$(cd "$worktree" && gh issue view "$parent" --json body --jq .body) ||
        fail 'GitHub could not read the campaign parent charter.'
    case "$charter" in *'<!-- quantick-campaign:v1 -->'*) ;; *) fail 'Campaign-parent is not a campaign charter.' ;; esac
    # The branch must appear as a whole ref token, in backticks or plain text:
    # split on characters a ref cannot hold and drop a sentence's final dot,
    # so campaign/x-2 never stands for campaign/x.
    printf '%s\n' "$charter" | tr -c 'A-Za-z0-9._/-' '\n' | sed 's/\.*$//' | grep -Fxq -- "$branch" ||
        fail "The campaign charter does not name \`$branch\`."
    goal_note="Consolidated campaign: charter $parent names \`$branch\`; delivery-review grades its criteria."
else
    # A body saved from the GitHub web editor ends its lines in CRLF.
    lf_body=$(printf '%s\n' "$body" | tr -d '\r')
    summary_count=$(printf '%s\n' "$lf_body" |
        grep -c '^<!-- quantick-mission-summary:v1 -->$')
    summary_end_count=$(printf '%s\n' "$lf_body" |
        grep -c '^<!-- end quantick-mission-summary:v1 -->$')
    [ "$summary_count" -eq 1 ] && [ "$summary_end_count" -eq 1 ] ||
        fail 'Mission and synchronization PRs require exactly one quantick-mission-summary:v1 block.'
    summary=$(printf '%s\n' "$lf_body" |
        sed -n '/^<!-- quantick-mission-summary:v1 -->$/,/^<!-- end quantick-mission-summary:v1 -->$/p')
    # A template placeholder starts with `<`; it is not a filled field.
    for field in Objective Tier Source Criteria Validation; do
        printf '%s\n' "$summary" | grep -Eq "^$field: *[^ <]" ||
            fail "The quantick-mission-summary:v1 block has no filled $field: line."
    done
fi

require_green_checks

arch_url=$(sh "$script_dir/review_report.sh" verify arch-review "$pr" "$worktree") ||
    fail 'The current architecture marker has no matching durable PASS report.'
delivery_url=$(sh "$script_dir/review_report.sh" verify delivery-review "$pr" "$worktree" 2>/dev/null)
delivery_status=$?
if [ "$delivery_status" -eq 2 ]; then
    fail 'GitHub could not verify the durable delivery-review report.'
elif [ "$delivery_status" -ne 0 ]; then
    # Reaching here means the shared readiness gate already proved that the
    # bounded small-tier exemption applies; every other tier was denied there.
    delivery_url='bounded-small-tier-exemption'
fi
ai_url=$(sh "$script_dir/review_report.sh" verify ai-review "$pr" "$worktree") ||
    fail 'The current AI completion marker has no matching durable COMPLETE report.'

body_tmp=$(mktemp) || fail 'Cannot create the PR body evidence file.'
report_tmp=$(mktemp) || fail 'Cannot create the completion report file.'
trap 'rm -f "$body_tmp" "$report_tmp"' EXIT HUP INT TERM
printf '%s\n' "$body" | sed \
    '/<!-- quantick-delivery-evidence:v1 -->/,/<!-- end quantick-delivery-evidence:v1 -->/d' > "$body_tmp"
cat >> "$body_tmp" <<EOF

<!-- quantick-delivery-evidence:v1 -->
Head: $head
Review key: $key
Architecture report: $arch_url
Delivery report: $delivery_url
AI report: $ai_url
CI: full ci and windows passed at the exact head; every other check that ran passed
AI review threads: zero returned by ai_review_threads.sh list
<!-- end quantick-delivery-evidence:v1 -->
EOF

clauses=$(sed -n '/<!-- what-done-means:v1 -->/,/<!-- end what-done-means:v1 -->/p' "$skill" |
    grep '^- \*\*D[0-9][0-9]*\*\*')
[ -n "$clauses" ] || fail 'The canonical What done means block has no machine-readable clauses.'

printf '## Mission completion - PR #%s @ %s\n\n' "$pr" "$head" > "$report_tmp"
printf 'Caller: %s\nPR: %s\nBranch: %s\nBase: %s\nBase tip: %s\nReview key: %s\n\n' \
    "$mode" "$pr_url" "$branch" "$base" "$base_tip" "$key" >> "$report_tmp"
printf '%s\n\n' "$goal_note" >> "$report_tmp"
seen=
while IFS= read -r clause; do
    case "$clause" in
        '- **D1**'*) clause_id=D1; evidence="open non-draft PR $pr_url matches branch $branch at $head and base $base" ;;
        '- **D2**'*) clause_id=D2; evidence='full ci and windows passed at the exact head; every other registered check that ran is pass' ;;
        '- **D3**'*) clause_id=D3; evidence="$arch_url" ;;
        '- **D4**'*) clause_id=D4; evidence="$delivery_url" ;;
        '- **D5**'*) clause_id=D5; evidence="$ai_url; ai_review_threads.sh list returned zero" ;;
        '- **D6**'*) clause_id=D6; evidence='the PR body contains the concise mission summary and current quantick-delivery-evidence:v1 block' ;;
        '- **D7**'*) clause_id=D7; evidence='this literal report is published and verified before the command returns PASS' ;;
        '- **D8**'*) clause_id=D8; evidence='this verifier performs no merge; main remains a human action' ;;
        *) fail "Unknown What done means clause: $clause" ;;
    esac
    seen="$seen $clause_id"
    printf -- '- [x] %s\n  Evidence: %s\n' "$clause" "$evidence" >> "$report_tmp"
done <<EOF
$clauses
EOF
[ "$seen" = ' D1 D2 D3 D4 D5 D6 D7 D8' ] ||
    fail "What done means clause IDs are missing, duplicated or reordered:$seen"
printf '\nMISSION-COMPLETION: PASS\n' >> "$report_tmp"

# PR-body reconciliation is not atomic with GitHub checks or review-thread
# state. Observe both again immediately before durable publication so a
# change during reconciliation cannot deposit a positive receipt.
final_threads=$(list_threads) ||
    fail 'AI-review threads could not be relisted after reconciliation.'
[ -z "$final_threads" ] || fail 'AI-review threads changed during final reconciliation.'
require_green_checks
(cd "$worktree" && gh pr edit "$pr" --body-file "$body_tmp") ||
    fail 'The current-head evidence block could not be published in the PR body.'

completion_url=$(cd "$worktree" && sh "$script_dir/review_report.sh" \
    publish mission-completion "$pr" "$report_tmp") ||
    fail 'The literal completion reconciliation could not be published.'
verified_completion=$(sh "$script_dir/review_report.sh" \
    verify mission-completion "$pr" "$worktree") ||
    fail 'The published completion reconciliation could not be verified.'
[ "$completion_url" = "$verified_completion" ] ||
    fail 'The verified completion receipt is not the report just published.'

printf 'MISSION-COMPLETION:PASS caller=%s pr=%s head=%s report=%s\n' \
    "$mode" "$pr" "$head" "$verified_completion"
