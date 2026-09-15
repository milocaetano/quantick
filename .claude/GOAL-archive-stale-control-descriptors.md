# Prevent stale control descriptors from hiding the live instance

Prevent stale control descriptors from delaying or hiding live Quantick instances by cleaning owned descriptors, pruning dead or identity-mismatched publishers, and ranking discovery candidates newest-first.

This matters because accumulated descriptors from dead processes can consume the discovery cap and multiply connection timeouts until the running application is slow to find or invisible.

**Tier:** medium — the change is confined to local descriptor lifecycle and discovery and requires no product or safety decision, but its 301 changed lines exceed the repository's 300-line small-tier exemption ceiling.

## Request ledger

- **R1** — Discovery must not let the 64-entry cap exclude a live instance; order candidates by newest `published_at` before applying the cap.
- **R2** — Publishing and discovery must prune descriptors whose `process_id` is dead or whose available process identity does not match the descriptor.
- **R3** — The application publisher must remove its own descriptor on clean exit.
- **R4** — Liveness checking must be injected so control-local tests are headless and deterministic.
- **R5** — Regression coverage must prove that 100 stale descriptors plus one live descriptor returns the live instance first without stale connection attempts, and that clean exit leaves no descriptor.
- **R6** — The resulting discovery path must prevent stale state from making `quantick_describe` take minutes or miss the live application.

## Decisions

None. No medium-tier question qualifies because the source and existing control-plane contract settle the process identity, cleanup, ordering, and test requirements.

## Assumptions

- **S1** — “Fast” means stale candidates are rejected by the injected liveness probe before transport connection attempts. This is safe because it directly removes the reported timeout multiplier and is observable in a deterministic test.
- **S2** — Process identity will use the strongest portable evidence already represented by the descriptor and supported by the platform layer. This is safe because repository inspection determines the existing contract without changing the requested outcome.
- **S3** — Descriptor discovery and publication are rare startup or on-demand paths, not per-trade, per-depth, or per-frame paths. This is safe because their call sites define their execution rate.

## Acceptance criteria

- [x] **A1** — Discovery sorts valid candidates by descending `published_at` before applying its cap and returns the live descriptor first in a fixture containing 100 stale descriptors and one live descriptor.
      *Evidence:* named control-local regression test and passing targeted test output summarized in the PR.
      → PR mission summary and CI test results. *(R1, R5, R6)*
- [x] **A2** — Both publication and discovery prune dead or identity-mismatched descriptors through an injected liveness probe, without attempting transport connections to rejected descriptors.
      *Evidence:* control-local unit tests with a deterministic fake probe and inspected call-count assertions.
      → PR mission summary and AI/architecture review reports. *(R2, R4, R5, R6)*
- [x] **A3** — Clean publisher shutdown removes exactly the descriptor owned by that publisher and leaves no owned descriptor behind.
      *Evidence:* named publisher lifecycle regression test and passing targeted test output.
      → PR mission summary and CI test results. *(R3, R5)*
- [x] **G1** — Every authored artifact is English under `CLAUDE.md`.
      *Evidence:* language guard and architecture-review dimension 8.
      → CI and durable architecture-review report.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and `cargo test --workspace` pass after rebasing on latest `main`.
      *Evidence:* local final-head command results and exact-head CI.
      → PR mission summary and CI checks.
- [x] **G3** — Performance impact is declared for every touched path: publication is rare and discovery is startup/on-demand; stale discovery work becomes bounded local liveness probing before connection.
      *Evidence:* call-site inspection, regression connection-attempt assertions, and PR performance declaration.
      → PR mission summary.
- [x] **G4** — Architecture review runs at the small-tier bug-pass depth and resolves or explicitly defers every Blocker or Should-fix finding.
      *Evidence:* current-key durable PASS report and `arch-review-ok` projection.
      → PR architecture-review report.

<!-- required-ai-review-goal-gates:v1 -->
- [x] **G-AI1** — AI review is executed for the current PR review.
- [x] **G-AI2** — A durable AI-review report is published on the PR.
- [x] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [x] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

## Not applicable

- UI gates do not apply because no visible surface changes.
- Hot-path measurement does not apply because descriptor publication is rare and discovery is startup/on-demand rather than per-trade, per-depth, or per-frame.
- Extension gates do not apply because no capability, feed, bar type, indicator, layer, panel, or crate is added.
- Second-operator gates do not apply because no new trader action or tool is added.
- Engine determinism gates do not apply because the engine and bar construction are untouched.
- MT5 and Python checks do not apply because neither folder nor Python source is touched.

## Closing steps

- [x] **C1** — Publish an open draft PR with the concise mission summary and current-head evidence.
- [x] **C2** — Obtain a current-head delivery-review completeness PASS after architecture and AI review.
- [ ] **C3** — Run `sh .claude/hooks/mission_ship_gate.sh mission <pr>` after exact-head CI is green and the PR is ready; retain its PASS reconciliation.
- [ ] **C4** — Leave merging to the user and delete this ignored local goal only after the final verifier passes.

## Verbatim request

> $mission small Stop stale control descriptors from hiding the live Quantick instance. %LOCALAPPDATA%\Quantick\control\instances held 107 descriptors from dead processes; discovery examines only the first 64 ("more than 64 entries; only the first 64 were examined"), each dead one costs a connect timeout (control.instance_gone), and quantick_describe took ~5 minutes to reach the live instance or missed it. Fix in crates/control-local (discovery) and the app's publisher: remove the own descriptor on clean exit, prune descriptors whose process_id is not alive (or whose process_nonce/start time mismatch) at publish and at discovery, and order candidates newest published_at first before any cap so a live instance is never cut. Keep it headless and deterministic in tests (injected liveness probe). Tests: 100 stale + 1 live finds the live one first and fast; clean exit leaves no descriptor.
