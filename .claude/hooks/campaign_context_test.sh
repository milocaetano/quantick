#!/bin/sh
# Hermetic campaign boundary tests. No real GitHub mutation or credentials.
set -eu
script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
helper="$script_dir/campaign_context.sh"
temp_parent=$(CDPATH='' cd -- "${TMPDIR:-/tmp}" && pwd)
fixture=$(mktemp -d "$temp_parent/quantick-campaign.XXXXXX")
cleanup() {
    case "$fixture" in "$temp_parent"/quantick-campaign.*)
        [ -f "$fixture/fixture-owner" ] && rm -rf "$fixture" ;;
    esac
}
trap cleanup EXIT
touch "$fixture/fixture-owner"
mkdir -p "$fixture/repo" "$fixture/bin" "$fixture/hooks"
git init -q -b main "$fixture/repo"
git -C "$fixture/repo" config user.name fixture
git -C "$fixture/repo" config user.email fixture@example.invalid
git -C "$fixture/repo" config core.autocrlf false
printf 'base\n' > "$fixture/repo/data"
git -C "$fixture/repo" add data
git -C "$fixture/repo" commit -qm baseline
base=$(git -C "$fixture/repo" rev-parse HEAD)
git -C "$fixture/repo" update-ref refs/remotes/origin/main "$base"
git -C "$fixture/repo" update-ref refs/remotes/origin/campaign/test "$base"
git -C "$fixture/repo" checkout -qb feat/child
printf 'child\n' >> "$fixture/repo/data"
git -C "$fixture/repo" commit -qam child
head=$(git -C "$fixture/repo" rev-parse HEAD)
git_dir=$(git -C "$fixture/repo" rev-parse --absolute-git-dir)
cp "$script_dir/guardrails.sh" "$helper" "$fixture/hooks/"
cat > "$fixture/hooks/ai_review_threads.sh" <<'STUB'
#!/bin/sh
cat "$(dirname "$0")/threads"
STUB
printf '0\n' > "$fixture/hooks/threads"
cat > "$fixture/bin/gh" <<'STUB'
#!/bin/sh
case "$1 $2" in
    'pr view') cat "$(dirname "$0")/identity" ;;
    'pr checks') cat "$(dirname "$0")/checks" ;;
    *) exit 1 ;;
esac
STUB
chmod +x "$fixture/bin/gh"
PATH="$fixture/bin:$PATH"
export PATH
passed=0
context() {
    printf 'feat/child origin/campaign/test https://github.com/owner/repo/issues/1 %s\n' "$1" > "$git_dir/mission-base"
}
identity() {
    printf '%s feat/child %s OPEN false false %s CLEAN\n' "$1" "$2" "$3" > "$fixture/bin/identity"
}
allow() {
    description=$1; shift
    if "$@" > "$fixture/output" 2>&1; then passed=$((passed + 1));
    else printf 'FAIL %s\n' "$description"; cat "$fixture/output"; exit 1; fi
}
reject() {
    description=$1; shift
    if "$@" > "$fixture/output" 2>&1; then printf 'FAIL %s unexpectedly passed\n' "$description"; exit 1;
    else passed=$((passed + 1)); fi
}
gate() {
    payload=$(printf '{"tool_name":"%s","cwd":"%s","tool_input":{"command":"%s"}}' "$1" "$fixture/repo" "$2")
    output=$(printf '%s' "$payload" | sh "$fixture/hooks/guardrails.sh" pr-gate)
    case "$3" in
        allow) [ -z "$output" ] ;;
        deny) printf '%s' "$output" | grep -q '"permissionDecision":"deny"' ;;
    esac || return 1
    if [ -n "${4:-}" ]; then printf '%s' "$output" | grep -qF -- "$4"; fi
}
stamp_required() {
    sh "$helper" key "$fixture/repo" > "$git_dir/arch-review-ok"
    cp "$git_dir/arch-review-ok" "$git_dir/delivery-review-ok"
}
stamp() {
    stamp_required
    printf '%s %s\n' "$(git -C "$fixture/repo" symbolic-ref --quiet --short HEAD)" \
        "$(sh "$helper" key "$fixture/repo")" > "$git_dir/ai-review-complete"
}

