# Prove partial workspace import recovery

Prove the documented recovery from a later installation rename failure through the real workspace import orchestration, preserving cockpit, machine-local settings and paper-sidecar rules. This closes campaign #330 Q5 / issue #336's persistence-boundary evidence gap without inventing a group transaction.

**Tier:** high, because persistent settings replacement requires independent partial-failure and recovery oracles.

Branch: `feat/workspace-import-recovery`. Worktree: `C:/src/quantick-worktrees/feat-workspace-import-recovery`. Base: `origin/campaign/architecture-a`, start SHA `0bd50f9b815a05e2ba8d0c9804324dbb415f6658`. Coordinator owns remote operations and review markers; main merge belongs exclusively to the user.

## Request ledger

- R1: Close the missing persistence-boundary proof behind the documented re-import recovery instruction.
- R2: Expose the smallest private injectable rename operation used by real `apply` orchestration.
- R3: Keep the production adapter on the same standard filesystem rename operation.
- R4: Preserve validation, staging and install ordering; never substitute a copied test algorithm.
- R5: Preserve the existing `Result<Vec<&str>, String>` interface.
- R6: Preserve per-file atomicity.
- R7: Introduce no public test API.
- R8: Independently force failure on the second rename after a successful replacement.
- R9: Independently force failure on a later rename after successful replacements.
- R10: Use real owned scratch files and record operation order.
- R11: Distinguish injected rename failures from validation, staging and setup failures.
- R12: Prove changed and unchanged live groups through actual bytes/readback.
- R13: Prove existing staging artifact behavior after failure.
- R14: Prove the error's replacement count.
- R15: Use ordinary production re-import to complete the same bundle and resolve staging artifacts.
- R16: Prove recovery against independently specified cockpit values.
- R17: Prove repeating a successful import is consistent.
- R18: Include a registered foreign store without hardcoded production identities.
- R19: Prove invalid sections leave every live store unchanged.
- R20: Prove first-phase write failures leave every live store unchanged.
- R21: Preserve local machine keys through partial failure and recovery.
- R22: Preserve paper-sidecar bytes and exclusion through partial failure and recovery.
- R23: Preserve existing golden, round-trip, local-key, sidecar and store-registration tests.
- R24: Classify affected rates as rare imports and preserve operation counts/order.
- R25: Record added abstraction/runtime cost or zero-cost dispatch evidence without an unsupported speedup claim.
- R26: Commit reproducible evidence identifying the exact source SHA.
- R27: Run ordered fmt/clippy/build/workspace tests before every commit and guards after edits.
- R28: Archive the complete source-backed mission before independent reviews.
- R29: Preserve scoring criteria, tests and guard ceilings.
- R30: Keep edits within assigned owned paths; check overlap before any expansion.
- R31: Capture literal host release and UTC receipt before arming, verify target ownership, run guards build and app all-target check, then save mission before source edits.
- R32: Retain exact external logs and cumulative per-signature attempts; report failures before retry, maximum three per task/signature.
- R33: Return host immediately after arming, avoid cargo/measurement overlap and hand back to coordinator without a new user prompt or campaign closure.
- R34: Add no group-transaction guarantee.
- R35: Add no retry policy.
- R36: Add no user interaction.
- R37: Add no feature.
- R38: Add no application-root field.
- R39: Add no dependency.
- R40: Preserve existing error semantics.
- R41: Prove the error's total settings-group count.
- R42: Prove the error's failing path.
- R43: Preserve and prove the current recovery instruction.
- R44: Record independently specified fixture oracles in committed evidence.
- R45: Record per-step actual disk readbacks.
- R46: Record exact reproducible normal/failure/recovery commands.
- R47: Record actual command results.
- R48: Close independent architecture review before campaign merge.
- R49: Close independent AI review before campaign merge.
- R50: Obtain independent source-first delivery PASS before campaign merge.
- R51: Close exact-head CI before campaign merge.
- R52: Keep main merge exclusively human and campaign merge within explicit authority.
- R53: Use the assigned isolated worktree and branch.
- R54: Depend on no unintegrated implementation result.
- R55: Retain work and coordinate update/revalidation when the base advances.
- R56: Keep campaign merges serialized through the coordinator under D4.

## Decisions and assumptions

- D1: Coordinator delegation selects high tier, owned paths and local handoff. Coordinator retains remote writes, review markers and serialized campaign integration.
- D2: D4 permits up to three independent implementation missions; its attributed authority record is quoted below. Main merge remains human.
- D3: Explicit release at issue comment 5569083917 permits arming and requires immediate host return before Q3 measurements. Literal release and UTC receipt are saved externally.
- S1: A private generic `FnMut(&Path, &Path) -> io::Result<()>` is the narrow rename seam; the production capture-free closure calls `std::fs::rename`. This is a reversible implementation choice.
- S2: Separate failures at rename 2 and 4 of five included fixture stores cover changed prefixes and surviving staging suffixes.
- S3: An owned directory occupying a later `.importing` path deterministically fails the real stage write without platform-specific permission assumptions.
- S4: Expected recovered TOML uses independent test literals and ordinary serialization, never production check/apply/local-key merging.
- S5: Source work may proceed while Q3 measures; mandatory batch guards wait for explicit host return. No cargo process overlaps another lane's host ownership.
- S6: No question qualifies: scope, authority, financial boundaries and ownership are explicit; remaining decisions are reversible test mechanics.

