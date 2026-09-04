# Make Quantick's agent workflows equally available to Claude Code and Codex

Quantick's repository-specific development workflows must be discoverable and
usable from Codex without creating a second copy that can drift from the Claude
Code workflow.

**Tier:** medium — this touches the complete agent workflow surface and its
enforcement hooks, so it needs the normal completeness review.

## Request ledger

- **R1** — Import the repository skills currently exposed to Claude Code into
  Codex.
- **R2** — Include workflows such as `mission`, not merely one isolated skill.
- **R3** — Preserve equivalent development behavior in Claude Code and Codex.

## Assumptions

- **S1** — "Skills such as mission" means every tracked repository workflow
  under `.claude/skills`, because importing only `mission` would leave the
  orchestrated review and shipping skills unavailable. This is safe because
  adapters are small and remain scoped to the existing workflow set.
- **S2** — The existing `.claude/skills` files remain the canonical workflow
  definitions. This avoids duplicated process rules and follows the request
  for equal behavior.
- **S3** — Codex-native invocation uses `$skill-name`; Claude Code keeps its
  existing `/skill-name` spelling. Each product's documented syntax is used.

## Acceptance criteria

- [x] **A1** — Every tracked top-level workflow under `.claude/skills` has a
      valid, discoverable Codex skill with the same name under
      `.agents/skills`.
      *Evidence:* parity check and skill validator outputs in
      `.claude/evidence/codex-skill-compatibility/verification.md`. *(R1, R2)*
- [x] **A2** — Codex adapters load the existing Claude workflow as their
      canonical instruction source instead of duplicating its body.
      *Evidence:* adapter structure and parity test in
      `.claude/evidence/codex-skill-compatibility/verification.md`. *(R3)*
- [x] **A3** — A shared compatibility contract translates Claude-specific
      invocation, question, review, subagent, goal, shell, and tool concepts
      into Codex-native behavior without weakening user authority.
      *Evidence:* compatibility contract inspection and focused guard tests in
      `.claude/evidence/codex-skill-compatibility/verification.md`. *(R1, R3)*
- [x] **A4** — Codex project hooks invoke the repository's existing worktree,
      PR, commit, and guard-watch policies through the same canonical script.
      *Evidence:* hook configuration tests in
      `.claude/evidence/codex-skill-compatibility/verification.md`. *(R3)*
- [x] **A5** — An automated repository guard fails when the Claude and Codex
      workflow inventories diverge or an adapter stops pointing to its
      canonical workflow.
      *Evidence:* positive and negative tests in
      `.claude/evidence/codex-skill-compatibility/verification.md`. *(R1, R3)*
- [x] **G1** — Every authored repository artifact is in English.
      *Evidence:* `cargo test -p quantick-guards` and architecture-review
      dimension 8 in `.claude/evidence/codex-skill-compatibility/verification.md`.
- [x] **G2** — `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets`, `cargo build --workspace`, and
      `cargo test --workspace` pass after comparison with current
      `origin/main`. Runtime performance impact is none: only agent lifecycle
      discovery and hook execution are touched.
      *Evidence:* command results in
      `.claude/evidence/codex-skill-compatibility/verification.md`.
- [x] **G3** — Arch-review reports no unresolved Blocker or Should-fix finding.
      *Evidence:* verdict in
      `.claude/evidence/codex-skill-compatibility/verification.md`.

## Not applicable

- Visual QA, trader UX review, and UI harness coverage are not applicable: no
  Quantick UI or trader-facing runtime surface changes.
- New-extension review is not applicable: this adds repository agent metadata,
  not a Quantick runtime capability or crate.
- Hot-path measurement is not applicable: no per-trade, per-depth, or per-frame
  production path changes.
- Engine golden tests are not applicable: no engine or deterministic market
  computation changes.

## Closing steps

- **C1** — Delivery-review returns PASS.
- **C2** — The pull request is open and CI is green.

## Request as received

The following is retained verbatim as an attributed quotation because it is
the source request; all operative repository text above is in English.

> consegue importar os kills como /mission do claude para codex para gnt conseguir desenvovler igual tnato para claude quanto para codex?
