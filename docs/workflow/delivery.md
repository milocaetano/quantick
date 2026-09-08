# Delivery without repeated reconstruction

This contract owns requirement reconciliation, review progress and proportional
validation. `CLAUDE.md` delegates those decisions here; task skills retain their
review roles, tier rules and exact-diff markers. Apply it in Claude and Codex.
It never grants a merge, waives an outcome or changes the score rubric.

## Reconcile requirements before implementation

Retain the original user request and any delegated task request verbatim. Map
each distinct requested outcome or constraint to stable `R` IDs and observable
`A` criteria. Equivalent clauses may share an ID; preserve their source spans.
Record applicable workflow/authority obligations once as `G` gates or `C`
closing steps, with their authoritative source and evidence destination. Do not
turn each repeated tool instruction, handoff or review reminder into a product
requirement. An explicitly requested workflow change is itself a product
outcome for that mission and must have `R/A` coverage.

Before code, reconcile the source against this map. For high/max missions use
an independent source-first completeness pass: give it the source first, then
the proposed map. Resolve missing asks and record the reviewed map/source
revision. This is a bounded read-only preflight, not an extra user approval.
Lower tiers reconcile inline. Amend the map when source requirements change;
never freeze away a real omission. Existing missions can establish this map
from their retained sources without restarting implementation.

At delivery, independently identify distinct outcomes, constraints and gates
from the retained source before reading the map. Then reconcile them with the
existing IDs and evidence; do not renumber equivalent asks. A new omission must cite
the source span and explain the distinct uncovered outcome/constraint or gate;
a different wording or atomization alone is not a finding.

Distinguish:

- **Outcome/evidence gap:** behavior, constraint or required proof is absent.
  Repair and prove it; all relevant acceptance and authority gates still apply.
- **Traceability gap:** the outcome and evidence exist but the map omits their
  connection. Correct the record and have it independently checked before PASS.
  Do not infer a product defect or rerun runtime tests solely from this label.
- **Operational obligation:** verify its recorded gate or closing step. Missing
  authority or a failed required check blocks delivery even if all `A` pass.
  A duplicate operational sentence needs no additional `R/A` pair.

## Review once, follow up on the delta

The initial required reviews cover the entire declared-base diff. Every review
report names head, base ref/tip, review key, scope, evidence and findings.
After edits, old markers are stale: never copy or re-stamp them as approval.
An independent follow-up may combine the prior report with an inspected delta
and the full current diff. It verifies affected findings, source/map changes,
evidence validity and new regressions; it explicitly carries forward unaffected
conclusions and issues a new verdict for the current key. Missing prior report,
changed base, changed scope or uncertain impact requires the applicable full
review. A full final campaign review remains mandatory. A follow-up is a real
review, not permission to bless old evidence without reading the change.

Batch compatible corrections before freezing the branch for review. A report
on a moving head is not final evidence. Do not repeatedly rebuild dossiers or
restart successful reviews while the reviewed inputs remain unchanged.

## Finding identity and bounded progress

Give each finding a persistent ID, source span/behavior, severity, evidence,
attempt count and disposition. Equivalent rediscoveries retain their ID and
counter. Separate old findings closed, old findings still open, regressions
introduced by repairs, and newly discovered independent findings. Closing an
old finding and discovering a different one is progress, not a failed repair.

Default limits are three repair attempts per finding and three repair batches
per mission after the initial review; an existing stricter authorized budget
still applies. A batch may fix several findings. Preserve counts across new
reviewers, commits, sessions and rebases. After two consecutive batches close
none of the targeted findings, or a limit is exhausted with findings open,
checkpoint that task and escalate with the concrete patch/evidence. New IDs do
not reset the mission budget. If the last allowed batch closes everything,
finish delivery. Scope, authority or safety decisions escalate immediately.
No open required finding is waived by progress or budget exhaustion; only the
user can approve a deferral. Independent campaign tasks may continue.

### Durable review record

Use `sh .claude/hooks/ai_review_threads.sh progress-show <pr>` to read the
append-only `quantick-review-progress:v1` records on the PR. A missing record is unknown
history, not zero attempts. One coordinator writes with `progress-record <pr>`
and a JSON body on stdin; it supplies `schema: 1`, `mission`, `revision`, `head`,
`stage`, cumulative `batches`, `stalled_batches`, `limits`, `findings`, `evidence`
and `authorization`.
Each finding maps its stable ID (the review thread ID when available) to
`attempts`, `disposition` and evidence URLs. Initial adoption records observed
prior counts with source evidence; never guess away missing history.

