---
name: ship
description: Verify and deliver a Quantick branch through independent reviews and green CI. Prepare main PRs for the user; merge only into an explicitly authorized campaign branch.
---

# Ship the current branch

[The delivery contract](../../../docs/workflow/delivery.md) owns local
validation, evidence reuse, follow-ups and bounded repairs. Campaign children
take base, review key and merge command from [the integration
contract](../../../docs/campaign/integration.md); other tasks use `main`.

Never ship from `main` — work in the mission's worktree. Identify the linked
issue from the mission or branch, asking only if repository state cannot
settle it. Never manufacture review evidence.

1. **Validate the frozen change.** Classify the delta under the delivery
   contract. Code/config/test/script changes run, stopping to fix the first
   failure: `cargo fmt --all`, then `CONTRIBUTING.md`'s four checks in order, then
   its affected non-Cargo checks. Prose-only changes and qualifying evidence
   corrections take the contract's targeted checks. Record commands, outputs
   and input identity; mark reused evidence as reused; never infer success
   from an empty log or an old checkmark.

2. **Freeze.** Conventional English commits. Never commit `GOAL-archive-*`,
   logs, screenshots, NDJSON, dossiers or other raw evidence — keep it
   temporary or in GitHub/CI and summarize it in the PR. Batch compatible
   repairs; a later tracked edit needs new verdicts before stale markers are
   replaced.

3. **Draft PR.** Push the owned branch; create or reuse its draft with
   explicit base and linked issue via `gh pr create --draft --body-file -` and
   a heredoc (the form `pr-gate` exempts). Follow the PR template, include the
   `quantick-mission-summary:v1` block from `mission`, name the tier, and label
   verification as local, reused or CI. A draft is not permission to merge.
   On a campaign base, `Closes #N` closes nothing at an intermediate merge;
   close explicitly after integration proof.

4. **Review the final diff.** `arch-review` with its bug pass; `ai-review`,
   one resolvable thread per finding, recording `ai-review-complete` at every
   tier even with zero findings; `delivery-review` last under its tier rules.
   Resolve findings; only deferrals authorized under the delivery contract
   ship, listed in the PR. On PASS the skills' producer
   (`.claude/hooks/review_report.sh`) publishes the report and records
   `arch-review-ok` and `delivery-review-ok` for the current diff — never
   write or refresh a marker directly. Read the tier with `arch-review`'s step
   0 command after assigning `WT` (a bare `cd ""` reads the main checkout's
   file). The `small` exemption holds only under the hook ceiling. Delta
   follow-ups may carry unaffected evidence forward, still issuing a verdict
   for the current key.

5. **Checks and repairs.** Observe required CI at the actual PR head (`gh pr
   checks <n>`, or run/job metadata when unregistered). Read failure logs,
   repair within scope and budget, validate, push, refresh affected reviews.
   Never rerun unchanged passing checks; pending, missing or red is not
   success.

6. **Ready and deliver.** With current reviews, checks and findings proven:
   a draft gets `gh pr ready <n>` from the reviewed worktree; an already-ready
   PR does not, and its missing transition is not evidence. Either way finish
   with `sh .claude/hooks/mission_ship_gate.sh ship <n>` from that worktree and
   refuse delivery without `MISSION-COMPLETION:PASS`. Mission and sync PRs
   carry one mission-summary block; consolidated campaign PRs carry the
   `Campaign-parent:` charter (integration steps 6-7). Report the PR URL and
   exact-head CI. The user alone merges to `main`. An authorized campaign child
   returns to its coordinator for the head-pinned merge, keeping its worktree
   until integration is verified; then clean up only your own clean worktree.
