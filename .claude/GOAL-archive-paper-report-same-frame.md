# Mission: settle the paper account before the report paints

Fix the two findings the architecture and AI reviews raised on the second
consolidated PR #468 of campaign #367: the open paper report must never paint
the journal as it stood before a close journaled in the same frame, and the
docs must stop saying the backtest harness drives the paper account, which it
reaches only as a dev-dependency. Why: a report one frame behind the journal is
a regression from #453 (main re-read at once), and a map that draws a test-only
edge as a real one misleads every agent that reads it.

**Tier:** `small`. One call moved in the frame, doc comments, one map edge and
two sentences of prose; no question that is the trader's (no money, safety or
irreversibility call). `delivery-review` is exempt within the small-tier diff
ceiling (117 of 300 changed lines before this archive).

Campaign fix for the #468 review findings (thread `PRRT_kwDOTfuoRs6iBN1z`):
architecture report https://github.com/milocaetano/quantick/pull/468#issuecomment-5660445463
(C1, C2), AI report https://github.com/milocaetano/quantick/pull/468#issuecomment-5660447403.
Campaign #367 (https://github.com/milocaetano/quantick/issues/367); base
`campaign/lean-a-plus` at `0638af61`.

## Request ledger

- **R1** — the frame settles the paper account before `draw_report_window`,
  so a close journaled through a core method reached by `DerefMut` shows in
  the open report in the same frame. *("so the report never lags the
  journal")*
- **R2** — the doc comments at `paper_account.rs:21-23` and `:356` (and the
  frame's own) state the rule exactly. *("Correct the doc comments to state
  the rule exactly")*
- **R3** — test first: a failing test is committed before the fix; it closes
  a position through `dispatch` / `place_intent` with the report open and
  asserts the report shows the close in the same frame. *("commit a failing
  test before the fix")*
- **R4** — scope stays minimal; #456's timeline-reset case is untouched.
  *("Do not touch #456's separate case")*
- **R5** — `AGENTS.md` draws `backtest -.-> paper` dotted, and the `paper` row
  and `crates/paper/src/lib.rs:9-13` say "the chart drives it; the backtest
  proves it in a test".
- **R6** — neither `AGENTS.md` nor the context total grows, and no ceiling is
  raised. *("must not grow `AGENTS.md` or the total")*
- **R7** — no UI-free lines are added to `crates/app` unpaid.

## Assumptions

- **S1** — the simulator closes a position only on a print, so no in-tree
  `dispatch` or `place_intent` journals a close synchronously. The test queues
  the close through `dispatch` (via `DerefMut`) and fills it with a print fed
  to the core's own `on_trade`, also via `DerefMut` — the class the finding
  names. Safe: it exercises exactly the unshadowed path, and fails at the tip.
- **S2** — the settle is moved, not duplicated. The one visible consequence:
  a toast the report window itself posts is drained on the next frame's
  settle (16 ms later) instead of the same frame. Safe: the outbox keeps it,
  and `cargo test --workspace` is green.
- **S3** — the frame comment reuses the two lines of the old one, because
  the extension-boundary guard caps `QuantickApp` root lines at 10,240 and a
  net +3 comment tripped it. Safe: the old comment described the call order
  this change replaces.

## Acceptance criteria

- [x] **A1** — `app::tests::paper_trading_tests::an_open_report_shows_a_close_in_the_frame_that_journaled_it`
      fails at `e21839b5` (`left: None, right: Some(1)`) and passes at the fix
      commit `2845ff71`. *Evidence:* the test log and `git log` order. →
      PR body. *(R1, R3)*
- [x] **A2** — `paper_account.rs` module doc and `settle` doc, `ticket.rs`
      `settle` doc and the `frame.rs` comment name the rule: `on_trade` and
      `handle_events` re-read at once; any other core call's close waits for
      `settle`, which runs before the report paints. *Evidence:* the diff. →
      PR body. *(R2)*
- [x] **A3** — the diff touches no reset/seek path (`reset_timeline`,
      `timeline_rebuilt`). *Evidence:* `git diff --stat` against the base. →
      PR body. *(R4)*
- [x] **A4** — `AGENTS.md` carries `backtest -.-> paper` and the reworded row;
      `crates/paper/src/lib.rs` says the chart drives the account and the
      backtest proves it in a test. *Evidence:* the diff. → PR body. *(R5)*
- [x] **A5** — `AGENTS.md` 13,612 → 13,609 bytes; `ratchet.context.measured`
      295,633 → 295,630 of 295,668; `ratchet.app-ui-free.measured` 47,665
      unchanged; `context-baseline.txt` and `ui-free-baseline.txt` untouched.
      *Evidence:* `wc -c` and `quantick-guards --report`. → PR body. *(R6, R7)*
- [x] **G1** — every artifact in English. *Evidence:* `arch-review`
      dimension 8 and `cargo test -p quantick-guards`. → arch-review report.
- [x] **G2** — four checks green locally, each on its own: `cargo fmt --all
      -- --check`, `cargo clippy --workspace --all-targets`, `cargo build
      --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`; plus
      `cargo test -p quantick-guards`. Full CI at the final head.
      *Evidence:* exit codes; CI checks. → PR body.
- [x] **G3** — performance impact declared: the moved call is per-frame and
      does the same work (a bool take per tab, two `try_recv` polls); only
      its order changes. Flat. → PR body.
- [ ] **G4** — `arch-review` run with every Blocker/Should-fix resolved.
      *Evidence:* the published report. → PR comment.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

G-AI evidence destinations: the `ai-review` report comment on the PR, the
`ai_review_threads.sh list` output, and the `ai-review-complete` projection for
the campaign review key.

## Closing steps

- **C1** — the PR is open, non-draft, CI green at its head.
- **C2** — `mission_ship_gate.sh ship <pr>` prints PASS.

## Not applicable

- Hot path: the change is per-frame and reorders one call; no per-trade or
  per-depth path changes.
- User-visible surface (`ui-harness`, `visual-qa`, `trader-ux-review`): no new
  or changed surface; the existing report window paints a close one frame
  sooner, proven by the frame-driven test rather than a screenshot.
- New capability, trader action: none.
- Engine / determinism: no engine code; the test-first split is kept anyway
  because the request asked for it.
- `delivery-review`: `small` tier, within the diff ceiling.

## Verbatim request

> Fix the two findings the architecture and AI reviews raised on the second consolidated PR #468 of campaign #367 (https://github.com/milocaetano/quantick/issues/367). Reports:
> - Architecture: https://github.com/milocaetano/quantick/pull/468#issuecomment-5660445463
> - AI: https://github.com/milocaetano/quantick/pull/468#issuecomment-5660447403
> - Thread: `PRRT_kwDOTfuoRs6iBN1z`
>
> Mission tier: **small**, through this repo's `/mission` workflow. You are a subagent and cannot ask anyone.
>
> **Finding 1 (regression from #453).**
> - **Where:** `crates/app/src/paper_account.rs:270` (the `DerefMut` to the core), the doc at `:21-23` and `:356`, and `crates/app/src/app/frame.rs:718-719`.
> - **What breaks:** a close journaled through a core method reached via `DerefMut` (`market`, `place_intent`, `reverse_position`, `apply_strategy_command`) only sets `journal_changed`. `settle()` re-reads the report only after `draw_report_window` has painted. An open report therefore shows pre-close numbers for one frame, where on main it re-read immediately.
> - **Repair:** in `frame.rs`, call `self.settle_paper_panels(now)` (or the equivalent settle) before `draw_report_window`, so the report never lags the journal. Correct the doc comments to state the rule exactly.
> - **Test first:** commit a failing test before the fix. It closes a position through `dispatch` / `place_intent` with the report open and asserts the report shows the close in the same frame. Then commit the fix.
> - **Scope:** keep it minimal. Do not touch #456's separate case (timeline reset after a seek).
>
> **Finding 2 (docs).** `AGENTS.md`'s `backtest --> paper` edge, the `paper` row, and `crates/paper/src/lib.rs:9-13` say the backtest harness drives the paper account. It only reaches it as a dev-dependency (`crates/backtest/Cargo.toml:22-25`).
> - Make the edge dotted (`backtest -.-> paper`).
> - Reword the row and the doc to "the chart drives it; the backtest proves it in a test".
> - The context budget has only 35 bytes of headroom (295,633 of 295,668), and `AGENTS.md` is at its ceiling. The edit must not grow `AGENTS.md` or the total. Trim wording elsewhere in the same file if needed, and never raise a ceiling.
>
> **Worktree:** `C:\src\quantick-worktrees\fix-paper-report-same-frame` (Git Bash `/c/src/quantick-worktrees/fix-paper-report-same-frame`), branch `fix/paper-report-same-frame` at campaign tip `0638af61`. No upstream is set. `mission-base` and `mission-tier` (small) are recorded.
> - Start every command with `cd /c/src/quantick-worktrees/fix-paper-report-same-frame &&`, then run `cargo build -p quantick-guards`.
> - Push explicitly with `git push -u origin fix/paper-report-same-frame`.
> - Never write to `C:\src\quantick` or other worktrees.
>
> **Checks,** each on its own:
> - `cargo fmt --all -- --check` (rustfmt directly if the shim is blocked)
> - `cargo clippy --workspace --all-targets`
> - `cargo build --workspace`
> - `env -u QUANTICK_BUBBLES cargo test --workspace`
> - `cargo test -p quantick-guards`
>
> Never truncate test output with head. The UI-free ratchet sits at its ceiling of 47,665, so do not add UI-free lines to `crates/app`, or pay for them.
>
> **Delivery:**
> 1. Open a draft PR with `cd <wt> && gh pr create --draft --base campaign/lean-a-plus --head fix/paper-report-same-frame --title "fix(app): settle the paper account before the report paints" --body-file -`. The heredoc body states: tier small, "Campaign fix for the #468 review findings (thread PRRT_kwDOTfuoRs6iBN1z)", the test and the fix. It ends with `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 2. Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 3. Archive the small-tier goal file last, including G-AI1 to G-AI4.
> 4. Run arch-review (step 0 code-review) and ai-review. Publish reports through `review_report.sh` and record markers with `sh .claude/hooks/campaign_context.sh key <wt>`.
> 5. Wait for CI green, run `gh pr ready <n>` once, then `sh .claude/hooks/mission_ship_gate.sh ship <n>`.
>
> Do not reply to or resolve the #468 thread; the coordinator does that. **Never merge, never touch main, never use the PowerShell tool for `gh pr`.** Use `python`, not `python3`.
>
> Return a HANDOFF BLOCK: PR URL, head, test and fix commits, byte counts for `AGENTS.md` and the context total, review URLs, markers and key, CI, and ship-gate output.
>
> — the campaign coordinator, relaying the trader's campaign #367
