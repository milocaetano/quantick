#!/bin/sh
# Publish and verify the durable report that is paired with a private review
# projection. A marker file is only a cache: readiness trusts it only when the
# PR also carries this exact current-review receipt.

set -u

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
REPORT_MARKER='quantick-review-report:v1'

fail() {
    printf '%s\n' "$1" >&2
    exit "${2:-1}"
}

kind_contract() {
    case "$1" in
        arch-review) printf '%s\t%s\t%s\n' arch-review-ok PASS 'ARCH-REVIEW: PASS' ;;
        delivery-review) printf '%s\t%s\t%s\n' delivery-review-ok PASS 'DELIVERY-REVIEW: PASS' ;;
        ai-review) printf '%s\t%s\t%s\n' ai-review-complete COMPLETE 'AI-REVIEW: COMPLETE' ;;
        mission-completion) printf '%s\t%s\t%s\n' none PASS 'MISSION-COMPLETION: PASS' ;;
        *) return 1 ;;
    esac
}

local_identity() {
    identity_dir=$1
    IDENTITY_BRANCH=$(git -C "$identity_dir" symbolic-ref --quiet --short HEAD) ||
        fail 'Review evidence requires a named task branch.'
    case "$IDENTITY_BRANCH" in
        *[!A-Za-z0-9._/-]*) fail 'The branch name cannot be represented in a review receipt.' ;;
    esac
    IDENTITY_HEAD=$(git -C "$identity_dir" rev-parse HEAD) || fail 'Cannot read the task HEAD.'
    IDENTITY_BASE=$(sh "$script_dir/campaign_context.sh" base "$identity_dir") ||
        fail 'Cannot read the declared review base.'
    case "$IDENTITY_BASE" in
        *[!A-Za-z0-9._/-]*) fail 'The review base cannot be represented in a review receipt.' ;;
    esac
    IDENTITY_BASE_TIP=$(git -C "$identity_dir" rev-parse "$IDENTITY_BASE") ||
        fail 'Cannot read the review base tip.'
    IDENTITY_KEY=$(sh "$script_dir/campaign_context.sh" key "$identity_dir") ||
        fail 'Cannot compute the review key.'
    case "$IDENTITY_HEAD$IDENTITY_BASE_TIP$IDENTITY_KEY" in
        *[!0-9a-f]*) fail 'Review identity contains a non-hexadecimal object id.' ;;
    esac
    IDENTITY_STATUS=$(git -C "$identity_dir" status --porcelain=v1 --untracked-files=all) ||
        fail 'Cannot read the task worktree status.'
    [ -z "$IDENTITY_STATUS" ] || fail 'Review evidence requires a clean task worktree.'
}

remote_identity() {
    remote_pr=$1
    remote_dir=$2
    remote_line=$(cd "$remote_dir" && gh pr view "$remote_pr" \
        --json headRefName,headRefOid,baseRefName,baseRefOid,state,isCrossRepository \
        --jq '[.headRefName,.headRefOid,.baseRefName,.baseRefOid,.state,(.isCrossRepository|tostring)] | join(" ")') ||
        return 2
    set -- $remote_line
    [ "$#" -eq 6 ] || return 2
    [ "$1" = "$IDENTITY_BRANCH" ] && [ "$2" = "$IDENTITY_HEAD" ] &&
        [ "$3" = "${IDENTITY_BASE#origin/}" ] && [ "$4" = "$IDENTITY_BASE_TIP" ] &&
        [ "$5" = OPEN ] && [ "$6" = false ]
}

receipt_prefix() {
    printf '<!-- %s kind=%s branch=%s head=%s base=%s base-tip=%s key=%s verdict=%s -->' \
        "$REPORT_MARKER" "$1" "$IDENTITY_BRANCH" "$IDENTITY_HEAD" \
        "$IDENTITY_BASE" "$IDENTITY_BASE_TIP" "$IDENTITY_KEY" "$2"
}

