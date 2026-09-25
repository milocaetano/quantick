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

Add a handoff only when pausing or transferring work: revision, remaining action/owner and blocker. The coordinator updates the same managed section at a real handoff or final delivery, preserving human/template content and review threads. A checkbox alone does not prove completion.

Delegate bounded work with expected SHA, role, owned paths, acceptance and accessible rules/checkout. Include the PR URL when available; reuse supplied current context rather than refetching it or sending chat history. Verify the revision, load more only as needed, and return findings, tested SHA, evidence and next action. Give parallel writers separate worktrees and non-overlapping ownership. Keep test/review evidence tied to a stable revision; after edits, revalidate what changed and check current remote HEAD/CI before completion. Only the coordinator edits the brief or pushes the integrated branch.
