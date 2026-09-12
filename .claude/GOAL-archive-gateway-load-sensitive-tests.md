# Mission: make two more load-sensitive gateway tests deterministic

Force the interleavings that `gateway_a_client_that_never_reads_does_not_stall_another`
and `a_retry_that_races_its_own_first_call_is_refused_rather_than_acted_on` assert,
so neither test depends on a single application frame winning a race against
machine load. Both are pre-existing flakes on `campaign/lean-a-plus`; they make
every child PR's CI, and the campaign's own green-CI gate, load-sensitive.

**Tier:** `small` — test-only change in one test module, reusing the helper and
the shape PR #382 already established and reviewed; no production file, no new
design decision. Campaign child Q6 of #367, issue #385.

## Request ledger

- **R1** — Make `gateway_a_client_that_never_reads_does_not_stall_another`
  deterministic under load. *(A1, A2)*
- **R2** — Make `a_retry_that_races_its_own_first_call_is_refused_rather_than_acted_on`
  deterministic under load. *(A1, A2)*
- **R3** — Keep the property each test asserts: a non-reading client does not
  stall another; a retry racing its own first call is refused, not acted on.
  "Weakening an assertion is forbidden." *(A2)*
- **R4** — No production change: not to the gateway, its UI budget, its
  timeouts or its authority. *(A3)*
- **R5** — Prove it: each test 200 of 200 filtered runs under a concurrent
  `cargo build --workspace`, before and after, tables in the PR body and on
  issue #385. *(A1)*
- **R6** — So that the campaign base and every child PR stop inheriting a
  CI-red coin flip. *(A1)*

## Decisions (from the coordinator)

- **D1** — Both tests ship in one PR.
- **D2** — No production change. If one were genuinely required: stop and report.
- **D3** — Fix shape is #382's: drain the gateway with `drain_gateway_requests`
  (or the same shape) until the expected effect is observed within a bounded
  budget, instead of asserting after one frame.
- **D4** — Proof shape: 200 filtered runs per test, before and after, under a
  concurrent workspace build; both tables in the PR body and on #385.
- **D5** — Tier `small`: `arch-review` with `code-review` at `low`; `ai-review`
  completion required; no `delivery-review`.
- **D6** — `gh pr ready` once, cd-prefixed, from the Bash tool; never merge.

## Assumptions

- **S1** — The first test's premature break (`iteration >= 10 && queued == 0`)
  is the load exposure: under contention the live client's request has not yet
  reached the queue at 50 ms, the loop breaks on an empty queue, and the
  subsequent `read()` has nothing to read. Waiting for the request to be queued
  before draining removes the guess. Safe to assume: it is read off the test's
  own code, and the after-table measures it.
- **S2** — The expected queue depth at that point is
  `CONTROL_MAX_IN_FLIGHT_PER_CONNECTION + 1` (8 stalled snapshot requests plus
  the live one), under the test's queue capacity of 16, because no frame runs
  between the sends and the wait and the worker-side `describe` never queues.
  Safe to assume: both constants are in the test's own scope, and the wait
  fails loudly with the count if it is wrong.

## Acceptance criteria

- [ ] **A1** — Each of the two tests passes 200 of 200 filtered runs while a
      concurrent `cargo build --workspace` competes for CPU, and the before
      table shows the failures the fix removes.
      *Evidence:* before/after run tables. → PR body and a comment on #385. *(R1, R2, R5, R6)*
- [ ] **A2** — Every assertion each test made before the change is still made
      after it; only the waiting shape changed.
      *Evidence:* the diff, read in `arch-review`. → PR diff and review verdict. *(R1, R2, R3)*
- [ ] **A3** — No production file in the diff; the changed paths are the test
      module(s) only.
      *Evidence:* `git diff --stat` against the campaign base. → PR body. *(R4)*
- [ ] **G1** — Every artifact English; conventional commits.
      *Evidence:* the commits and the PR body. → PR.
- [ ] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace` green on the final
      head, run one at a time; `cargo test -p quantick-guards` after edits.
      *Evidence:* command output, plus CI at the head. → PR body and the CI run.
- [ ] **G3** — `arch-review` over the exact diff against `campaign/lean-a-plus`
      with its step 0 bug pass; `ai-review` completion recorded.
      *Evidence:* review verdicts and the marker files. → PR threads and the git dir.

## Performance impact

None on any production path: the diff is test-only. The changed code runs at
test rate, and the new waiting shape runs fewer frames on an idle machine than
the fixed 400-iteration loop it replaces.

## Not applicable

- Hot path, user-visible surface, new capability, trader action, engine
  determinism: the diff changes no production code and no UI surface.
- `delivery-review`: tier `small` (D5).
- `guardrails_test.sh`: no hook changes.

## Closing steps

- **C1** — Draft PR open against `campaign/lean-a-plus`, reviews and markers
  recorded, CI green at the head, `gh pr ready` run once.
- **C2** — Handoff block returned to the campaign coordinator.

## The request as received (verbatim)

> You are executing campaign child mission **Q6** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick:
> make two more load-sensitive gateway tests deterministic. You are a subagent:
> you cannot ask the trader; decisions D1..Dn below answer the mission's step-3
> questions. A doubt that would need a new decision is reported back, never
> guessed.
>
> [...] D1: both tests in one PR:
> `gateway_a_client_that_never_reads_does_not_stall_another` and
> `a_retry_that_races_its_own_first_call_is_refused_rather_than_acted_on`.
> D2: no production change to the gateway, its UI budget, timeouts or authority.
> If you conclude a production change is genuinely required, stop, write the
> reasoning in your handoff, and do not make it.
> D3: the fix shape is the one #382 established: drain the gateway with the
> helper (or the same shape) until the expected effect is observed within a
> bounded budget, instead of asserting after one frame; the property under test
> stays asserted (a non-reading client does not stall another; a retry racing
> its own first call is refused, not acted on). Weakening an assertion is
> forbidden.
> D4: proof of A1: run each filtered test 200 times while a concurrent
> `cargo build --workspace` (or `cargo test --workspace --no-run`) runs in
> another process, before and after the fix; record both tables in the PR body
> and as a comment on #385.
> D5: tier `small`: `arch-review` with `code-review` at `low`; `ai-review`
> completion is still required (every tier); no delivery-review.
> D6: `gh pr ready` works with the cd-prefixed form from the Bash tool. Never
> use the PowerShell tool for `gh pr` commands; never merge; never touch main.
