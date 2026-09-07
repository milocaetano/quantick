# Durable state contract v1

GitHub is authoritative. Local files, conversation and host goals are caches.
Use a parent issue body for the stable charter and index, child issues for
acceptance/evidence, and append-only checkpoint comments for execution state.
Never store credentials, authorization headers, private instance descriptors,
environment dumps or unredacted sensitive logs. Store sanitized evidence URLs.

## Charter and task records

The parent body includes: campaign ID (`owner/repo#parent`), schema version,
objective, expected outcome, scope and exclusions, completion criteria with
stable IDs, constraints, dependencies, risks/mitigations, baseline and target
metrics with revision/rubric/evidence, granted actions with source and limits,
Project URL/ID and field mapping (or explicit pending setup), integration mode,
exact campaign branch, initial main/current campaign SHA, merge authorization
source, final PR and status under [integration branches](integration.md), child issue
index, decision log links, and latest checkpoint link. Add marker
`<!-- quantick-campaign:v1 -->` for discovery.

Each child has a stable task key, parent URL, reviewable outcome, criteria and
evidence destinations, priority, risk/validation plan, owner class, dependency
keys with satisfaction conditions, issue URL/ID, Project item ID, branch,
worktree owner, PR URL/head/base and checkpoint links. Unknown IDs are null,
never guessed. Use native parent/sub-issue and dependency relationships where
available; the explicit issue links and conditions remain the portable record.

Owner classes: `autonomous`, `human_decision`, `manual_validation`,
`external_authorization`. Human feedback does not confer external authority
unless the authorized user explicitly grants the particular action. Record
decision ID, actor, date, source URL, decision, rationale and affected task
keys. Proposed decisions remain proposed; agents cannot accept their own
deferrals. Changed scope requires a new recorded decision.

## Checkpoint envelope

Post a fenced JSON object in a comment starting with
`<!-- quantick-campaign-checkpoint:v1 -->`. This is the compact handoff format;
replace illustrative values with real URLs/SHAs. Required keys:

```json
{
  "schema": 1,
  "campaign": "owner/repo#123",
  "sequence": 4,
  "publication_key": "owner/repo#123/checkpoint-4/writer-nonce",
  "previous": "https://github.com/owner/repo/issues/123#issuecomment-1",
  "created_at": "2026-09-06T18:00:00Z",
  "writer": "codex/session-id",
  "phase": "running",
  "base_sha": "full resolved SHA",
  "integration": null,
  "authorization": {"source": "charter decision D1", "allowed": ["issues", "branches", "commits", "prs"], "excluded": ["merge", "deploy", "spend"]},
  "lease": {"owner": "codex/session-id", "expires_at": "2026-09-06T18:15:00Z"},
  "project": {"url": null, "id": null, "states": {}, "pending": ["create campaign Project"]},
  "tasks": [
    {"key": "T1", "issue": "https://github.com/owner/repo/issues/124", "class": "autonomous", "priority": 1, "state": "ready", "depends_on": [], "owner": null, "branch": null, "worktree": null, "pr": null, "head": null, "attempts": {"operation": 0, "repair": 0}, "evidence": [], "next_action": "Read child acceptance criteria and start new-task"}
  ],
  "inflight": null,
  "decisions": [],
  "metrics": {"baseline": "charter metric/evidence link", "target": "charter criterion IDs", "latest": null},
  "retry_policy": {"operation_attempts": 3, "repair_attempts": 3, "lease_minutes": 15, "ci_stall_minutes": 30, "review_stall_hours": 24},
  "next_action": {"task": "T1", "operation": "new-task", "reason": "No prerequisites"},
  "stop_reason": null
}
```

`phase` is `running`, `waiting`, `blocked`, `ready_for_evaluation`,
`integrated_main` or `complete`. Integration campaigns require observed user
merge and final evidence before those last two; evidence-only campaigns can
complete without a main merge. Candidate readiness does not close the parent. Task `state` is
`backlog`, `ready`, `in_progress`, `blocked`, `awaiting_human`,
`awaiting_ci`, `awaiting_review`, `awaiting_merge` or `done`.
`depends_on` entries are objects with `task`, `condition` and `evidence`:
normally `integrated_campaign_with_green_ci` for campaign children (record
merge SHA, target ref and evidence), `merged_with_green_ci` for ordinary code,
or `accepted_evidence` for documents
or human tasks. All prerequisites must pass. Never equate issue closure with
satisfaction. Rejected/closed-unmerged PRs require recovery, not `done`.

`publication_key` is stable across retries of the same snapshot publication.
`integration` is null for evidence-only campaigns; implementation campaigns
copy their integration record and both criterion sets from the charter into
each complete checkpoint, with current campaign SHA and final PR status.
`inflight`, when non-null, names the stable operation key, target, intended
mutation, attempt count, starting checkpoint and expected readback. Persist
before a business mutation; clear only after observed success or a recorded failure.
Checkpoint publication is the journal write itself, exempt from recursively
journaling another checkpoint. After a lost publication response, find its
exact `publication_key` and read it back before retrying. Do not publish a new
sequence until that uncertain journal write is reconciled. Parent creation is
the bootstrap exception: first search the stable objective key specified in
GitHub operations, create the charter with that marker, read it back and publish
checkpoint 1 before further business mutations.
The pending Project list carries precise target/field/option payloads once IDs
are known. An authorization source points to an actual user statement retained
in the charter or a verifiable GitHub decision, not merely this JSON assertion.