Before dispatch, `progress-check <pr> <finding-id>...` must pass; persist the
incremented batch/target attempt counters and `stalled_batches` before repair.
After independent review, append dispositions/evidence; only closing a prior
open finding resets the stall streak. On interruption reconcile the reserved
batch before resuming it; do not reserve another batch for the same action.
Ordinary discussion replies do not count as attempts. The reader rejects
conflicting revisions, lost IDs and decreasing counters. Retry a lost write
using the identical record; readback reconciles it without another increment.
Store stage timestamps and the input-bound validation/reuse report as linked
artifacts in `evidence`; campaign checkpoints link this PR record instead of
reconstructing counts from session memory. Inspect the actual user decision
behind any authorization URL: stored text cannot grant a larger budget or deferral.

## Validation follows changed inputs

Full required CI at the final PR head is mandatory in every path. A missing,
pending or failed check never counts as green. Local verification is:

| Changes since the last verified tree | Required local work |
| --- | --- |
| Runtime, tests/fixtures, schemas, dependencies, runtime/build configuration, hooks, scripts, generated inputs, or uncertain impact | Applicable targeted regression checks and the full ordered fmt/clippy/build/test loop before commit. |
| Only prose, skill instructions or instruction-budget metadata, with no executable/runtime-config/contract/test-input changes | Repository guards, diff hygiene, changed relative links and skill validation when applicable. Workflow/authority/validation changes additionally require the shared hook suite and independent behavioral exercises. |
| Only an evidence/mission-record correction after successful code verification | Inspect the entire delta and referenced artifacts; run guards, diff hygiene and affected links/record checks. Reuse runtime evidence only under the proof below. |

A Markdown extension alone proves nothing: a changed contract, executable
snippet used by tests, fixture, generated input or build-consumed document uses
the first row. Instruction changes with operational consequences use the
second row's behavioral proof, never the evidence-only row. Tier and line count
do not independently require repeating runtime checks for a verified prose
delta; existing tier-based review obligations are unchanged.

For evidence reuse record: successful command outputs and their source tree,
the exact verified commit/tree and current tree, unchanged base, the full
name/status diff plus inspected content, relevant toolchain/dependency/config
and environment identity, and why no tested input changed. Check tracked and
relevant untracked/generated inputs, not just a filename filter. With unknown
inputs, a base change, relevant environment change or a current related failure,
rerun the affected verification under the first row. Never turn a failed run
into a pass or silently relabel prior evidence as current execution. Report
`reused from <tree>` separately from `run at <tree>`. Evidence reuse is not review
reuse: current review keys, independent verdicts and final-head CI are still due.

## Finish work before starting more

Campaign coordinators first reconcile active PRs and merge authorized ones
whose gates pass; next unblock bounded repairs and reviews; then start new
ready implementation within the granted concurrency. Reviews/CI waiting do
not reserve an idle implementation slot, but pending repair work does. Do not
start new work that competes for the build host needed to finish a ready PR.
Never bypass a gate or merge authority to reduce queue length.

Record stage transitions (implementation, local validation, review, repair,
CI, ready-to-merge, integrated) with timestamps and head. Summarize queue age,
repair batches and reasons for revalidation at milestones; separate overlapping
durations and offline/human waits. Use the campaign state's compact operation
journal before mutations and full checkpoints at recovery boundaries; do not
publish a full snapshot for an unchanged status or every individual mutation.
After integration reassess the campaign target and schedule remaining verified
gaps. Exhausting the original backlog below target requires replanning, not a
claim of completion. Child completion returns control to the coordinator;
do not ask the user to paste a new goal for every mission.

## Adopt changes in an existing campaign

Do not apply an unmerged workflow proposal to another writer's branch. After
the user integrates it into main, adopt it through the campaign's normal
reviewed synchronization PR and record the source revision/checkpoint. Keep
original requests, findings, counters and failed evidence. Reconcile existing
escalations under the adopted rules; a raw-count false stall may resume only
within remaining recorded authority and budgets. An exhausted budget or a real
user decision remains blocked. Adoption does not grant a deferral or reset work.

A missing link to an existing historical artifact is repairable traceability.
A historical action that never happened is not: a later mission record cannot
prove pre-edit planning. Preserve the observed chronology, recover original
artifacts when available, and request a specific user decision for an unmet
historical obligation or exhausted budget. Do not manufacture retrospective proof.
