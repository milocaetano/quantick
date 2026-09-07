---
name: ship
description: Verify, review and deliver the current branch through green CI. Use for /ship or requests to finish or deliver a task.
---

Campaign children override the main-based examples via the
[integration contract](../../../docs/campaign/integration.md), including review keys.

# Ship the current branch

## Guards

- Check `git branch --show-current`; on `main`, stop and direct to `/new-task`.
- Identify the issue from the In Progress card, branch or conversation before
  opening the PR. Ask only if ambiguous.

## Steps

1. **Verification** — run in order; stop and fix the first failure:

   ```sh
   cargo fmt --all
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets
   cargo build --workspace
   cargo test --workspace
   ```

2. **Commit** pending work: conventional style (`feat: ...`, `fix: ...`),
   imperative mood, English. For `/mission`, archive `.claude/GOAL.md` as the
   last commit **before** reviews, using `mission` step 8's commands. Changed
   review keys stale markers; never restamp without rerunning the review.

3. **Arch-review** (mandatory): run the skill over `git diff origin/main...HEAD`
   or the explicit campaign base. Wait for its background step 0 `code-review`
   findings and handle them before calling this closed. Fix every Blocker and
   Should-fix; note deliberate deferrals in the PR body. Rerun step 1 after
   changes. Never push or open a PR before this step closes. The skill itself
   records `arch-review-ok` when closed.

4. **Delivery-review**: assign `WT=/path/to/worktree` in the same shell call,
   then `cd "$WT" && cat "$(git rev-parse --absolute-git-dir)/mission-tier"`.
   Read the gate's private file, never the goal's `**Tier:**` prose. Only a
   current-branch `small` declaration within the diff-size ceiling exempts this
   review. Missing/wrong-branch records or an outgrown ceiling grant no exemption;
   `.claude/hooks/README.md` owns that mechanism.

   After step 3, use the skill's fresh-context subagent to grade every ledger
   ask and criterion in `.claude/GOAL-archive-$SLUG.md`, which replaced the live
   goal in step 2. A `/new-task` branch without a mission is graded against its
   linked issue's `## Acceptance criteria`, named in the verdict. PASS requires
   nothing MISSING, PARTIAL or UNPROVEN. Fix findings, rerun step 1, commit,
   rerun step 3 over the new head, then rerun delivery review. Only the skill
   records `delivery-review-ok`, on PASS.

   Keep `CLAUDE.md`'s stall rule: runs are not rationed, but if the open set does
   not shrink, stop, retain every remainder with severity in the PR body and
   take it to the user. Never discard findings or defer an open Blocker.

5. **Push**: `git push -u origin <branch>`.

6. **Open a draft PR** using `.github/PULL_REQUEST_TEMPLATE.md`: what/why,
   `Closes #<N>`, the four verified checkboxes, reviewer notes and mission tier
   (state a delivery exemption publicly). Title/body stay English under
   `CLAUDE.md`; the file language guard cannot check a PR body. Use
   `gh pr create --draft --body-file -` with a heredoc and an explicit campaign
   `--base` when applicable. Run `ai-review` against the PR. It owns the durable
   report and `ai-review-complete` recording procedure; completion is required
   even with zero findings, at every tier. Close all its resolvable threads
   under its existing fix/acceptance and follow-up rules. Changes require step 1,
   commit, stale reviews rerun and another push; never merely restamp.

7. **Watch CI**: `gh pr checks <pr> --watch`. If unregistered, locate it with
   `gh run list --branch <branch>`, then `gh run watch <id> --exit-status`.
   Read failing logs, fix, validate and push until green.

8. **Mark ready and report**: after current required reviews, zero open AI
   threads and green CI, run `gh pr ready <pr>` from the task worktree. Report
   its URL and CI status. The user alone merges to `main`; never enable
   auto-merge or enqueue it. Authorized campaign merges follow the integration
   contract from the reviewed worktree; retain its private evidence through merge.
   After verified integration remove only the owned clean worktree and merged
   local branch. Explicitly prove/synchronize campaign issue closure; merging to
   a non-default base does not close it.
