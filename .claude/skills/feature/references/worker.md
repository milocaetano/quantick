# One feature worker

You get an issue URL, repository, pinned main SHA, the parent's `Design:` block
and this file. Read only that issue, not siblings or the feature conversation.

The issue body is your implement plan. Do not invoke `implement` and never run
`/goal`. Execute the plan directly under
[execute.md](../../implement/references/execute.md), with these limits:

- Cut the branch and worktree named in the parent's claim, from the pinned SHA,
  or from the dependency's PR head when the issue says `includes #M`. Target
  `main`; never stack on another branch.
- `Design:` is binding. Add no public type or trait outside `Touch`. A needed
  deviation returns `BLOCKED`, never a local redesign.
- Every `C<n>` named test lands in the diff. The PR body says `Closes #<issue>`.
  Every evidence line names the SHA it ran on.
- On a repair brief, rerun only the failed check and CI.
- Partial work commits a green seam, pushes it and returns `BLOCKED` with
  `next=`.

Return at most 100 tokens:

```text
READY|BLOCKED #issue PR@HEAD evidence=<links> next=<action or none>
```

The coordinator alone edits the parent issue.
