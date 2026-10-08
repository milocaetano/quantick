# Codex compatibility

The Claude workflow owns outcomes, gates, evidence, markers, authority and
done. Translate only these host mechanics:

- `/name` and `Skill(name)` mean Codex `$name`.
- `AskUserQuestion` means Codex user input under the session policy.
- A mission authorizes the Codex goal facility; otherwise use `GOAL.md`.
- For `implement` preparation, replace only the final `/goal` prefix with
  `Create a Codex goal:` and preserve the rest of that line. This is a handoff
  for a later explicit user request: do not call `create_goal` or start work
  while preparing the plan. Resolve its `Rules:` relative to
  `.claude/skills/implement/` in the same checkout; preparation only verifies
  that file exists, as the canonical skill requires.
- `/code-review` means native review or direct inspection of the named diff.
- Give a fresh subagent only its dossier; use fast models for retrieval,
  balanced for checklists and the strongest for judgment. A campaign
  `executor:` line maps the same way: `haiku` fast, `sonnet` balanced, `opus`
  the default coding model, `fable` the strongest.
- Translate POSIX examples without changing order, worktree, markers or failure.
- `.codex/hooks.json` runs shared guardrails without adding permissions.
- Durable review publication and final mission/ship verification are repository
  commands, so Codex runs the canonical scripts through `exec_command` exactly
  as written. Host goal status and local marker writes never substitute for
  their successful current-PR receipts.
- The mission skill's phase two dispatches: after the draft PR, each review,
  the CI watch and each repair round is a fresh Codex subagent given the same
  PR-only prompt and answering in the same one line.
