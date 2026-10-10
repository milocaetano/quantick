# One feature worker

You get an issue URL, repository, `feature/<slug>`, the parent and grant URLs,
the parent's `Design:` block and this file. Read only that issue, not siblings
or the feature conversation.

The issue body is your implement plan. Do not invoke `implement` and never run
`/goal`. Execute the plan directly under
[execute.md](../../implement/references/execute.md), with these limits:

- Cut the branch and worktree named in the parent's claim from
  `origin/feature/<slug>`, and write its `mission-base` record as
  [the integration contract](../../../../docs/campaign/integration.md) says.
  Open the PR with `--base feature/<slug>`; this overrides execute.md's
  main-only target. Never merge.
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