## Acceptance criteria

Evidence document: `docs/quality/workspace-import-recovery-evidence.md`. Exact logs: `C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q5-validation/`.

- [x] **A1** — One private rename seam retains the real validation/staging/install body, std adapter, interface, errors and order. Evidence: final source diff and focused tests in the evidence document. *(R2, R3, R4, R5, R40)*
- [x] **A2** — No public API, root field, dependency, retry, transaction, user interaction or feature is added. Evidence: scoped diff audit in the evidence document. *(R6, R7, R34, R35, R36, R37, R38, R39)*
- [x] **A3** — Independent second/fourth rename fixtures record exact operations and unique errors after real replacements. Evidence: focused tests and external traces. *(R8, R9, R10, R11)*
- [x] **A4** — Every live group and staging file has expected bytes or absence at each install boundary and after failure. Evidence: per-step assertions and disk traces. *(R12, R13)*
- [x] **A5** — Both errors retain exact count, total, path and recovery instruction. Evidence: exact error assertions and logged errors. *(R14, R41, R42, R43)*
- [x] **A6** — Ordinary `apply` recovers the same bundle to independent expected values and clears staging artifacts. Evidence: recovery assertions and disk traces. *(R1, R15, R16)*
- [x] **A7** — Repeated ordinary success preserves recovered bytes and returned keys. Evidence: repeat-import assertions and traces. *(R17)*
- [x] **A8** — A foreign registered store participates without production identity branches. Evidence: fixture registry and readbacks. *(R18)*
- [x] **A9** — Invalid real sections refuse import before path resolution or staging; live bytes remain unchanged. Evidence: validation-failure test. *(R19)*
- [x] **A10** — A real later stage-write failure invokes no rename, preserves all live bytes and cleans earlier staging artifacts. Evidence: stage-failure test. *(R20)*
- [x] **A11** — All real workspace local keys and the foreign machine-local key survive failure, recovery and repeat. Evidence: independent values and readbacks. *(R21)*
- [x] **A12** — Sidecar bytes never change or stage, even with an explicitly supplied excluded section. Evidence: per-step exclusion assertions. *(R22)*
- [x] **A13** — Existing tests stay meaningful and pass; scoring criteria and guard ceilings remain unchanged. Evidence: diff audit and focused/full test logs. *(R23, R29)*
- [x] **A14** — Evidence declares rare rates, unchanged filesystem counts/order and static dispatch cost boundaries without unsupported timing claims. Evidence: source audit and traces. *(R24, R25)*
- [ ] **A15** — Committed evidence identifies exact tested base/final commit linkage, independent oracles, readbacks and commands/results. Evidence: document and raw logs. *(R26, R44, R45, R46, R47)*
- [ ] **A16** — Each commit follows passing ordered fmt/clippy/build/workspace tests; guards pass after edit batches. Evidence: dated exact logs. *(R27)*
- [ ] **A17** — Complete source-backed archive is in the reviewed diff; independent architecture/AI/source-first delivery and exact-head CI close before campaign merge. Evidence: archive and coordinator review/CI records. *(R28, R48, R49, R50, R51)*
- [ ] **A18** — Ownership remains disjoint and independent; base advances receive coordinated revalidation before serialized authorized integration. Evidence: Git audit and coordinator handoff. *(R30, R52, R53, R54, R55, R56)*
- [x] **A19** — Release/UTC receipt and target audit preceded passing arming checks, then mission persistence before source edits. Evidence: `host-release-receipt.md`, `01-arm-guards.log`, `02-arm-app-check.log`, mission write ordering. *(R31)*
- [x] **A20** — Logs/signature counters persist; failures are reported before retries and limits stay enforced. Evidence: external attempt ledger and handoff. *(R32)*
- [ ] **A21** — Host returned immediately after arming; later cargo waits for release and local completion returns to coordinator. Evidence: coordinator messages and final handoff. *(R33)*

## Injected gates

Provenance: canonical `.claude/skills/mission/SKILL.md`, step 4; `CLAUDE.md` verification/English rules; `docs/campaign/integration.md` replaces main with the declared campaign base.

- [ ] **G1** — Authored artifacts comply with the repository English rule. Evidence: guards and architecture review.
- [ ] **G2** — Ordered fmt/clippy/build/workspace tests pass on the latest declared campaign base before every commit. Evidence: exact logs and base audit.
- [ ] **G3** — Performance impact is declared and architecture review closes every Blocker/Should-fix. Evidence: rare-import dispatch analysis and review record.

## Not applicable

