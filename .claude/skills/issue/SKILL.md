---
name: issue
description: File or start a GitHub issue. `/issue <idea>` turns an idea into a well-formed issue with context, scope, acceptance criteria, labels, milestone and board placement; `/issue start <N>` picks one up — reads it, names the branch, moves the board card to In Progress, and hands over to mission. Use when the user types /issue or /new-task, or asks to create, file, start or pick up an issue.
---

Campaign children override the main-based examples via the
[integration contract](../../../docs/campaign/integration.md), including review keys.

# Issues

Board: project 1, ID `PVT_kwHOA0fkv84BeK9c`, status field
`PVTSSF_lAHOA0fkv84BeK9czhYnKUg`, options Todo `f75ad846` · In Progress
`47fc9ee4` · Done `98236657`. Labels: one `area:*` (`area:engine`,
`area:feed`, `area:app`) and one `type:*` (`type:feat`, `type:fix`,
`type:docs`, `type:test`, `type:ci`) or `bug`. Milestone: `v0.1 - Engine core`
unless told otherwise.

```sh
# ITEM for an issue already on the board, else add it:
ITEM=$(gh project item-list 1 --owner milocaetano --format json \
  --jq '.items[] | select(.content.number==<N>) | .id')
ITEM=$(gh project item-add 1 --owner milocaetano --url <issue-url> --format json --jq '.id')
gh project item-edit --id "$ITEM" --project-id PVT_kwHOA0fkv84BeK9c \
  --field-id PVTSSF_lAHOA0fkv84BeK9czhYnKUg --single-select-option-id <option>
```

## `/issue <idea>` — file one

1. No concrete deliverable yet: suggest a GitHub Discussion and stop; issues
   are actionable work only.
2. Ask at most 2–3 questions, only for what cannot be inferred: context (the
   problem, why it matters), scope (in and explicitly out), acceptance
   criteria (checkboxes for "done").
3. Title `type(area): imperative description`, like existing issues
   (`feat(engine): tick bars (close after N trades)`); all English.
4. `gh issue create --body-file -` with `## Context`, `## Scope`,
   `## Acceptance criteria`, the labels and the milestone.
5. Add it to the board in Todo; report number and URL.

## `/issue start <N>` — pick one up (was `/new-task`)

No number: list open issues in the milestone (`gh issue list --milestone
"v0.1 - Engine core"`) and ask which.

1. `gh issue view <N>`; missing or closed → stop and report. Summarize scope
   and acceptance criteria back — they are the work checklist.
2. Branch `<prefix>/<short-kebab-slug-from-title>`, prefix from labels: `bug`
   or `type:fix` → `fix/`, `type:docs` → `docs/`, else `feat/`.
3. `mission` step 6 cuts or reuses the worktree (never a second one for the
   same issue) and arms the guards.
4. Move the card to In Progress; report branch, worktree, card and the
   criteria as a checklist. For `area:engine`: test-first — fixture trades and
   expected bars before the implementation.
