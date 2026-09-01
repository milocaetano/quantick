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

## The one branch this does not grade

A mission that declared the `small` tier at its outset — a one-line fix, where
a ledger has nothing to grade — is exempt, and its PR opens on `arch-review-ok`
alone.

**That is the mission's decision, taken before the work, and never this
skill's.** Invoked at all — by `/delivery-review`, by `ship`, by a session that
wants the answer — this skill grades the branch and records its marker exactly
as it would at any other tier. Reaching this section while looking for a way
past a denial is the wrong turn: the tier is declared when a mission starts,
the exemption is bounded by how large the branch turns out to be, and neither
is something to arrange at the point of shipping. If you are here because a
gate refused a branch, the branch owes this review — run it.

## Two modes, and the tier picks one

**Full** (`high`, `max`, and any direct invocation): everything below — the
dossier, the fresh subagent, all three passes, every `A` and `G` graded.

**Completeness-only** (`medium`): step 1, then the **completeness pass alone**,
run inline in the calling session. No dossier, no subagent, no criteria pass.
Read the verbatim request at the foot of the goal file, derive the atomic asks
from it, and compare against the ledger. PASS when nothing is `UNLEDGERED`;
record the marker on PASS, and **say in the verdict which mode ran** — a marker
from a completeness pass must never read as one from a full review.

Why that is the half worth keeping when only one is affordable: the
completeness pass is the only check in the entire pipeline that can see an ask
which never became a criterion, and it costs reading two blocks of text. The
criteria pass is what needs the dossier and the stranger, because grading
"is this outcome really in the branch" against the author's own account is
exactly what a self-grading session gets wrong.

And it is the half that survives being run inline. "Does every ask in the
request appear as a numbered line?" is close to mechanical; a session can
answer it about its own ledger without much room to flatter itself. "Is `A7`
delivered?" is a judgement about work you just did, which is why that one keeps
the stranger. Say plainly that the weaker mode ran; do not imply the stronger.

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
WT=/path/to/worktree            # the branch under review, not the session cwd
mkdir -p "$DOSSIER" &&
  cd "$WT" &&
  git fetch origin &&
  git diff origin/main...HEAD         > "$DOSSIER/branch.diff" &&
  git diff origin/main...HEAD --stat  > "$DOSSIER/branch.stat" &&
  git log origin/main..HEAD --oneline > "$DOSSIER/branch.log"