allow 'default base remains main' test "$(sh "$helper" base "$fixture/repo")" = origin/main
reject 'main merge is always human' sh "$helper" check-pr "$fixture/repo" 42 merge
context none
identity campaign/test "$head" "$base"
printf 'pass\npass\n' > "$fixture/bin/checks"
allow 'campaign ready does not require merge authority' sh "$helper" check-pr "$fixture/repo" 42 ready
reject 'missing campaign merge grant' sh "$helper" check-pr "$fixture/repo" 42 merge
context https://github.com/owner/repo/issues/1#issuecomment-2
allow 'authorized campaign merge with green CI' sh "$helper" check-pr "$fixture/repo" 42 merge
identity main "$head" "$base"
reject 'retargeted PR cannot borrow campaign authorization' sh "$helper" check-pr "$fixture/repo" 42 merge
identity campaign/test "$base" "$base"
reject 'another head cannot borrow review' sh "$helper" check-pr "$fixture/repo" 42 merge
identity campaign/test "$head" "$head"
reject 'remote base advance requires refresh' sh "$helper" check-pr "$fixture/repo" 42 merge
identity campaign/test "$head" "$base"
printf 'pending\n' > "$fixture/bin/checks"
reject 'pending CI blocks merge' sh "$helper" check-pr "$fixture/repo" 42 merge
: > "$fixture/bin/checks"
reject 'absent CI is not green' sh "$helper" check-pr "$fixture/repo" 42 merge
printf 'pass\n' > "$fixture/bin/checks"
old_key=$(sh "$helper" key "$fixture/repo")
old_diff=$(git -C "$fixture/repo" diff origin/campaign/test...HEAD)
base_tree=$(git -C "$fixture/repo" rev-parse "$base^{tree}")
advanced=$(printf 'independent base advance\n' | git -C "$fixture/repo" commit-tree "$base_tree" -p "$base")
git -C "$fixture/repo" update-ref refs/remotes/origin/campaign/test "$advanced"
new_key=$(sh "$helper" key "$fixture/repo")
allow 'fixture advances base without changing task diff' test "$old_diff" = "$(git -C "$fixture/repo" diff origin/campaign/test...HEAD)"
allow 'campaign advancement stales key' test "$old_key" != "$new_key"
git -C "$fixture/repo" update-ref refs/remotes/origin/campaign/test "$base"
printf 'feat/other origin/campaign/test https://github.com/owner/repo/issues/1 none\n' > "$git_dir/mission-base"
reject 'context from another branch cannot be reused' sh "$helper" base "$fixture/repo"
context https://github.com/owner/repo/issues/1#issuecomment-2
stamp

