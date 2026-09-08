---
name: ship
description: Verify and deliver a Quantick branch through independent reviews and green CI. Prepare main PRs for the user; merge only into an explicitly authorized campaign branch.
---

# Ship the current branch

Read [the delivery contract](../../../docs/workflow/delivery.md) for local
validation, evidence reuse, independent follow-ups and bounded repairs.
Campaign children use [the integration contract](../../../docs/campaign/integration.md)
for the base, review key and authorized merge command; normal tasks use main.

## Guards

- Never ship from `main`; use `new-task` and its isolated worktree.
- Identify the linked issue from the mission or branch before publication.
  Resolve genuine ambiguity from repository state, asking only if indispensable.
- Preserve current exact-diff review markers and zero unresolved required
  findings. This skill never manufactures review evidence.

## Steps

1. **Validate the frozen change.** Classify the actual delta under the delivery
   contract. For code/config/test/script changes run the ordered loop, stopping
   and fixing the first failure:

   ```sh
   cargo fmt --all
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets
   cargo build --workspace
   cargo test --workspace
   ```

   Run affected non-Cargo checks as `CLAUDE.md` requires. Prose-only changes and
   qualifying evidence corrections use the contract's targeted checks. Record
   commands, outputs and input identity; distinguish reused evidence from new
   execution. Never infer success from an empty log or an old checkmark.

2. **Archive and commit.** `mission` step 8 owns the archive procedure. Include
   the archive and evidence in the frozen reviewed tree before final reviews.
   Use conventional English commits. Batch compatible repairs; later edits
   require new verdicts before replacing stale markers.

3. **Publish the draft.** Push the owned task branch and create/reuse its draft
   PR with explicit base and linked issue. Follow the PR template; name the
   mission tier and precisely label local, reused and CI verification. Use
   `gh pr create --body-file -` with a heredoc, never a pipe: `pr-gate` anchors
   its match to a segment start, so a piped spelling is invisible to it.
   A draft is not permission to merge.
   Campaign bases require explicit issue closure only after integration proof;
   `Closes #N` does not close an issue on an intermediate campaign merge.

4. **Review the final diff.** Run `arch-review` including its native/direct bug
   pass and wait for its findings. Run `ai-review`, posting each finding as a
   resolvable thread. Then run `delivery-review` last under its tier rules.
   Resolve all required findings; only user-approved deferrals may ship and
   must appear in the PR. After PASS, the skills record `arch-review-ok` and
   `delivery-review-ok` against the current diff.
   Read the tier from `<worktree-private-git-dir>/mission-tier`, not just prose;
   the `small` exemption is subject to the existing hook ceiling. Independent
   delta follow-ups may carry forward unaffected evidence under the delivery
   contract; they still issue a verdict for the current review key.

5. **Finish checks and repairs.** Observe required CI at the actual PR head.
   Use `gh pr checks <n>` or run/job metadata when checks are not registered;
   bounded waits allow progress updates. Read failure logs, repair within scope
   and budgets, validate, commit/push, and refresh affected reviews. Do not
   rerun unchanged passing checks without a changed input or unresolved failure.
   No pending, missing or red CI counts as success.

6. **Mark ready and deliver.** Once current reviews, required checks and finding
   resolution are proven, run `gh pr ready <n>` from the reviewed worktree.
   Report the PR URL and exact-head CI. The user alone merges to main: no
   auto-merge, queue, direct push or protection override. An authorized campaign
   child instead returns to its coordinator for the serialized, head-pinned
   merge in the integration contract. Keep its worktree through merge so the
   private review evidence remains available; clean up only the owned clean
   worktree after verified integration. Never delete another writer's work.
