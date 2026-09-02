---
name: delivery-review
description: Grade a finished branch against what was actually asked for — every ask in the mission's request ledger and every acceptance criterion marked DELIVERED, PARTIAL, MISSING or UNPROVEN by a reviewer that did not write the code. Runs after arch-review and before the PR; records the delivery-review-ok marker the pr-gate hook requires. Use when the user types /delivery-review, before opening a PR, or when asked whether what shipped is what was requested.
---

# Delivery review

One question, asked by someone who did not build the thing: **is what shipped
what was asked for?**

`arch-review` grades shape and its step 0 grades bugs; both take the change as
given and ask whether it is *well made*. Neither opens the request and checks
that all of it arrived. The failure this skill catches is the quiet one: eight
asks in, six criteria written, five delivered, everything green, and the trader
finds the other three by using the product.

The reasoning behind the model split, the reviewer's type and the round budget
is `references/why.md`. Read it when changing a rule, not when following one.

## What this skill is not

- **Not a code review.** Bugs belong to `arch-review`'s step 0. A correct
  implementation of the wrong thing still fails here; an incorrect
  implementation of the right thing passes here and fails there.
- **Not an architecture review.** Send modularity, naming and performance
  findings to `arch-review` and grade only conformance.
- **Not a judge of the request.** Grade the branch against the request, never
  the request against good sense.

## The one branch this does not grade

A mission that declared the `small` tier at its outset is exempt, and its PR
opens on `arch-review-ok` alone.

**That is the mission's decision, taken before the work, and never this
skill's.** Invoked at all, this skill grades the branch and records its marker
exactly as at any other tier. If you are here because a gate refused a branch,
the branch owes this review — run it.

## Two modes, and the tier picks one

**Full** (`high`, `max`, and any direct invocation): everything below — the
dossier, the fresh subagent, all three passes, every `A` and `G` graded.

**Completeness-only** (`medium`): step 1, then the **completeness pass alone**,
run inline in the calling session. No dossier, no subagent, no criteria pass.
Read the verbatim request at the foot of the goal file, derive the atomic asks
from it, and compare against the ledger. PASS when nothing is `UNLEDGERED`;
record the marker on PASS, and **say in the verdict which mode ran** — a marker
from a completeness pass must never read as one from a full review.

It is the half worth keeping when only one is affordable: the completeness pass
is the only check in the pipeline that can see an ask which never became a
criterion, it costs reading two blocks of text, and it survives being run
inline. "Is `A7` delivered?" is a judgement about work you just did, which is
why that one keeps the stranger.

### Which model each pass runs on

- **The completeness pass keeps the strong model and is never escalated.** Its
  failure mode is a false *negative* — an ask nobody noticed — which produces
  no line for an escalation to pick up. It is also the cheap pass.
- **The criteria pass starts on `sonnet` and escalates per line.**

1. **Dispatch the criteria reviewer with `model: "sonnet"`.** Name it in the
   `Agent` call; omitting the field inherits the caller's model.
2. **Re-grade on the strong model every line it returned as other than
   `DELIVERED`** — a second dispatch, same dossier, carrying only those lines
   and their evidence tails. The strong pass is paid per disputed line rather
   than per branch, and a clean branch never pays it.
3. **On a re-graded line, the strong pass is the verdict.** It can overturn a
   `PARTIAL` and it can confirm one; record both readings whenever they differ.
   What it may never do is re-open a line the first pass graded `DELIVERED`: an
   escalation returning *more* lines than it was given has exceeded its scope
   and its verdict is discarded.

**The reviewer keeps the full diff.** Handing it `branch.stat` and letting it
read the files as they stand opens a false pass with no floor under it — a
reviewer grading from current files can quote a sentence that was already on
`origin/main` and mark the criterion `DELIVERED`. The diff is the only input
that distinguishes what this branch did from what it inherited.

## Step 1 — Find the checklist

In order of preference. Say which source was used; the answer changes how much
the verdict is worth.

