---
name: feature
description: Split a large feature into a few implement-sized child issues, run fresh workers on them and judge the combined result.
disable-model-invocation: true
---

`/feature <outcome>` plans and runs. When the first token is exactly `plan`,
`run`, `resume` or `status`, it is a subcommand: `plan <outcome>` only files
issues, `run|resume|status <parent-URL>` reuses them, and `status` is read-only.
The user's `/feature` or `run` command authorizes execution; `plan` never does.
Ask once when the outcome or parent is ambiguous.

Parallel workers save wall-clock time, not tokens; the child count drives cost.

## Plan

1. Search paths, symbols and existing issues/PRs; do not read code bodies here
   (hand that to one `haiku` retrieval agent that returns paths). Reuse
   matching work.
2. Split into the fewest children (default 1-3), only at a crate or port
   boundary or above implement's `high` floor. Fold a linear chain into one
   child. One PR is the default when the feature fits one.
3. Write a binding `Design:` block of no more than 150 tokens, chosen with
   [new-extension](../new-extension/SKILL.md): the crate/module per child, the
   port signature, the shared types, and the dock file plus registration line.
   When the feature needs a new port, shared type or crate, child #0 is a
   behaviour-free skeleton that owns every hub file (registries, `Surfaces`,
   workspace `Cargo.toml`, ratchet baselines); later children add new files.
4. Write each acceptance criterion as `C<n>: given/when/then -> test
   <crate>::<name> | command <cmd>` and give it to one child. A criterion that
   spans children goes to the last child as a committed integration test and
   is marked `combined`.
5. File one parent and the child issues. Each child body *is* its implement
   plan: `Do`, `Touch` (owned paths), `Done` (its `C<n>` lines verbatim),
   `Check`, `No`, `Review` (`low` unless a floor applies) and `Depends on #M`,
   about 200 tokens. Link children as sub-issues:
   `gh api -X POST repos/{o}/{r}/issues/{parent}/sub_issues -F sub_issue_id=<REST id>`
   (the REST `id`, not the number; on failure keep the plain link).
   Pass multiline bodies with `--body-file`. No Project board.
6. Keep one coordinator section in the parent, preserving human text. Write it
   after planning and once per worker return:

   ```text
   <!-- quantick-feature:v1 -->
   Design: ...
   coordinator: <session> @ <UTC ISO time>
   Issue | Depends | State | PR / branch | Retry, next
   #N    | #M      | todo  | -           | 0, -
   <!-- end quantick-feature:v1 -->
   ```

   States: `todo`, `running`, `ready`, `blocked`. `done` is derived: the child
   PR says `Closes #N`, so a merge to `main` closes it.

## Run

7. On `run`/`resume` only, reconcile in two reads: the parent body, then one
   `gh pr list --state all --search "<child refs>" --json number,headRefOid,state,statusCheckRollup`.
   A `coordinator` stamp under 24h old with recent PR activity means another
   coordinator is live: stop. If it is older, ask the user before taking over.
8. Dispatch the next unblocked child; at most two run at once, as a host CPU
   cap, never to save tokens. A dependency clears when it is on `main`, or when
   it is `ready`: then the dependent branches from that PR head, still targets
   `main`, says `includes #M` and rebases after the merge.
9. Before launch, write the claim: `running`, branch `<prefix>/<issue>-<slug>`,
   worktree `../quantick-worktrees/<prefix>-<issue>-<slug>`. Spawn with
   `model: opus` (Codex: its strongest configured model) and pass only the issue
   URL, repository, pinned main SHA, the `Design:` block and the absolute path
   of [worker.md](references/worker.md). No conversation.
10. Accept `READY` mechanically, with no review in this context. Run
    `gh pr view <PR> --json state,headRefOid,statusCheckRollup` and
    `gh pr diff <PR> --name-only`, and check that:
    - the PR is open;
    - `headRefOid` equals the receipt's HEAD, with every required check green
      at that SHA;
    - every `C<n>` test named in the issue appears in the diff;
    - the changed files stay inside `Touch` (the skeleton's hubs belong to #0).

    Anything missing counts as a failed check.
11. A failed criterion or check earns one repair dispatch per child, across
    resumes. Brief it with only the failed `C<n>`, the evidence and the HEAD;
    the worker reruns that check and CI, with a `low` delta review only when
    the fix goes beyond the criterion. A `BLOCKED` receipt that needs a
    decision skips the repair. Either way, after that the row is `blocked`, and
    one comment on the child issue gives the reason and next action. Continue
    independent work. Never waive acceptance.
12. Do not refresh `ready` PRs when `main` moves. Rebase one only when GitHub
    reports a conflict or the user names it next to merge; that rebase spends
    the child's retry.

## Verdict

When every child is `ready` or `done` and a `combined` criterion exists, merge
the ready heads onto the pinned main SHA in a scratch worktree and spawn one
fresh judge (`model: opus`). It gets the `C<n>` list and the merged SHA, runs
the combined tests and returns PASS or FAIL per criterion, with the gap. A FAIL
earns one repair worker briefed with the failing `C<n>`, the output and the
merged SHA; then the parent is `blocked`. With no `combined` criterion, the
child checks are the verdict.

When nothing can advance, write the table and return ready PRs (merge order),
blockers and `/feature resume <URL>`. Do not poll for merges. Close the parent
when every child is closed. Keep only the table and receipts in context.
