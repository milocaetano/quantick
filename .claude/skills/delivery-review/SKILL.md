---
name: delivery-review
description: Grade a finished branch against what was actually asked for — every ask in the mission's request ledger and every acceptance criterion marked DELIVERED, PARTIAL, MISSING or UNPROVEN by a reviewer that did not write the code. Runs after arch-review and before the PR; records the delivery-review-ok marker the pr-gate hook requires. Use when the user types /delivery-review, before opening a PR, or when asked whether what shipped is what was requested.
---

# Delivery review

One question, asked by someone who did not build the thing: **is what shipped
what was asked for?**

That question has never had an owner in this repo. `arch-review` grades shape —
does it dock, is it fast, is it tested, is it English. Its step 0 grades bugs.
Both take the change as given and ask whether it is *well made*. Neither ever
opens the request and checks that all of it arrived. So the failure this skill
exists to catch is the quiet one: eight asks in, six criteria written, five
delivered, everything green, and the trader finds the other three by using the
product.

## What this skill is not

- **Not a code review.** Bugs belong to `arch-review`'s step 0. A correct
  implementation of the wrong thing still fails here; an incorrect
  implementation of the right thing passes here and fails there. Both gates
  exist because they catch different things.
- **Not an architecture review.** Do not report modularity, naming or
  performance findings. Send them to `arch-review` and grade only conformance.
- **Not a judge of the request.** "Was this a good idea?" is the trader's
  question, not this skill's. Grade the branch against the request, never the
  request against good sense.

## Step 1 — Find the checklist

In order of preference. Say which source was used; the answer changes how much
the verdict is worth.

1. **`.claude/GOAL.md`** on the branch (or `.claude/GOAL-archive-<slug>.md` if
   the mission already archived it). Written by `mission`, in its documented
   format: a request ledger `R1`…`Rn`, decisions `D1`…`Dn`, assumptions
   `S1`…`Sn`, criteria `A1`…`An` and `G1`…`Gn`. This is the strong source.
2. **The linked issue**, for a branch started from `/new-task` rather than
   `/mission`. `gh issue view <N>`. Its `## Acceptance criteria` are the
   criteria, and its `## Context` / `## Scope` are the request the completeness
   pass reads. There is no `R` ledger, so say so, and derive the asks from the
   issue body yourself.

**There is no third source.** A branch's own commit messages and PR
description are not a statement of what was asked for — they are the author's
account of what they did, which is the one thing this skill exists not to take
on trust. If neither source above exists, return **NOT GRADEABLE**, say that
nothing independent of the branch states what it was supposed to do, and stop.
The session's move then is to get that statement — from the trader, or from
the issue — not to grade the branch against its own homework.

Return **NOT GRADEABLE** and record nothing in these cases too:

- the source is `GOAL.md` but it carries no criteria in the documented format;
- the source is `GOAL.md` and it carries no verbatim request section, so the
  completeness pass cannot run at all.

Both are the same failure wearing different clothes: **an absent input makes
every check over it vacuously true.** With no request to compare against,
nothing can be `UNLEDGERED`; with no ledger, every `R` is trivially `COVERED`.
A verdict assembled from empty sets satisfies every PASS clause below and
records the marker, which is precisely the "gate grades its own summary" hole
this skill was built to close. Missing input is never a quiet pass — it is a
refusal to grade, and it is loud.

## Step 2 — Assemble the dossier, then dispatch

The reviewer reads artifacts, never the story. Assemble the inputs first, as
files, because anything that cannot be written to a file is exactly the kind of
claim this skill exists to disbelieve.

```sh
# Point this at the session's scratchpad directory, never at the repo: the
# dossier is working material, and a diff of the branch committed into the
# branch is a mess the next review has to read past. Substitute a real path —
# an unquoted placeholder in angle brackets is not a placeholder to `sh`, it is
# two redirections, and the line would truncate a file at the filesystem root.
DOSSIER="/path/to/scratchpad/delivery-review"
mkdir -p "$DOSSIER"
git fetch origin
git diff origin/main...HEAD         > "$DOSSIER/branch.diff"
git diff origin/main...HEAD --stat  > "$DOSSIER/branch.stat"
git log origin/main..HEAD --oneline > "$DOSSIER/branch.log"
```

