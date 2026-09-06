# Campaign integration branches

Use this mode by default for campaigns with multiple implementation PRs,
especially refactoring. Documentation-only exercises can use issue evidence
without an integration branch. This mode changes the task base, not the
review/CI bar or the permission to merge `main`.

## Authority and lifecycle

The user alone merges to `main`. Agents must not merge, enable auto-merge,
enqueue a merge, push integration commits directly to `main`, change its
protections or use another API/identity to bypass that boundary. Prepare the
final PR and stop at `ready_for_evaluation`; only observe `integrated_main`
after the user's merge and required main CI actually succeed.
Read back the final PR's `mergedBy`, head/base and merge commit; verify the
expected user performed the merge and that main contains it. An unexpected
actor is an authority discrepancy to report, never evidence of user approval.

The parent records `integration.branch` (an exact `campaign/<slug>` name),
initial main SHA, current campaign SHA, final PR URL/head/base, final status
and the user's authorization source for merges into that exact branch. Separate
`candidate_criteria` (ready for evaluation) from `main_completion_criteria`
(objective delivered on main); retain every original criterion in one or both.
Default-branch evidence awaiting the user's merge remains explicitly pending,
not silently waived to make the candidate pass. An
agent-authored plan is not a grant. Authorization is scoped to this campaign,
its reviewed child PRs and its named destination, never to `main` or another
campaign. Revocation or changed target invalidates local context and pauses
affected merges. Preserve branch protection; never use admin bypass or change
repository rules to obtain access.

1. Fetch and create the integration branch from current `origin/main`, after
   searching local/remote branches and existing campaign records. Publish it
   under the campaign's existing branch-creation authority. Never overwrite an
   existing branch; verify its parent campaign and expected SHA before reuse.
2. Each child gets its own branch/worktree from the latest fetched
   `origin/campaign/<slug>`, its own mission and review evidence, and a PR whose
   base is exactly `campaign/<slug>`. Do not work directly in the shared
   integration branch. The coordinator serializes merges.
3. Use the base and key procedure below in `new-task`, `mission`, `ship`,
   `arch-review`, `delivery-review` and `ai-review`. It supersedes their
   main-only examples for campaign children; normal tasks still use main.
4. Before a child merge, read the authorization source again, fetch, rebase
   the child onto the current campaign base, validate affected behavior and
   rerun any invalidated reviews. Verify exact PR base/head, required CI and
   unresolved threads. No missing/pending checks count as green. Merge only
   after the shared gate accepts the explicit, head-pinned command below.
5. Read back `mergedAt`, merge commit and the campaign ref. Record
   `integrated_campaign` plus evidence as the child's integration outcome.
   Dependents require that commit reachable from the campaign branch and its
   green integration evidence. Close the child issue explicitly after proof:
   GitHub does not automatically close issues merely because a PR merged into
   a non-default branch. Do not confuse that closure with delivery to main.
6. Incorporate new main work through a separate synchronization branch/PR into
   the campaign, preserving campaign history. No force push or destructive
   reset of the integration branch. Validate conflicts and integration risk,
   then apply the same authorized campaign merge gate. Dependent reviews stale
   when the campaign tip moves. Record the source main SHA and resulting tip.
7. At the campaign completion criteria, create the consolidated PR from the
   campaign branch to main. Its review base is main, with fresh full-diff
   architecture, AI and delivery reviews and integration CI. Run the applicable
   scorecard at the campaign SHA, label it as a campaign candidate and retain
   rubric limitations: a default-branch gate does not become a campaign gate
   by renaming it. Never present that candidate as main's score. Record the
   final PR/evidence, set `ready_for_evaluation`, and give the user the PR.

`close` may finalize autonomous execution as `ready_for_evaluation` once the
candidate criteria, final PR reviews and CI pass. It leaves the parent open
and Project in Awaiting human. After a later read confirms the user's main
merge, reassess required final/default-branch evidence and then mark complete.
Rejected final evaluation reopens the relevant tasks with the user's findings;
it does not authorize changing main. No autonomous merge is performed in this
coordinator implementation or its simulation.

## Branch-bound task context

Resolve the actual worktree before running these examples. Shell variables do
not persist across tool calls; set them again or substitute resolved values.
After creating the child worktree, write one UTF-8/LF line to its private git
directory's `mission-base` file (never the shared main git directory):

```text
feat/child origin/campaign/example https://github.com/OWNER/REPO/issues/123 https://github.com/OWNER/REPO/issues/123#issuecomment-456
```

The four fields are current child branch, exact remote base, campaign parent
URL and verified user merge-grant URL. Use `none` for the last field until a
grant exists; work and review can proceed but merges cannot. This file is a
local projection of GitHub authority, not authority itself. Reconstruct it from
the parent on resume and verify ownership before writing; never copy it between
branches. It is invalid for a final campaign-to-main PR, which uses the default
main context (no `mission-base` record). Remove an obsolete record only after
checking its branch and preserving its persisted source.

For every diff, task base and review dossier, resolve the base explicitly:

```sh
WT=/absolute/task/worktree
BASE=$(sh .claude/hooks/campaign_context.sh base "$WT") || exit 1
git -C "$WT" diff "$BASE...HEAD"
git -C "$WT" log "$BASE..HEAD" --oneline
```

Before task worktree creation, take the initial base from the verified parent;
after creation, this helper validates the branch-bound record. Use
`git worktree add -b <child> <isolated-path> <verified-base>` and arm the guards
as `new-task` requires. `gh pr create` must specify `--base <campaign-branch>`.
Native reviewers receive that same explicit base/diff, never an implicit main
comparison. Full final-campaign reviews instead compare the entire campaign
against current main. `delivery-review` reads the child mission for child PRs
and the parent campaign criteria plus child evidence for the consolidated PR.

Record both review markers only after their reviews pass, using the shared
key command instead of the legacy direct `git diff | git hash-object --stdin`
examples. The helper preserves the existing key for main and includes target
ref/tip for campaign children, so changing the base invalidates approvals:

```sh
WT=/absolute/task/worktree
KEY=$(sh .claude/hooks/campaign_context.sh key "$WT") || exit 1
GIT_DIR=$(git -C "$WT" rev-parse --absolute-git-dir) || exit 1
printf '%s\n' "$KEY" > "$GIT_DIR/arch-review-ok"
# Only after delivery-review PASS:
printf '%s\n' "$KEY" > "$GIT_DIR/delivery-review-ok"
```

After fresh checks/authorization readback, the only agent merge forms supported
by the gate are (substitute the explicit PR number and full reviewed HEAD):

```text
gh pr merge 42 --merge --match-head-commit FULL_HEAD_SHA
gh pr merge 42 --squash --match-head-commit FULL_HEAD_SHA
```

No `--auto`, `--admin`, alternate repository or queue shortcut is accepted.
Run that command alone from the task worktree; compound commands are rejected
so a second merge or retarget cannot share the first command's authorization.
Campaign readiness likewise uses a single `gh pr ready NUMBER` command in the
task worktree, with no repository override or other statement.
The helper verifies the live base/head and clean, non-draft, same-repository PR
with passing checks, and rejects a remote base that advanced since fetch.
The existing hook's documented command-detection limits still apply: this is
an operational guard, not a sandbox. GitHub main protections remain essential;
do not claim the shell hook can secure arbitrary shell/API access.