No engine algorithm, per-trade/depth/frame hot path, financial behavior, public capability or user-visible surface changes. Engine fixture-first, hot-path benchmarks, new-extension registration, new act/read/discover surfaces, UI hooks, visual QA and trader UX gates therefore do not apply. Existing UI text and behavior stay identical. Python, shell guardrail, Cargo-lock/license and CI-registration changes are outside this mission. Full architecture and delivery reviews remain required.

## Closing steps

- C1: Coordinator obtains independent full source-first delivery PASS, AI completion with zero unresolved findings and exact-head green CI after architecture findings close.
- C2: Coordinator creates/delivers the draft PR into `campaign/architecture-a`, then applies existing readiness and explicitly authorized serialized campaign merge gates. Main merge remains human.
- C3: Return local code, archive, logs and attempt counters to coordinator without a new user prompt or campaign closure.

## Plan and progress

Arming passed before mission creation: guards build 4.84s; app all-target check 1m39s, both in a fresh worktree-local target. The complete mission and verbatim source quotations were verified at 2026-09-07T10:23:10.2476254Z with an empty source diff; its then-current blob SHA was `aa03c9a6e742bcab1809126870e9713d2bc22dbe`. Coordinator received confirmation before the first source edit. External `mission-before-source-proof.md` preserves the observed sequence.

Focused verification now passes: batch/post-format guards each 172 tests; new recovery tests 5/5; all workspace bundle tests 20/20, including 15 unchanged originals; separate normal/second/fourth cases each 1/1 with readable disk transcripts. Exact tested source blobs are recorded in the evidence document. Host returned immediately after focused results. This archive is prepared before independent reviews.

The first full ordered loop passed guards/fmt/clippy/build but failed the unchanged observer capture budget: best median 258 us versus 250 us, app 1912 passed/1 failed/4 ignored. Every workspace bundle test passed. Raw output remains in `13-full-workspace-tests.log`; failure occurrence 1, production repair 0. Source blobs and thresholds remain unchanged. The exact existing isolated diagnostic then passed with medians 79/81/79 us (`14-observer-isolated-diagnostic.log`); no cause is inferred and isolation is not a full-suite substitute.

Coordinator release at campaign comment 5569851037 authorizes final guards and one unchanged full ordered retry after this documentation update. Exact logs `15-retry1-guards.log` through `19-retry1-workspace-tests.log` and the frozen-tree handoff provide its actual outcomes. No successful retry is claimed before execution; all mandatory checks must exit 0 before commit. Independent review/CI completion remains coordinator-owned before integration. Earlier tool failures and counters remain in `attempt-ledger.md`; no counter reset or production repair occurred.

## Request as received: complete attributed sources

The complete source quotations follow. Current issue capture supersedes the older Q5-spec.md D3 wording. Coordinator messages are delegation sources, not fabricated direct user quotations. D3/D4 records identify their own translation provenance.


### Attributed source: issue

```text
test(app): prove partial workspace import recovery

<!-- campaign-task:milocaetano/quantick#330/Q5 -->
## Context

Parent: https://github.com/milocaetano/quantick/issues/330. Stable key Q5. At campaign SHA c3a92d58bb8a41ec4d78d73e60312b5f765b4da5, `crates/app/src/workspace_bundle.rs:191-236` validates/stages all sections and then renames each file individually. Its documented residual rename failure reports the number replaced and asks the user to reopen the bundle, but maintained tests do not force a second/later rename failure and prove that recovery. Unchanged quantick-score v1.0 SE8 remains4/5. Existing #318/F2 concerns projection ownership and is not this missing persistence-boundary proof; #319 is an earlier broad architecture tracker.

## Scope

Expose the smallest private filesystem-operation seam needed to deterministically fail an actual installation rename, while the production `apply` path continues using the same real filesystem operations, order, data and errors. Exercise that same orchestration through a faulting test adapter, then read actual files and successfully re-import. Preserve existing `Result<Vec<&str>, String>` semantics, per-file atomicity rather than group transactionality, store registration/validation, local-key preservation, paper-sidecar exclusion, temp-file behavior and public/financial contracts. No new transactional guarantee, retry policy, user interaction or feature. No test-only implementation substituted for production orchestration.

## Acceptance criteria

- [ ] The real apply orchestration uses a narrow injectable operation at the rename boundary; the live adapter delegates to the same std filesystem operation. The failure fixture cannot bypass production validation/staging/install ordering. No public test API, copied apply algorithm, new application-root field or dependency is introduced.
- [ ] Independent fixtures force failure on the second and a later rename after at least one successful replacement, with real owned scratch files and recorded operation order. Assertions distinguish the injected rename failure from a staging/validation/setup error.
- [ ] Actual persisted bytes/readback prove exactly which settings groups changed, which retained their original values and which staging files remain under the existing semantics. The returned existing error reports the correct replacement count, total and failing path and retains the current recovery instruction. Do not fabricate an all-or-nothing guarantee.
- [ ] A subsequent ordinary production re-import of the same bundle completes every intended section, removes/replaces staging artifacts as current behavior dictates and produces the independently specified recovered cockpit. Repeating a successful import remains consistent. Include a registered foreign/fake store to ensure the seam does not hardcode current store identities.
- [ ] Invalid sections and first-phase write failures still leave every live store unchanged; existing workspace golden/round-trip/local-key/paper-sidecar and store-registration tests remain meaningful and pass. Local machine keys and sidecar bytes remain unchanged across partial failure and recovery.
- [ ] Declare affected rates as rare import operations; preserve operation counts/order and record actual extra abstraction/runtime cost or zero-cost dispatch evidence. No hot-path benchmark or speedup claim without a touched hot path.
- [ ] Commit a reproducible evidence record with exact SHA, deterministic fixture oracles, per-step disk readbacks and normal/failure/recovery commands and results. Full ordered fmt/clippy/build/workspace-test passes before every commit, guards after edits, and independent architecture/AI/source-first delivery plus exact-head CI close before campaign merge. Preserve all scoring criteria, tests and guard ceilings.

## Campaign record

Owner class:autonomous. Priority:7. Risk:high (persistence failure/refactoring). Estimated scope: workspace_bundle owner and focused test support/fixtures plus unique mission/evidence. Dependencies:none; latest integrated campaign base required. D4 (https://github.com/milocaetano/quantick/issues/330#issuecomment-5568580109) permits up to three independent disjoint implementations, each owned branch/worktree; coordinator serializes campaign merges and revalidates affected branches after base advance. Main merge exclusively human. Owner `codex/01a07841-8868-75a1-b6f6-9cb93837d72d/implement_q5`; branch `feat/workspace-import-recovery`; worktree `C:/src/quantick-worktrees/feat-workspace-import-recovery`; start SHA `0bd50f9b815a05e2ba8d0c9804324dbb415f6658`; Project item `PVTI_lAHOA0fkv84Bipkmzg5uV1I`. PR pending validation. Owned paths: `crates/app/src/workspace_bundle.rs`, `crates/app/src/workspace_bundle/recovery_tests.rs`, `docs/quality/workspace-import-recovery-evidence.md`, `.claude/GOAL-archive-workspace-import-recovery.md`.

Evidence:this issue, committed archive/evidence, raw verification logs and linked PR/CI. Initial counters operation0/repair0; retain per-signature history and campaign retry limits. This fills a verified evidence gap; completion only earns score credit after reassessment at the integrated SHA under unchanged quantick-score v1.0.
```


