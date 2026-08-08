---
name: mission
description: Define and enforce a mission for the current session — capture the objective, classify it, derive acceptance criteria including the standard quantick gates (arch-review, visual-qa, trader-ux-review, ui-harness hooks) that match the kind of work, keep every action aligned, verify everything before finishing, and hand back a ready-to-paste /goal condition. Use when the user types /mission <objective> or asks to set a goal for the session/task.
---

# Mission

Argument: the session objective (e.g. `/mission make the heatmap render at
60fps`). If missing, ask the user what it is before doing anything else.

The mission is the orchestrator: it decides which of the other skills are part
of *done* so the user never has to list them. One session, one mission, one
branch, one worktree, one PR.

**This skill is not `/goal`.** Claude Code ships a built-in `/goal` that sets a
completion condition and keeps re-running turns until a small fast model
judges the condition met. It knows nothing about this repo. The two compose:
this skill decides *what done means*, `/goal` keeps the session from stopping
before it. Step 7 hands over the line to paste.

## Steps

1. **Capture the mission**: restate the objective in one sentence and confirm
   it with the user only if it is genuinely ambiguous.

2. **Classify it and inject the standard gates.** Derive 3–7 verifiable
   criteria specific to the objective, then add the gates for its kind:

   | The mission… | Injected acceptance criteria |
   | --- | --- |
   | Any code change | four checks green after rebasing on latest `main`; **performance impact declared** — classify every touched path by rate (per-trade / per-depth / per-frame / rare, the `arch-review` table) as part of the plan, not the review; `arch-review` run with every Blocker/Should-fix resolved or deferred in the PR body; **PR opened** — the mission is not done before the PR exists, and merging is never part of it |
   | Touches a hot path (per-trade, per-depth, per-frame) | evidence that performance is flat or better, not a belief: `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape vs. a `main` control run, or a bench over a fixture — measured before the PR, numbers in its body |
   | Touches anything user-visible | follow `ui-harness`: every new/changed surface reachable by env hook (hook added in the same change); `visual-qa` pass with all surfaces PASS or defects explicitly accepted; `trader-ux-review` with no unresolved Blocker |
   | Adds a capability (feed, bar type, indicator, layer, panel, crate) | follow `new-extension`: port named, registration-only edits, defaults preserve today's behaviour, fake second implementation tested, blast radius (added vs. edited files) stated in the PR body |
   | Engine / determinism territory | test-first: fixture + expected output written before the code; golden test guards determinism |
   | Docs/skills only | four checks still run (they are cheap when nothing compiled changed); arch-review waived |

   Present the merged checklist to the user before starting work.

3. **Persist it**: write the mission and its criteria to `.claude/GOAL.md` so
   it survives context compaction. Overwrite any previous one. The file keeps
   its name: fourteen archives already use it, and renaming the record would
   buy nothing.

4. **Set up the ground**: fresh worktree from updated `main` under
   `../quantick-worktrees/` per CLAUDE.md — never work in the main checkout,
   and check the worktree for a live writer before the first write. The
   `worktree-guard` hook denies the write if this step is skipped.

5. **Stay on track**: refuse scope creep. A necessary detour is stated
   explicitly and tied back to the mission (or taken to the user). Keep the
   checklist in the todo list so progress is visible. Narrowing the user's
   stated scope is not a judgement call to make alone — ask.

6. **Verify before finishing**: check off each criterion with evidence —
   command output, test result, screenshot path, review verdict. A criterion
   without evidence is unmet. Report mission, criteria and pass/fail for each.

   Before `gh pr create`, record the arch-review the `pr-gate` hook checks:

   ```sh
   git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/arch-review-ok"
   ```

   Only then archive `.claude/GOAL.md` (move to
   `.claude/GOAL-archive-<slug>.md`).

7. **Hand over the `/goal` condition.** Right after step 2, print the built-in
   command for the user to paste, so the session keeps working across turns
   without them prompting each step:

   ```text
   /goal <the criteria from step 2, as one measurable end state, plus "or stop after N turns">
   ```

   Rules the built-in imposes on that line:

   - **4,000 characters maximum.** If the criteria from step 2 do not fit,
     compress rather than drop: state each criterion as a terse observable
     outcome ("clippy/fmt/build/test exit 0", "PR URL printed", "GOAL.md
     archived"), strip all rationale and repo context, and collapse per-surface
     detail into one line ("all visual-qa surfaces PASS"). Only if it still
     overflows, keep the gates (checks, arch-review, PR, archive) and summarize
     the mission-specific criteria into the fewest observable outcomes that
     still prove them. Count the characters before printing the line.
   - The evaluator **does not run commands or read files**. It only judges what
     has appeared in the conversation, so every criterion must be something
     this session's own output demonstrates — "`cargo test --workspace` exits
     0 and the PR URL is printed", not "the code is correct".
   - Include a bound (`or stop after 20 turns`) so a stuck mission ends.
   - It does not change permissions. Pair with auto mode for unattended runs.

## What done means

Done = the PR is open with green CI and the evidence in its body. Not
merged — merging is the user's call, always. Do not ask permission to push or
open the PR; opening it *is* the mission's final step.
