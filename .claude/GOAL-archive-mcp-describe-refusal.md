# Goal: a refused describe is returned, never replaced by version 1

A version-less `quantick_invoke` whose `control.describe` is refused returns
that refusal to the caller unchanged — code and `retryable` flag — instead of
silently calling version 1. Version 1 of eight `layout.*` calls applies the
change and then answers `control.capability_unavailable`, so under load an
agent could move a pane, be told it did not, and move it again on retry.

**Tier:** small — the coordinator assigned it: one `None` arm in
`crates/mcp/src/tools.rs`, one inverted test and comment/doc wording; no new
capability, no UI, no hot path. Exempt from `delivery-review` within the
small-tier diff ceiling.

Campaign fix for the #449 architecture Blocker (AI-review thread
`PRRT_kwDOTfuoRs6h7KP7`), campaign #367. Base: `campaign/lean-a-plus` at
`317772a5`.

## Request ledger

- **R1** — In the `None` arm of `quantick_invoke`'s version resolution, a
  refused describe returns describe's own refusal unchanged, preserving its
  code and `retryable` flag, and the capability is never invoked.
- **R2** — The version-1 fallback stays only when describe succeeded and does
  not list the ID.
- **R3** — Test-first: the inverted
  `a_refused_describe_leaves_the_first_version_rather_than_its_error` (a
  describe answering `control.backpressure` returns that error and never
  invokes the capability) is committed failing before the fix.
- **R4** — The comments at `tools.rs:83-87` and `:520-524`, and any doc or
  MCP tool description that states the old fallback, describe the new
  behaviour.
- **R5** — `tools.rs` stays under 1,500 production lines (`!budget 0`).
- **R6** — The purpose: a retryable gateway refusal under load can never turn
  into a mutating call that reports failure.

## Assumptions

- **S1** — A transport-level describe failure (`link.invoke` returning `Err`)
  is returned the same way as a refusal the instance answers: both are
  "describe's own refusal", and `search` already treats them alike. Safe: the
  request says "including the retryable refusals the gateway gives", and
  `described` already maps both to one `ToolResult`.
- **S2** — The test keeps the brief's name, with the assertions inverted, so
  the ledger and the #449 thread can find it. Safe: the name is a label; the
  assertions carry the contract.
- **S3** — `docs/control-plane/control-contract.md` and `crates/mcp/README.md`
  gain one sentence naming the refusal behaviour; no doc stated the old
  fallback. Safe: they are where the omitted-version rule already lives.

## Acceptance criteria

- [x] **A1** — A describe answering `control.backpressure` (retryable) makes
      `quantick_invoke` return `is_error` with that code and `retryable: true`,
      and the capability is never invoked; the same for a transport error.
      *Evidence:* `cargo test -p quantick-mcp a_refused_describe` passes after
      the fix. → PR body. *(R1, R6)*
- [x] **A2** — The test is a separate earlier commit that fails before the
      fix. *Evidence:* the test commit SHA and its failing run. → PR body.
      *(R3)*
- [x] **A3** — The unlisted-ID fallback still works. *Evidence:* the existing
      `quantick_invoke` default tests pass. → PR body. *(R2)*
- [x] **A4** — No comment or doc states the old fallback. *Evidence:* the diff
      of the `tools.rs` comments and the contract sentence. → PR diff. *(R4)*
- [x] **A5** — `tools.rs` production lines under 1,500. *Evidence:*
      `cargo test -p quantick-guards` green. → PR body. *(R5)*
- [ ] **G1** — Every artifact in English. *Evidence:* arch-review dimension 8.
- [x] **G2** — fmt, clippy, build, test green; performance impact declared
      (rare path: one describe per version-less invoke, unchanged); arch-review
      with every Blocker/Should-fix resolved or deferred. → PR body.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

AI-review evidence lands on the PR: the durable report comment, the thread
list, and the private `ai-review-complete` projection. G-AI1–4 are not
claimed here.

## Evidence (local, head `a4d2cd33`)

- A2 — `ff686057` (test only) failed: `a version-less call went ahead without
  describe (by_transport false): [("layout.pane.move", 1)]`; `a4d2cd33` (fix)
  passes it.