### Attributed source: delegation

```text
Implement campaign #330 Q5 (issue https://github.com/milocaetano/quantick/issues/336): prove partial workspace import recovery through the real production orchestration, preserving behavior/public/financial rules. Your existing isolated worktree is C:/src/quantick-worktrees/feat-workspace-import-recovery; branch feat/workspace-import-recovery; start/latest campaign SHA 0bd50f9b815a05e2ba8d0c9804324dbb415f6658. Claimed Project/issue and private high tier/mission-base already set. Read current AGENTS.md, CLAUDE.md, canonical mission/new-task/ship and campaign integration, Codex mappings. Apply high tier. User authorizes implementation, testing, commits, delegation and campaign merges, up to THREE independent implementation missions via D4 https://github.com/milocaetano/quantick/issues/330#issuecomment-5568580109. Main merge exclusively user. No routine questions. Coordinator owns remote writes, checkpoints, review markers, draft PR then independent reviews/CI then serialized authorized campaign merge; send local handoff.

Scope source is exact current issue, saved C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q5-start-issue.json and Q5-spec.md. Owned paths only crates/app/src/workspace_bundle.rs, crates/app/src/workspace_bundle/recovery_tests.rs, docs/quality/workspace-import-recovery-evidence.md, .claude/GOAL-archive-workspace-import-recovery.md (live .claude/GOAL.md during mission). Q3 owns indicator worker/pane/control_plane_tests; Q4 discovery/Windows CI; Q2 separate schema tests is stalled. No unintegrated dependencies, root fields, public API or Cargo changes. Other paths require coordinator overlap check. Tests must exercise exact production validation/staging/install sequence, force second and later rename failures, assert disk/temp/count/error/recovery/repeat/foreign store/local keys/sidecar behavior; no invented transactional guarantee or copied apply algorithm. Preserve existing tests and thresholds.

START READ-ONLY PREPARATION NOW: Q4 currently owns build/test host, Q3 next for measurements. Do not arm, run cargo, create mission or edit repository until explicit coordinator host release. You may inspect/read and write external scratch preparation now. Save plan in Q5-validation/preparation.md, then report preparation + host need; don't spawn reviewers. Once released, save literal release and UTC receipt before arming, run guards build and app all-target check, create mission BEFORE implementation with complete verbatim issue and delegation sources, atomic R ledger and A mapping, then implement. Full ordered fmt/clippy/build/test before every commit, guards after edits. Archive before independent review. Report failures and cumulative per-signature attempts BEFORE retries (max3 per task/signature, no resetting); retain exact commands/output in external Q5-validation. No target cache reuse without checking ownership; no tests/builds overlap Q3 measurements. If base advances, retain work and coordinate update/revalidation. Do not end campaign or give user a new prompt; return to coordinator for next work.
```


