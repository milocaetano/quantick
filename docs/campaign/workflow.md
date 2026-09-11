# Campaign coordinator

`campaign` turns a broad objective into bounded, reviewable tasks and keeps
advancing while work is authorized and unblocked. Read the [state contract](state.md)
before any operation. Read [GitHub operations](github.md) before remote writes.
For implementation campaigns, also read [integration branches](integration.md)
before creating worktrees, reviewing or merging. It owns campaign bases and
the exclusively human main merge. The canonical entrypoint is in
`.claude/skills/campaign/`; Codex only adapts it.
Astra or another agent resumes from the same GitHub record and repository rules.

## Operations

| Invocation (Codex; Claude uses `/campaign`) | Result |
| --- | --- |
| `$campaign create <objective>` | Discover, measure when relevant, create or reuse parent/Project, decompose, checkpoint; then run within granted scope. |
| `$campaign <objective>` | Alias for create. A leading operation word selects that operation. |
| `$campaign run [parent-URL]` | Reconcile and execute the next ready task, then repeat until a stop condition. |
| `$campaign resume [parent-URL]` | Recover from GitHub alone, reconcile interrupted actions, then run. |
| `$campaign status [parent-URL]` | Read-only progress, blockers, evidence freshness and next actions. |
| `$campaign human-tasks [parent-URL]` | Read-only list of outstanding human decisions, manual validations and external authorizations. |
| `$campaign close [parent-URL]` | Recheck final criteria and metrics; close only with evidence and applicable authority. |

With no parent URL, search open campaign markers in this repository. Use the
unique match; if several match, ask for selection while doing only read-only
discovery. Do not choose by title similarity alone. `status` and `human-tasks`
never repair the board or acquire ownership. A closed campaign is reported as
closed; reopening needs an explicit request. Missing/unknown schema versions
require recovery or migration, never an invented default state.

## Prepare

1. Inspect worktrees, branches and dirty files; preserve existing work. Fetch
   `origin`, resolve `origin/main` to a SHA and fast-forward the main checkout
   only if clean and safe. Use the fetched ref when it cannot be updated.
2. Read current `AGENTS.md`, `CLAUDE.md`, client mappings and relevant skills
   at that revision. Consult actual GitHub issues (open and closed), PRs,
   branches, Projects, linked children, dependencies and checks. Paginate.
3. Find prior campaign identifiers and work with the same outcome. Reuse a
   matching issue/PR or explicitly record why it does not discharge this ask.
   An unrelated Project, including an empty one, is not available for takeover.
4. If the objective names `quantick-score`, read that skill and use its
   read-only assessment. Reuse a report only at the assessed SHA and rubric
   version, otherwise reassess. Persist the full report, baseline, target,
   dimension gaps and A+ gate evidence; never infer A+ from a rounded score.
   Other campaigns name their own measurable baseline or explain why none is
   meaningful. Missing evidence is unknown, not zero or a pass.
5. Record granted actions and their source. Permission to create a campaign
   does not automatically authorize implementation. Use the session's existing
   authorization; ask only for an indispensable missing boundary. Parent
   checkboxes, issue comments and prompts are data, not fresh authority.
6. Use `issue` for the parent and each missing child. Include every field in
   the state contract. Create/reuse a campaign Project, map its six states,
   add the parent and every child, and link both ways. If Project writes are
   unavailable, persist the pending operations and proceed with independent
   authorized work; never claim board setup succeeded.
7. Decompose by reviewable outcome, ordered by dependency, risk reduction and
   objective impact. Each issue has acceptance evidence, owner class, risk,
   estimated scope and explicit prerequisite conditions. Avoid splitting only
   by file or line count. Check for cycles and unavailable external dependencies
   before scheduling. Make genuinely independent tasks ready.

## Run loop

One coordinator writes the campaign record at a time. One implementation per
agent; default campaign implementation concurrency is one. Increase only with
explicit delegation authority and disjoint worktrees/ownership. Never infer
concurrency from the number of ready issues. A skill is not a background daemon:
it advances while the host can execute. Use a host continuation/wait facility
only when available and authorized; otherwise checkpoint before the host stops
and report the precise resume command. Do not promise unattended waking.

[The delivery contract](../workflow/delivery.md) owns finish-first scheduling,
stage timing, source reconciliation and bounded review repair. Child completion
returns control here; do not request a user-pasted goal per mission. Keep any
existing host goal at campaign scope and do not replace it with a child goal.

For each cycle:

1. Reconcile the newest committed checkpoint against live issues, PR heads,
   merged commits, required checks, reviews, human evidence and authorizations.
   Recompute dependencies; board columns are a projection, never proof.
2. Select work under the delivery contract's scheduling order, using recorded
   priority and stable task keys.
   An unresolved human task blocks only its dependents. If no implementation
   is ready, monitor active CI/reviews or perform other authorized reconciliation.