1. **`.claude/GOAL.md`** on the branch (or `.claude/GOAL-archive-<slug>.md` if
   the mission already archived it), in `mission`'s documented format: a ledger
   `R1`…`Rn`, decisions `D1`…`Dn`, assumptions `S1`…`Sn`, criteria `A1`…`An`
   and `G1`…`Gn`. The strong source.
2. **The linked issue**, for a branch started from `/new-task`. `gh issue view
   <N>`. Its `## Acceptance criteria` are the criteria and its `## Context` /
   `## Scope` are the request. There is no `R` ledger — say so, and derive the
   asks from the issue body yourself.

**There is no third source.** Commit messages and the PR description are the
author's account of what they did, which is the one thing this skill exists not
to take on trust.

Return **NOT GRADEABLE**, record nothing, and stop when: neither source exists;
the `GOAL.md` carries no criteria in the documented format; or it carries no
verbatim request section, so the completeness pass cannot run.

All three are the same failure: **an absent input makes every check over it
vacuously true.** With no request, nothing can be `UNLEDGERED`; with no ledger,
every `R` is trivially `COVERED`. A verdict assembled from empty sets satisfies
every PASS clause and records the marker, which is the hole this skill closes.
Missing input is never a quiet pass — it is a refusal to grade, and it is loud.

## Step 2 — Assemble the dossier, then dispatch

The reviewer reads artifacts, never the story. Assemble the inputs first, as
files, because anything that cannot be written to a file is exactly the kind of
claim this skill exists to disbelieve.

```sh
# Point this at the session's scratchpad, never at the repo. Substitute a real
# path — an unquoted placeholder in angle brackets is not a placeholder to
# `sh`, it is two redirections that would truncate a file at the root.
DOSSIER="/path/to/scratchpad/delivery-review"
WT=/path/to/worktree            # the branch under review, not the session cwd
mkdir -p "$DOSSIER" &&
  cd "$WT" &&
  git fetch origin &&
  git diff origin/main...HEAD         > "$DOSSIER/branch.diff" &&
  git diff origin/main...HEAD --stat  > "$DOSSIER/branch.stat" &&
  git log origin/main..HEAD --oneline > "$DOSSIER/branch.log"
```

Both the `cd` and the fetch are load-bearing and both fail the same quiet way.
An agent session's cwd is the main checkout, so without the `cd` the diff
compares `main` to `origin/main` and comes back empty; without the fetch, a
stale local `main` makes the diff carry other branches' merged work. Neither
announces itself — both read as a suspiciously generous pass. **Check the stat
before dispatching**; if it does not look like the branch you are reviewing,
stop.

**Inputs the reviewer may receive:** the checklist from step 1, verbatim; the
diff, its stat and its log; every path named in a criterion's `→` evidence
tail; the repository itself, read-only.

A criterion whose evidence tail says "the PR body" has nowhere to point yet —
this skill runs before the PR exists. Write that evidence into the dossier as a
file first and let the PR body be authored from it: evidence written down and
then published is a record, evidence recalled while writing the PR is a story.

**Inputs it may not receive — this list is the skill:** the implementing
session's transcript, summary, plan or narrative; your explanation of why a
criterion is met; any "I ran X and it passed" not backed by output written to a
file, or that the reviewer cannot re-run itself.

Dispatch with the `Agent` tool.

- **Never `fork`.** A fork inherits this session's context, which is exactly
  the contamination this skill exists to remove.
- **Pass `model: "sonnet"`** on the first dispatch, and the strong model on the
  escalation for disputed lines only. Omitting the field is not neutral: it
  inherits the caller's model, so the largest subagent in the pipeline
  silently bills every grade at open-judgement rates.
- **Pick `general-purpose`**, with the read-only instruction in the prompt. The
  search-shaped types read *excerpts*, and the anti-rubber-stamp rules require
  citing `file:line`, reading a named test's assertions and quoting prose
  verbatim.

The failure this guards against is the sharpest in the mechanism: a reviewer
that finds a criterion `MISSING`, writes the missing line, and grades it
`DELIVERED` returns a PASS whose evidence it manufactured. Nothing downstream
sees it. So do not rest on the prose — **check that the branch did not move**:

```sh
cd "$WT" && git rev-parse HEAD && git status --porcelain
```

Once before the **first** dispatch, once after the **last** verdict returns —
the bracket goes around both dispatches. Any difference invalidates the
verdict: discard it, record no marker, and say what changed.

The markers live in the worktree's **git dir**, outside both `HEAD` and the
working tree, so read the marker before dispatch alongside those two. What
condemns a verdict is the marker *appearing or changing* while the review ran,
not its merely being there — a re-run after a deferral edit finds the previous
round's marker still on disk.

### The escalation dispatch

- **When**: after the criteria pass returns, and only if it graded at least one
  line other than `DELIVERED`.
- **What it gets**: the same dossier paths, the same read-only instruction, and
  **only the disputed lines** — each criterion verbatim, with its evidence tail
  and the first pass's grade and reasoning. Never the whole checklist.
- **What it returns**: the same grade table shape, restricted to those lines.
- **How they merge**: undisputed lines keep the first pass's `DELIVERED`;
  disputed lines take the escalation's grade. A line the escalation did not
  answer keeps the first pass's grade — silence never promotes.

## Step 3 — Grade every line

Three passes, in this order because each catches what the next structurally
cannot: the completeness pass catches asks that never reached the ledger, the
ledger pass catches asks that never became criteria, the criteria pass catches
criteria that never became code. Grade the criteria pass as one table — the
merge happens before you read it.

**Completeness pass** — the ledger against the request that produced it. Read
the verbatim request yourself and derive the atomic asks from it — the same way
`mission` was supposed to — *before* looking at the ledger. Then compare.

An ask you found in the request that no `R` line carries is **UNLEDGERED**, the
most serious grade here: every other finding is about work that fell short of a
written promise, this one is about a promise that was never written, and it is
the only failure the rest of the pipeline is blind to by construction. Report
it with the trader's own words beside it. For a source-2 branch the issue body
is the request.

**Ledger pass** — one grade per `R`:

| Grade | Meaning |
| --- | --- |
| `COVERED` | at least one criterion discharges it, and that criterion graded DELIVERED |
| `PARTLY COVERED` | a criterion claims it but delivers less than the ask says — name the part that did not arrive |
| `DROPPED` | no criterion discharges it, or every criterion that does failed |

**Criteria pass** — one grade per `A` and per `G`, and no others. A `C` line
under **Closing steps** is deliberately not a criterion: none of them can have
happened while you are writing this verdict, and two are unblocked *by* it. A
checklist that puts them among the `A` or `G` lines gets a finding against the
checklist, and the rest is graded.

| Grade | Meaning |
| --- | --- |
| `DELIVERED` | the outcome is observable in the shipped branch **and** the named evidence exists at its path |
| `PARTIAL` | part of the outcome landed; name exactly which part did not |
| `MISSING` | not in the branch at all |
| `UNPROVEN` | plausibly delivered, but the evidence is absent, unreadable, or is a claim rather than an artifact |

`UNPROVEN` is the grade that does the work. It is not a softer `DELIVERED`; it
is the honest answer when the outcome may well be there and nothing on disk
says so. Treat it as a failure, and fix it by recording the evidence.

## Anti-rubber-stamp rules

A reviewer that agrees with everything has reviewed nothing. These are binding
on the subagent and go into its prompt.

1. **"The code looks right" is not evidence.** Cite the `file:line` that
   implements the outcome, or grade it `MISSING`.
2. **A criterion naming a test** is graded by reading that test's assertions. A
   test that exists but asserts nothing about the criterion is `UNPROVEN`.
3. **A criterion naming a command** needs that command's output recorded — and
   the reviewer may simply re-run it, which is faster than arguing.
4. **A prose criterion** is graded by quoting the lines that say it. A
   paraphrase is not a quote.
5. **Grade the branch, not the plan.** Something living only in a `TODO`, a doc
   sentence describing future work, or a function nothing calls, is `MISSING`.
