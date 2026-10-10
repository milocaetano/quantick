# One implement worker

Receive issue URL, repository, pinned main SHA, owned paths and any checkpoint.
Read that issue and applicable repository instructions, not sibling issues or
the feature conversation. Block if the checkout or ownership is inconsistent.

1. Preparation invokes [implement](../../implement/SKILL.md). It writes the
   minimal local plan and prints a goal; preparation never executes it. Reuse a
   matching plan/PR on resume; reconstruct a missing plan from the issue and PR.
2. Execution is a separate stage authorized by the feature run request. Follow
   the plan's `Rules` exactly, including isolation, architecture, agent operation,
   checks, independent review/testing and current-head CI. Target `main` only;
   no campaign branch, stacked target or mission fallback.
3. Execute the emitted goal only with explicit goal authorization and an
   isolated host goal facility. Otherwise execute the same plan/Rules as a
   bounded task, report host-limited continuation and never replace shared goals.
   Obtain the coordinator's review/test slot before spawning either agent;
   release it when that agent finishes.
4. Keep acceptance/check results and review/test evidence in the PR, bound to
   the tested revision. Preserve review-round counts across handoffs. On failure,
   leave a safe branch checkpoint and concrete next action; do not start siblings.

Return at most 100 tokens:

```text
READY|BLOCKED #issue PR@HEAD base=<SHA> evidence=<links> next=<action or none>
```

Include branch/checkpoint if no PR exists. `READY` requires all implement gates.
The coordinator alone edits the parent and Project.
