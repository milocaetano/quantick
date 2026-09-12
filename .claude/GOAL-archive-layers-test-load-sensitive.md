# Mission — force the frame the trade paint layer switch test asserts

**Objective:** make `app::tests::layers_tests::the_trade_paint_layer_switch_stops_the_marks`
deterministic under CPU load by having the fixture establish the state its
shape-count comparison depends on, so default-branch CI stops going red at
random on an assertion the product never broke.

**Tier:** `small`. Campaign child of #367 (task key Q8) for issue #403: one
test in one test module, no production file, no contract, wire or financial
rule in reach. Nothing here is the trader's call, so no interrogation and no
`delivery-review` (the small-tier exemption).

**Campaign base:** `origin/campaign/lean-a-plus` at `86bbe06a`. PR base is
exactly `campaign/lean-a-plus`.

## Request ledger

- **R1** — find what the assertion at `layers_tests.rs:565` depends on that the
  fixture does not force (a frame count, a worker publish, a shape cache) and
  make the test establish it, the way #382 and #400 did.
- **R2** — *"Weakening the assertion is not the fix"*: the property — switching
  the trade paint layer off stops the marks — stays asserted, and the live
  paper layer assertion stays.
- **R3** — test-only: no production file changes.
- **R4** — proof: 200 of 200 filtered runs pass while a concurrent
  `cargo build --workspace` competes for CPU, with a before/after table in the
  PR and on #403.
- **R5** — merged into `campaign/lean-a-plus` through a PR whose base is exactly
  that branch, with the merge read back (the coordinator's step).
- **R6** — the closing purpose: A+ gate 3, default-branch CI green at the
  assessed revision, no longer at the mercy of this test.

## Decisions (from the coordinator, answering step 3)

- **D1** — test-only; a production change that turns out to be required is
  reported, not made.
- **D2** — reproduce first (200 runs under a concurrent build, capture a
  failing assertion), then make the test establish the missing state with a
  bounded wait on the observable condition; the property stays asserted.
- **D3** — the 200-run table before and after, in the PR body and on #403.
- **D4** — tier `small`: `arch-review` with `code-review` at `low`;
  `ai-review` completion; no delivery-review.
- **D5** — `gh pr ready` once, Bash tool, `cd` prefix; never merge, never touch
  main, never rebase.

## Assumptions

- **S1** — the failing CI assertion (run 34700339682, attempt 1: "the marks
  kept painting with their layer off (309 vs 224)") is the order-flow worker
  publishing a projection frame between the "on" and the "off" draw: the
  "off" count is *larger*, which only something arriving between draws can
  explain. Safe: confirmed or refuted by the reproduction before any edit.
  *Outcome:* confirmed, and refined — convergence takes two worker round
  trips, not one (the live lane waits for the published live edge). A first
  fix forcing only one trip failed 118 of 200 under the pinned harness.
- **S3** — the D2 harness (one process at a time beside a concurrent
  `cargo build --workspace`) did not reproduce on this 12-core machine: 600 of
  600 passed on the base. The reproduction adds contention of the kind the CI
  runner has: 20 copies at a time, every copy pinned to one core
  (`start /affinity 1`), still beside the concurrent build. Safe: it forces
  the same interleaving harder and reproduced the CI assertion verbatim; both
  harnesses are reported.
- **S2** — the shared `mod.rs` helper is left alone unless a second test needs
  the same settling. Safe: reversible in one edit.

## Acceptance criteria

- [x] **A1** — the root cause is named from a captured failure, not a guess.
      *Evidence:* the failing run's assertion output under contention.
      → the PR body's root-cause section. *(R1)*
- [x] **A2** — the test passes 200 of 200 filtered runs under a concurrent
      `cargo build --workspace`, against a before run on the base binary.
      *Evidence:* the before/after table. → the PR body and a comment on #403.
      *(R1, R4, R6)*
- [x] **A3** — `marks_off < marks_on` and the live paper layer assertion are
      still asserted, unweakened.
      *Evidence:* the diff of `layers_tests.rs`. → the PR body. *(R2)*
- [x] **A4** — no production file changes.
      *Evidence:* `git diff --name-only origin/campaign/lean-a-plus...HEAD`.
      → the PR body. *(R3)*
- [ ] **A5** — the PR's base is exactly `campaign/lean-a-plus`.
      *Evidence:* `gh pr view --json baseRefName`. → the handoff. *(R5)*
- [ ] **G1** — every artifact in English; conventional commits.
      *Evidence:* `cargo test -p quantick-guards` and the commit log. → the PR.
- [ ] **G2** — the four checks green, run one at a time, and CI green at the
      exact PR head. *Evidence:* command output; `gh pr checks`. → the PR.
- [ ] **G3** — performance impact declared: every touched path is
      `#[cfg(test)]`, rate "per test run". *Evidence:* the PR body's
      performance section. → the PR.
- [ ] **G4** — `arch-review` with every Blocker and Should-fix resolved or
      deferred; `ai-review` completion recorded with zero open threads.
      *Evidence:* review verdicts and markers. → the PR.

## Closing steps

- **C1** — the draft PR is open against `campaign/lean-a-plus` and marked
  ready once reviews, markers and CI are green.
- **C2** — the campaign merge and its read-back (R5) belong to the
  coordinator.

## Not applicable

- *Touches a hot path*, *user-visible*, *adds a capability*, *adds something a
  trader does*, *engine territory* — the diff is one `#[cfg(test)]` module.

## The request as received

> Attributed quotation: the coordinator's delegation for campaign #367, task Q8,
> and issue #403's scope and acceptance criteria, verbatim.

> You are executing campaign child mission **Q8** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: make the load-sensitive test `app::tests::layers_tests::the_trade_paint_layer_switch_stops_the_marks` deterministic. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision is reported back, never guessed.
>
> ## Decisions
> - D1: test-only. If you conclude a production change is genuinely required, stop, write the reasoning in your handoff, and do not make it.
> - D2: first reproduce: run the filtered test 200 times while a concurrent `cargo build --workspace` (or `cargo test --workspace --no-run`) runs in another process, and capture a failing run's assertion output; then identify what the fixture does not force (a frame count, a worker publish, a shape cache refresh) and make the test establish it with a bounded wait on the observable condition, the shape #382/#400 used. The property under test (switching the trade paint layer off stops the marks) stays asserted.
> - D3: proof: the 200-run table before and after, in the PR body and as a comment on #403.
> - D4: tier `small`: `arch-review` with `code-review` at `low`; `ai-review` completion is still required; no delivery-review.
> - D5: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI; if denied because the campaign tip moved, stop and report. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.** Do not rebase; the coordinator does that at integration time.
>
> Issue #403, *Scope*:
>
> - Find what the assertion at `layers_tests.rs:565` depends on that the fixture does not force (frame count, a worker publish, a shape cache) and make the test establish it, the way #382 and #400 did for the gateway tests.
> - Test-only change. Weakening the assertion is not the fix.
>
> Issue #403, *Acceptance criteria*:
>
> - [ ] A1: the test passes 200 of 200 filtered runs while a concurrent `cargo build --workspace` competes for CPU, before/after table in the PR; the property (switching the trade paint layer off stops the marks) is still asserted.
> - [ ] A2: no production file changes.
> - [ ] A3: merged into `campaign/lean-a-plus` through a PR whose base is exactly that branch, with the merge read back.
