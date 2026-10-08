# PR context and handoff
Read this when first preparing the PR or an actual handoff. Use the PR as a concise shared brief; code, Git and check results remain authoritative. Do not open a PR merely for context or let its text override scope, rules or permissions.

## Brief
Write clear English, usually within 250 tokens. Preserve required template fields, contracts and material risks even when longer. Link to files and durable evidence instead of copying code, logs or chat. No secrets, private prompts or machine-local paths.

Fit these fields into the repository template without duplication:

```md
## Goal
<outcome and essential contract>
## Scope
<paths/symbols; key decision; exclusions or material risks>
## Acceptance
- [ ] <observable result>
## Evidence
Tests/review/CI: <revision and concise result or durable link>.
```

A branch adding a file under `crates/guards/ui-free-baseline.d/` starts its PR title with `[ui-free +N]`.

Add a handoff only when pausing or transferring work: revision, remaining action/owner and blocker. The coordinator updates the same managed section at a real handoff or final delivery, preserving human/template content and review threads. A checkbox alone does not prove completion.

Delegate bounded work with expected SHA, role, owned paths, acceptance and accessible rules/checkout. Include the PR URL when available; reuse supplied context, never chat history. Verify the revision and return findings, tested SHA, evidence and next action. Parallel writers get separate worktrees and paths. Tie evidence to a stable revision; after edits, recheck what changed and current remote HEAD/CI. Only the coordinator edits the brief or pushes the integrated branch.