### Attributed source: release

```text
UTC receipt: 2026-09-07T10:15:01.3287164Z

Q3 has finished its focused lane tests and candidate desktop build, all owned cargo has exited, no GUI process is running, and candidate timing has not begun. Q5 may acquire the build/test host NOW. Save this literal release and UTC receipt BEFORE arming; verify target ownership, build guards and check app all targets, then persist the complete mission BEFORE any source edit. Release the host to coordinator after arming so Q3 can measure while Q5 implements. Report failures with cumulative signatures before retries; no routine user permission is needed. Coordinator owns remote writes and draft-first independent delivery through authorized serialized campaign merge. Main merge remains human. Durable release https://github.com/milocaetano/quantick/issues/336#issuecomment-5569083917; checkpoint5569084600. Q5-host-release-source.txt also retained. Continue preparation in your existing WT, complete attributed source capture and atomic ledger, no historical action guesses. Return host immediately after arming; send mission saved confirmation before first source edit.
```


### Attributed source: D3

```text
## Authority update D3: conditional implementation concurrency

Source: authenticated user's concurrency update in the active Codex campaign session, after returning from shutdown. The following is an attributed English translation of the user's instruction:

> You may execute up to two implementation missions in parallel after verifying that they do not depend on unintegrated results and that their changes do not conflict. Each mission must have its own branch, worktree and responsibilities. Keep campaign-branch merges serialized, and update and revalidate affected PRs when the base advances. Decide when to parallelize without asking approval for each pair. Continue sequentially when independence is insufficient. Main merge remains exclusively mine; campaign-branch merge belongs to the coordinator.

This supersedes D1's initial one-implementation concurrency limit only. All existing scope, behavioral preservation, financial rules, tests, review gates, retry counters, campaign-only merge authority and exclusions remain binding. Maximum active implementation missions: two. Benchmark/build-host access is coordinated to avoid contaminating measurements or accidental shared-target contention.

Initial scheduling candidate after F1 integration: Q1 (#331), review completion enforcement in shared hooks and canonical workflow documentation; Q2 (#333), released schema compatibility fixtures/tests. They have no unintegrated implementation dependency on each other. Before claiming both, verify their actual file ownership and registration edits; any overlap is serialized. F1 remains in delivery review with green exact-head CI; this update does not bypass its remaining gate.
```


### Attributed source: D4

```text
## Decision D4: up to three independent implementation missions

Source: authenticated user in the active campaign Codex session on 2026-09-07. The user asked to add one more parallel mission to the existing two-mission authorization. English translation: "I think we can work on one more in parallel."

The coordinator may run up to three independent implementation missions with separate branches, worktrees and verified nonconflicting ownership, with no dependency on unintegrated results. D3's remaining conditions continue: serialize campaign merges and update/revalidate affected PRs when the integration base advances. Build/benchmark host use stays serialized so CPU contention does not contaminate timing evidence. Main merge/push/auto-merge/queue/protection restrictions and all other scope/review/retry/paid-service limits remain unchanged. This records the actual user's steering, not authority inferred from issue prose.

Initial lanes: Q2 released-schema compatibility, Q3 incremental live-lane transport, Q4 Windows local-transport authority tests. Q4 owns control-local test paths and Windows CI registration; Q2/Q3 do not edit those paths. If that independence fails, schedule the affected work sequentially.
```

## Completeness repair 1: appended obligations

The independent completeness-only report for HEAD
`7485184d6176a1129b0467824320c9bdf8cfb63e` found 19 missing obligation fragments
among 76 source-derived asks. Report:
https://github.com/milocaetano/quantick/pull/343#issuecomment-5570288878.
Repair journal:
https://github.com/milocaetano/quantick/issues/330#issuecomment-5570290210.
This appendix adds the missing ledger and criterion mappings without changing
R1-R56, A1-A21, G1-G3, C1-C3 or any original attributed source. Nothing is
withdrawn, renumbered, waived or newly claimed delivered. The original 76-ask
derivation remains immutable; its S identifiers below are source-ask IDs, not
new mission assumptions. Independent review must assess the complete archive.

### Request ledger additions

