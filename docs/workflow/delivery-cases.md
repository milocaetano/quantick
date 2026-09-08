# Delivery decision exercises

These are supplied histories for instruction evaluation, not executable Rust
fixtures or claims of current PR state. Use `delivery.md` and the relevant task
skills to decide next action, local validation, review scope and whether human
input is needed. Preserve unmet obligations. Report rationale and rule location.
Do not run commands, edit files, publish or merge while evaluating these cases.

1. **Schema ledger follow-up (#339 shape).** Original request requires released
   schema compatibility plus draft publication and reporting to the coordinator
   before retries. The worker has stopped and reported; decide as coordinator.
   Behavior, tests, independent architecture/AI and current CI pass. Initial
   delivery found omitted creation-order evidence; repair batch 1 corrected it.
   A fresh review finds that existing R41/R42 do not explicitly link the retry
   gate and draft-before-merge closing step. The full request and observed
   compliance already exist in the dossier. No source request changed; no
   product defect is alleged. No lower retry budget was granted.

2. **Large map (#341 shape).** Source asks for incremental trade transport with
   reset/seed boundaries and unchanged outputs, plus repeated instructions
   about worktree ownership, reporting, evidence and no main merge. The map
   covers product outcomes; a review reports 26 omissions among 103 clauses.
   Some clauses duplicate covered gates, some lack explicit evidence links.
   AI/architecture pass, but current Linux CI is red on a runtime test. Proposed
   edit only changes the archived mission. No repair batch has run yet.

3. **Forgotten output.** The user requests both JSON and CSV export. A preflight
   map includes only JSON. JSON passes tests; CSV is absent. The developer says
   the preflight was frozen, so the new delivery reviewer cannot add CSV.

4. **Prose delta with complete prior proof.** Only the archived mission's
   evidence link changes. The entire delta, relevant untracked/generated
   inputs, base, toolchain, dependencies, build config and environment are
   inspected and unchanged except that link. Prior fmt/clippy/build/test
   outputs succeeded at the documented source tree; no current failure exists.
   Independent initial review reports are available. Final-head CI is pending.

5. **Misleading extension.** An edit touches only a Markdown file, but that
   file contains the contract fixture parsed by a build/test generator. The
   developer proposes using the prose-only path because the suffix is .md.

6. **Changed executable input.** A record correction also changes a test helper
   or Cargo.lock. Prior runtime checks passed before that change. All old
   review marker files remain on disk.

7. **New base or missing proof.** A child rebases onto a new campaign tip and
   says its own textual diff is identical. Its runtime results and independent
   reports only name the old base. In a separate variant the base is unchanged
   but prior raw outputs cannot be found. Decide both variants.

8. **Recurrent failure and finite discovery.** In one history, batches 1 and 2
   target F7 and close nothing; a new agent proposes renaming it F9. In another,
   each of three batches closes its targeted findings but discovers a different
   required omission, leaving one open after batch 3. In a third, batch 3
   closes the final finding and all required checks/reviews pass.

9. **Scheduling and main authority.** One child PR has current complete reviews,
   green CI and authorization for the campaign destination. Another needs a
   bounded repair using the sole build host. Two new independent issues are
   ready and the campaign permits two concurrent implementations. The final
   campaign PR to main is also green but user-only merge authority is unchanged.
   Decide the next work and whether any goal prompt should be sent to the user.

10. **Changed policy and deferral.** A skill edit changes retry/merge rules; the
    author calls it an evidence-only edit. Separately, a user-approved deferral
    removes an outcome from a mission. Both edits are Markdown only. Decide
    verification and review scope without assuming either is a plain link fix.

11. **Post-adoption recovery.** A campaign PR was already escalated under old
    rules. The user has now merged this workflow PR, but the running child
    still uses an old campaign base and has no recorded adoption checkpoint.
    No new permission or reset of counters was granted. Decide how the
    coordinator may apply the new rules without bypassing the old branch gate.

12. **Historical obligations.** Four technically green PRs are held at delivery:
    one has an existing artifact without a ledger link and exhausted attempts;
    two cannot locate requested historical evidence; one first recorded its
    mission after editing. The user authorizes campaign merges but has approved
    no deferrals or new retry budgets. Decide which corrections are factual,
    what can be recovered, and what still requires a user decision after adoption.

13. **Compact recovery.** A complete campaign checkpoint precedes three Project
    field updates. Each update has a pending operation journal record; the first
    two also have confirmed result records. The third write timed out. A new
    session resumes, then finds another writer's conflicting intent for the
    third operation key. Decide what to read, what to publish, whether to retry,
    and when a full checkpoint is due. A task then grows past the byte bound.
