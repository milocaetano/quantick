# Mission: gateway-request-id-ordering

Release a request ID before (or atomically with) writing its answer in the
local control gateway (#425), and force what five load-sensitive app tests
assert (#427, #428, #429, #430, #432). Why it matters: a client that reads an
answer and reuses its request ID at once is refused with a non-retryable
`control.invalid_request` on a loaded machine, which contract §5.2 does not
allow; and the contention CI step (#414) cannot hold ten green rounds while
these five tests fail under CPU load.

**Tier:** `high` — set by the campaign coordinator (issue #434: a production
ordering change in the gateway, risk medium-high).

Campaign child of #367 (Q13). Base: `campaign/lean-a-plus`, cut at `989f2945`
and rebased onto `6ba1a7f3` (#431) before review.
Issue: #434.

## Request ledger

- **R1** — #425: every terminal path in `gateway/server.rs` releases the
  request ID before, or atomically with, writing the answer; every other
  guarantee holds (no answer lost, no duplicate accepted while the first is in
  flight, per-connection limits unchanged). *(#434 scope 1, D1)*
- **R2** — A real-gateway regression test that reuses the ID immediately after
  reading the answer. *(#434 scope 1, A1, D1)*
- **R3** — Remove #411's workaround wait from its test once the ordering is
  fixed. *(#434 scope 1, A1, D1)*
- **R4** — No wire-visible behaviour change beyond "an immediate reuse after
  the answer is accepted"; anything more stops the mission with a
  `human_decision`. *(D1, #434 A3 "no wire-shape change")*
- **R5** — #427, #428, #429, #430, #432: force the interleaving each assertion
  depends on (bounded waits on observable conditions, worker flushes, frame
  settles), the #382/#400/#404/#426 shape. *(#434 scope 2, D2)*
- **R6** — No assertion weakened. *(#434 A3, D2)*
- **R7** — #429: the property is asserted with counts ("no trade lost and no
  queue overflow inside the envelope"); the keep-up timing claim moves to the
  measured artifact in `docs/quality/live-envelope*`; the PR says so. *(D2,
  #434 scope 2)*
- **R8** — Proof: each of the five plus the #425 regression test, 0 failures
  over at least 10 contention rounds (24 copies on 4 cores, or Windows
  affinity `0xF` locally), before/after table in the PR. *(#434 A2, D3)*
- **R9** — List the skip-list lines (`tools/ci/contention-known-issues.txt`,
  #414) the fixes allow removing. *(#434 scope 3, D3)*
- **R10** — Purpose: the contention step can land with these tests no longer
  excused, and a client may reuse an answered ID. *(#434 context)*
- **R11** — Integration into `campaign/lean-a-plus` through a PR whose base is
  exactly that branch, merge read back. *(#434 A4)* The merge is the
  coordinator's; this child delivers the PR (C2).

## Decisions (from the coordinator)

- **D1** — #425 is a race fix restoring the documented ordering (D23 on #367),
  not a contract change; keep every other guarantee; real-gateway regression
  test; remove #411's wait; stop with a `human_decision` on any wider
  wire-visible change.
- **D2** — the five tests: force what each assertion depends on; never weaken
  one; #429's timing bound becomes counts, the timing claim moves to the
  measured artifact.
- **D3** — proof over at least 10 contention rounds per test, table in the PR;
  list the skip-list lines.
- **D4** — tier `high`: `arch-review` with `code-review` at `medium`, full shape
  pass; `ai-review` completion; `delivery-review` in full; at most three step-0
  rounds.
- **D5** — the readiness command once, cd-prefixed, from the Bash tool; never
  merge, never touch main.

## Assumptions

- **S1** — "Local equivalent" of 24 copies on 4 cores is the Windows harness
  Q10 used: 24 concurrent copies of the whole `quantick-app` test binary, all
  held to processor mask `0xF` (4 of 12 cores), `--test-threads=4`, from
  `crates/app`. Safe: Q9's and Q10's reproductions use it; CI runs the Linux
  step at the final head.
- **S2** — "Atomically with writing" is met by releasing the ID under the
  connection's writer lock, immediately before the frame is written. A
  duplicate the reader accepts after the release is answered after the first
  answer, because every answer takes the same lock. Safe: that is the only
  observable order a client can rely on; today a duplicate is already accepted
  once the release has run.
- **S3** — Only the request ID moves. The global buffered-response slot is still
  released after the write (it bounds buffered, unwritten answers), and the
  per-connection in-flight count is untouched. Safe: D1 says per-connection
  limits unchanged, and moving the global slot would change a bound, not the
  reported race.
- **S4** — The deterministic regression test needs to hold an answering thread
  after its write. That needs a `#[cfg(test)]` hook in the gateway's options,
  read by the answer path in `gateway/server.rs` (the owned file) and set from
  a `#[cfg(test)]` seam in `control/gateway.rs` or `retry_seams.rs`. Safe:
  compiled into the test build only, so no production path or wire-visible
  behaviour changes and D1's stop trigger is not reached; D1 itself asks for a
  real-gateway test that forces the reuse, and #426 set the precedent
  (`begin_frame_over_budget_for_test`, its S3). Reviewed by the source-first
  pass, which asked whether this should be a decision: kept as an assumption
  for that reason, and named in the PR.

## Acceptance criteria

- [ ] **A1** — Every terminal path in `gateway/server.rs` that answers a tracked
      request releases its ID before the answer's frame is written, under the
      writer lock; the global and per-connection counters keep their order.
      *Evidence:* the diff; the PR section "The ordering change". → PR body.
      *(R1, R4)*
- [ ] **A2** — A real-gateway test holds the answering thread after its write,
      reads the answer, reuses the ID at once and gets a success; it fails on
      the old ordering. *Evidence:* the test's name, its failure on the old
      order (captured output), its pass after. → PR body. *(R2)*
- [ ] **A3** — #411's test no longer retries the reuse; it sends it once and
      asserts success. *Evidence:* the diff of
      `gateway_rejects_a_duplicate_request_id_while_a_wait_is_parked`. → PR
      body. *(R3)*
- [ ] **A4** — No wire type, field, code or retryability changes; the duplicate
      refusal still refuses a second request sent while the first is
      unanswered (existing tests unchanged and green). *Evidence:* the diff
      touches no `quantick-control` type; `cargo test --workspace` green. → PR
      body. *(R4)*
- [ ] **A5** — #427, #428, #430, #432 each force the state their assertion
      depends on, with a root cause captured or reasoned from the code; every
      original assertion is still made. *Evidence:* the per-test table; the
      diff. → PR body. *(R5, R6)*
- [ ] **A6** — #429's test asserts no loss and no queue fill with counts, the
      `late_frames` bound is removed from the test, and
      `docs/quality/live-envelope.md` says the keep-up claim is measured, not
      asserted. *Evidence:* the diff; the PR section. → PR body. *(R5, R7)*
- [ ] **A7** — Contention table: before (the base binary) and after (>= 10
      rounds of 24 copies on mask `0xF`), 0 failures after for the six tests.
      *Evidence:* the table. → PR body. *(R8, R10)*
- [ ] **A8** — The skip-list lines the fixes allow removing are listed.
      *Evidence:* PR body section. → PR body. *(R9, R10)*
- [ ] **A9** — The PR's base is exactly `campaign/lean-a-plus`. *Evidence:* the
      PR's `baseRefName`. → PR / handoff. *(R11)*

## Gates

- **G1** — English everywhere (`CLAUDE.md`), graded by `arch-review` dimension 8.
- **G2** — Four checks green, run separately; performance impact declared (the
  answer path is per request, rare relative to frames; the change moves one
  mutex-guarded set removal inside an already held lock).
- **G3** — `arch-review` (`code-review` at `medium`, full shape pass) with every
  Blocker/Should-fix resolved or deferred in the PR body; `ai-review` complete
  with zero unresolved threads.
- **G4** — Final-head CI green.
- **G5** — Write scope: only this worktree. Owned: `gateway/server.rs`, the
  control-plane test modules, `retry_readback_tests.rs`, the live-envelope
  test modules. A `#[cfg(test)]` seam in `control/gateway.rs` or
  `retry_seams.rs` (S4), the shared `app/tests/mod.rs` and
  `docs/quality/live-envelope.md` (R7) are named detours. Not touched: the
  tape (`state*`, `tab/*`, Q12), `.github/workflows/ci.yml` and `tools/ci/*`
  (Q9); `tools/ci/contention.sh` is never committed here.

- **G6** — Operational instructions of the prompt: `cargo check -p
  quantick-app --all-targets` before the first edit and `df -h /c` before the
  first build (both run: 268 GB free); `python`, not `python3`; the four checks
  one at a time; scratchpad files under `-q13` paths; the two commit trailers
  on every commit; readiness never through the PowerShell tool.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open against `campaign/lean-a-plus`, readiness accepted,
  or the denial reported to the coordinator.
- **C3** — The HANDOFF BLOCK is returned to the coordinator with every field
  the prompt's *Delivery* step 8 names.

## Not applicable

- *Touches a hot path*: no. The answer path runs once per control request, not
  per trade, depth or frame.
- *User-visible*, *adds a capability*, *adds something a trader does*: no
  surface, capability or action changes.
- *Engine / determinism*: the engine is not touched.
- *Docs/skills only*: no; runtime and tests change.

## The request as received

Attributed quotation (the campaign coordinator's prompt, verbatim, English):

> You are executing campaign child mission **Q13** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: fix the gateway ordering race #425 (the answer is written before the request ID is released, so an immediate reuse is refused) and five load-sensitive tests (#427, #428, #429, #430, #432). You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision becomes a `human_decision` in your handoff, never a guess.
>
> ## Read first, in this order
> 1. `C:\src\quantick\CLAUDE.md`.
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign child at tier `high`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`, guards armed); still run `cargo check -p quantick-app --all-targets` before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md`, `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #434 (`gh issue view 434`), and #425, #427, #428, #429, #430, #432 (`gh issue view <n>`), each with its failing output and reproduction config.
> 5. How the campaign fixed this class: PRs #382, #400, #404, #426 (`gh pr view <n>`), especially #426 (Q10) for the screenshot race fix, the `settle_*` helpers, the rate-limit pacing of retries, and #411's workaround wait that #425's fix lets you remove.
> 6. The contention harness (from PR #414, not yet merged): `git -C C:\src\quantick fetch origin ci/app-tests-under-contention`, then `git show origin/ci/app-tests-under-contention:tools/ci/contention.sh` and `:docs/quality/app-tests-under-contention.md`; copy it into your scratchpad to run locally; do NOT commit it. Q10's local logs and `summarize_q10.py` are in `C:\Users\User\AppData\Local\Temp\claude\C--src-quantick\e68b97ed-23bb-4a09-ae2a-1a9455accef9\scratchpad\contention-q10\` (read-only for you).
> 7. The code: `crates/app/src/control/gateway/server.rs` (where request IDs are tracked and answers written), `crates/app/src/control/gateway.rs`, the control-plane test modules, `crates/app/src/app/tests/retry_readback_tests.rs`, `crates/app/src/live_envelope_tests*`.
>
> ## Worktree (the only place you write)
> - `C:\src\quantick-worktrees\fix-gateway-request-id-ordering` (Git Bash `/c/src/quantick-worktrees/fix-gateway-request-id-ordering`), branch `fix/gateway-request-id-ordering`, cut from `origin/campaign/lean-a-plus` at `989f2945`. Every command starts with `cd /c/src/quantick-worktrees/fix-gateway-request-id-ordering &&`. Never write to `C:\src\quantick` or other worktrees.
> - You own `crates/app/src/control/gateway/server.rs`, the control-plane test modules, `retry_readback_tests.rs`, the live-envelope test module(s). A sibling (Q12, PR #431) owns the tape (`state*`, `tab/*`); another (Q9, PR #414) owns `.github/workflows/ci.yml` and `tools/ci/*`. Do not edit those.
>
> ## Decisions
> - D1: #425 is a race fix restoring the documented ordering (coordinator decision D23 on #367), not a contract change: release the request ID before, or atomically with, writing the answer; keep every other guarantee (no answer lost, no duplicate accepted while the first is in flight, per-connection limits unchanged). Real-gateway regression test that reuses the ID immediately after reading the answer; then remove #411's workaround wait from its test. If the fix would change any wire-visible behaviour beyond "an immediate reuse after the answer is accepted", stop and record a `human_decision`.
> - D2: the five tests: force what each assertion depends on (bounded waits on observable conditions, worker flushes, frame settles); never weaken an assertion. For #429 (the burst test's keep-up bound `late_frames * 2 < frames`, a timing assertion): if the property under test is "no trade lost and no queue overflow inside the envelope", assert that with counts and move any timing claim to the measured artifact in `docs/quality/live-envelope*`; say so in the PR.
> - D3: proof: each of the five plus the #425 regression test: 0 failures over at least 10 contention rounds (24 copies on 4 cores, or Windows affinity 0xF locally), before/after table in the PR. List the skip-list lines your fixes allow removing (they are being added to #414's list for #427, #428, #429, #430, #432 by Q9 in parallel; whichever lands second reconciles).
> - D4: tier `high`: `arch-review` with `code-review` at `medium`, full shape pass; `ai-review` completion; `delivery-review` in full. At most three step-0 rounds.
> - D5: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI; if denied because the campaign tip moved or the PR is DIRTY, stop and report; the coordinator rebases. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**
>
> ## Environment notes
> - Use `python`, not `python3`. Run the four checks one at a time, never `||` or `| head`; read whole failures. Foreground commands only, adequate timeouts.
> - App tests: `env -u QUANTICK_BUBBLES cargo test -p quantick-app ...`.
> - Keep every scratchpad file of yours under a `-q13` path.
> - Check `df -h /c` before the first build; if under 15 GB free, `cargo clean` only in your own worktree and report.
> - If writing review markers into the git dir is denied by the permission classifier, put the exact `printf` lines in your handoff, marked pending.
> - Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
>
> ## Delivery
> 1. Implement with the verification loop.
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately.
> 3. Archive `GOAL.md` per mission step 8 (slug `gateway-request-id-ordering`) as the last commit before reviews.
> 4. Push; open a **draft** PR: `cd /c/src/quantick-worktrees/fix-gateway-request-id-ordering && gh pr create --draft --base campaign/lean-a-plus --title "fix(control): release the request ID before writing the answer, and force five more load-sensitive interleavings" --body-file -` with a heredoc body per the PR template: tier, "Campaign child of #367 (Q13); closes on integration: #434, #425, #427, #428, #429, #430, #432", root causes, the ordering change and its guarantees, the contention table, skip-list lines to remove, local verification, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. `/arch-review`, `/ai-review`, `/delivery-review`; resolve Blockers/Should-fixes; record markers with the shared key per integration.md.
> 6. Watch CI at the head (`gh pr checks <n> --watch`, bounded).
> 7. `gh pr ready <n>` once (D5).
> 8. Return a HANDOFF BLOCK: issues; branch; worktree; PR URL; head SHA; base tip; #425's fix and its guarantees; per-test root cause and fix; contention table; skip-list lines; review verdicts with URLs; markers yes/no; CI run URL and conclusion; findings closed/open; ready accepted or denied; any human_decision; the coordinator's next action.