Large campaigns keep terminal task details in children and linked prior
checkpoints. The current checkpoint retains every active task, dependency
frontier, terminal prerequisite proof links, counters and next action; paginate
children rather than loading full histories. At 50 active entries, partition
into linked checkpoint comments and publish a final manifest listing all part
URLs/counts. Only the final manifest commits the snapshot. A missing part is
an incomplete checkpoint and blocks dependent writes. Never truncate silently.

## State projection

| Project state | Checkpoint states | Entry condition |
| --- | --- | --- |
| Backlog | backlog | Needs decomposition or prerequisite proof |
| Ready | ready | Authorized autonomous task; prerequisites satisfied |
| In progress | in_progress, awaiting_ci, awaiting_review | Claimed work or submitted PR; substate visible in checkpoint |
| Blocked | blocked | Failed dependency, exhausted retry, unsafe/conflicting state |
| Awaiting human | awaiting_human, awaiting_merge; parent ready_for_evaluation | Explicit human task, missing campaign merge authorization or final main handoff |
| Done | done | Criteria proven; integration condition met where required |

Preserve existing Project fields. Use a dedicated `Campaign state` field with
these six options if the default Status cannot be safely adapted. Do not
overwrite a shared roadmap's status vocabulary. A view grouped by this field
may require UI setup: create a manual task with instructions; retain the field
and values as the machine-readable board until the view is confirmed.

## Ownership, recovery and attempts

GitHub issue comments are not an atomic lock. Default to a single coordinator.
Before claiming, read the latest complete snapshot and subsequent checkpoint
metadata, append the lease,
then re-read to detect another claim. Before each mutation recheck ownership
and expiry. Renew before expiry. Conflicting claims or sibling sequence values
stop writes to the affected campaign; do not resolve by timestamp alone. An
expired lease permits recovery only after inspecting PRs/branches/worktrees
and confirming the old writer is inactive. If that cannot be established,
escalate ownership and perform only independent read-only work. Parallel child
agents report to the coordinator; they do not publish competing checkpoints.

Every complete checkpoint is a recovery boundary: it carries current
authorization sources, decisions, cumulative retry counters, active tasks,
dependency proof links and pending mutations without requiring older bodies.
Resume reads the charter, that boundary, referenced active children/decisions/
evidence and live GitHub state. Inspect subsequent checkpoint metadata for
forks; older bodies are read only to resolve a specific uncertainty. If the
index is stale, paginate comment metadata and filter checkpoint markers with
CLI/API tooling before loading bodies into agent context. An interrupted
publication with no final
manifest is uncommitted: use the prior complete checkpoint plus readback. A
fork, missing prerequisite proof or contradictory decision needs reconciliation
before scheduling. Restore counters, ownership and authorization limits; never
reset attempt budgets because the host changed. Derive and explain the next
action from live prerequisites, not the cached `next_action` alone.

For each `inflight` operation search by stable campaign/task/operation key,
issue links, branch and PR head, then read the resulting object. Reuse a
confirmed result and repair missing links. Do not blindly retry uncertain
creation or any irreversible operation. External commits or changed PR heads
invalidate stale checks/reviews. Dirty abandoned work is inspected and retained,
never cleaned destructively. Failed/cancelled CI is classified before repair.

Defaults bound unattended cost, and the parent may tune them with rationale:
three total API attempts per operation, with 5 then 20 second backoff for
transient failures; three repair attempts per task/failure signature. Permission
denials get no blind retry. Honor rate-limit reset times and checkpoint longer
waits. Each attempt records error signature, head, evidence, action and result
in the child; counters persist in checkpoints. A changed hypothesis does not
erase history. Repository review stall rules can stop earlier and always win.
Review repairs also follow [the delivery contract](../workflow/delivery.md):
retain finding IDs and the mission repair-batch counter across checkpoints.
Record newly discovered findings separately from attempted repairs; do not use
the raw total-open count as the stall test. Existing failed evidence and
stricter authorized budgets remain in force.
Exhaustion blocks that task and creates an actionable escalation, then selects
other ready tasks. A new budget needs an explicit decision, not a new session.

Poll/wait for changes in bounded host-supported intervals (at most 60 seconds
per blocking call). No progress in CI for 30 minutes or review for 24 hours
triggers diagnosis using run/job timestamps, head, logs and review threads.
These are investigation thresholds, not proof of failure. Record last observed
progress and next check time; do not keep resetting the timer on every poll.
Re-run a transient failed job only within authority and budget. Queued/running
jobs are observed, not duplicated. Never cancel another writer's run. If still
stalled, create one escalation per failure key and work on independent tasks.

## Human task template

Create a linked issue (or Project draft item with its text copied into the
parent so it survives missing Project access). Include its class, dependencies
and stable key. Show only unresolved actionable human tasks in `human-tasks`.
Group duplicate requests and distinguish waiting prerequisites from actions
the human can take now. Use this format in the user's language in conversation;
persist English artifacts under repository rules:

```text
Human task: <specific outcome> [key; issue URL]
Class: human_decision | manual_validation | external_authorization
Reason: <why agent evidence or current authority cannot settle this>
Steps:
1. <precise environment, revision and entry point>
2. <scenario and observations or decision alternatives>
3. Record the result on <issue URL>.
Prompt to execute: <complete copyable prompt with context and expected return;
                    or "Not applicable: direct manual action", with reason>
Expected result: <approved OR rejected with observations/evidence>
Evidence to return: <specific artifact, decision or sanitized result>
Completion criterion: <objective rule; silence is not approval>
Blocks: <task keys; independent work continues>
```

For missing GitHub Project scope, retain prepared payloads and offer one action
such as `gh auth refresh -h github.com -s project` when OAuth is actually used.
Never ask the user to paste a token. For another credential mechanism identify
the correct permission change without replacing credentials or logging them.
