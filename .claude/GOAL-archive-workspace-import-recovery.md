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