- R57: Preserve all existing public contracts. *(Source S11: issue Scope, "public/financial contracts".)*
- R58: Preserve all existing financial contracts. *(Source S11: issue Scope, "public/financial contracts".)*
- R59: Introduce no hot-path benchmark or speedup claim without a touched hot path. *(Source S36: issue acceptance criterion 6.)*
- R60: Start and validate against the latest integrated campaign base; retain R55's coordinated update/revalidation when that base advances. *(Source S44: issue Campaign record.)*
- R61: Claim completion score credit only after reassessment at the integrated SHA under unchanged quantick-score v1.0. *(Source S46: issue Campaign record.)*
- R62: Read current AGENTS.md, CLAUDE.md, canonical mission/new-task/ship, campaign integration and Codex mappings. *(Source S47: delegation paragraph 1.)*
- R63: Apply high tier and its required gates. *(Source S48: delegation paragraph 1.)*
- R64: Ask no routine questions throughout execution. *(Source S49: delegation paragraph 1.)*
- R65: Leave remote writes, checkpoints and review markers exclusively to the coordinator. *(Source S50: delegation paragraph 1.)*
- R66: Preserve the coordinator-owned sequence: draft PR, then independent reviews and CI, then serialized authorized campaign merge; send local handoff under R33. *(Source S50: delegation paragraph 1.)*
- R67: Introduce no public API of any kind, retaining R7's prohibition of a public test API. *(Source S53: delegation paragraph 2.)*
- R68: Make no Cargo changes. *(Source S53: delegation paragraph 2; R38, R39 and R54 retain its root-field, dependency and unintegrated-result restrictions.)*
- R69: Begin with read-only preparation; do not arm, run Cargo, create the mission or edit the repository until explicit coordinator host release. *(Source S54: delegation paragraph 3.)*
- R70: Save the preparation plan in external `Q5-validation/preparation.md`. *(Source S55: delegation paragraph 3.)*
- R71: After saving the preparation plan, report preparation and host need to the coordinator. *(Source S55: delegation paragraph 3.)*
- R72: Do not spawn reviewers as the delegated implementer; independent review remains coordinator-owned. *(Source S56: delegation paragraph 3.)*
- R73: Check target-cache ownership before any reuse, including reuse after initial arming. *(Source S63: delegation paragraph 3.)*
- R74: Send the coordinator mission-saved confirmation before the first source edit. *(Source S68: release paragraph 1.)*
- R75: Make no guesses about historical actions; retain complete attributed source capture and the atomic ledger under R28, and exact logs under R32. *(Source S69: release paragraph 1.)*
- R76: Schedule affected implementation work sequentially whenever independence is insufficient or fails. *(Source S72: D3 and D4.)*
- R77: Preserve D4's ceiling of up to three independent implementation missions under the coordinator, with separate branches/worktrees and verified nonconflicting ownership and no unintegrated dependencies. *(Source S74: D4; R30, R53 and R54 retain the independence conditions.)*
- R78: Serialize all build/benchmark host use, including build/build contention, so CPU contention cannot contaminate timing evidence. *(Source S75: D4.)*
- R79: Preserve the inherited main merge, push, auto-merge, merge-queue and protection restrictions: main merge is exclusively the user's action; no agent main push, auto-merge, queue, protection change or bypass. *(Source S76: D4; campaign integration contract states the named main boundaries.)*
- R80: Retain all inherited scope, behavioral-preservation, financial, test, review, retry, paid-service, campaign-only authority and exclusion limits unchanged. Refer to the actual governing authority for any detail absent from the captured D3/D4 text; invent neither a limit nor permission. *(Source S76: D3 paragraph 2 and D4 paragraph 2.)*

### Acceptance criteria additions

These unchecked criteria specify evidence for later independent grading. Their
addition is not an author grade. Historical assertions need actual records;
preparation prose, the original progress narrative, receipt metadata without
message content and this appendix cannot substitute for missing execution or
message evidence. Unknown historical facts remain UNPROVEN. Coordinator
closing outcomes remain future obligations until their actual evidence exists.

External evidence root for the following paths:
`C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/`.

