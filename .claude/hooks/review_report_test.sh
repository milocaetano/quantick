#!/bin/sh
# Hermetic tests for durable review reports and their private projections.

set -u

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
producer="$script_dir/review_report.sh"
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT HUP INT TERM
passed=0
failed=0

git init -b main -q "$root/repo"
git -C "$root/repo" config user.email t@t
git -C "$root/repo" config user.name t
git -C "$root/repo" config core.autocrlf false
printf 'one\n' > "$root/repo/a.txt"
git -C "$root/repo" add a.txt
git -C "$root/repo" commit -qm first
git -C "$root/repo" update-ref refs/remotes/origin/main HEAD
git -C "$root/repo" checkout -qb feat/reports
printf 'two\n' > "$root/repo/a.txt"
git -C "$root/repo" commit -qam second

mkdir -p "$root/bin" "$root/comments"
cat > "$root/bin/gh" <<'STUB'
#!/bin/sh
state=${QUANTICK_REPORT_FIXTURE:?}
case "${1:-} ${2:-}" in
    'pr view')
        printf '%s %s main %s OPEN false\n' \
            "$(git symbolic-ref --quiet --short HEAD)" \
            "$(git rev-parse HEAD)" "$(git rev-parse origin/main)"
        ;;
    'repo view') printf 'owner/repo\n' ;;
    'pr comment')
        while [ $# -gt 0 ]; do
            if [ "$1" = --body-file ]; then
                count=$(find "$state/comments" -type f | wc -l | tr -d ' ')
                count=$((count + 1))
                cp "$2" "$state/comments/$count"
                printf 'https://example.test/report/%s\n' "$count"
                exit 0
            fi
            shift
        done
        exit 64
        ;;
    'api '*)
        query=
        while [ $# -gt 0 ]; do
            if [ "$1" = --jq ]; then query=$2; break; fi
            shift
        done
        for comment in "$state"/comments/*; do
            [ -f "$comment" ] || continue
            prefix=$(sed -n '1p' "$comment")
            if printf '%s' "$query" | grep -qF -- "$prefix"; then
                printf 'https://example.test/report/%s\n' "$(basename "$comment")"
            fi
        done
        ;;
    *) exit 64 ;;
esac
STUB
chmod +x "$root/bin/gh"

run_ok() {
    name=$1
    shift
    if out=$(cd "$root/repo" && QUANTICK_REPORT_FIXTURE="$root" PATH="$root/bin:$PATH" "$@" 2>&1); then
        passed=$((passed + 1))
    else
        printf 'FAIL %s\n  output: %s\n' "$name" "$out"
        failed=$((failed + 1))
    fi
}

run_fail() {
    name=$1
    want=$2
    shift 2
    if out=$(cd "$root/repo" && QUANTICK_REPORT_FIXTURE="$root" PATH="$root/bin:$PATH" "$@" 2>&1); then
        printf 'FAIL %s: unexpectedly passed\n  output: %s\n' "$name" "$out"
        failed=$((failed + 1))
    elif printf '%s' "$out" | grep -qF -- "$want"; then
        passed=$((passed + 1))
    else
        printf 'FAIL %s: missing "%s"\n  output: %s\n' "$name" "$want" "$out"
        failed=$((failed + 1))
    fi
}

key=$(cd "$root/repo" && sh "$script_dir/campaign_context.sh" key .)
git_dir=$(git -C "$root/repo" rev-parse --absolute-git-dir)
printf '%s\n' "$key" > "$git_dir/arch-review-ok"
run_fail 'a manually written marker has no durable report' 'No current durable arch-review report' \
    sh "$producer" verify arch-review 42 .

printf '## Architecture review\n\nARCH-REVIEW: FAIL\n' > "$root/arch-fail.md"
run_fail 'an incomplete report cannot record a marker' 'must contain' \
    sh "$producer" publish arch-review 42 "$root/arch-fail.md"

printf '## Architecture review\n\nARCH-REVIEW: PASS\n' > "$root/arch-pass.md"
run_ok 'a durable architecture PASS records its projection' \
    sh "$producer" publish arch-review 42 "$root/arch-pass.md"
run_ok 'the current architecture report reads back' \
    sh "$producer" verify arch-review 42 .

printf '## AI review\n\nAI-REVIEW: COMPLETE\n' > "$root/ai.md"
run_ok 'a durable AI report records branch-bound completion' \
    sh "$producer" publish ai-review 42 "$root/ai.md"
case "$(cat "$git_dir/ai-review-complete")" in
    "feat/reports $key") passed=$((passed + 1)) ;;
    *) printf 'FAIL AI completion is not branch-bound to the review key\n'; failed=$((failed + 1)) ;;
esac

printf 'three\n' > "$root/repo/a.txt"
git -C "$root/repo" commit -qam third
run_fail 'a prior durable report is stale after a source change' 'No current durable arch-review report' \
    sh "$producer" verify arch-review 42 .

printf '%s passed, %s failed\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