Fetch first. A diff against a stale local `main` grades another branch's merged
work as though this one had done it — the same trap `arch-review`'s *Scope the
review* names, and it reads as a suspiciously generous pass rather than as an
error.

**Inputs the reviewer may receive:**

- the checklist from step 1, verbatim;
- the diff, its stat and its log;
- every path named in a criterion's `→` evidence tail;
- the repository itself, read-only, to see the shipped state.

A criterion whose evidence tail says "the PR body" has nowhere to point yet —
this skill runs before the PR exists. Write that evidence into the dossier as
a file first and let the PR body be authored from it. The order matters:
evidence written down and then published is a record, evidence recalled while
writing the PR is a story.

**Inputs it may not receive — this list is the skill:**

- the implementing session's transcript, summary, plan or narrative;
- your explanation of why a criterion is met;
- any "I ran X and it passed" that is not backed by output written to a file,
  or that the reviewer cannot re-run itself.

Dispatch with the `Agent` tool. Two constraints on the agent type, and both are
structural — neither is satisfied by telling the reviewer to behave:

- **Never `fork`.** A fork inherits this session's context, which is exactly
  the contamination this skill exists to remove.
- **It must not be able to write.** Pick an agent type whose tools exclude
  `Edit`, `Write` and `NotebookEdit` — `Explore` is the read-only type
  available today. A reviewer with write access that notices a `MISSING`
  criterion can write the missing line and then grade it `DELIVERED`, and the
  result is a PASS whose evidence the reviewer manufactured. That is the one
  failure in this whole mechanism with no downstream detector: `arch-review`
  has already run, `pr-gate` only compares a sha, and the calling session never
  sees the subagent's transcript *by design*. "The reviewer must not edit the
  branch" written in prose is a rule enforced by the party it constrains, which
  is no enforcement at all. Take the capability away instead.

Hand it the checklist and the dossier paths in the prompt, tell it to read the
repo itself for anything else, and ask for the grade table and verdict below.

## Step 3 — Grade every line

Three passes, and they run in this order because each one catches what the
next one structurally cannot. The completeness pass catches asks that never
reached the ledger; the ledger pass catches asks that never became criteria;
the criteria pass catches criteria that never became code.

**Completeness pass** — the ledger against the request that produced it.

`mission` writes the trader's request into `GOAL.md` verbatim, as its last
section, precisely so this pass is possible. Read that request yourself and
derive the atomic asks from it — the same way `mission` was supposed to —
before looking at the ledger. Then compare.

An ask you found in the request that no `R` line carries is **UNLEDGERED**,
and it is the most serious grade in this skill. Every other finding is about
work that fell short of a written promise; this one is about a promise that was
never written, and it is the only failure the rest of the pipeline is blind to
by construction. Report it with the trader's own words beside it.

For a source-2 branch the issue body is the request: derive the asks from it
the same way. Step 1 already refuses to grade a `GOAL.md` with no verbatim
request section, so this pass always has something to read — an absent request
is a refusal to grade, never a pass with one check skipped.

**Ledger pass** — one grade per `R`:

| Grade | Meaning |
| --- | --- |
| `COVERED` | at least one criterion discharges it, and that criterion graded DELIVERED |
| `PARTLY COVERED` | a criterion claims it but delivers less than the ask says — name the part that did not arrive |
| `DROPPED` | no criterion discharges it, or every criterion that does failed |

**Criteria pass** — one grade per `A` and per `G`.

Grade those two groups and no others. A `C` line under **Closing steps** —
this review returning PASS, `GOAL.md` archived, the PR open — is deliberately
not a criterion: none of them can have happened at the moment you are writing
this verdict, and two of them are unblocked *by* it. Grading them would fail
every branch that ever ran this skill, which is a gate nobody would keep. If a
checklist puts them among the `A` or `G` lines anyway, say so as a finding
against the checklist and grade the rest.

| Grade | Meaning |
| --- | --- |
| `DELIVERED` | the outcome is observable in the shipped branch **and** the named evidence exists at its path |
| `PARTIAL` | part of the outcome landed; name exactly which part did not |
| `MISSING` | not in the branch at all |
| `UNPROVEN` | plausibly delivered, but the evidence is absent, unreadable, or is a claim rather than an artifact |

