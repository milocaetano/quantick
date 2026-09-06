# Codex compatibility

The Claude workflow owns outcomes, gates, evidence, markers, authority and
done. Translate only these host mechanics:

- `/name` and `Skill(name)` mean Codex `$name`.
- `AskUserQuestion` means Codex user input under the session policy.
- A mission authorizes the Codex goal facility; otherwise use `GOAL.md`.
- `/code-review` means native review or direct inspection of the named diff.
- Give a fresh subagent only its dossier; use fast models for retrieval,
  balanced for checklists and the strongest for judgment.
- Translate POSIX examples without changing order, worktree, markers or failure.
- `.codex/hooks.json` runs shared guardrails without adding permissions.