verify_report() {
    verify_kind=$1
    verify_pr=$2
    verify_dir=$3
    verify_verdict=$4

    local_identity "$verify_dir"
    remote_identity "$verify_pr" "$verify_dir" || {
        verify_status=$?
        [ "$verify_status" -eq 2 ] && fail 'GitHub could not provide the PR identity for durable-report verification.' 2
        fail 'The PR identity does not match this reviewed task.'
    }

    verify_repo=$(cd "$verify_dir" && gh repo view --json nameWithOwner --jq .nameWithOwner) ||
        fail 'GitHub could not resolve the repository for durable-report verification.' 2
    [ -n "$verify_repo" ] || fail 'GitHub returned no repository for durable-report verification.' 2
    verify_prefix=$(receipt_prefix "$verify_kind" "$verify_verdict")
    verify_urls=$(cd "$verify_dir" && gh api "repos/$verify_repo/issues/$verify_pr/comments" \
        --paginate --jq ".[] | select(.body | startswith(\"$verify_prefix\")) | .html_url") ||
        fail 'GitHub could not read durable review reports.' 2
    verify_url=$(printf '%s\n' "$verify_urls" | sed -n '/./h;${x;p;}')
    [ -n "$verify_url" ] || fail "No current durable $verify_kind report exists on PR #$verify_pr."
    printf '%s\n' "$verify_url"
}

operation=${1:-}
kind=${2:-}
subject_pr=${3:-}
subject_arg=${4:-}
contract=$(kind_contract "$kind") ||
    fail 'Usage: review_report.sh publish|verify arch-review|delivery-review|ai-review|mission-completion PR [REPORT].' 64
old_ifs=$IFS
IFS=$(printf '\t')
set -- $contract
IFS=$old_ifs
marker_name=$1
verdict=$2
required_line=$3
[ "$marker_name" != none ] || marker_name=

case "$operation" in
    verify)
        verify_pr=$subject_pr
        verify_dir=${subject_arg:-.}
        case "$verify_pr" in ''|*[!0-9]*) fail 'Name one numeric PR for durable-report verification.' 64 ;; esac
        verify_report "$kind" "$verify_pr" "$verify_dir" "$verdict"
        ;;
    publish)
        publish_pr=$subject_pr
        report_path=$subject_arg
        case "$publish_pr" in ''|*[!0-9]*) fail 'Name one numeric PR for report publication.' 64 ;; esac
        [ -f "$report_path" ] || fail 'The review report file does not exist.' 64
        grep -qF -x -- "$required_line" "$report_path" ||
            fail "The report is incomplete: it must contain '$required_line' as its own line."

        publish_dir=.
        local_identity "$publish_dir"
        remote_identity "$publish_pr" "$publish_dir" ||
            fail 'The PR identity does not match this reviewed task before publication.'

        report_tmp=$(mktemp) || fail 'Cannot create a temporary report file.'
        trap 'rm -f "$report_tmp"' EXIT HUP INT TERM
        receipt_prefix "$kind" "$verdict" > "$report_tmp"
        printf '\n\n' >> "$report_tmp"
        cat "$report_path" >> "$report_tmp"

        publish_url=$(gh pr comment "$publish_pr" --body-file "$report_tmp") ||
            fail 'The durable PR report could not be published.' 2
        [ -n "$publish_url" ] || fail 'GitHub returned no URL for the durable PR report.' 2

        before_branch=$IDENTITY_BRANCH
        before_head=$IDENTITY_HEAD
        before_base=$IDENTITY_BASE
        before_base_tip=$IDENTITY_BASE_TIP
        before_key=$IDENTITY_KEY
        local_identity "$publish_dir"
        [ "$IDENTITY_BRANCH" = "$before_branch" ] && [ "$IDENTITY_HEAD" = "$before_head" ] &&
            [ "$IDENTITY_BASE" = "$before_base" ] && [ "$IDENTITY_BASE_TIP" = "$before_base_tip" ] &&
            [ "$IDENTITY_KEY" = "$before_key" ] ||
            fail 'The review identity changed during report publication.'
        remote_identity "$publish_pr" "$publish_dir" ||
            fail 'The PR identity changed during report publication.'

        verified_url=$(verify_report "$kind" "$publish_pr" "$publish_dir" "$verdict") || exit $?
        [ "$verified_url" = "$publish_url" ] ||
            fail 'The published report URL was not the current durable receipt returned by GitHub.'

        if [ -n "$marker_name" ]; then
            marker_git_dir=$(git rev-parse --absolute-git-dir) || fail 'Cannot resolve the private review directory.'
            if [ "$kind" = ai-review ]; then
                printf '%s %s\n' "$IDENTITY_BRANCH" "$IDENTITY_KEY" > "$marker_git_dir/$marker_name" ||
                    fail 'The verified AI-review projection could not be recorded.'
            else
                printf '%s\n' "$IDENTITY_KEY" > "$marker_git_dir/$marker_name" ||
                    fail "The verified $kind projection could not be recorded."
            fi
        fi
        printf '%s\n' "$verified_url"
        ;;
    *) fail 'Usage: review_report.sh publish|verify KIND PR [REPORT].' 64 ;;
esac
