# Mission: contention-listed-tests

Fix the eight load-sensitive tests the CPU-contention CI step found (#408, #409,
#410, #411, #413, #415, #416, #417), and first answer whether the three Null
`capture_revision` failures (#413, #416, #417) are a product race in the
evidence path. Why it matters: the contention step (#414) cannot merge until the
skip list is empty of these tests, and an evidence bundle must say which
revision its picture shows (data honesty).

**Tier:** `high` — set by the campaign coordinator (issue #418: one item may be a
product race in evidence capture, so a production change is possible and must
be reviewed under the full gates).

Campaign child of #367 (Q10). Base: `campaign/lean-a-plus`. Issue: #418.

## Request ledger

- **R1** — Answer, with evidence, whether a bundle or snapshot can be published
  with a Null `capture_revision` in production under load ("a product race in
  the evidence path"). *(#418 scope 1, D2)*
- **R2** — If product: fix the product path so the revision is always stamped or
  the bundle states it is unavailable (typed, not a silent Null), with a
  real-gateway regression test that forces the interleaving; no wire-shape
  change, and if one were needed, stop and record a `human_decision` instead.
  If not: fix the tests. *(#418 scope 1, D2)*
- **R3** — #408, #409, #410, #411, #415: force the interleaving each assertion
  depends on (a frame, a worker flush, a publish), bounded waits on observable
  conditions, the #382/#400/#404 shape. *(#418 scope 2, D3)*
- **R4** — No assertion weakened ("Weakening an assertion is not a fix"). *(#418
  scope 2, A3)*
- **R5** — Each of the eight passes 24-copies-on-4-cores contention (or local
  equivalent) with 0 failures over at least 10 runs; before/after table in the
  PR. *(#418 A2, D4)*
- **R6** — Record which skip-list lines (`tools/ci/contention-known-issues.txt`
  on #414's branch) the fixes allow removing; the coordinator applies them.
  *(#418 scope 3, D4)*
- **R7** — Purpose: the contention step can land with these tests no longer
  excused, so load-sensitive tests fail in their own PR. *(#418 context)*

## Decisions (from the coordinator)

- **D1** — one PR for all eight.
- **D2** — #413/#416/#417 first; product fix only if the product can emit such a
  bundle; a wire-shape change is a `human_decision`, not a guess.
- **D3** — the other five: force what each assertion depends on.
- **D4** — proof: contention runs, 0 failures over >= 10 runs after, the issue's
  before evidence; table in the PR body; skip-list lines to remove.
- **D5** — `arch-review` with `code-review` at `medium`, full shape pass;
  `ai-review` completion; `delivery-review` in full; at most three step-0 rounds.
- **D6** — `gh pr ready` once, cd-prefixed, from the Bash tool; never merge.

## Assumptions

- **S1** — "Local equivalent" of 24 copies on 4 cores is the Windows harness in
  the scratchpad: 24 concurrent copies of the whole `quantick-app` test binary,
  all held to processor mask `0xF` (4 of 12 cores), `--test-threads=4`, as Q9's
  doc names for its local reproductions. Safe: the doc itself reports local
  reproductions this way; CI runs the same step at the final head.
- **S2** — The gateway server's own ordering (answer written before the request
  ID is released) is outside D2's evidence-path authorization; #411 is fixed at
  the test layer under D3 and the product ordering is raised as a
  `human_decision`. Safe: no product behaviour changes without a decision.
- **S3** — A `#[cfg(test)]` seam in `control/gateway.rs` that runs one frame
  whose budget is already spent is part of the evidence regression test, not a
  production change. Safe: compiled into the test build only.

- **S4** — A ninth test with #415's exact mechanism,
  `an_operator_detaching_its_own_slot_leaves_the_traders_slot_of_the_same_number`
  (same module, 200 back-to-back frames after an attach), failed this
  mission's own `cargo test --workspace` while contention runs shared the
  host, and one local contention round. It gets the same fix as #409 and
  #415 (the suite's `settle_indicators`): a detour stated here and in the
  PR, because the verification loop cannot be green without it and R7 needs
  the step to land. Other tests the local harness found failing are filed,
  not fixed: #427, #428, #429, #430; the #411 gateway ordering is #425.

## Acceptance criteria

- [ ] **A1** — The Null `capture_revision` question is answered with evidence
      (the code path and a captured failing bundle's coverage) in the PR body.
      *Evidence:* PR body section "D2 finding". → PR body. *(R1)*
- [ ] **A2** — If the product can drop a delivered image, a capture parked for an
      image is served in the frame whose image it waited for even when the
      frame's time budget is spent, and a real-gateway test forcing a spent
      budget proves it (fails before the fix, passes after); no serialized
      type or field changes (had one been needed, the mission stops with a
      `human_decision`). *Evidence:* the new test's name and its before/after
      result; the diff touches no wire type. → PR body. *(R2, R4)*
- [ ] **A3** — #408, #409, #410, #411, #415 each force the state their assertion
      depends on; every original assertion is still made.
      *Evidence:* the diff; the per-test table. → PR body. *(R3, R4)*
- [ ] **A4** — Contention table: before (issue evidence plus local runs of the
      base binary) and after (>= 10 whole-binary rounds of 24 copies on mask
      `0xF`, 0 failures in the eight). *Evidence:* the table. → PR body. *(R5, R7)*
- [ ] **A5** — The skip-list lines each fix allows removing are listed.
      *Evidence:* PR body section. → PR body. *(R6, R7)*

## Gates

- **G1** — English everywhere (`CLAUDE.md`), graded by `arch-review` dimension 8.
- **G2** — Four checks green, run separately; performance impact declared (the
  product change is on a rare path: once per frame only while a capture is
  parked and an image is held).
- **G3** — `arch-review` (`code-review` at `medium`, full shape pass) with every
  Blocker/Should-fix resolved or deferred in the PR body; `ai-review` complete
  with zero unresolved threads.
- **G4** — Final-head CI green.

## Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open against `campaign/lean-a-plus` and marked ready (D6).
- **C3** — Owned by the coordinator, not this mission: #418's A4 (merge into
  `campaign/lean-a-plus`, merge read back, #408-#417 closed with integration
  evidence). Archiving this file is step 8's, before the reviews, not a
  closing step.

## Preflight record

The source-first completeness pass (delivery contract, high tier) ran after
implementation had started, not before: an independent read-only pass over
#418 and D1-D6, then this map. It found the D2 stop clause and #418 A4 without
a line here; R2/A2 and C3 now carry them. Chronology recorded as it happened.

## Not applicable

- Hot-path bench: the product change runs only while a capture waits for an
  image (rare), not per trade, per depth, or per idle frame.
- UI rows (`ui-harness`, `visual-qa`, `trader-ux-review`): nothing user-visible
  changes; the screenshot toast and the bundle's wire shape are unchanged.
- New capability / trader action / engine determinism: none added or touched.

## The request as received

> Attributed quotation, verbatim: the campaign coordinator's task prompt for
> Q10 (campaign #367), followed by issue #418's acceptance criteria.

> You are executing campaign child mission **Q10** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: fix the eight load-sensitive tests the new CPU-contention CI step found, and first answer whether the three Null `capture_revision` failures are a product race in the evidence path. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision becomes a `human_decision` in your handoff, never a guess.
>
> ## Read first, in this order
> 1. `C:\src\quantick\CLAUDE.md` (data honesty: evidence must say which revision it shows; inferred or incomplete data is labelled, never silently patched).
> 2. `C:\src\quantick\.claude\skills\mission\SKILL.md` — you are a **campaign child at tier `high`**; follow steps 1, 2, 4, 5, 7, 8, 9. Step 3 is answered below. Step 6 is done (worktree, `mission-base`, `mission-tier`, guards armed); still run `cargo check -p quantick-app --all-targets` before the first edit.
> 3. `C:\src\quantick\docs\campaign\integration.md`, `C:\src\quantick\docs\workflow\delivery.md`.
> 4. The task: issue #418 (`gh issue view 418`) and each listed issue: #408, #409, #410, #411, #413, #415, #416, #417 (`gh issue view <n>`), each with the failing copy's output and the configuration that reproduced it.
> 5. The contention harness from Q9, not yet merged: `git -C C:\src\quantick fetch origin ci/app-tests-under-contention` then read `git show origin/ci/app-tests-under-contention:tools/ci/contention.sh`, `...:tools/ci/contention-known-issues.txt` and `...:docs/quality/app-tests-under-contention.md`. You may copy the script into your scratchpad to run it locally; do NOT commit it to your branch (it lands with PR #414).
> 6. How the campaign fixed this class before: PR #382, #400, #404 (`gh pr view <n>`): force the interleaving the assertion depends on; never weaken an assertion.
> 7. The evidence path: `crates/app/src/control/evidence.rs` and its `evidence/` siblings (store, image), the screenshot arc in `crates/app/src/control/gateway/screenshot.rs`, `crates/control/src/evidence.rs`, and the tests `crates/app/src/app/tests/screenshot_evidence_tests.rs` and the control-plane test modules.
>
> ## Worktree (the only place you write)
> - `C:\src\quantick-worktrees\fix-contention-listed-tests` (Git Bash `/c/src/quantick-worktrees/fix-contention-listed-tests`), branch `fix/contention-listed-tests`, cut from `origin/campaign/lean-a-plus` at `c55a4222`. Every command starts with `cd /c/src/quantick-worktrees/fix-contention-listed-tests &&`. Never write to `C:\src\quantick` or other worktrees.
> - You own the control-plane test modules under `crates/app/src/app/tests/`, `screenshot_evidence_tests.rs`, and (only if D2 finds a product race) the evidence capture path under `crates/app/src/control/evidence*` and `gateway/screenshot.rs`. A sibling (Q11) owns `crates/mcp/*`; Q4's PR (in integration) touches `crates/app/src/app/tests/mod.rs`: avoid editing that shared helper file unless indispensable, and say so.
>
> ## Decisions
> - D1: one PR for all eight.
> - D2: #413, #416, #417 first. Determine from the code whether a bundle or snapshot can be published with a Null `capture_revision` in production under load (a race between frame capture and revision stamping, or a slot read before it is filled). If yes: fix the product path so the revision is always stamped, or the bundle states typed "revision unavailable" instead of a silent Null; add a real-gateway regression test that forces the interleaving; this is a product fix on the evidence contract's semantics but not a wire-shape change; if it would need a wire-shape change (a new field or type), stop and record a `human_decision`. If no: fix the tests.
> - D3: the other five: force what each assertion depends on (a frame, a worker flush, a publish), bounded waits on observable conditions, the shapes #382/#400/#404 used.
> - D4: proof: for each of the eight, contention runs (24 copies on 4 cores or its local equivalent) with 0 failures over at least 10 runs after, and the before evidence from the issue; a table in the PR body. Record in the PR body which lines of Q9's skip list (`tools/ci/contention-known-issues.txt` on #414's branch) your fixes allow removing; the coordinator applies that to #414.
> - D5: tier `high`: `arch-review` with `code-review` at `medium`, full shape pass; `ai-review` completion; `delivery-review` in full. At most three step-0 rounds.
> - D6: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI; if denied because the campaign tip moved or the PR is DIRTY, stop and report; the coordinator rebases. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**
>
> ## Environment notes
> - Use `python`, not `python3`. Run the four checks one at a time, never `||` or `| head`; read whole failures. Do not wait on background commands; foreground with an adequate timeout. `taskset` is Linux-only; locally use Windows processor affinity (Q9's doc says how) or a throwaway push-only branch on CI (delete it afterwards; never a PR, never targeting `campaign/lean-a-plus` or `main`).
> - App tests: `env -u QUANTICK_BUBBLES cargo test -p quantick-app ...`.
> - Keep every scratchpad file of yours under a `-q10` path; never a generic shared path.
> - Check `df -h /c` before the first build; if under 15 GB free, `cargo clean` only in your own worktree and report.
> - If writing review markers into the git dir is denied by the permission classifier, put the exact `printf` lines in your handoff, marked pending.
> - Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
>
> ## Delivery
> 1. Implement with the verification loop.
> 2. Before every commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `env -u QUANTICK_BUBBLES cargo test --workspace`, each separately.
> 3. Archive `GOAL.md` per mission step 8 (slug `contention-listed-tests`) as the last commit before reviews.
> 4. Push; open a **draft** PR: `cd /c/src/quantick-worktrees/fix-contention-listed-tests && gh pr create --draft --base campaign/lean-a-plus --title "test(app): force the interleavings eight load-sensitive tests assert, and stamp every evidence revision" --body-file -` (adjust the title if D2 finds no product race) with a heredoc body per the PR template: tier, "Campaign child of #367 (Q10); closes on integration: #418, #408, #409, #410, #411, #413, #415, #416, #417", the D2 finding with evidence, the per-test table, the skip-list lines to remove, local verification, then `🤖 Generated with [Claude Code](https://claude.com/claude-code)` and `https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> 5. `/arch-review`, `/ai-review`, `/delivery-review`; resolve Blockers/Should-fixes; record markers with the shared key per integration.md.
> 6. Watch CI at the head (`gh pr checks <n> --watch`, bounded).
> 7. `gh pr ready <n>` once (D6).
> 8. Return a HANDOFF BLOCK: issues; branch; worktree; PR URL; head SHA; base tip; the D2 answer with evidence; per-test root cause and fix; the contention table; skip-list lines to remove; review verdicts with URLs; markers yes/no; CI run URL and conclusion; findings closed/open; ready accepted or denied; any human_decision; the coordinator's next action.

> Issue #418, *Acceptance criteria*, verbatim:
>
> - [ ] A1: the Null `capture_revision` question answered with evidence (product race or test timing), and fixed at the right layer, with a real-gateway regression test if product.
> - [ ] A2: each of the eight tests passes 24-copies-on-4-cores contention runs with 0 failures over at least 10 runs (the harness `tools/ci/contention.sh` from #414 with `CONTENTION_ROUNDS=10`, or its local equivalent), before/after table in the PR.
> - [ ] A3: no assertion weakened; any production change reviewed under the full gates.
> - [ ] A4: merged into `campaign/lean-a-plus` through a PR whose base is exactly that branch, with the merge read back; #408-#417 closed with integration evidence.
