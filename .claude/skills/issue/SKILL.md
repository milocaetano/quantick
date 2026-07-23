---
name: issue
description: Turn an idea into a well-formed GitHub issue with context, scope and acceptance criteria, correct labels, milestone and board placement. Use when the user types /issue <idea> or asks to create or file an issue.
---

# Create a well-formed issue

## Steps

1. **Vague-idea check**: if there is no concrete deliverable yet, suggest opening a GitHub Discussion instead and stop. Issues are actionable work items only.

2. **Gather** — ask at most 2–3 questions, and only for what cannot be inferred:
   - **Context** — what problem this solves, why it matters
   - **Scope** — what is in, and what is explicitly out
   - **Acceptance criteria** — checkboxes answering "how do we know it's done"

3. **Title**: `type(area): imperative description`, matching existing issues (e.g. `feat(engine): tick bars (close after N trades)`). Everything in English.

4. **Create** with `gh issue create --body-file -` using body sections `## Context`, `## Scope`, `## Acceptance criteria`. Apply one `area:*` label plus one `type:*` label (or `bug`), and the current milestone (`v0.1 - Engine core`) unless told otherwise.

5. **Add to the board** in Todo:

   ```sh
   ITEM=$(gh project item-add 1 --owner milocaetano --url <issue-url> --format json --jq '.id')
   gh project item-edit --id "$ITEM" --project-id PVT_kwHOA0fkv84BeK9c \
     --field-id PVTSSF_lAHOA0fkv84BeK9czhYnKUg --single-select-option-id f75ad846
   ```

6. **Report** the issue number and URL.

## Label reference

- Areas: `area:engine`, `area:feed`, `area:app`
- Types: `type:feat`, `type:fix`, `type:docs`, `type:test`, `type:ci`, `bug`
