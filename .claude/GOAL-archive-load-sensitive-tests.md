# Mission: force the interleavings the two load-sensitive tests assert

Two tests make CI red at random: `delayed_producer_bookkeeping_does_not_block_real_flush_or_sample_recovery`
(#361) pins one producer/worker interleaving that nothing in the fixture
establishes, and `gateway_client_reads_the_running_application_and_wrong_tokens_fail_closed`
(#364) asserts a refusal code where a loaded machine can instead produce a
transport timeout. A+ gate 3 requires default-branch CI green at the assessed
revision, and every child PR of campaign #367 inherits the exposure.

**Tier:** `medium`. Campaign child Q1 of #367, assigned at `medium` by the
coordinator: two independent test harnesses in two subsystems, each needing a
design decision about where the ordering belongs, but no product change.

## Request ledger

- **R1** — #361: give the fixture a way to establish the delayed-producer
  interleaving instead of hoping for it. Verbatim: *"so the delayed-producer
  interleaving is established, not hoped for"*. Weakening the assertion to
  `Some(1) | Some(2)` is forbidden. *(issue #361 "What a fix needs"; Q1
  adoption comment, Scope bullet 1; D1, D3)*
- **R2** — #364: assert the refusal reason separately from the transport
  outcome, so a handshake that timed out under load reports as a timeout and
  not as a wrong-code failure; or retry a transport-level failure a bounded
  number of times before asserting the code. The test must still assert
  `control.auth_failed` for each wrong credential. *(issue #364 "What a fix
  probably needs"; Q1 adoption comment, Scope bullet 2; D1, D4)*
- **R3** — No production change to worker-progress semantics, handshake
  deadlines or authority, and no other tests touched. Verbatim: *"Out of
  scope: any change to worker progress semantics, handshake deadlines in
  production, or other tests."* *(Q1 adoption comment, Scope; D2)*
- **R4** — `cargo test -p quantick-app delayed_producer_bookkeeping` passes
  200 of 200 filtered runs under a concurrent `cargo build --workspace`, with
  the property under test (sample recovery after a delayed producer) still
  asserted. *(Q1 adoption comment, A1; D5)*
- **R5** — `gateway_client_reads_the_running_application_and_wrong_tokens_fail_closed`
  passes 200 of 200 runs under the same contention, still asserting
  `control.auth_failed` for each wrong credential. *(Q1 adoption comment, A2;
  D5)*
- **R6** — Both issues' rate tables are extended with the after numbers, in
  the PR body and as a comment on each issue. *(Q1 adoption comment, A3; D5)*
- **R7** — The change ships through a PR whose base is exactly
  `campaign/lean-a-plus`. *(Q1 adoption comment, A4; integration contract)*
- **R8** — Purpose: default-branch CI stops going red at random on these two
  tests, so gate 3 can be assessed. *(Q1 adoption comment, opening paragraph)*

## Decisions (from the coordinator)

- **D1** — Both tests are fixed in this one PR.
- **D2** — No production change to worker-progress semantics, handshake
  deadlines or authority. Only test/fixture/harness code changes. A genuinely
  required production change stops the mission and is reported instead.
- **D3** — #361 is fixed by giving the fixture a way to hold the producer's
  sampling (the same shape `Gate::hold` gives the worker's phases), so the
  delayed-producer interleaving is established. Weakening the assertion to
  accept `Some(1) | Some(2)` is forbidden.
- **D4** — #364 is fixed by asserting the refusal reason separately from the
  transport outcome, or by retrying a transport-level failure a bounded number
  of times before asserting the code; `control.auth_failed` stays asserted for
  each wrong credential when the handshake completes.
- **D5** — Proof of A1/A2: 200 filtered iterations of each test while a
  concurrent `cargo build --workspace` competes for CPU, before and after the
  fix; both tables recorded in the PR body and on both issues.
- **D6** — Tier `medium`: `arch-review` with `code-review` at `low` and the
  full shape pass; `delivery-review` completeness pass only, inline;
  `ai-review` completion required.

## Assumptions

- **S1** — *Wanted to ask.* D3 asks for a hold on the producer's sampling. The
  producer's sampling is `WorkerProgress::record_send`, which the fixture calls
  on its own thread: its timing is already fully under the test's control, so a
  producer-side hold cannot remove any interleaving. The unforced party is the
  worker consumer, which reads the sample slot inside `SharedProgress::begin`
  before it signals `Phase::Applying`, so no existing phase callback can fence
  that read, and adding one would be the production change D2 forbids. The
  fixture therefore establishes the interleaving with the mechanism D3 names —
  `Gate::hold` — applied to the worker: the worker is parked in the `Phase::Idle`
  callback that ends the previous cycle, so the producer's whole S2 step
  completes before the worker can admit it. This is exactly the pattern the S3
  step of the same schedule already uses; S2 was the one step that omitted it.
  Safe to assume because it satisfies D3's operative requirement (the
  interleaving is established, not hoped for) with no production change, and
  because the alternative readings all require touching `worker_progress.rs`.
- **S2** — *Wanted to ask.* Parking the worker in the `Phase::Idle` callback of
  the S1 cycle moves the S1 flush acknowledgement after the producer's late
  `record_send`, because the worker sends flush acknowledgements only after
  `finish()` returns. The S1 property ("delayed producer bookkeeping does not
  block the real flush") is preserved and strengthened by asserting, at the
  parked point, that the worker completed the whole cycle — `Phase::Idle`,
  `cycles == 1`, `mailbox_replacements == 1` — while the producer had still
  recorded no acceptance at all. Safe to assume because the observable property
  is stronger, not weaker.
- **S3** — A handshake that misses its deadline surfaces as
  `control.instance_gone` (`instance_gone_error()` in
  `crates/control-local/src/client.rs`), which is the transport outcome D4
  separates from the refusal reason. Read from the code, not from a captured
  failure — #364 records that the failing assertion was never captured.
- **S4** — The bounded retry count for a transport-level handshake failure is
  four attempts. Nobody chose a number; four keeps a genuinely broken gateway
  loud while absorbing a loaded machine, and the exhaustion message names the
  transport outcome so it can never be read as a wrong-code failure.
- **S5** — *Wanted to ask.* #364's presumed mechanism is disproved by the first
  capture of its failure. Under this mission's contention the test failed at
  `control_plane_tests.rs:852` — "the application frame must drain the queued
  gateway request", left 1 right 0 — never at a handshake assertion. The drain
  is bounded by `CONTROL_UI_BUDGET_US` (250 microseconds per frame), which a
  loaded machine can spend before the drain starts. The evidence-driven repair
  is therefore the one shipped; D4's hardening is also applied, because it
  costs little and the handshake path carries the same class of exposure. Safe
  to assume because A2 — 200 of 200 under contention, `control.auth_failed`
  still asserted for each wrong credential — is met either way, and applying
  only D4's reading would have left the test failing at the same rate.
- **S6** — A third load-sensitive point in the same test, its teardown
  (`disable_test_gateway`, "test gateway did not stop cleanly"), surfaced after
  the first repair. Its waiters counted 400 sleep iterations rather than
  elapsed time; they are now bounded by wall clock. These are shared harness
  helpers rather than "other tests": no other test's asserted property changes,
  only how long each waits, so this stays inside the scope line.
- **S7** — Two sibling control-plane tests
  (`a_retry_that_races_its_own_first_call_is_refused_rather_than_acted_on`,
  `gateway_a_client_that_never_reads_does_not_stall_another`) carry the same
  exposure and are explicitly out of scope. Measured at the same rate before
  and after this diff (3 failures in 26 full-suite runs on each binary), so
  nothing here regressed them; reported to the coordinator instead of fixed.

## Acceptance criteria

- [x] **A1** — The #361 schedule establishes the producer/worker interleaving
      it asserts: the producer's ticket-2 acceptance is recorded before the
      worker can admit it, and `last_sampled_ticket` is `Some(1)` by
      construction rather than by luck. The assertion is not weakened.
      *Evidence:* the diff of `crates/app/src/worker_progress/tests/protocol.rs`
      plus the after table below. → PR body. *(R1, R3)*
- [x] **A2** — `cargo test -p quantick-app delayed_producer_bookkeeping` passes
      200 of 200 filtered runs under a concurrent `cargo build --workspace`.
      *Evidence:* the before/after run table. → PR body and a comment on #361.
      *(R4, R6)*
- [x] **A3** — The #364 test separates the refusal reason from the transport
      outcome: a handshake that fails at the transport level is retried a
      bounded number of times and, if it never completes, reported as a
      transport failure naming `control.instance_gone`, never as a wrong-code
      failure. `control.auth_failed` stays asserted for the wrong bearer token,
      the wrong instance id and the wrong process nonce.
      *Evidence:* the diff of `crates/app/src/app/tests/control_plane_tests.rs`.
      → PR body. *(R2, R3)*
- [x] **A4** — `gateway_client_reads_the_running_application_and_wrong_tokens_fail_closed`
      passes 200 of 200 runs under the same contention.
      *Evidence:* the before/after run table. → PR body and a comment on #364.
      *(R5, R6)*
- [x] **A5** — No file outside the two test modules changes behaviour; nothing
      under `crates/app/src/worker_progress.rs`, `crates/app/src/control/` or
      `crates/control*/src/` is modified. *Evidence:* `git diff --stat` against
      the campaign base. → PR body. *(R3)*
- [ ] **A6** — The PR base is exactly `campaign/lean-a-plus` and the PR links
      #361, #364 and campaign #367. *Evidence:* `gh pr view` base field. → PR.
      *(R7)*

## Injected gates

- [x] **G1** — Every artifact in English; conventional commits.
      *Evidence:* `cargo test -p quantick-guards` and the commit log. → PR body.
- [x] **G2** — `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets`, `cargo build --workspace`, `cargo test --workspace`, each
      run on its own, green at the final head. *Evidence:* command exit codes.
      → PR body.
- [x] **G3** — Performance impact declared per touched path by rate.
      *Evidence:* a paragraph in the PR body. Expected: test-only, rare path.
      → PR body.
- [ ] **G4** — `arch-review` over the exact diff against `origin/campaign/lean-a-plus`,
      its step 0 bug pass included, every Blocker and Should-fix resolved or
      deferred in the PR body; `ai-review` completion recorded;
      `delivery-review` completeness pass. *Evidence:* review verdicts and the
      recorded markers. → PR body.
- [ ] **G5** — CI green at the final PR head. *Evidence:* `gh pr checks`.
      → PR.

## Not applicable

- *Touches a hot path* — the diff is confined to `#[cfg(test)]` modules; no
  per-trade, per-depth or per-frame path is touched.
- *Touches anything user-visible* — no UI surface changes, so `ui-harness`,
  `visual-qa` and `trader-ux-review` do not apply.
- *Adds a capability* — nothing docks; no new feed, bar type, indicator, layer,
  panel or crate.
- *Adds something a trader does* — no new action, tool, trade or lock.
- *Engine / determinism territory* — the change is itself a determinism repair
  in a test harness; no engine code and no golden fixture changes.
- *Docs/skills only* — this is a code change, so the full shape pass applies.

## Measured evidence

All runs on this machine, one filtered test per process, `--test-threads=1`,
while a loop of `cargo build --workspace` over the whole workspace competed for
all twelve cores.

| Test | Binary | Runs | Failures | Failure seen |
| --- | --- | ---: | ---: | --- |
| #361 `delayed_producer_bookkeeping…` | before | 200 | 1 | `protocol.rs:92`, left `Some(2)` right `Some(1)` |
| #361 `delayed_producer_bookkeeping…` | after | 200 | 0 | — |
| #364 `gateway_client_reads…` | before | 200 | 1 | `control_plane_tests.rs:852`, "the application frame must drain the queued gateway request", left 1 right 0 |
| #364 `gateway_client_reads…` | after, first repair | 200 | 1 | `mod.rs:1970`, "test gateway did not stop cleanly" |
| #364 `gateway_client_reads…` | after, final | 200 | 0 | — |

Whole `quantick-app` test binary, 26 runs of each binary, same machine:

| Binary | Runs | Failures | Which tests |
| --- | ---: | ---: | --- |
| before | 26 | 3 | `a_retry_that_races_its_own_first_call_is_refused_rather_than_acted_on` ×3 |
| after | 26 | 3 | `a_retry_that_races…` ×1, `gateway_a_client_that_never_reads_does_not_stall_another` ×2 |

Neither in-scope test dropped in any of those 52 full-suite runs; the two
sibling tests that did are explicitly out of scope (S7) and fail at the same
rate on both binaries.

## Closing steps

- **C1** — `delivery-review` returns PASS (completeness pass, tier `medium`).
- **C2** — The PR is open against `campaign/lean-a-plus` and marked ready.
- **C3** — The after numbers are posted as a comment on #361 and on #364.

## The request as received, verbatim

> *Attributed quotation under `CLAUDE.md`'s language exemption; the campaign
> task comment is reproduced as it stands on both issues.*

### Q1 adoption comment (identical on #361 and #364)

> <!-- campaign-task:milocaetano/quantick#367/Q1 -->
> ## Campaign child: gate 3 reliability
>
> Campaign child of https://github.com/milocaetano/quantick/issues/367 (task key Q1). Base: `campaign/lean-a-plus`. Mission tier: medium. This comment adopts this issue and #364 together as one mission (it owns criterion SE5, scored 4 at this SHA for exactly these two tests): both are load-sensitive tests that make default-branch CI red at random (about one run in eighty under CPU contention; main run 34619523514 at `47004db5` failed on the #361 test and the rerun at `e8eb23e543dcc827c16c7c552478e7ebf2150a4f` passed). A+ gate 3 requires default-branch CI green at the assessed revision, and every child PR of this campaign inherits the exposure, so this mission dispatches first.
>
> ### Scope
>
> - #361: give the fixture a way to hold the producer's sampling the way `Gate::hold` holds the worker's phases, so "the producer's bookkeeping is still delayed" is a fact the test establishes. Weakening the assertion to `Some(1) | Some(2)` is not the fix.
> - #364: assert the refusal reason separately from the transport outcome, so a handshake that timed out under load reports as a timeout, not as a wrong-code failure; or retry a transport failure before asserting, whichever the transport owner's reading supports. No product change to timeouts or authority.
> - Out of scope: any change to worker progress semantics, handshake deadlines in production, or other tests.
>
> ### Acceptance criteria
>
> - [ ] A1: `cargo test -p quantick-app delayed_producer_bookkeeping` passes 200 of 200 filtered runs while a concurrent `cargo build --workspace` competes for CPU, and the property under test (sample recovery after a delayed producer) is still asserted.
> - [ ] A2: `gateway_client_reads_the_running_application_and_wrong_tokens_fail_closed` passes 200 of 200 runs under the same contention, still asserting `control.auth_failed` for each wrong credential.
> - [ ] A3: both issues' rate tables are extended with the after numbers in the PR.
> - [ ] A4: merged into `campaign/lean-a-plus` through a PR whose base is exactly that branch, with the merge read back.
>
> ## Gates
>
> - G1: every artifact English; conventional commits.
> - G2: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo build --workspace`, `cargo test --workspace` green on the final head, run one at a time.
> - G3: `arch-review` over the exact diff against the campaign base, its step 0 bug pass included; every Blocker and Should-fix resolved or deferred in the PR body. `ai-review` completion recorded. `delivery-review` per tier.
> - G4: performance impact declared per touched path by rate; no per-trade, per-depth or per-frame path grows with session length.
>
> ## Campaign assignment
>
> Parent: https://github.com/milocaetano/quantick/issues/367. Stable key: Q1. Class: autonomous. Priority: 1. Risk: low: test-only change in two test modules. Executor: opus - test harness design in two subsystems. Dependencies: none. Project item ID: null (Project writes pending token scope). Branch, worktree owner, PR URL/head: null until claimed in a parent checkpoint. Retry counters start at operation=0, repair=0 and persist by failure signature. Evidence destination: this issue's comments and the linked PR.
> Validation plan: first row (tests changed): the two filtered tests under contention, then the full ordered loop before every commit, CI at the final head.

### Issue #361 — "What fails"

> `orderflow_worker::progress_tests::delayed_producer_bookkeeping_does_not_block_real_flush_or_sample_recovery`, at `crates/app/src/worker_progress/tests/protocol.rs:92`:
>
> ```text
> assertion `left == right` failed
>   left: Some(2)
>  right: Some(1)
> ```
>
> The second panic in the log, `fixture releases worker: Disconnected` at `crates/app/src/worker_progress/tests.rs:81`, is teardown unwinding after the first — not a separate defect.
>
> Observed in CI on [run 34528139191](https://github.com/milocaetano/quantick/actions/runs/34528139191/job/103041998612).

### Issue #364 — "What fails"

> `app::tests::control_plane_tests::gateway_client_reads_the_running_application_and_wrong_tokens_fail_closed`, during `cargo test --workspace`.
