---
name: delivery-review
description: Independently grade a branch against retained requests, criteria and evidence before PR readiness. Records delivery-review-ok only after PASS. Use for delivery review or to check whether the requested outcomes shipped.
---

Campaign children override the main-based examples via the
[integration contract](../../../docs/campaign/integration.md), including review keys.

# Delivery review

**Is what shipped what was asked for?** Grade conformance only: bugs are
`arch-review` step 0's, shape is `arch-review`'s, and the request itself is
never graded against good sense. [The delivery
contract](../../../docs/workflow/delivery.md) owns source/map reconciliation,
operational gates, traceability-only repairs, delta follow-ups and retry
limits — read it first. Rationale: `references/why.md`.

A mission that declared `small` at its outset is exempt from this marker only —
the mission's decision, never this skill's. Invoked at all, this skill grades
and records exactly as at any tier; a gate that refused a branch means it owes
this review.

## Modes and models

- **Full** (`high`, `max`, any direct invocation): steps 1–6, a fresh subagent,
  all three passes, every `A` and `G`.
- **Completeness-only** (`medium`): step 1, then the completeness pass alone,
  inline — no dossier, subagent or criteria pass. PASS when nothing is
  `UNLEDGERED`; the verdict **names the mode**.
- The completeness pass keeps the strong model and is never escalated.
- The criteria pass dispatches with `model: "sonnet"` (named — omitting it
  inherits the caller's model). Every line it grades other than `DELIVERED` is
  re-graded in a second dispatch on the strong model, which gets only those
  lines, each verbatim with its evidence tail and the first grade and reasoning.
  The re-grade is the verdict on those lines (record both when they differ); a
  line it did not answer keeps the first grade; an escalation returning more
  lines than it was given is discarded.
- The reviewer always gets the **full diff** — current files alone let a
  sentence already on `origin/main` pass as this branch's work.

## Step 1 — Find the checklist

Say which source was used:

1. The ignored local `.claude/GOAL.md` in the task worktree, or the mission
   goal in the PR body, in `mission`'s format (`R`, `D`, `S`, `A`, `G`).
2. The linked issue (`gh issue view <N>`) for a branch started by `/issue
   start`: `## Acceptance criteria` are the criteria, `## Context` / `## Scope`
   the request; say there is no ledger and derive the asks yourself.

No third source — commit messages and PR prose are the author's account.
Return **NOT GRADEABLE**, record nothing, and stop when neither source exists,
the goal has no criteria in the documented format, or it has no verbatim
request. An absent input makes every check over it vacuously true.

## Step 2 — Dossier, then dispatch

```sh
# Real paths only: an unquoted <placeholder> is two redirections to `sh`.
DOSSIER="/path/to/scratchpad/delivery-review"
WT=/path/to/worktree            # the branch under review, not the session cwd
mkdir -p "$DOSSIER" &&
  cd "$WT" &&
  git fetch origin &&
  git diff origin/main...HEAD         > "$DOSSIER/branch.diff" &&
  git diff origin/main...HEAD --stat  > "$DOSSIER/branch.stat" &&
  git log origin/main..HEAD --oneline > "$DOSSIER/branch.log"
```

Without the `cd` the diff is empty; without the fetch it carries other
branches' work. **Check the stat** is this branch before dispatching.

- **May receive:** retained request, reviewed source map, checklist, full
  diff/stat/log, evidence written to files, read-only repository; on a
  follow-up, the prior verdict, finding IDs and exact delta.
- **May not receive:** the implementing session's transcript, summary, plan or
  narrative; your case for a criterion; any "I ran X" not backed by a file or
  re-runnable.
- Evidence for a not-yet-open PR goes into the scratch dossier; never commit
  the dossier or raw output.
- `Agent` tool, **never `fork`**, type `general-purpose` (search types read
  excerpts), read-only instruction in the prompt.
- **Bracket both dispatches**: `cd "$WT" && git rev-parse HEAD && git status
  --porcelain`, plus the marker files in the git dir, before the first
  dispatch and after the last verdict. Any change to HEAD or tree, or a marker
  appearing or changing (one merely present from a previous round is fine),
  voids the verdict: record nothing, say what changed.

## Step 3 — Grade every line

Three passes, in order; merge escalations before reading the criteria table.

**Completeness** — read the original request first, derive its distinct asks
independently, then compare the reconciled map (establish it now if no
preflight exists; do not restart implementation). A real uncovered ask is
**UNLEDGERED**: cite the source span, the missing outcome/constraint, and
whether it is an outcome/evidence or a traceability-only gap. Operational
obligations are checked through their `G`/`C` evidence. Rewording or a
different clause count proves no omission; a frozen map never suppresses a real
gap. Source 2 uses the issue body.

**Ledger** — per `R`: `COVERED` (a criterion discharges it and graded
DELIVERED) · `PARTLY COVERED` (name the part missing) · `DROPPED`.

**Criteria** — per `A` and `G` only. `C` closing steps are sequencing: verify
completed ones, leave later ones pending; a misclassified one needs a map
correction. Grades: `DELIVERED` (observable in the branch **and** the evidence
exists at its path) · `PARTIAL` (name what did not land) · `MISSING` ·
`UNPROVEN` (plausible, but evidence absent or only a claim — a failure, fixed
by recording evidence).

## Evidence rules — binding, and copied into the subagent's prompt

1. "The code looks right" is not evidence: cite `file:line` or grade `MISSING`.
2. A named test is graded by its assertions; one asserting nothing about the
   criterion is `UNPROVEN`.
3. A named command needs recorded output and input identity; reuse follows the
   delivery contract; rerun when missing, stale or contradicted.
4. A prose criterion is graded by quoting the lines; a paraphrase is not a
   quote.
5. A `TODO`, a doc about future work, or code nothing calls is `MISSING`.
6. Commit messages and PR bodies are not proof; read the diff.
7. An `S` that drove the design is reported as a question `mission` should have
   asked.
8. A gate listed N/A that applies to the shipped diff is a finding at the
   weight of a failed criterion.
9. All `DELIVERED` on the first round: say what you checked that could have
   failed.

## Step 4 — Verdict

**PASS** only when the source was 1 or 2 and the completeness pass ran;
nothing `UNLEDGERED`; every `R` `COVERED`; every `A`/`G` `DELIVERED`; nothing
`UNPROVEN`, `MISSING` or `PARTIAL`. Check the inputs existed first — every
later clause is vacuous over an empty set. An approved deferral counts as
satisfied, ledger included; name it and who approved it. **FAIL** otherwise:
failing lines first, each with the smallest change that flips it; close with
the source, whether completeness ran, the counts and rule 9's answer.

## Step 5 — Fix loop (the calling session's)

The reviewer never edits; the trader does not close gaps; the session does.
Batch compatible fixes, then an independent follow-up under the delivery
contract (source map, prior verdict, full diff, exact delta; recheck affected
findings at the current key). Track finding-level progress within finite
budgets; new independent gaps do not undo closed ones; repeated failures and
exhausted budgets escalate. Escalate at once when a fix would change scope,
contradict a `D`, or need the trader's call.

**Deferral** needs user approval or an expressly delegated retrospective
exception under the delivery contract. Record it in the goal under
`## Deferred` (ID, what is missing, why, the grant) and in the PR body; this
stales both markers and needs the applicable full review, not the
traceability path. An ungranted request goes under
`## Deferral requested — NOT granted`.

## Step 6 — Publish PASS

Run after `arch-review`, never before. Write the verdict to a scratch report
whose last line is exactly `DELIVERY-REVIEW: PASS`; on PASS only, from the
clean reviewed worktree:

```sh
cd "$WT" &&
  sh .claude/hooks/review_report.sh publish delivery-review "$PR" "$REPORT_PATH"
```

The producer verifies branch, HEAD, base, review key and PR before and after,
reads the report back, then records `delivery-review-ok`; a hand-written marker
is rejected. No PR: print the verdict, record nothing. A later tracked edit
needs a current follow-up, never a marker refresh.