```

Both the `cd` and the fetch are load-bearing, and both fail the same quiet way.
An agent session's cwd is the main checkout, not the worktree — this repo's own
hook treats that as established fact — so without the `cd` the diff compares
`main` to `origin/main` and comes back empty, and the reviewer then grades every
criterion against a diff containing none of the branch's work. Without the
fetch, a stale local `main` makes the diff carry other branches' merged work as
though this one had done it. Neither mistake announces itself: both read as a
suspiciously generous pass. Check the stat before dispatching — if it does not
look like the branch you are reviewing, stop.

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

Dispatch with the `Agent` tool.

- **Never `fork`.** A fork inherits this session's context, which is exactly
  the contamination this skill exists to remove. This one *is* structural.
- **Pick the type that can read whole files.** `general-purpose`, with the
  read-only instruction written into the prompt. The obvious alternative is a
  search-shaped type whose tools exclude `Edit` and `Write` — but those types
  are specified to read *excerpts* rather than whole files, and the
  anti-rubber-stamp rules below require citing `file:line`, reading a named
  test's assertions and quoting prose verbatim. A reviewer that grades from
  excerpts on the one gate whose entire value is that it did not take the work
  on trust is the wrong trade.

  The capability argument for the narrower type does not survive contact
  either: every type here keeps `Bash`, and `printf … >> file` writes a file as
  surely as `Edit` does. So "the reviewer must not edit the branch" is prose in
  every case, and prose is a rule enforced by the party it constrains. Choose
  for reading depth and enforce the rest with the check below.

The failure that guards against is the sharpest in this whole mechanism: a
reviewer that finds a criterion `MISSING`, writes the missing line, and then
grades it `DELIVERED` returns a PASS whose evidence it manufactured. Nothing
downstream sees it — `arch-review` has already run, `pr-gate` compares a sha
and nothing else, and the calling session never reads the subagent's transcript
*by design*.

So do not rest on the prose. **Check that the branch did not move**, which the
calling session can do and the reviewer cannot forge:

```sh
cd "$WT" && git rev-parse HEAD && git status --porcelain
```

Once before dispatch, once after the verdict returns. Any difference — a new
commit, a dirty file — invalidates the verdict: discard it, record no marker,
and say what changed. A review that edited what it was grading is not a
review.

Know what that check cannot see. The markers live in the worktree's **git
dir**, outside both `HEAD` and the working tree, so a subagent that wrote
`delivery-review-ok` itself would leave both readings unchanged. So the
calling session records the marker — after reading the verdict, on PASS
only — and reads the marker **before** dispatch alongside `HEAD` and the
porcelain status. What condemns a verdict is the marker *appearing or
changing* while the review ran, not its merely being there: a re-run after
a deferral edit finds the previous round's marker still on disk, and a rule
keyed on existence alone would discard every verdict from the second round
onward and leave the branch permanently denied.

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
`DELIVERED`; nothing `UNPROVEN`, `MISSING` or `PARTIAL`.

**An approved deferral is exempt from every one of those clauses, the ledger
included.** A deferred `A` line does not grade `DELIVERED`, so every `R` it
discharges would grade `DROPPED` — and "every `R` `COVERED`" would then make
PASS unreachable for the one route by which a gap is *allowed* to ship. Read a
deferred line as satisfied for the purpose of the verdict, and say in the
verdict that it was deferred and by whom, so the exemption is visible rather
than silent.

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
- **Three rounds, then tell the trader** — report the surviving gaps and what
  was tried on each, and let them decide whether to keep going. The bound
  exists so a stuck loop ends, not so a productive one does, and the
  difference between those two is a judgement the trader should get to make
  with the findings in front of them.

  Say which it looks like. A round whose findings are smaller and fewer than
  the last is converging; a round still returning Blockers, especially in code
  the previous round's fix introduced, is a sign the approach is wrong rather
  than incomplete — and that is worth naming, because more rounds will not fix
  a design. The branch that introduced this skill took **four** rounds of
  `arch-review` with the count flat at 15 and the severity climbing into newly
  written code, which is exactly the shape that should have prompted the
  conversation instead of a fifth round.
- **Escalate immediately, without spending a round**, when closing the gap
  would change the mission's scope, contradict a recorded `D` decision, or
  require a call that belongs to the trader. Those are step-3 questions in
  `mission`, arriving late.

**Deferral** is the only way a gap ships, and only the trader grants it. A
granted deferral is written into the goal file under a `## Deferred` heading —
the line's ID, what is missing, why, and that the trader approved it — and
repeated in the PR body. Note *which* file that is: the mandated order archives
`.claude/GOAL.md` to `.claude/GOAL-archive-<slug>.md` before either review
runs, so by the time a deferral exists the archive is the file to edit. And
editing it is a commit, which stales both markers by design — so both reviews
run again over the new head before either is re-recorded. Re-stamping instead
is the cheap exit this whole mechanism is built to make unattractive.

**`## Deferred` means granted.** A gap still waiting on an answer goes under a
heading that says so — `## Deferral requested — NOT granted` reads correctly at
a glance. Otherwise a later reader, or a later reviewer keying on the heading,
takes an approval nobody gave; a subtitle correcting the heading is not enough,
because the heading is the part that gets skimmed.

A deferral the session grants itself is not a deferral; it is the failure this
skill was built to stop.

## Step 6 — Record the marker

On **PASS** only:

```sh
WT=/path/to/worktree
cd "$WT" && git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/delivery-review-ok"
```

`pr-gate` denies `gh pr create` until this file holds the exact HEAD being
shipped, alongside `arch-review-ok`. Recording it on a FAIL, or before the last
commit, is lying to the gate — and since the marker stores a sha, the second
one is caught automatically and the first one is not caught by anything but
you.

Run this skill **after** `arch-review`, never before: it grades the branch as
shipped, including whatever the shape review made you change.