- [ ] **A22** — Existing public and financial contracts are preserved. Evidence: base-to-reviewed-head source diff and unchanged public/financial behavior audit, retained with `Q5-validation/repair1-mapping.md`; independent architecture review verifies it. *(R57, R58)*
- [ ] **A23** — No hot-path benchmark or speedup claim is introduced without a touched hot path. Evidence: source/evidence diff and rare-import cost record, indexed by `Q5-validation/repair1-mapping.md`. *(R59)*
- [ ] **A24** — Start and validation use the latest integrated campaign base, with coordinated update/revalidation after any advance. Evidence: actual initial Git commands and current local/remote base readbacks, in `Q5-action-audit/808309e2f8a3e240/` and coordinator validation records under `Q5-validation/`. *(R60)*
- [ ] **A25** — No completion score credit is claimed before integrated-SHA reassessment under unchanged quantick-score v1.0. Evidence: coordinator score-claim records and, when available, the integrated-SHA scorecard; future reassessment is C6. Destination: campaign #330 and `Q5-validation/repair1-mapping.md`. *(R61)*
- [ ] **A26** — Actual read records cover every workflow document named in R62. Evidence: completed command events in `Q5-action-audit/808309e2f8a3e240/execution-events.jsonl`, indexed by `Q5-validation/repair1-mapping.md`; `preparation.md` alone is not proof of those reads. *(R62)*
- [ ] **A27** — The mission retains high tier and its applicable gates, including the branch-bound tier record. Evidence: archived tier declaration, actual tier-file write/read records and coordinator gate evidence, indexed by `Q5-validation/repair1-mapping.md`. *(R63)*
- [ ] **A28** — Execution contains no routine questions. Evidence: actual implementer/coordinator interaction records for the delegated interval; tool-call names or receipt times alone cannot establish missing message content. Destination: `Q5-validation/repair1-mapping.md`, with any evidence gap explicitly unproven. *(R64)*
- [ ] **A29** — Remote writes, checkpoints and review-marker operations remain coordinator-exclusive. Evidence: actor-attributed operation records and GitHub readbacks, indexed by `Q5-validation/repair1-mapping.md` and the campaign journal. *(R65)*
- [ ] **A30** — The recorded delivery plan assigns the draft-before-review/CI-before-merge sequence to the coordinator and requires local handoff. Evidence: C1-C5 and the attributed delegation in this archive; execution of the remaining closing sequence is C4-C5, not claimed here. *(R66, R33)*
- [ ] **A31** — The diff adds no public API and changes no Cargo file. Evidence: complete base-to-reviewed-head diff and tracked-file hashes in `Q5-validation/repair1-before.json` and `repair1-after.json`, indexed by `repair1-mapping.md`. *(R67, R68)*
- [ ] **A32** — Preparation remained read-only until explicit host release, before any arming, Cargo, mission creation or repository edit. Evidence: actual completed command/file-change records and attributable release content in the action audit and `Q5-validation/host-release-receipt.md`; receipt metadata alone does not prove message content. *(R69)*
- [ ] **A33** — The preparation plan exists at the specified external path. Evidence: `Q5-validation/preparation.md`, its actual write event and preserved hash in `repair1-before.json`. *(R70)*
- [ ] **A34** — The preparation-and-host-need report was sent after saving the plan. Evidence: the actual saved-plan event and actual coordinator-directed report, indexed by `Q5-validation/repair1-mapping.md`; chronology is unproven if the report content is unavailable. *(R71)*
- [ ] **A35** — The delegated implementer spawned no reviewers. Evidence: actual tool-call/execution coverage for that delegated phase in `Q5-action-audit/808309e2f8a3e240/`, with its coverage limits retained in `Q5-validation/repair1-mapping.md`. *(R72)*
- [ ] **A36** — Ownership was checked before every target-cache reuse, not only initial arming. Evidence: actual ownership probes ordered before each applicable Cargo invocation, indexed from the action audit and later validation records in `Q5-validation/repair1-mapping.md`; uncovered reuse remains unproven. *(R73)*
- [ ] **A37** — The coordinator received mission-saved confirmation before the first source edit. Evidence: actual sent confirmation and first file-change event, indexed by `Q5-validation/repair1-mapping.md`; `mission-before-source-proof.md` identifies the claim but does not replace missing message evidence. *(R74)*
- [ ] **A38** — Historical-action claims cite actual records, complete attributed source quotations and an atomic ledger are retained, and missing facts remain explicitly unproven. Evidence: source hashes, action-audit provenance and identified evidence gaps in `Q5-validation/repair1-mapping.md`. *(R75, R28, R32)*
- [ ] **A39** — Work with insufficient independence is scheduled sequentially. Evidence: actual ownership/dependency checks and scheduling decisions, including any observed fallback, in campaign #330 coordinator records; do not invent a triggering event if none occurred. Index: `Q5-validation/repair1-mapping.md`. *(R76)*
- [ ] **A40** — Coordinator scheduling stays within three independent implementation missions with the stated separation and independence conditions. Evidence: D4 plus actual lane/ownership records in campaign #330, indexed by `Q5-validation/repair1-mapping.md`. *(R77)*
- [ ] **A41** — All build/benchmark host use is serialized across lanes, including build/build use. Evidence: actual host grants, returns and execution intervals from every applicable lane in coordinator records, indexed by `Q5-validation/repair1-mapping.md`; one lane's log alone cannot prove global non-overlap. *(R78)*
- [ ] **A42** — Main merge/push/auto-merge/queue/protection restrictions remain intact. Evidence: actor-attributed operation and GitHub state records against the integration contract, indexed by `Q5-validation/repair1-mapping.md` and coordinator campaign records. *(R79)*
- [ ] **A43** — All inherited limits in R80 remain unchanged, with no invented paid-service permission or undisclosed-limit detail. Evidence: governing source records, scoped diff and actual operation audit in `Q5-validation/repair1-mapping.md` and campaign #330; unspecified details are not treated as granted. *(R80)*

### Closing steps additions

- C4: The coordinator preserves the draft-first sequence, obtains fresh independent architecture/AI/source-first delivery and exact-head CI evidence for the repaired head, then applies readiness and serialized authorized campaign merge gates. No future review, CI or merge outcome is asserted by this repair. *(R48-R52, R56, R65, R66)*
- C5: The coordinator records actual campaign integration readback and completes the existing handoff/checkpoint duties while retaining exclusive remote-write and review-marker ownership. Revalidate affected branches if the campaign base advances. Main delivery remains subject to the human-only integration contract. *(R33, R52, R55, R56, R65, R66, R79)*
- C6: After campaign integration, reassess the integrated SHA under unchanged quantick-score v1.0 before assigning completion score credit. Keep campaign-candidate evidence distinct from any main-completion requirement. *(R61)*

### Source mapping and retained evidence