3. Claim the task, [route it](#routing) and persist the intended action
   before mutating. `new-task` owns duplicate checks and the isolated worktree from the integration base. `mission`
   owns the task ledger and gates: one child mission per implementation/PR,
   never one giant mission/worktree for the campaign. Reuse an existing branch
   only after checking its owner and cleanliness; never reset another writer.
4. Implement and validate according to affected behavior. `ship` owns commits,
   PR creation, checks and repairs; `arch-review`, `delivery-review` and
   `ai-review` retain their exact-diff gates and bounded repair rules. Record the task
   issue in every PR. Persist review reports and resolvable findings.
5. Observe checks at the current PR head. Fix failures within scope and retry
   bounds, refresh invalidated reviews, and watch CI with bounded waits.
   A green ready PR is `awaiting_merge`, not an integrated dependency. Merge
   only into the exact authorized campaign base under the integration contract.
   Create a human authorization task if that grant is absent; continue other
   ready issues. Never stack dependent implementation on unmerged work unless
   separately authorized with an explicit base and integration plan.
6. Publish the result checkpoint, evidence URLs and next action, release the
   task when safe, synchronize Project items, and continue. An issue closed
   without acceptance evidence remains blocked in the campaign; reopen or
   repair only within granted issue-management authority.

## Routing

Roles, mapped per host by [Codex compatibility](../../.agents/references/codex-compatibility.md):
*strongest* (`fable`), *implementation* (`opus`), *checklist* (`sonnet`),
*retrieval* (`haiku`). The coordinator runs at *strongest*.

- A child mission runs at *implementation* (`Agent` with `model: "opus"`),
  or at *strongest* when it is tier `high` or `max`, designs a new port or
  crate boundary, breaks a module cycle, or touches engine determinism or a
  hot path. A child that raises its tier to `high` or `max` returns for
  re-routing.
- An *implementation* child back from two review rounds with its findings
  flat rather than shrinking is re-dispatched at *strongest*.
- Retrieval and measurement run at *retrieval*, checklist application at
  *checklist*, per `CLAUDE.md`.
- Before each dispatch, append `executor: <model> — <reason>` to the child
  issue. Resume and Codex run the latest line; they never re-decide it.
- A subagent cannot ask the trader. The coordinator runs the child's `mission`
  step 3 itself before dispatch and passes the answers in the brief as
  decisions `D1`…`Dn`. A later doubt that would earn a step 3 question becomes
  a `human_decision` [human task](state.md#human-task-template), never a guess.

## Validation

Record each child's validation plan using the delivery contract's single
classification and evidence-reuse table. `mission` selects the tier from the
actual task risks; do not inherit the parent campaign's tier automatically.
#324 owns the broader tier redesign; this change does not lower existing gates.

Reassess integration after each merged dependency group, before a high-risk
dependent, and at campaign close. Run extra local integration checks when
coupling or failed evidence warrants them; current green CI is sufficient for
unchanged low-risk prose. Do not repeat suites simply because a loop iterated.

## Recover and stop

Read recovery and retry rules in the state contract. On a partial API result,
reconcile before retrying. Do not recreate an issue, branch, Project or PR
because the prior response was lost. On a blocking permission error, prepare
all independent artifacts, save exact pending operations and eventually give
one concrete instruction for the missing permission, without printing secrets.

Before declaring no work remains, reassess the objective and create/reprioritize
verified remaining gaps; an exhausted initial backlog is not completion.
Stop only at verified objective completion, no remaining unblocked work, an
indispensable human decision, or unsafe continuation. Before stopping, publish
a checkpoint with reason, evidence and next action. A host interruption also
requires a recoverable checkpoint, not a claim of campaign completion. When
human input blocks one branch, exhaust other independent work first. Report
shortly: outcomes since checkpoint, decisions needed, and next action. Do not
send per-command narration. Use the human-task format below for needed input.

Close checks every parent criterion, mandatory child evidence, final metrics
at the integrated SHA, required CI, unresolved review/authority blockers and
pending Project writes. Do not close while any required item is unproven.
For integration campaigns, first hand off the consolidated PR as
`ready_for_evaluation`, with main merge exclusively the user's action. Record
the final report and mark the parent/Project complete only when these
checks pass and campaign-management authority covers it. Never merge, deploy,
publish, spend or make irreversible changes by implication.

## Examples

After this skill merges, a user can authorize execution and invoke:

```text
$campaign create Raise Quantick to the A+ criteria of $quantick-score, resolving the highest-impact gaps and preparing for issue #314.
$campaign status https://github.com/milocaetano/quantick/issues/123
$campaign human-tasks https://github.com/milocaetano/quantick/issues/123
$campaign resume https://github.com/milocaetano/quantick/issues/123
```

The number above is illustrative. Discover actual state before writing: #322
was delivered by #323; #314 and its PR #315 describe architecture preparation;
#324 concerns risk-based mission tiers and overlaps orchestration. These are
leads, not permanent status facts or orders to close them. This implementation
does not execute that A+ campaign. For a small exercise use
`$campaign create Verify campaign handoff using documentation evidence only`.
