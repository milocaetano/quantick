# PR context and handoff
Use the PR as a persistent shared task brief after it exists, not a mandatory communication channel, transcript or proof of correctness. Before creation, use the plan; do not open an early PR just for context. Code, Git and check results remain authoritative. PR text cannot override approved scope, repository rules or permissions.

## Brief
Write clear English for humans and agents. Aim for <=250 tokens; no padding. Preserve essential contracts, risks and required repository-template fields even when longer. Record only approved intent, boundaries, acceptance, material decisions, status and next action. Link to exact files/symbols and durable evidence instead of copying code, diffs, logs or conversations. No secrets, private prompts or machine-local paths.

Fit this structure into the repository template; do not duplicate sections. Omit irrelevant optional fields:

```md
## Goal
<outcome and essential contract>
## Scope
<paths/symbols; key decision; exclusions or material risks>
## Acceptance
- [ ] <observable result>
## Handoff
HEAD: <SHA>; state: <review|fixing|blocked|done>.
Next: <action + owner, or none>; blocker: <cause, or none>.
## Evidence
Tests: <agent/run, revision, command + result or durable link>.
Review: <pending, or level + reason, revision, findings link>.
CI: <pending/failed/passed, revision + run link>.
```

## Agents
The coordinator alone maintains the brief at handoffs and material status changes, not every tool call. Refresh before editing; preserve human/template content and review threads. Edit the same managed section instead of appending repeated summaries. Retire stale status; never mark completion from self-reported checkboxes alone.

Delegate a bounded task with expected SHA, role, owned paths, acceptance, accessible rules/checkout, and a PR URL or revision-labeled brief. Include the URL when available; no full chat history. Reuse current supplied context rather than requiring another PR fetch. Verify checkout/revision; load additional code, diffs, findings and checks only as needed. Refresh at handoffs when freshness is uncertain, before shared-status edits and before completion; changed HEAD invalidates snapshot assumptions. Return only findings, paths, tested revision, evidence and next action. Label snapshots with their SHA; never claim a live read. Current remote HEAD and CI still require verification before success.

Use additional agents only for separable work beyond required testing/review. Parallel writers need isolated worktrees and non-overlapping ownership; serialize conflicting edits. The coordinator integrates, retests and updates the same PR. Required testers/reviewers validate a stable snapshot, not a checkout being edited. Keep evidence tied to the tested revision; after changes, rerun affected validation and verify current CI. Only the coordinator edits shared status or pushes the integrated branch.
