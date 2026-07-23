---
name: new-task
description: Start work on a GitHub issue the right way - read it, branch from updated main with the correct prefix, move the board card to In Progress. Use when the user types /new-task <issue-number> or asks to start or pick up an issue.
---

# Start work on an issue

Argument: the issue number (e.g. `/new-task 3`). If missing, list open issues in the current milestone (`gh issue list --milestone "v0.1 - Engine core"`) and ask which one to pick.

## Steps

1. **Read the issue**: `gh issue view <N>`. If it does not exist or is already closed, stop and report. Summarize scope and acceptance criteria back to the user — they become the work checklist.

2. **Guard the working tree**: `git status --short` must be clean. If not, stop and ask what to do with the pending changes — never carry unrelated work into a new branch.

3. **Branch from updated main**:

   ```sh
   git checkout main
   git pull origin main
   git checkout -b <prefix>/<short-kebab-slug>
   ```

   Prefix from the issue's labels: `bug` or `type:fix` → `fix/`, `type:docs` → `docs/`, everything else → `feat/`. Slug: short kebab-case from the issue title.

4. **Move the board card to In Progress** (project "quantick roadmap"):

   ```sh
   ITEM=$(gh project item-list 1 --owner milocaetano --format json \
     --jq '.items[] | select(.content.number==<N>) | .id')
   # If empty, the issue is not on the board yet - add it first:
   # ITEM=$(gh project item-add 1 --owner milocaetano --url <issue-url> --format json --jq '.id')
   gh project item-edit --id "$ITEM" --project-id PVT_kwHOA0fkv84BeK9c \
     --field-id PVTSSF_lAHOA0fkv84BeK9czhYnKUg --single-select-option-id 47fc9ee4
   ```

5. **Report**: branch name, card moved, and the acceptance criteria as a checklist. For `area:engine` issues, remind: test-first — fixture trades and expected bars are written before the implementation.

## Board reference (project 1)

- Project ID: `PVT_kwHOA0fkv84BeK9c`
- Status field ID: `PVTSSF_lAHOA0fkv84BeK9czhYnKUg`
- Status options: Todo `f75ad846` · In Progress `47fc9ee4` · Done `98236657`
