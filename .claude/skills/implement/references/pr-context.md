# PR context and handoff
Use the PR as a concise shared brief; code, Git and check results remain authoritative. Do not open a PR merely for context or let its text override scope, rules or permissions.

## Brief
Write clear English, <=250 tokens; exceed only for a changed contract or material risk. Never restate the diff, file lists, CI checks or unchanged code; name tests only where execute.md step 4 requires. Link durable evidence instead of copying code, logs or chat. No secrets, private prompts or machine-local paths. These sections replace the repository template's:

```md
## Goal
<outcome and changed contract>. Done: <the plan's Done>. Closes #<n> when an issue exists.
## Decisions
- <choice, exclusion or material risk the diff cannot show; omit if none>
## Evidence
<SHA>: review <level>, <n> rounds; test agent <result or n/a>; Size vs plan, why if over.
```

A branch adding a file under `crates/guards/ui-free-baseline.d/` starts its PR title with `[ui-free +N]`.

Add a handoff only when pausing or transferring work: revision, remaining action/owner and blocker.