6. **A commit message is not proof.** Neither is a PR body. Read the diff.
7. **Audit the assumptions.** An `S` that turned out to drive the design is
   reported as a question `mission`'s interrogation should have asked.
8. **Audit the exclusions.** A gate listed as not applicable that in fact
   applies to the shipped diff is a finding at the weight of a failed criterion.
9. **If every line grades DELIVERED on the first round**, say what you checked
   that could have failed and did not. An all-green verdict with no reasoning
   is the shape a rubber stamp takes.

## Step 4 — Verdict

**PASS** only when all of: the checklist source was 1 or 2 and the completeness
pass actually ran; nothing `UNLEDGERED`; every `R` `COVERED`; every `A` and `G`
`DELIVERED`; nothing `UNPROVEN`, `MISSING` or `PARTIAL`.

Those grades are the **merged** ones — a re-graded line counts at the
escalation's reading. Where the two differed, say so beside the line.

**An approved deferral is exempt from every one of those clauses, the ledger
included.** A deferred `A` does not grade `DELIVERED`, so every `R` it
discharges would grade `DROPPED`, and PASS would be unreachable for the one
route by which a gap is *allowed* to ship. Read a deferred line as satisfied,
and say in the verdict that it was deferred and by whom.

The first clause is not a formality: every clause after it quantifies over a
set, and an empty set satisfies all of them. Check that the inputs existed
before checking what they say.

**FAIL** otherwise, with the failing lines first, each naming the smallest
concrete thing that would change the grade. Close with the checklist source
used, whether the completeness pass could run, the counts, and the answer to
rule 9.

## Step 5 — The bounded fix loop

This loop belongs to the session that called the skill. The reviewer grades and
returns; it never edits the branch it is judging. The trader does not close
these gaps either — the session does.

- **Fix everything the review reported, then re-run** — a fresh dossier and a
  fresh subagent, because a reviewer that has already seen the branch is no
  longer a stranger to it.
- **Spend from the chain's budget, not a second one.** `CLAUDE.md`'s *review
  chain has a budget* is the owner: three rounds per branch across **both**
  reviews together, then the remainder ships as recorded PR follow-ups. Count
  the rounds this branch has already spent before opening another.

  When the budget is out, report the surviving gaps and what was tried on each,
  and let the trader decide whether to keep going. **Say which shape it is**: a
  round whose findings are smaller and fewer is converging; a round still
  returning Blockers in code the previous round's fix introduced means the
  approach is wrong rather than incomplete, and more rounds will not fix a
  design.
- **Escalate immediately, without spending a round**, when closing the gap
  would change the mission's scope, contradict a recorded `D` decision, or
  require a call that belongs to the trader.

**Deferral** is the only way a gap ships, and only the trader grants it. A
granted deferral is written into the goal file under a `## Deferred` heading —
the line's ID, what is missing, why, and that the trader approved it — and
repeated in the PR body. By the time a deferral exists the archive is the file
to edit, and editing it is a commit, which stales both markers by design: both
reviews run again over the new head before either is re-recorded.

**`## Deferred` means granted.** A gap still waiting on an answer goes under
`## Deferral requested — NOT granted`, which reads correctly at a glance; a
subtitle correcting the heading is not enough, because the heading is what gets
skimmed. A deferral the session grants itself is not a deferral; it is the
failure this skill was built to stop.

## Step 6 — Record the marker

On **PASS** only:

```sh
WT=/path/to/worktree
cd "$WT" &&
  git diff origin/main...HEAD |
    git hash-object --stdin > "$(git rev-parse --absolute-git-dir)/delivery-review-ok"
```

`pr-gate` denies `gh pr create` until this file holds the hash of the exact
change being shipped, alongside `arch-review-ok`. Recording it on a FAIL, or
before the last edit, is lying to the gate — the second is caught
automatically, the first is caught by nothing but you.

Run this skill **after** `arch-review`, never before: it grades the branch as
shipped, including whatever the shape review made you change.
