#!/bin/sh
# Merge origin/main into the current branch and regenerate the hook registry
# instead of hand-merging it.
#
#   sh tools/merge_main.sh             fetch and merge origin/main
#   sh tools/merge_main.sh --continue  after the hand-written conflicts are
#                                      resolved and staged: regenerate, commit
#
# The registry is generated from `declare_hooks!` and
# docs/ui-harness/hook-prose.md, so two branches that each document a hook
# conflict on it even when their sources merge cleanly — and a clean text
# merge of it can still differ from what the merged sources generate. So it
# is regenerated whenever both sides changed it, last, because the generator
# compiles the app from the merged sources.
#
# Baseline raises need nothing here: they live one file per branch under
# crates/guards/*-baseline.d/ and never conflict.
set -eu

registry=.claude/skills/ui-harness/references/hook-registry.md
marker="$(git rev-parse --absolute-git-dir)/merge-main-regenerate"

cd "$(git rev-parse --show-toplevel)"

if [ "${1:-}" != "--continue" ]; then
    git fetch origin
    base=$(git merge-base HEAD origin/main)
    rm -f "$marker"
    if ! git diff --quiet "$base" HEAD -- "$registry" &&
        ! git diff --quiet "$base" origin/main -- "$registry"; then
        : >"$marker"
    fi
    git merge --no-commit --no-ff origin/main || true
fi

if ! git rev-parse -q --verify MERGE_HEAD >/dev/null; then
    rm -f "$marker"
    if git merge-base --is-ancestor origin/main HEAD; then
        exit 0
    fi
    echo "No merge in progress: git refused the merge (see above)." >&2
    rm -f "$marker"
    exit 1
fi

others=$(git diff --name-only --diff-filter=U | grep -vx "$registry" || true)
if [ -n "$others" ]; then
    echo "Resolve and stage these by hand, then run: sh tools/merge_main.sh --continue" >&2
    echo "$others" >&2
    exit 1
fi

if [ -f "$marker" ]; then
    # Written beside, then moved: a build failure must not leave the registry
    # truncated. A stale live bubbles file breaks the launch the generator runs.
    env -u QUANTICK_BUBBLES cargo run -q -p quantick-app --features harness -- \
        --dump-hook-registry >"$registry.new"
    mv "$registry.new" "$registry"
    git add "$registry"
    rm -f "$marker"
fi

git commit --no-edit
