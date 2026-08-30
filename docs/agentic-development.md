# Building quantick with agents

Most of this repository is written by AI agents working under a human
maintainer. That is a claim worth being precise about, because "built with AI"
usually means an autocomplete was on. Here it means something narrower and
checkable: the workflow that turns an objective into a merged pull request is
itself committed to the repository, as skills the agent loads, gates it has to
pass, and hooks that refuse the work when it skips one.

This document describes that workflow. It is not a rule for contributors —
[`CONTRIBUTING.md`](../CONTRIBUTING.md) is — and it changes nothing about what
the code has to prove. It exists because the process is one of the more
interesting parts of the project, and it was previously legible only by
reading `.claude/`.

## The problem it solves

An instruction written in markdown is advice. A long agent session drifts away
from advice — not maliciously, just by attention decay: it is forty tool calls
deep, the objective has narrowed to the bug in front of it, and the rule about
worktrees was in a file it read an hour ago.

So the rules that matter most are not written as advice. They are written as
walls the harness enforces, and everything else is written as a *skill*: a
loadable procedure with its own acceptance criteria, invoked by name.

## The skills

Each is a directory under `.claude/skills/` with a `SKILL.md`. The agent loads
one when the task matches its description, or when the maintainer types its
name as a slash command.

| Skill | What it owns |
| --- | --- |
| `mission` | The orchestrator. Captures the session objective in English, classifies it, and derives the acceptance criteria — including which of the gates below are part of *done* for this kind of work, so the maintainer never has to list them. One session, one mission. |
| `new-task` | Starts work from a GitHub issue: reads it, branches from updated `main` with the right prefix, moves the board card. |
| `new-extension` | The build-time twin of the review question below. `arch-review` asks after the fact whether a feature could have been a new file plus one registration line; this skill designs it that way from the start. |
| `arch-review` | The pre-PR review. Step 0 runs a correctness pass; then it grades *shape* — does the change dock like a module, does it declare its performance impact, do its tests stay out of the shipped binary, is it drivable without a mouse, does it hide anything behind a magic number, is it English throughout. |
| `visual-qa` | Autonomous visual QA. Drives every affected surface through the harness hooks, **asks the live control plane what the application believes is on screen**, captures a state matrix, and reads the images against a defect checklist. |
| `trader-ux-review` | The same screenshots, judged by trader personas against order-flow heuristics: does this cost attention, clicks or trust at a moment the market is moving? |
| `ui-harness` | The contract that makes the two above possible: every user-visible surface must be reachable from a fresh launch by environment hooks alone, zero clicks. A new panel that cannot be opened without a mouse is an incomplete panel. |
| `issue` | Turns an idea into a well-formed issue with scope, acceptance criteria, labels and board placement — or redirects it to Discussions when there is no concrete deliverable yet. |
| `ship` | The delivery: the four-check loop, the commit, the push, the PR with `Closes #N`, and CI watched until green. |

`ui-harness` deserves the emphasis. It is the same rule as the product's
fourth design principle — *operable without a hand* — applied to the
development loop. The control plane exists so an agent can operate the
application; the harness hooks exist so an agent can *test* it. A capability
reachable only by mouse fails both.

## The gates

Three things stand between a change and `main`, and none of them is the
agent's own judgement that it is finished.

**The four-check verification loop.** `cargo fmt --all -- --check`, then
clippy with `-D warnings`, then `cargo build --workspace`, then
`cargo test --workspace`. CI runs the same four on every PR and on every push
to `main`, plus two the workspace cannot see: `ruff check --select F` over the
Python under `tools/mt5/` and `bridge/mt5/`, and that folder's own test
script. A PR with red CI is never merged.

**`arch-review` over `git diff main...HEAD`.** Every Blocker and Should-fix
finding is resolved before the PR opens. A finding deliberately deferred is
named in the PR body, so the deferral is a decision on the record rather than
an omission.

**The review gates the work actually earns.** `mission` decides which apply:
a change a trader touches mid-session gets `trader-ux-review`; anything
visual gets `visual-qa`; a docs-only change gets neither, but never skips the
English check or the correctness pass.

## The hooks that make the gates real

Two of those rules were enforceable only by an agent remembering them. They
are now enforced by the harness, in `.claude/hooks/guardrails.sh` — POSIX `sh`
with no `jq`, so it runs under Git Bash on Windows and `dash` in CI, and it is
covered by its own test script that CI runs as a separate step.

| Mode | Effect |
| --- | --- |
| `worktree-guard` | Denies any write that lands in the main checkout while that checkout is on `main`. One goal, one worktree — not by discipline. |
| `pr-gate` | Denies `gh pr create` until an arch-review is recorded for the exact `HEAD` being shipped. |
| `commit-reminder` | Cannot block, since the commit already landed. After a commit on a branch ahead of `origin/main`, it says the gate is coming. |

`pr-gate` is honest in a specific way worth copying: the marker it reads holds
**the commit sha the review covered**, not a timestamp or a boolean. Commit
again after reviewing and the sha no longer matches, so the gate denies and
names both shas. A marker that only recorded "a review happened" would pass
while the newest three commits went unreviewed — which is the failure this
repository actually hit.

It proves a review was *recorded*, not that it was *good*. Nothing outside the
review can prove the latter, and the file says so rather than implying
otherwise.

Both hooks **fail open**. Anything the script cannot determine — no
`file_path` in the payload, a path outside a git repository, a `git`
invocation that errors — exits 0 and the normal permission flow applies. A
guardrail that blocks a session over its own bugs would be worse than no
guardrail, and the rules it protects are written down in
[`CLAUDE.md`](../CLAUDE.md) as well.

## The mission archive

Each completed objective leaves its mission file behind as
`.claude/GOAL-archive-<slug>.md` — 45 of them at the time of writing. They are
not changelogs. A mission file records what the objective *was*, the decisions
taken with the trader and on what date, and the acceptance criteria the work
had to meet, all written before the code existed.

That makes them the design record git history cannot reconstruct: a commit
shows what changed, and the archived mission shows what question the change
was answering, and which alternatives were rejected in a conversation that
would otherwise have evaporated. When a later reader asks why the strategy's
audible alarm deliberately fires before the strategy could ever place an
order, the answer is in the mission file, in the trader's own reasoning.

## What this does not claim

The agents do not merge their own work; a human reviews and merges. The gates
catch classes of failure — drift, unreviewed commits, untested surfaces,
mouse-only capabilities — not all failure. And the process is only as good as
the acceptance criteria a mission writes down, which is a human judgement at
the start of every session.