| Missing source fragment | Appended R entries | Appended A entries | Future closing step |
| --- | --- | --- | --- |
| S11: all public/financial contracts | R57, R58 | A22 | — |
| S36: benchmark and speedup prohibition | R59 | A23 | — |
| S44: latest integrated base | R60 | A24 | C5 |
| S46: integrated-SHA score reassessment | R61 | A25 | C6 |
| S47: complete workflow-read list | R62 | A26 | — |
| S48: high tier | R63 | A27 | — |
| S49: no routine questions | R64 | A28 | — |
| S50: coordinator ownership and delivery sequence | R65, R66 | A29, A30 | C4, C5 |
| S53: any public API and Cargo prohibition | R67, R68 | A31 | — |
| S54: initial read-only and full release gate | R69 | A32 | — |
| S55: preparation file and subsequent report | R70, R71 | A33, A34 | — |
| S56: no implementer-spawned reviewers | R72 | A35 | C4 |
| S63: every target-cache reuse | R73 | A36 | — |
| S68: sent mission-saved confirmation | R74 | A37 | — |
| S69: no historical-action guesses | R75 | A38 | — |
| S72: sequential fallback | R76 | A39 | — |
| S74: maximum three independent missions | R77 | A40 | — |
| S75: all build/benchmark serialization | R78 | A41 | — |
| S76: inherited main and service restrictions | R79, R80 | A42, A43 | C5 |

`Q5-validation/repair1-mapping.md` preserves the full 76-ask mapping, exact
source/report hashes and evidence destinations. `repair1-before.json`,
`repair1-after.json`, `repair1-archive-before.bin` and `repair1.patch` preserve
the edit boundary and raw patch. The original source-derived-asks SHA256 is
`3b92c49bc96b43460da8db522f02221e4e7164c3d908b8ecc70d1b59cbb1a4e2`.

Retry history remains cumulative. Original observer failure: median 258 us
against unchanged 250 us; isolated diagnostic subsequently passed, followed
by full ordered retry 1 with 3463 passed, 0 failed and 12 ignored. Those prior
passes do not validate this appended archive. Earlier mission-write/tooling
failures and repairs remain in `Q5-validation/attempt-ledger.md`; no signature
is reset. The first completeness failure identified 19 fragments; this is
completeness repair 1, awaiting fresh independent review after validation and
commit. Guards and the full ordered four-command loop remain required before
that commit, under exclusive coordinator host release.

## Completeness repair 2: explicit criterion mapping

- R81: Before implementation, create the mission with an atomic request ledger and an explicit mapping from each request to its acceptance criteria. *(Source: original delegation, "atomic R ledger and A mapping".)*
- [ ] **A44** — The mission persisted before the first implementation edit contains an explicit R-to-A mapping. Evidence: `Q5-validation/mission-before-source-proof.md`, its original mission blob and the corresponding objective command/file-change records in `Q5-action-audit/808309e2f8a3e240`. Those original artifacts require independent inspection; this appended statement does not manufacture historical compliance. *(R81)*

The clean-source follow-up closed all 19 original omissions and identified this
one explicit mapping obligation, which the first review had accepted implicitly.
Both readings remain recorded. This is completeness repair 2, with no change to
the original source, existing IDs, implementation, tests, or prior evidence.


## Deferred

These narrowly scoped retrospective process exceptions implement the authenticated user's [standing process-resolution instruction](https://github.com/milocaetano/quantick/issues/330#issuecomment-5584842294), granted after the Q2-Q5 blockers were disclosed. The user delegated their resolution without repeated approval prompts. The coordinator records the specified historical exceptions under that instruction; it does not claim the original actions were delivered. Original request and criterion IDs, failure signatures, counters and adverse evidence remain retained.

- **A28 / R64 - complete historical interaction coverage proving no routine questions.** Historical grade remains UNPROVEN; tool names and receipt times do not prove missing bodies.
- **A34 / R71 - original preparation-and-host-need report.** Historical grade remains UNPROVEN; the saved plan proves preparation, not transmission.
- **A37 / R74 - original mission-saved confirmation receipt.** Historical grade remains UNPROVEN; actual mission persistence before the first source edit remains proven separately and is not relabeled as communication evidence.
- **A43 / R80 - complete historical coordinator/implementer operation coverage only.** Historical grade remains UNPROVEN where retained audits are incomplete. This exception does not grant paid services, relax actual authority/service/retry limits, or waive any observed violation. Current scope and financial/public-contract preservation remain mandatory.

All current product requirements, deterministic behavior, public/financial contracts, independent reviews, tests, exact-head CI and score criteria remain unchanged. The linked R obligations are exempt only to the extent discharged by the historical portions listed here; their other current/observable portions still require proof. Missing encrypted/private originals are not reconstructed from authored quotations.

The recovery applies these recorded exceptions and validates the latest campaign integration through one bounded repair attempt for the historical-process signature, preserving the previous attempts. Further actual failures retain their own existing finite per-signature limits; the same unavailable-history search is not repeated. Only reviewed green intermediate PRs may merge into `campaign/architecture-a`. Main merge remains exclusively the user's action.
