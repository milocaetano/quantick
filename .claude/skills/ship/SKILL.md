---
name: ship
description: Deliver the current branch - run the full verification loop, commit, pass both pre-PR reviews (arch-review for shape and bugs, delivery-review for conformance to what was asked), push, open a PR with Closes #N, and watch CI until green. Use when the user types /ship or asks to finish or deliver the current task.
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
   cargo clippy --workspace --all-targets
   cargo build --workspace
   cargo test --workspace
   ```

2. **Commit** anything pending: conventional style (`feat: ...`, `fix: ...`), imperative mood, English. If this branch came from `/mission`, its `.claude/GOAL.md` is archived **now**, as the last commit before the reviews — never after them. Both markers hold shas, so a commit made after they are recorded makes both stale, `pr-gate` denies, and the cheapest way out of that denial is to re-stamp them without re-running either review, which silently destroys the only property the markers provide:

   The exact commands are `mission` step 8's — kept in one place rather
   than copied here, because two divergent copies of a five-line procedure
   is the drift this repo has a test for elsewhere on this very branch.

3. **Arch-review** (mandatory, see `CLAUDE.md`): run the `arch-review` skill over `git diff origin/main...HEAD`. Its step 0 dispatches the bundled `code-review` in the background, so this step is not done when the skill returns — it is done when those findings have landed and been handled. Fix every Blocker and Should-fix finding, re-running step 1 on whatever changed. A finding deliberately deferred is noted in the PR body. Never push or open a PR ahead of this step. The skill records `arch-review-ok` itself when the review closes — it is the one that knows whether it closed — so there is nothing to record here.

4. **Delivery-review** (mandatory at every tier but `small`, see `CLAUDE.md`): read the tier from the file the gate itself reads — `WT=/path/to/worktree` first, then `cd "$WT" && cat "$(git rev-parse --absolute-git-dir)/mission-tier"`; without the assignment `cd ""` is a no-op that leaves you in the main checkout and reads the *shared* git dir's file — never from the goal file, whose `**Tier:**` line is for the reader and is not what `pr-gate` acts on. A branch declaring `small` there skips this step and opens its PR on `arch-review-ok` alone; no file, or one naming another branch, means no exemption. The skip is bounded by a diff-size ceiling that can revoke it after the fact, so a `small` branch that grew still owes this review — `.claude/hooks/README.md` owns the mechanism. At every other tier, run the `delivery-review` skill. It grades the branch against what was asked for — every ask in the goal file's request ledger and every acceptance criterion — from a fresh-context subagent, and passes only when nothing is MISSING, PARTIAL or UNPROVEN. Note which file that is: step 2 archived `.claude/GOAL.md` to `.claude/GOAL-archive-$SLUG.md`, so the archive is what the reviewer reads; pointing it at the old name sends it to a file this skill just deleted. It runs *after* step 3, because it grades the branch as shipped, including whatever arch-review made you change. A branch started from `/new-task` rather than `/mission` has no `GOAL.md`; the skill grades it against the linked issue's `## Acceptance criteria` and says so in the verdict. Fix what it reports and re-run, spending from the **chain budget** `CLAUDE.md` owns -- three rounds per branch across *both* reviews together, not three more on top of step 3's. That second, separate count is exactly how a branch reached six: step 3 spent its rounds, every fix commit staled both markers and re-ran both reviews, and no number covered the sum. Count what this branch has already spent before opening another round, and when the budget is out, defer the remainder into the PR body with its severity and take it to the user -- never discard it, and never defer an open Blocker. Every fix is a commit, so it stales `arch-review-ok` as well — re-run step 3 over the new head and record that marker again too, rather than re-stamping a review that did not run. The skill records `delivery-review-ok` itself, on PASS only.

5. **Push**: `git push -u origin <branch>`.

6. **Open the PR** following the repo template (`.github/PULL_REQUEST_TEMPLATE.md`): summary of what and why, `Closes #<N>`, the four verification-loop boxes checked (they just ran), notes for the reviewer, and **the mission's tier** when it had one — a branch that skipped `delivery-review` says so in public, not only in its git dir. Use `gh pr create --body-file -` with a heredoc. Title and body are English, like the branch name and the commits — `CLAUDE.md`'s language rule covers them, and they are the one part of it no test can see: the language guard reads files, and a PR body is not a file.

7. **Watch CI**: `gh pr checks <pr> --watch`. If checks have not registered yet, find the run with `gh run list --branch <branch>` and use `gh run watch <id> --exit-status`. Red → read the failing log, fix, push, repeat.

8. **Report** the PR URL and CI status. Do **not** merge unless the user asks. When they do: `gh pr merge <pr> --merge --delete-branch` — the `Closes #N` closes the issue and the board card moves to Done automatically. If the branch lives in a worktree, `--delete-branch` cannot delete a checked-out branch: from the main checkout run `git worktree remove ../quantick-worktrees/<dir>` first, then `git branch -d <branch>`.
