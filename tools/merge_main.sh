#!/bin/sh
# Merge sources first; regenerate every current artifact before staging.
set -eu
cd "$(git rev-parse --show-toplevel)"
marker="$(git rev-parse --absolute-git-dir)/merge-main-regenerate"
case "${1:-}" in
    '')
        git fetch origin
        if ! git merge --no-commit --no-ff origin/main; then
            git rev-parse -q --verify MERGE_HEAD >/dev/null || exit 1
        fi
        if git rev-parse -q --verify MERGE_HEAD >/dev/null; then : >"$marker"; fi
        ;;
    --continue) ;;
    *) echo 'Usage: sh tools/merge_main.sh [--continue]' >&2; exit 2;;
esac

if ! git rev-parse -q --verify MERGE_HEAD >/dev/null; then
    if [ -f "$marker" ]; then
        sh tools/regen.sh --check all || {
            echo 'The merge was committed before regeneration; run sh tools/regen.sh --write all and commit its output.' >&2
            exit 1
        }
        rm -f "$marker"
        exit 0
    fi
    git merge-base --is-ancestor origin/main HEAD && exit 0
    echo 'No merge in progress.' >&2
    exit 1
fi

unmerged=$(git diff --name-only --diff-filter=U)
others=
while IFS= read -r path; do
    [ -n "$path" ] || continue
    case "$(git check-attr linguist-generated -- "$path")" in
        *': linguist-generated: set') ;;
        *) others="$others
$path";;
    esac
done <<EOF
$unmerged
EOF
if [ -n "$others" ]; then
    echo 'Resolve and stage these sources, then run sh tools/merge_main.sh --continue:' >&2
    echo "$others" >&2
    exit 1
fi

# Failure leaves the merge pending and generated conflicts unstaged.
: >"$marker"
sh tools/regen.sh --write all
# Stage only generated files, including unmerged and newly produced paths.
git ls-files --cached --others --exclude-standard | sort -u | while IFS= read -r path; do
    case "$(git check-attr linguist-generated -- "$path")" in
        *': linguist-generated: set') git add -- "$path";;
    esac
done
git commit --no-edit
rm -f "$marker"
