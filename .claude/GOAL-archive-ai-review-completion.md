# Require exact-diff AI-review completion

Close issue #331 (campaign #330, task Q1): readiness must distinguish a
completed clean AI review from one that never ran. Reuse the shared review
key, bind completion to its task branch, and retain the separate finding gate.

**Tier:** high, because this changes review enforcement and authority guards.

## Request ledger

- R1: Missing completion denies readiness, even with zero open findings.
- R2: Missing completion denies merge, at every tier including small.
- R3: Clean current completion and existing evidence permit supported gates.
- R4: Completed reviews with unresolved findings remain blocked by threads.
- R5: A changed source diff invalidates completion.
- R6: A changed task branch invalidates completion even with an equal raw key.
- R7: A changed campaign base tip invalidates completion.
- R8: A changed campaign target ref invalidates completion.
- R9: Unrelated worktrees cannot supply completion.
- R10: Draft creation stays available before the reviews.
- R11: Completed AI review has a durable PR report with full HEAD SHA.
- R12: The report records explicit base ref, base tip and shared review key.
- R13: Stable worktree, branch, HEAD, status, base and key are checked around review.
- R14: Only a completed, published review produces the private projection.
- R15: Claude and Codex share the hook, with hermetic boundary regressions.
- R16: Canonical instructions identify producer and consumer without policy copies.
- R17: Preserve architecture/delivery reviews and every existing tier constraint.
- R18: Preserve command limits, campaign authorization and exclusive human main merge.
- R19: Reuse existing comparator/key; no generic subsystem, copied algorithm or bypass.
- R20: Stay within assigned workflow/docs paths; no crates, manifests or lock changes.
- R21: Keep Q1 independent of F1 code and serialize build-host use and campaign merges.
- R22: Arm guards and app all-target check before editing; validate each edit batch.
- R23: Run full fmt/clippy/build/test in order before every commit and affected checks.
- R24: Save raw command/exit artifacts outside the repository and report failure counters.
- R25: Preserve all IDs and archive this ledger before reviews; hand off exact local head.
- R26: If base advances, coordinate update and rerun affected validation and stale reviews.
- R27: Coordinator owns publication, checkpoints, review records and integration.
- R28: Independent architecture, AI and delivery reviews plus exact-head CI remain owed.
- R29: No new runtime, financial or public product contract; declare path rates.
- R30: Do not claim a score increment before reassessment.

## Decisions

