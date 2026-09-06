#!/bin/sh
# Branch-bound review context. Local records are workflow evidence, not a
# security boundary; GitHub protections still own enforcement against bypass.
set -eu

operation=${1:-}
context_dir=${2:-.}
context_git=$(git -C "$context_dir" rev-parse --absolute-git-dir)
context_branch=$(git -C "$context_dir" symbolic-ref --quiet --short HEAD)
context_file="$context_git/mission-base"
context_base=origin/main
context_parent=none
context_grant=none

fail() { printf '%s\n' "$1" >&2; exit 1; }

if [ -e "$context_file" ]; then
    [ "$(wc -l < "$context_file" | tr -d ' ')" = 1 ] || fail 'Invalid campaign context record.'
    read -r recorded_branch context_base context_parent context_grant extra < "$context_file"
    [ "$recorded_branch" = "$context_branch" ] && [ -z "$extra" ] || fail 'Campaign context belongs to another branch or is malformed.'
    case "$context_base" in origin/campaign/?*) ;; *) fail 'Campaign base must be an explicit origin/campaign branch.' ;; esac
    git check-ref-format "refs/remotes/$context_base" >/dev/null || fail 'Invalid campaign base ref.'
    printf '%s\n' "$context_parent" | grep -Eq '^https://github.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/issues/[0-9]+$' || fail 'Campaign parent must be an issue URL.'
    if [ "$context_grant" != none ]; then
        printf '%s\n' "$context_grant" | grep -Eq '^https://github.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/issues/[0-9]+(#issuecomment-[0-9]+)?$' || fail 'Campaign merge grant must reference a persisted user decision.'
    fi
    git -C "$context_dir" rev-parse --verify "$context_base^{commit}" >/dev/null 2>&1 || fail 'Campaign base is unavailable; fetch and reconcile.'
fi

case "$operation" in
    base) printf '%s\n' "$context_base" ;;
    key)
        context_tip=$(git -C "$context_dir" rev-parse --verify "$context_base^{commit}")
        git -C "$context_dir" merge-base "$context_base" HEAD >/dev/null
        # Preserve legacy main review keys. Campaign keys additionally bind the
        # target ref and its tip, so another child merge invalidates reviews.
        if [ "$context_base" = origin/main ]; then
            git -C "$context_dir" diff "$context_base...HEAD" | git -C "$context_dir" hash-object --stdin
        else
            context_diff=$(git -C "$context_dir" diff "$context_base...HEAD" | git -C "$context_dir" hash-object --stdin)
            printf '%s\n%s\n%s\n' "$context_base" "$context_tip" "$context_diff" | git -C "$context_dir" hash-object --stdin
        fi
        ;;
    check-pr)
        context_pr=${3:-}
        context_action=${4:-ready}
        case "$context_pr" in ''|*[!0-9]*) fail 'Name one explicit PR number.' ;; esac
        if [ "$context_action" = merge ]; then
            [ "$context_base" != origin/main ] || fail 'Merge to main is reserved exclusively for the user.'
            [ "$context_grant" != none ] || fail 'No persisted authorization for campaign merges.'
        fi
        cd "$context_dir"
        context_head=$(git rev-parse HEAD)
        context_remote=$(gh pr view "$context_pr" --json baseRefName,headRefName,headRefOid,state,isDraft,isCrossRepository,baseRefOid,mergeStateStatus --jq '[.baseRefName,.headRefName,.headRefOid,.state,(.isDraft|tostring),(.isCrossRepository|tostring),.baseRefOid,.mergeStateStatus] | join(" ")') || fail 'Cannot verify the PR target and head.'
        set -- $context_remote
        [ "$#" = 8 ] || fail 'Incomplete PR identity.'
        [ "$1" = "${context_base#origin/}" ] && [ "$2" = "$context_branch" ] && [ "$3" = "$context_head" ] && [ "$4" = OPEN ] && [ "$6" = false ] || fail 'PR target or head differs from this reviewed campaign task.'
        [ "$7" = "$(git rev-parse "$context_base")" ] || fail 'The remote base advanced; fetch and review again.'
        if [ "$context_action" = merge ]; then
            [ "$5" = false ] || fail 'A draft PR cannot be integrated.'
            [ "$8" = CLEAN ] || fail 'GitHub has not confirmed a clean, unblocked merge.'
            git merge-base --is-ancestor "$context_base" HEAD || fail 'Update the task from the campaign base and validate again.'
            context_checks=$(gh pr checks "$context_pr" --json bucket --jq '.[].bucket') || fail 'Campaign PR checks are not green.'
            [ -n "$context_checks" ] || fail 'No CI evidence is registered for the campaign PR.'
            [ -z "$(printf '%s\n' "$context_checks" | grep -v '^pass$')" ] || fail 'Campaign PR checks are not all passing.'
        fi
        ;;
    *) fail 'Usage: campaign_context.sh base|key|check-pr WORKTREE [PR ready|merge]' ;;
esac
