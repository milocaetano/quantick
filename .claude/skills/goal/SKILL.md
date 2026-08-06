---
name: goal
description: Define and enforce a goal for the current session — capture the objective, classify it, derive acceptance criteria including the standard quantick gates (arch-review, visual-qa, trader-ux-review, ui-harness hooks) that match the kind of work, keep every action aligned, and verify everything before finishing. Use when the user types /goal <objective> or asks to set a goal for the session/task.
---

# Goal

Argument: the session objective (e.g. `/goal make the heatmap render at
60fps`). If missing, ask the user what the goal is before doing anything
else.

The goal is the orchestrator: it decides which of the other skills are part
of *done* so the user never has to list them. One session, one goal, one
branch, one worktree, one PR.

## Steps

1. **Capture the goal**: restate the objective in one sentence and confirm
   it with the user only if it is genuinely ambiguous.

2. **Classify it and inject the standard gates.** Derive 3–7 verifiable
   criteria specific to the objective, then add the gates for its kind:

   | The goal… | Injected acceptance criteria |
   | --- | --- |
   | Any code change | four checks green after rebasing on latest `main`; **performance impact declared** — classify every touched path by rate (per-trade / per-depth / per-frame / rare, the `arch-review` table) as part of the plan, not the review; `arch-review` run with every Blocker/Should-fix resolved or deferred in the PR body; **PR opened** — the goal is not done before the PR exists, and merging is never part of the goal |
   | Touches a hot path (per-trade, per-depth, per-frame) | evidence that performance is flat or better, not a belief: `APP_HEALTH_SUMMARY` fps/frame_avg under a dense tape vs. a `main` control run, or a bench over a fixture — measured before the PR, numbers in its body |
   | Touches anything user-visible | follow `ui-harness`: every new/changed surface reachable by env hook (hook added in the same change); `visual-qa` pass with all surfaces PASS or defects explicitly accepted; `trader-ux-review` with no unresolved Blocker |
   | Adds a capability (feed, bar type, indicator, layer, panel, crate) | follow `new-extension`: port named, registration-only edits, defaults preserve today's behaviour, fake second implementation tested, blast radius (added vs. edited files) stated in the PR body |
   | Engine / determinism territory | test-first: fixture + expected output written before the code; golden test guards determinism |
   | Docs/skills only | four checks still run (they are cheap when nothing compiled changed); arch-review waived |

   Present the merged checklist to the user before starting work.

3. **Persist it**: write the goal and its criteria to `.claude/GOAL.md` so
   the goal survives context compaction. Overwrite any previous goal.

4. **Set up the ground**: fresh worktree from updated `main` under
   `../quantick-worktrees/` per CLAUDE.md — never work in the main
   checkout, and check the worktree for a live writer before the first
   write.

5. **Stay on track**: refuse scope creep. A necessary detour is stated
   explicitly and tied back to the goal (or taken to the user). Keep the
   checklist in the todo list so progress is visible.

6. **Verify before finishing**: check off each criterion with evidence —
   command output, test result, screenshot path, review verdict. A
   criterion without evidence is unmet. Report goal, criteria and pass/fail
   for each. Only then archive `.claude/GOAL.md` (move to
   `.claude/GOAL-archive-<slug>.md`).

## What done means

Done = the PR is open with green CI and the evidence in its body. Not
merged — merging is the user's call, always. Do not ask permission to push
or open the PR; opening it *is* the goal's final step.