- D1: The coordinator assigned issue #331 and this isolated branch/worktree.
- D2: User D3 permits at most two independent implementations, with serialized
  campaign merges and coordinated build-host use. Authority: parent
  [D3 record](https://github.com/milocaetano/quantick/issues/330#issuecomment-5564210429).
- D3: The coordinator released the host after F1 verification. Q1 can implement,
  test and commit after green checks, with notice before committing. Publication
  and reviews remain coordinator-owned; no routine permission question is needed.

## Assumptions

- S1: A branch prefix on the existing key record is the smallest way to satisfy
  branch-only invalidation while preserving legacy architecture/delivery keys.
  The coordinator explicitly confirmed this reading.
- S2: Completion attests that the review ran and its report/findings were
  published; a completed review may have findings. Their disposition stays
  independently enforced. Local records retain the existing operational trust model.
- S3: All changed paths run rarely at developer commands or are documentation;
  no product hot path or user-visible app surface is touched.
- S4: Existing command-detection limitations remain in scope as preserved limits;
  arbitrary shell/API bypass prevention is not claimed.

## Acceptance criteria

Each evidence destination below is `docs/campaign/ai-review-completion-evidence.md`
unless an explicit artifact path is given. Test evidence names the independent
fixture and exact command; a pending item is not a pass.

- [x] **A1** — Both readiness and merge deny absent AI evidence at every tier.
  *Evidence:* shared-client omission fixtures. *(R1, R2)*
- [x] **A2** — Current clean completion permits supported readiness/campaign merge;
  current completion with open findings still denies. *Evidence:* positive and
  open-thread fixtures with existing evidence held current. *(R3, R4)*
- [x] **A3** — Source changes alone stale AI evidence after other markers refresh.
  *Evidence:* committed source-change fixture. *(R5)*
- [x] **A4** — Branch-only change stales AI evidence while the raw key stays equal.
  *Evidence:* branch-only fixture and explicit equal-key premise. *(R6)*
- [x] **A5** — Base-tip and base-ref changes each stale AI evidence with other
  markers refreshed. *Evidence:* independent campaign fixtures. *(R7, R8)*
- [x] **A6** — Another worktree's record cannot satisfy the gate.
  *Evidence:* private git-directory fixture. *(R9)*
- [x] **A7** — Draft creation remains ungated.
  *Evidence:* both-client no-marker fixtures and existing draft spelling tests. *(R10)*
- [x] **A8** — The canonical AI producer requires a durable six-dimension PR
  report with full head/base ref/base tip/key and finding disposition references.
  *Evidence:* AI skill report and completion instructions plus drift checks. *(R11, R12)*
- [x] **A9** — Producer compares worktree/branch/head/status/base/key around review
  and publication; failed/incomplete publication cannot produce completion.
  *Evidence:* canonical producer procedure and final guard commands. *(R13, R14)*
- [x] **A10** — Shared Claude/Codex regressions cover missing, malformed, stale,
  current, findings and campaign context; producer/consumer names cannot drift.
  *Evidence:* shell suite and explicit recording-owner checks. *(R15, R16)*
- [x] **A11** — Existing review ordering, small size bound, tier validation,
  unresolved-thread handling and draft/non-draft creation rules still pass.
  *Evidence:* unchanged boundary tests and new isolated marker cases. *(R17)*
- [x] **A12** — Existing campaign identity/grant/CI checks, supported command
  restrictions and main prohibition still pass. *Evidence:* campaign suite. *(R18)*
- [x] **A13** — AI reuses the existing key and comparator with no skip/override.
  *Evidence:* focused implementation diff and architecture dossier. *(R19)*
- [x] **A14** — Diff stays within assigned workflow/docs ownership and adds no
  crate, dependency or product contract. *Evidence:* final path list and rate table. *(R20, R29)*
- [x] **A15** — Work is independent of F1 implementation; host acquisition and
  coordinator-only serialized merge handoff are recorded. *Evidence:* coordination
  log summarized with source messages in the evidence document. *(R21)*
- [x] **A16** — Arming commands precede the first edit and guard tests follow each
  batch. *Evidence:* numbered external raw logs. *(R22)*
- [ ] **A17** — Full ordered local checks and affected shell/skill/link/language/
  context checks exit zero before each commit. *Evidence:* external exact-command
  logs and final verification dossier. *(R23)*
- [x] **A18** — Raw output/exit records and cumulative failure signatures survive
  handoff. *Evidence:* `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q1-validation`
  and evidence index. *(R24)*
- [ ] **A19** — Stable mission IDs are archived before reviews and the exact
  clean local head is handed off. *Evidence:* committed archive and git status. *(R25)*
- [ ] **A20** — Any observed campaign-base advance is reconciled and affected
  checks rerun before review. *Evidence:* final base readback and conditional
  revalidation log; record explicitly when no advance occurred. *(R26)*
- [x] **A21** — No publication, checkpoint, review marker or merge is performed
  by this implementation worker. *Evidence:* action dossier and coordinator handoff. *(R27)*
- [ ] **A22** — Independent architecture and AI reviews close and full CI passes
  at the exact child head. *Evidence:* coordinator's durable PR review reports,
  finding disposition and exact-head CI URLs; pending at local handoff. The
  delivery PASS and PR-open parts of R28 are C1/C2 under the mission skill's
  closing-step rule, not a substitute completeness criterion. *(R28)*
- [x] **A23** — Evidence does not claim a score increment; reassessment stays with
  campaign coordination. *Evidence:* final evidence text. *(R30)*

## Injected gates

- [x] **G1** — English/encoding/language/context guards pass. *Evidence:* guard
  logs and final full workspace test log.
- [ ] **G2** — Four local checks pass on the final current campaign-based tree.
  *Evidence:* ordered exact-command logs.
- [x] **G3** — Performance impact is declared for every touched path as rare;
  no product hot-path changes. *Evidence:* evidence path-rate table.
- [ ] **G4** — Independent architecture review, including bug pass and full shape
  pass, closes every Blocker/Should-fix or records permitted deferrals in the PR.
  *Evidence:* coordinator's exact-head durable review report; pending at local handoff.

## Closing steps

- [ ] **C1** — Coordinator obtains independent architecture and delivery reviews
  over this committed archive and exact branch/base; delivery returns PASS.
- [ ] **C2** — Coordinator publishes the draft PR to `campaign/architecture-a`,
  obtains the independent AI review, publishes its report/findings and records
  completion only through the canonical stable-identity procedure.
- [ ] **C3** — Resolve all AI threads and rerun every stale review after fixes.
  Watch full exact-head CI until green; only then mark ready through the gate.
- [ ] **C4** — Coordinator verifies campaign authority/current base/head and
  serializes any authorized child merge. Human main merge remains excluded.

## Non-applicable gates

Hot-path benchmarks, UI harness/visual QA/trader UX, new-extension ports and
engine golden fixtures do not apply: this is developer workflow enforcement.
The docs-only shape waiver does not apply because shell code/tests change.
MT5 Python checks and dependency license checks apply only if their paths or
Cargo.lock change; those paths are excluded from this task. No such change is
planned or authorized. No child host goal or subagent is created.

## Request as received

The full coordinator assignment and issue body follow as attributed verbatim
quotations; D3 above is an authority reference, not a claimed original quotation.

Attributed verbatim quotation from the coordinator's task assignment:

> Own independent implementation Q1 under campaign #330; user D3 explicitly permits up to2 independent nonconflicting implementations, serialized campaign merges, no routine approvals. Issue https://github.com/milocaetano/quantick/issues/331. WT C:/src/quantick-worktrees/fix-ai-review-completion branch fix/ai-review-completion already created at current integrated a808b2d87b36d73041027e4d20c053544b454a96 origin/campaign/architecture-a; private mission-base and tier high set. Read AGENTS/CLAUDE, mission/new-task/ship, canonical AI-review and campaign integration, mission references/why when changing workflow. Authoritative issue body captured C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q1-start-issue.json; read Q1-investigation.md there. Scope: actual completed AI review attestation using existing exact-diff/campaign key, separate unchanged unresolved-thread gate; stable head/base/key/branch/status before and after review, durable PR report and private completion marker; ready/merge refuse absent/stale even zero threads and all tiers, draft creation stays; preserve every existing review/tier/authority/command limitation and main prohibition. Hermetic Claude+Codex tests missing/stale/current/findings/branch/worktree/base-tip/base-ref variants. Read full current code, no copied comparator/new generic subsystem. Known tension: raw diff key ignores branch name while issue explicitly requires task-branch invalidation; honor full issue with minimal branch-bound attestation while reusing existing review-key semantics, explain/prove it (do not silently weaken criterion).
> OWN .claude/hooks/guardrails.sh, guardrails_test.sh, campaign_context_test.sh, hooks README; canonical ai-review/ship/mission skill prose, docs/campaign/integration.md; unique .claude/GOAL-archive-ai-review-completion.md and docs/campaign/ai-review-completion-evidence.md. Exclude crates/Cargo manifests/lock, F1 unique mission/docs. No dependency on F1's unintegrated app code. If additional paths needed report overlap before expansion. F1 separate mission-only repair presently owns the build/test host: do read-only planning NOW, WAIT for coordinator release before arming builds or edits. Then arm guards build and app all-target check before first edit; create high-tier GOAL with exhaustive atomic R/A/G including every conditional obligation and verbatim request, preserve IDs/archive before review. All four checks in order before EVERY commit, shell guardrails suite and affected skill validators, raw log files saved outside repo with exact commands/exits. No weak tests, stale stamps, new exceptions or skipped gates. No new runtime/financial/public product contracts.
> Authorized implement, test, commit only after green; NO push/draft/markers/reviews/GitHub/checkpoints without coordinator publication-intent handoff. Return verified local head and full review dossier. Coordinator alone writes checkpoints, pushes/PRs and merges; notify before commit so intent recorded. If campaign base advances notify/coordinate update and rerun affected validation before reviews. Do not create child host goals or ask user for plan/command permission; never sandbox_permissions. Keep progress and failure signatures/counters, no retry reset. Read-only work can proceed while F1 verifies; no builds until released.

Attributed verbatim quotation from issue #331 as captured at task start:

> <!-- campaign-task:milocaetano/quantick#330/Q1 -->
> ## Context
>
> Parent: https://github.com/milocaetano/quantick/issues/330. Stable task Q1. At a808b2d87b36d73041027e4d20c053544b454a96, `ship/SKILL.md:40` acknowledges that zero open AI-review threads cannot distinguish a clean review from an AI review that never ran. Existing architecture/delivery markers bind the diff; the already-required AI review lacks equivalent completion evidence. This is verified score criterion AD4, not a request to change the rubric.
>
> ## Scope
>
> Use the shared exact-diff/campaign-base review key to attest completed AI review. Keep completion separate from its finding disposition: every existing unresolved-thread gate stays. The canonical AI-review workflow records a durable PR report and a private worktree completion projection only after checking reviewed HEAD/base/key stability. Both Codex and Claude shared hook paths reject absent or stale completion at readiness and campaign merge. Draft creation stays available to start the two-phase review. Preserve all other reviews, tier constraints, main authority and supported command limitations; no new skip or override path.
>
> ## Acceptance criteria
>
> - [ ] Missing AI-review completion denies readiness/merge even with zero open AI threads, at every tier.
> - [ ] A clean completed exact-diff AI review plus existing required evidence permits the supported gate, while unresolved threads still deny it.
> - [ ] Changed source diff, task branch, campaign base tip or target invalidates stale completion; unrelated worktrees cannot supply it.
> - [ ] Draft PR creation remains possible so the required two-phase workflow can begin.
> - [ ] Durable PR completion evidence records full head SHA, explicit base/ref tip and review key; local marker is only a projection of an actually completed review.
> - [ ] Hermetic regressions cover absent, stale, clean, open-thread and campaign-base variants, using the shared Claude/Codex hook route; document exact attestation producer/consumer in canonical instructions without duplicating policy.
> - [ ] Full local fmt/clippy/build/test before every commit and affected hook/skill/link/language/context checks pass; independent architecture, AI and delivery reviews plus full CI pass at the exact child head.
>
> ## Campaign record
>
> Owner class: autonomous. Priority: 3. Risk: high (authority/review guard). Estimated scope: shared hook/AI-review workflow and focused fixtures. Dependencies: none; scheduling waits while F1 owns the single implementation lane. Project item ID, owner, branch, worktree, PR URL/head: null until scheduled/read back. Base: latest origin/campaign/architecture-a; PR base exactly campaign/architecture-a. Final merge to main is human only. Validation rates: developer-command/rare; no product hot path or UI visual change. Evidence: this issue comments, committed mission archive, raw validation artifacts and linked PR. Retry counters: operation 0, repair 0 initially, cumulative thereafter. Source investigation: coordinator will attach Q1-investigation.md. Do not claim the score increment until reassessment.
>
> ## Current campaign projection
>
> Parent: https://github.com/milocaetano/quantick/issues/330. Task Q1: ready. Project: https://github.com/users/milocaetano/projects/3; item `PVTI_lAHOA0fkv84Bipkmzg5sN-4`. Project writes are available and the six-state machine field is synchronized. Earlier text describing unavailable Project access or null setup is historical and superseded by this readback.
> Owner: None. Branch: None. Worktree: None. PR/head: None / None. Base: origin/campaign/architecture-a. Current complete checkpoint: https://github.com/milocaetano/quantick/issues/330#issuecomment-5561940651; inspect later journal metadata for updates. Retry state: {"operation": 0, "repair": 0}; per-signature evidence remains in checkpoint and linked reports. Next action: start only when current implementation lane is free.
>