- A1, A3 — `cargo test -p quantick-mcp`: 36 passed, including the existing
  newest-version default tests.
- A5 — `tools.rs` 1,024 production lines (1,420 total); guards 172 passed.
- G2 — fmt exit 0, clippy exit 0, build exit 0,
  `env -u QUANTICK_BUBBLES cargo test --workspace` exit 0 (app 2,133 passed).
  Performance: rare path (one describe per version-less invoke), unchanged.

## Closing steps

- **C1** — Draft PR against `campaign/lean-a-plus`, ai-review complete with
  zero open threads, CI green, `gh pr ready` once, ship gate run.

## Request as received

> Fix the Blocker the consolidated-PR architecture review found (campaign #367, https://github.com/milocaetano/quantick/issues/367; report https://github.com/milocaetano/quantick/pull/449#issuecomment-5655399218; AI-review thread `PRRT_kwDOTfuoRs6h7KP7` on #449). Mission tier **small**, through this repo's `/mission` workflow. You are a subagent and cannot ask anyone.
>
> **Defect** — `crates/mcp/src/tools.rs:531` (introduced by #424, D20 "quantick_invoke defaults to the newest registered version"): when a version-less `quantick_invoke` has its `control.describe` call refused — including the retryable refusals the gateway gives under load (rate limiter, in-flight caps, parked-waiter capacity, `control.backpressure`) — it silently falls back to version 1. Version 1 of eight `layout.*` calls applies the change and then answers `control.capability_unavailable` (`crates/app/src/control/layout/v2.rs:1-14`), so an agent's `layout.pane.move` can move the pane, be told it did not, and move it again on retry.
>
> **Repair** — in the `None` arm return describe's own refusal to the caller unchanged (`Err(refused) => return Ok(refused)` or equivalent), preserving its `retryable` flag and code; keep the version-1 fallback only when describe succeeded and does not list the ID. Test-first: first commit an inverted `a_refused_describe_leaves_the_first_version_rather_than_its_error` (around `:1312`) — a describe answering `control.backpressure` must return that error and never invoke the capability — failing; then the fix. Fix the comments at `:83-87` and `:520-524`, and any doc (grep `docs/` and the MCP tool description text for the fallback wording) that describes the old behaviour. Keep `tools.rs` under 1,500 production lines (size ratchet, `!budget 0`).
>
> Worktree: `C:\src\quantick-worktrees\fix-mcp-describe-refusal` (Git Bash `/c/src/quantick-worktrees/fix-mcp-describe-refusal`), branch `fix/mcp-describe-refusal` at campaign tip `317772a5`; `mission-base` and `mission-tier` (small) already recorded. Start every command with `cd /c/src/quantick-worktrees/fix-mcp-describe-refusal &&`; first `cargo build -p quantick-guards`. Never write to `C:\src\quantick` or other worktrees. Other agents are building in parallel worktrees; that is expected.
>
> Checks, each on its own: `cargo fmt --all -- --check` (rustfmt directly if the shim is blocked), `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, `cargo test -p quantick-guards`. Never truncate test output with head.
>
> Delivery: draft PR `cd <wt> && gh pr create --draft --base campaign/lean-a-plus --title "fix(mcp): return a refused describe instead of falling back to version 1" --body-file -` (heredoc: tier small, "Campaign fix for the #449 architecture Blocker, campaign #367", the defect, the test, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`). Commit trailers `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`. Small-tier goal file archived last. Reviews: arch-review (step 0 code-review) and ai-review, reports through `review_report.sh`, markers via `sh .claude/hooks/campaign_context.sh key <wt>`. CI green, `gh pr ready <n>` once, `sh .claude/hooks/mission_ship_gate.sh ship <n>`. Do not reply to or resolve the #449 thread; the coordinator does. **Never merge; never use the PowerShell tool for `gh pr` commands; never touch main.** Use `python`, not `python3`.
>
> Return a HANDOFF BLOCK: PR URL; head SHA; the test commit and fix commit; review verdicts and URLs; markers and key; CI; ship-gate output.

— the campaign coordinator, 2026-09-13