# The clients share the policy; their tool names do not change its decision.
for client in Bash exec_command; do
    rm "$git_dir/ai-review-complete"
    for action in ready merge; do
        allow "$client campaign $action denies missing AI completion" gate "$client" "gh pr $action 42" deny ai-review-complete
    done
    printf 'feat/child %s\n' "$old_key" > "$git_dir/ai-review-complete"
    git -C "$fixture/repo" update-ref refs/remotes/origin/campaign/test "$advanced"
    stamp_required
    identity campaign/test "$head" "$advanced"
    allow "$client base tip stales AI after earlier reviews refresh" gate "$client" 'gh pr ready 42' deny ai-review-complete
    allow "$client base tip also blocks campaign merge" gate "$client" "gh pr merge 42 --merge --match-head-commit $head" deny ai-review-complete
    git -C "$fixture/repo" update-ref refs/remotes/origin/campaign/test "$base"
    identity campaign/test "$head" "$base"
    stamp
    git -C "$fixture/repo" update-ref refs/remotes/origin/campaign/other "$base"
    printf 'feat/child origin/campaign/other https://github.com/owner/repo/issues/1 https://github.com/owner/repo/issues/1#issuecomment-2\n' > "$git_dir/mission-base"
    stamp_required
    identity campaign/other "$head" "$base"
    allow "$client equivalent base ref stales AI after earlier reviews refresh" gate "$client" 'gh pr ready 42' deny ai-review-complete
    allow "$client changed target also blocks campaign merge" gate "$client" "gh pr merge 42 --merge --match-head-commit $head" deny ai-review-complete
    context https://github.com/owner/repo/issues/1#issuecomment-2
    identity campaign/test "$head" "$base"
    stamp
    allow "$client campaign merge" gate "$client" "gh pr merge 42 --merge --match-head-commit $head" allow
    context none
    allow "$client current AI cannot supply campaign merge authority" gate "$client" "gh pr merge 42 --merge --match-head-commit $head" deny 'authorization or CI'
    context https://github.com/owner/repo/issues/1#issuecomment-2
    printf 'pending\n' > "$fixture/bin/checks"
    allow "$client current AI cannot replace green CI" gate "$client" "gh pr merge 42 --merge --match-head-commit $head" deny 'authorization or CI'
    printf 'pass\n' > "$fixture/bin/checks"
    allow "$client auto-merge denied" gate "$client" "gh pr merge 42 --auto" deny
    allow "$client admin override denied" gate "$client" "gh pr merge 42 --admin" deny
    allow "$client alternate repository denied" gate "$client" "gh pr merge 42 --merge --match-head-commit $head --repo other/repo" deny
    allow "$client compound merges denied" gate "$client" "gh pr merge 42 --merge --match-head-commit $head && gh pr merge 99 --merge" deny
    allow "$client retarget before merge denied" gate "$client" "gh pr edit 42 --base main && gh pr merge 42 --merge --match-head-commit $head" deny
    allow "$client ready cannot borrow another repository" gate "$client" 'gh pr ready 42 --repo other/repo' deny
    allow "$client campaign ready accepts exact target" gate "$client" 'gh pr ready 42' allow
    # The agent's Bash tool resets its working directory between calls, so the
    # pinned statement reaches the task worktree only behind a single
    # `cd <dir> &&`. One prefix is accepted; anything beside the statement is
    # not, because a second statement would share the first's authorization.
    allow "$client cd-prefixed campaign merge" \
        gate "$client" "cd $fixture/repo && gh pr merge 42 --merge --match-head-commit $head" allow
    allow "$client cd-prefixed campaign ready" \
        gate "$client" "cd $fixture/repo && gh pr ready 42" allow
    allow "$client cd-prefixed merge cannot carry a second statement" \
        gate "$client" "cd $fixture/repo && gh pr merge 42 --merge --match-head-commit $head && echo x" deny
    allow "$client only one cd prefix is stripped" \
        gate "$client" "cd $fixture/hooks && cd $fixture/repo && gh pr merge 42 --merge --match-head-commit $head" deny
    allow "$client a cd prefix on a semicolon is not the accepted form" \
        gate "$client" "cd $fixture/repo ; gh pr merge 42 --merge --match-head-commit $head" deny
    allow "$client cd-prefixed ready cannot carry a second statement" \
        gate "$client" "cd $fixture/repo && gh pr ready 42 && gh pr merge 99 --merge" deny
    allow "$client a cd prefix does not excuse an alternate repository" \
        gate "$client" "cd $fixture/repo && gh pr merge 42 --merge --match-head-commit $head --repo other/repo" deny
    allow "$client a cd prefix does not excuse auto-merge" \
        gate "$client" "cd $fixture/repo && gh pr merge 42 --auto" deny
    printf 'feat/child small\n' > "$git_dir/mission-tier"
    printf '1\n' > "$fixture/hooks/threads"
    allow "$client small tier cannot skip open threads" gate "$client" 'gh pr ready 42' deny
    printf '0\n' > "$fixture/hooks/threads"
    rm "$git_dir/mission-base"
    stamp
    allow "$client small tier cannot merge main" gate "$client" "gh pr merge 42 --merge --match-head-commit $head" deny
    allow "$client main auto-merge denied" gate "$client" 'gh pr merge 42 --auto' deny
    context https://github.com/owner/repo/issues/1#issuecomment-2
    stamp
done
printf '%s campaign context tests passed\n' "$passed"