`UNPROVEN` is the grade that does the work. It is not a softer `DELIVERED`; it
is the honest answer when the outcome may well be there and nothing on disk
says so. Treat it as a failure, and fix it by recording the evidence — which
costs one command.

## Anti-rubber-stamp rules

A reviewer that agrees with everything has reviewed nothing. These are binding
on the subagent and go into its prompt.

1. **"The code looks right" is not evidence.** Cite the `file:line` that
   implements the outcome, or grade it `MISSING`.
2. **A criterion naming a test** is graded by reading that test's assertions.
   A test that exists but asserts nothing about the criterion is `UNPROVEN`.
3. **A criterion naming a command** needs that command's output recorded. "It
   passed" without output is `UNPROVEN` — and the reviewer may simply re-run
   the command, which is faster than arguing.
4. **A prose criterion** ("the skill must say X") is graded by quoting the
   lines that say it. A paraphrase is not a quote.
5. **Grade the branch, not the plan.** Something promised in `GOAL.md` and
   living only in a `TODO` comment, a doc sentence describing future work, or
   a function nothing calls, is `MISSING`.
6. **A commit message is not proof.** Neither is a PR body. Both are the
   author's claim about the diff; read the diff.
7. **Audit the assumptions.** For each `S`, ask whether it turned out to drive
   the design. One that did is reported as a question that should have been
   asked in `mission`'s interrogation round — not a criterion failure, but a
   finding the trader reads.
8. **Audit the exclusions.** A gate the mission listed as not applicable, that
   in fact applies to the shipped diff, is a finding at the same weight as a
   failed criterion.
9. **If every line grades DELIVERED on the first round**, say what you checked
   that could have failed and did not. An all-green verdict with no reasoning
   is the shape a rubber stamp takes, and it is rejected here.

## Step 4 — Verdict

**PASS** only when all of: the checklist source was 1 or 2 and the completeness
pass actually ran; nothing `UNLEDGERED`; every `R` `COVERED`; every `A` and `G`
`DELIVERED`; nothing `UNPROVEN`, `MISSING` or `PARTIAL` — except a line
carrying an approved deferral (below).

That first clause is not a formality. Every clause after it quantifies over a
set, and an empty set satisfies all of them: no request means nothing can be
`UNLEDGERED`, no ledger means every `R` is `COVERED` by vacuity. A PASS
assembled that way is indistinguishable from a real one at the marker, and the
marker is all `pr-gate` can see. So check that the inputs existed before
checking what they say.

**FAIL** otherwise, with the failing lines listed first, each naming the
smallest concrete thing that would change the grade.

Close with the checklist source used, whether the completeness pass could run,
the counts, and the answer to rule 9.

## Step 5 — The bounded fix loop

This loop belongs to the session that called the skill, not to the subagent —
the reviewer grades and returns; it never edits the branch it is judging. The
trader is not the one who closes these gaps either. The session is.

- **Fix everything the review reported, then re-run** — a fresh dossier and a
  fresh subagent, because a reviewer that has already seen the branch is no
  longer a stranger to it.
- **At most three rounds.** After the third, stop: report the surviving gaps,
  what was tried on each, and hand it to the trader. Three is a bound in this
  file so a stuck loop ends, and one edit to change if it proves wrong.
- **Escalate immediately, without spending a round**, when closing the gap
  would change the mission's scope, contradict a recorded `D` decision, or
  require a call that belongs to the trader. Those are step-3 questions in
  `mission`, arriving late.

**Deferral** is the only way a gap ships, and only the trader grants it. A
granted deferral is written into `GOAL.md` under a `## Deferred` heading — the
line's ID, what is missing, why, and that the trader approved it — and repeated
in the PR body. A deferral the session grants itself is not a deferral; it is
the failure this skill was built to stop.

## Step 6 — Record the marker

On **PASS** only:

```sh
cd <worktree> && git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/delivery-review-ok"
```

`pr-gate` denies `gh pr create` until this file holds the exact HEAD being
shipped, alongside `arch-review-ok`. Recording it on a FAIL, or before the last
commit, is lying to the gate — and since the marker stores a sha, the second
one is caught automatically and the first one is not caught by anything but
you.

Run this skill **after** `arch-review`, never before: it grades the branch as
shipped, including whatever the shape review made you change.
