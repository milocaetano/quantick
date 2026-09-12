# Refactor pull request #306 and leave it ready to merge

The mission matters because the existing feature branch is large, conflicts with current `main`, and must retain its documented deal-bar behavior while becoming maintainable and verifiably mergeable.

**Tier:** `high` — the existing pull request changes the engine, feeds, bridge protocol, application UI, persistence, control plane, generated contracts, and more than seven thousand lines, so it requires full interrogation, review, UI, determinism, and delivery gates.

## Request ledger

- **R1** — Refactor pull request #306.
- **R2** — Preserve the behavior and explicit trader decisions already documented by pull request #306.
- **R3** — Leave pull request #306 ready to merge.
- **R4** — Resolve every open AI-review comment and repeat the AI review before declaring the mission complete.

## Decisions taken by the trader

No new trader decision was required. The decisions already recorded in `.claude/GOAL-archive-trades-bars-b3.md` and the pull request body remain authoritative for feature behavior.

## Assumptions

- **S1** — "Refactor" means reducing structural concentration and resolving integration drift without redesigning the feature. This is safe because the pull request already contains a detailed behavioral contract and accepted deferrals.
- **S2** — The contaminated original worktree must be preserved. A clean integration worktree may use a temporary local branch and push its final head to the existing pull request branch because the remote pull request branch is the mission's delivery target.
- **S3** — Existing accepted follow-ups remain out of scope unless current `main` integration or a review exposes them as merge blockers. This prevents silently reopening settled trader decisions.
- **S4** — The final base refresh to `origin/main` commit `57767f25` changes only agentic workflow documentation and hooks; it does not alter the deal-bar implementation, but all final-head gates and reviews are repeated because it changes the reviewed diff identity.

## Acceptance criteria

- [x] **A1** — Pull request #306 is integrated with current `origin/main` with all conflicts resolved and no documented behavior lost.
  *Evidence:* merge commit, conflict-resolution diff, focused tests, and final PR mergeability.
  → `docs/evidence/pr-306-refactor/integration.md`. *(R1, R2, R3)*
- [x] **A2** — Structural concentration introduced by the pull request is reduced where repository reports, ratchets, or review identify an actionable ownership problem.
  *Evidence:* before/after guard reports and an ownership summary naming each refactored module boundary.
  → `docs/evidence/pr-306-refactor/refactor.md`. *(R1)*
- [x] **A3** — Existing deal-bar, recording, bridge, feed, UI, and control-plane behavior remains covered, with regression tests added for defects changed during this mission.
  *Evidence:* focused test commands and the final workspace test log.
  → `docs/evidence/pr-306-refactor/verification.md`. *(R2)*
- [ ] **A4** — Pull request #306 is open, reports mergeable against current `main`, has green CI, and its body records the final evidence and any accepted deferrals.
  *Evidence:* GitHub PR metadata, checks, and updated PR body.
  → pull request #306. *(R3)*
- [ ] **A5** — All five AI-review findings are fixed, their durable threads are resolved, and a follow-up AI review records no new FAIL or WEAK verdict.
  *Evidence:* focused regression tests, zero-thread readback, follow-up report URL, and `ai-review-complete` marker.
  → pull request #306 and the branch git dir marker. *(R1, R3, R4)*
- [x] **G1** — Every artifact authored by this mission is in English.
  *Evidence:* `quantick-guards` language scan and arch-review dimension 8.
  → `docs/evidence/pr-306-refactor/verification.md`.
- [x] **G2** — Every touched path has a declared performance rate; any semantic hot-path change is measured against a `main` control or an appropriate fixture benchmark.
  *Evidence:* performance classification and measurements or an explicit proof that edits are structural only.
  → `docs/evidence/pr-306-refactor/performance.md`.
- [x] **G3** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` all exit zero on the final head after integration with current `main`.
  *Evidence:* final-head command log.
  → `docs/evidence/pr-306-refactor/final-checks.log`.
- [ ] **G4** — Full high-tier `arch-review` finishes with every Blocker and Should-fix resolved or explicitly deferred in the pull request body.
  *Evidence:* final arch-review verdict and marker.
  → pull request #306 and the branch git dir marker.
- [ ] **G5** — Full `delivery-review` returns PASS over the archived mission and the final branch head.
  *Evidence:* final delivery-review verdict and marker.
  → pull request #306 and the branch git dir marker.
- [x] **G6** — Any user-visible behavior changed by the refactor remains hook-reachable and passes `visual-qa` and `trader-ux-review` with no unresolved Blocker.
  *Evidence:* UI harness inventory and review reports, or a diff-backed statement that no visible behavior changed.
  → `docs/evidence/pr-306-refactor/ui-validation.md`.
- [x] **G7** — Deal-bar output remains deterministic under fixed fixtures and live/rebuild equivalence tests.
  *Evidence:* named engine golden and equivalence tests.
  → `docs/evidence/pr-306-refactor/verification.md`.
- [x] **G8** — The deal-recording capability remains actionable, readable, and discoverable through the control-plane registry without requiring a mouse.
  *Evidence:* control-plane tests and generated-catalog consistency checks.
  → `docs/evidence/pr-306-refactor/verification.md`.

## Not applicable

- The `new-extension` workflow is not re-run as a new feature-design exercise: pull request #306 already added the capability and documented its ports, registration points, defaults, fake implementations, and blast radius. This mission preserves and reviews those boundaries.
- New engine behavior is not planned. Test-first fixtures apply if integration or review requires a semantic correction.
- Fresh screenshots are not required for purely structural changes. They become mandatory if any user-visible behavior or layout changes.

## Closing steps

- **C1** — `delivery-review` returns PASS over the final archived mission.
- **C2** — Pull request #306 is updated, open, mergeable, and green in GitHub.

## Request as received

> $mission refatorar https://github.com/milocaetano/quantick/pull/306 e deixar e deixar pronto para merge
>
> contiue corrigindo o pr para finlziar a missao
>
> resolva os comentario

The quotation above is an attributed verbatim request under the repository language-rule exemption.
