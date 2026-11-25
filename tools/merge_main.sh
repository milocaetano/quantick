#!/bin/sh
# Merge origin/main into the current branch and regenerate every generated
# file instead of hand-merging it.
#
#   sh tools/merge_main.sh             fetch and merge origin/main
#   sh tools/merge_main.sh --continue  after the hand-written conflicts are
#                                      resolved and staged: regenerate, commit
#
# Generated files are produced from the code: the hook registry, the
# capability inventory, the UI behaviour and retry matrices, and every
# document under schemas/control (public contracts, observer schemas and the
# observer capability catalog). When the branch and main each add a
# capability, each side generated them from its own code, so even a clean
# text merge differs from what the merged code generates and the drift tests
# fail. So after every merge they are all regenerated from the merged
# sources, last, and staged into the merge commit. A conflict in one of them
# is never resolved by hand: main's side is taken, then overwritten.
#
# Baseline raises need nothing here: they live one file per branch under
# crates/guards/*-baseline.d/ and never conflict.
set -eu

cd "$(git rev-parse --show-toplevel)"

registry=.claude/skills/ui-harness/references/hook-registry.md
inventory=docs/control-plane/capability-inventory.md
behaviour=docs/control-plane/ui-behaviour-matrix.md
retry=docs/control-plane/retry-matrix.md
schemas=schemas/control
marker="$(git rev-parse --absolute-git-dir)/merge-main-regenerate"

generated() {
    case "$1" in
    "$registry" | "$inventory" | "$behaviour" | "$retry" | "$schemas"/*) return 0 ;;
    *) return 1 ;;
    esac
}

if [ "${1:-}" != "--continue" ]; then
    git fetch origin
    git merge --no-commit --no-ff origin/main || true
    # Up to date, or refused: nothing was merged, so nothing to regenerate.
    if git rev-parse -q --verify MERGE_HEAD >/dev/null; then
        : >"$marker"
    else
        rm -f "$marker"
    fi
fi

# A stale live bubbles file breaks the launch the generators run.
# Written beside, then moved: a build failure must not truncate the file.
dump() {
    env -u QUANTICK_BUBBLES cargo run -q -p quantick-app --features harness -- "$1" >"$2.new"
    mv "$2.new" "$2"
}

regenerate() {
    # The public-contract writer fails whenever it rewrote a file, by design;
    # its verdict is the check after it, which must pass.
    env -u QUANTICK_BUBBLES cargo test -q -p quantick-control --test schema_snapshots -- \
        --ignored regenerate_public_contracts >/dev/null 2>&1 || true
    env -u QUANTICK_BUBBLES cargo test -q -p quantick-control --test schema_snapshots -- \
        generated_public_contracts_match_committed_files
    env -u QUANTICK_BUBBLES cargo test -q -p quantick-app --features harness -- \
        --ignored regenerate_observer_schemas
    dump --dump-hook-registry "$registry"
    dump --dump-capability-inventory "$inventory"
    dump --dump-ui-behaviour-matrix "$behaviour"
    dump --dump-retry-matrix "$retry"
    rm -f "$marker"
}

if ! git rev-parse -q --verify MERGE_HEAD >/dev/null; then
    if [ -f "$marker" ]; then
        # The merge was committed by hand before regeneration.
        regenerate
        if [ -n "$(git status --porcelain -- "$registry" "$inventory" "$behaviour" "$retry" "$schemas")" ]; then
            echo "The merge was committed with stale generated files; they are regenerated now: commit them." >&2
            exit 1
        fi
        exit 0
    fi
    if git merge-base --is-ancestor origin/main HEAD; then
        exit 0
    fi
    echo "No merge in progress: git refused the merge (see above)." >&2
    exit 1
fi

# One path per line, unexpanded, whatever characters a name holds.
others=""
unmerged=$(git diff --name-only --diff-filter=U)
set -f
old_ifs=$IFS
IFS='
'
for path in $unmerged; do
    # Index stages present: 2 is this branch's side, 3 is main's.
    stages=$(git ls-files -u -- "$path" | cut -f1 | cut -d' ' -f3 | tr -d '\n')
    if ! generated "$path"; then
        others="$others
$path"
    elif [ "$stages" = 123 ] || [ "$stages" = 23 ]; then
        # Both sides edited it: either parses, and regeneration overwrites it.
        git checkout --theirs -- "$path"
    else
        # One side deleted it with its registry entry; a file the merged
        # registry still names is written again by regeneration.
        git rm -q -- "$path"
    fi
done
IFS=$old_ifs
set +f
if [ -n "$others" ]; then
    echo "Resolve and stage these by hand, then run: sh tools/merge_main.sh --continue" >&2
    echo "$others" >&2
    exit 1
fi

regenerate
git add -A -- "$registry" "$inventory" "$behaviour" "$retry" "$schemas"
git commit --no-edit
