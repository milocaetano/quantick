#!/bin/sh
# Merge origin/main into the current branch and regenerate the generated files
# instead of hand-merging them.
#
#   sh tools/merge_main.sh             fetch and merge origin/main
#   sh tools/merge_main.sh --continue  after the hand-written conflicts are
#                                      resolved: regenerate, stage, commit
#
# The hook registry is generated from `declare_hooks!` and
# docs/ui-harness/hook-prose.md, so two branches that each document a hook
# conflict on it even when their sources merge cleanly. Its right content is
# whatever the merged sources generate, which a hand merge can only approach.
# It is regenerated last, once every hand-written conflict is resolved,
# because the generator compiles the app from the merged sources.
#
# Baseline raises need nothing here: they live one file per branch under
# crates/guards/*-baseline.d/ and never conflict.
set -eu

registry=.claude/skills/ui-harness/references/hook-registry.md

cd "$(git rev-parse --show-toplevel)"

conflicted() {
    git diff --name-only --diff-filter=U
}

if [ "${1:-}" != "--continue" ]; then
    git fetch origin
    if git merge --no-edit origin/main; then
        exit 0
    fi
fi

registry_conflicted=no
if conflicted | grep -qx "$registry"; then
    registry_conflicted=yes
fi

others=$(conflicted | grep -vx "$registry" || true)
if [ -n "$others" ]; then
    echo "Resolve and stage these by hand, then run: sh tools/merge_main.sh --continue" >&2
    echo "$others" >&2
    exit 1
fi

if [ "$registry_conflicted" = yes ]; then
    # A stale live bubbles file breaks the launch the generator runs.
    env -u QUANTICK_BUBBLES cargo run -q -p quantick-app --features harness -- \
        --dump-hook-registry >"$registry"
    git add "$registry"
fi

git commit --no-edit
