# Codex compatibility

The Claude workflow owns outcomes, gates, evidence, markers, authority and
done. Translate only these host mechanics:

- `/name` and `Skill(name)` mean Codex `$name`.
- `AskUserQuestion` means Codex user input under the session policy.
- For `implement` preparation, replace only the final `/goal` prefix with
  `Create a Codex goal:` and preserve the rest of that line. This is a handoff
  for a later explicit user request: do not call `create_goal` or start work
  while preparing the plan. Resolve its `Rules:` relative to
  `.claude/skills/implement/` in the same checkout; preparation only verifies
  that file exists, as the canonical skill requires.
- `/code-review` means native review or direct inspection of the named diff.
- Give a fresh subagent only its dossier; use fast models for retrieval,
  balanced for checklists and the strongest for judgment.
- Translate POSIX examples without changing order, worktree, markers or failure.
- `.codex/hooks.json` runs shared guardrails without adding permissions.
