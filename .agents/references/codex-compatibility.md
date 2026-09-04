# Codex compatibility

The linked Claude skill is canonical. Preserve its outcomes, gates, evidence,
markers, and authority boundaries while mapping host mechanics:

- `/name` or `Skill(name)` means the matching Codex `$name` skill.
- Map `AskUserQuestion` to the available Codex user-input mechanism, still
  obeying the session interaction policy.
- A mission request authorizes the available Codex goal facility. Use it
  instead of printing Claude's `/goal`; otherwise execute from `GOAL.md`.
- Map `/code-review` to native review, or inspect the requested diff directly.
- A required fresh subagent receives only the stated dossier. Map models by
  role: fast for retrieval, balanced for checklists, strongest for judgment.
- Translate POSIX shell examples to the active shell without changing their
  ordering, target worktree, marker contents, or failure behavior.
- `.codex/hooks.json` runs the shared guardrails. Trusting it never expands
  command permissions.

These mappings change mechanisms only; the canonical workflow decides done.
