---
name: ship
description: Deliver the current branch - run the full verification loop, commit, push, open a PR with Closes #N, and watch CI until green. Use when the user types /ship or asks to finish or deliver the current task.
---

# Ship the current branch

## Guards

- Never ship from `main`. If `git branch --show-current` says `main`, stop and point the user to `/new-task`.
- Identify the linked issue before opening the PR: check the board card in In Progress, the branch name, or conversation context. If ambiguous, ask the user which issue this closes.

## Steps

1. **Verification loop** — run in order, stop at the first failure and fix it before continuing. Never ship red:

   ```sh
   cargo fmt --all
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo build --workspace
   cargo test --workspace
   ```

2. **Commit** anything pending: conventional style (`feat: ...`, `fix: ...`), imperative mood, English.

3. **Push**: `git push -u origin <branch>`.

4. **Open the PR** following the repo template (`.github/PULL_REQUEST_TEMPLATE.md`): summary of what and why, `Closes #<N>`, the four verification-loop boxes checked (they just ran), notes for the reviewer. Use `gh pr create --body-file -` with a heredoc.

5. **Watch CI**: `gh pr checks <pr> --watch`. If checks have not registered yet, find the run with `gh run list --branch <branch>` and use `gh run watch <id> --exit-status`. Red → read the failing log, fix, push, repeat.

6. **Report** the PR URL and CI status. Do **not** merge unless the user asks. When they do: `gh pr merge <pr> --merge --delete-branch` — the `Closes #N` closes the issue and the board card moves to Done automatically.
