---
name: implement
description: "Prepare a minimal implementation goal; do not implement."
disable-model-invocation: true
---

Use the request and existing answers. Write plans and `/goal` instructions in English; preserve exact paths and identifiers. Ask questions in the user's language.

# Plan only
Allow targeted reads, essential questions and one plan MD. Do not change code/config/Git, create worktrees, run tests/agents/reviews, open PRs or execute `/goal`.

# Minimal context
Aim for <=200 tokens per plan; never pad. Exceed only for essential contracts, safety or acceptance. Include only implementation/validation facts: no history, copied code/logs, explanations, repetition, empty sections or permanent rules.
Search paths/symbols before reading files; stop when sufficient. Ask only blocking decisions, at most three short questions per round; never repeat answers available in the request/repo.

# Generate
1. Read repo root, status, base SHA, PR target and relevant paths. Do not assume pending changes belong to this task.
2. Resolve `references/execute.md` from this installed skill's directory. Verify existence without reading it; record its absolute path.
3. Save to the requested location, existing goals directory or `docs/goals/<slug>-<id>.md` without overwriting unrelated files:

```md
# <outcome>
Repo: <root>; base: <SHA>; PR: <remote/target>.
Rules: <absolute-path>/references/execute.md
Do: <change and essential contract>.
Touch: <paths/symbols>.
Done: <verifiable acceptance>.
Check: <known checks or discovery location; performance if relevant>.
No: <essential exclusion; optional>.
```

Use concise, unambiguous English. Omit empty optional fields. Never invent commands or choose review depth.

# Output
Recheck and compact the saved plan. Ask if a blocking decision remains; otherwise output only:
```text
/goal Read "<absolute-plan-path>", load its Rules, and deliver a tested, reviewed PR with green CI at the current remote HEAD. Do not merge.
```
Do not execute this command.
