# Mission: run the app tests under CPU contention in CI

Add a Linux CI step that runs the already-built `quantick-app` test binary as
many concurrent copies pinned to fewer cores, so a load-sensitive test fails in
the pull request that introduces it rather than later, on an unrelated one.
Four such tests (#361, #364, #385, #403) surfaced in sixteen hours of campaign
#367, each only when somebody else's CI went red; the interim assessment keeps
rubric criterion SE5 at 4 until a committed contention step exists.

**Tier:** `medium`. Campaign child Q9 of #367, assigned at `medium` by the
coordinator: a CI workflow change with a runtime cost to bound and a proof to
produce against a historical revision, but no Rust source change.

## Request ledger

- **R1** — A CI step on Linux runs the `quantick-app` test binary under
  deliberate CPU contention — N concurrent copies restricted with `taskset` to
  K cores, N clearly larger than K — on every pull request and push, and fails
  the check when any copy fails. *(#407 Scope bullet 1, A1; D1)*
- **R2** — The step reuses the binary the main test step builds: no second full
  build. *(#407 Scope bullet 2; D1)*
- **R3** — The log names which copy failed and which test failed in it, and the
  failing copy's output is kept (log or artifact). Verbatim: *"printing which
  test failed in which copy"*. *(#407 Scope bullet 1, A1; D1)*
- **R4** — The added wall-clock is bounded and stated: target under five
  minutes, measured on the PR's own CI runs, capped by a `timeout-minutes`, and
  the chosen N and K are explained. *(#407 Scope bullet 2, A3; D2)*
- **R5** — Proof the step catches the class: it fails against a revision where
  a known load-sensitive test is still unfixed, naming the test, and passes at
  the campaign tip; both outputs are recorded in the PR. *(#407 A2; D3)*
- **R6** — The step is documented in `ci.yml` comments, and in `CLAUDE.md`'s CI
  list only if the context ratchet allows, otherwise under `docs/`. *(#407
  Scope bullet 3)*
- **R7** — Out of scope: fixing any newly found load-sensitive test (each is
  filed as an issue with the failing copy's output and reported to the
  coordinator), and Windows. *(#407 Scope "Out of scope"; D4)*
- **R8** — Green at the final PR head. *(#407 A3)*
- **R9** — Delivered through a PR whose base is exactly `campaign/lean-a-plus`;
  the merge is the coordinator's and is read back by it. *(#407 A4;
  integration contract)*
- **R10** — Purpose: a load-sensitive test fails in the PR that introduces it,
  which is the assessor's observable condition for SE5's next point.
  *(#407 Context; the dispatch's opening paragraph)*

## Decisions (from the coordinator)

- **D1** — Linux only. Reuse the test binary the existing test step builds;
  no second full build. N concurrent copies, all restricted with `taskset` to
  K cores, N clearly larger than K (start from #404's 20 copies on one core;
  scale to the runner's cores and the time budget), `--test-threads` left at
  the default. Fail if any copy fails; print which copy and which test; keep
  each copy's output on failure.
- **D2** — Bound the added wall-clock: state the measured added time on the
  PR's own CI runs; target under five minutes; cap the step with
  `timeout-minutes`.
- **D3** — Proof by running the same script against a revision where a known
  flaky test is unfixed (the parent of #404's fix, or of #382's merge), showing
  it fails there naming the test and passes at the campaign tip. A throwaway
  branch is deleted afterwards and never targets `campaign/lean-a-plus` or
  `main`.
- **D4** — A new load-sensitive test found at the campaign tip is not fixed
  here: file an issue with the failing copy's output, report it in the
  handoff, and let the coordinator decide whether the step's first landing
  must pass.
- **D5** — Tier `medium`: `arch-review` with `code-review` at `low`, shape pass
  on the touched dimensions (CI, operability, English); `ai-review`
  completion; `delivery-review` completeness pass inline. At most three step-0
  rounds.
- **D14** — (coordinator decision, SendMessage to this child after the D4
  question, 2026-09-12) Option (a): land the contention step blocking, with a
  known-issue skip list that applies to the contention copies only. (1) The
  list lives in one committed file, one exact test name per line, each line
  citing its issue number; the step fails if a line lacks an issue. (2) The
  normal `Test` step is unchanged and still runs every test. (3) One issue per
  test found, with the failing copy output and the config that reproduced it,
  labelled `area:app` `type:fix`, milestone "v0.1 - Engine core", with the
  body line "Found by campaign #367 child Q9 (#407); a fix PR must remove the
  test's line from the contention skip list." (4) The skip list and the issue
  links are stated in the PR body and in the doc. (5) Recorded here with the
  coordinator's message as its source. No test source is edited.
- **D6** — `gh pr ready` once, cd-prefixed from the Bash tool, after reviews,
  markers and green CI. Never merge; never touch `main`.

## Assumptions

- **S1** — libtest's default `--test-threads` is
  `std::thread::available_parallelism()`, which on Linux honours the affinity
  mask `taskset` sets. So "left at the default" means each copy runs K test
  threads, not the runner's full core count. Safe to assume: it is what D1
  asks for, and it is the property that makes every copy's own worker threads
  compete for the same K cores.
- **S2** — The proof runs on a throwaway CI branch whose own workflow adds its
  name to the `push` trigger, so the real script runs on the real runner
  shape; no PR is opened for it. Safe to assume: D3 allows a throwaway branch,
  and a push trigger on that branch targets neither `campaign/lean-a-plus` nor
  `main`.
- **S3** — The helper script lives at `tools/ci/contention.sh` (POSIX sh),
  its skip list beside it, and the longer prose (calibration table, cost, the
  skip list at landing) at `docs/quality/app-tests-under-contention.md`, which
  the context ratchet does not track. `CLAUDE.md` is not touched: its own
  ceiling is 9,551 bytes and it measures 9,548, so a 118-byte pointer line
  failed `the_repository_is_within_its_context_budget` (+115). #407 says "only
  if the context ratchet allows; otherwise in `docs/`", so the ratchet did not
  allow and the prose went to `docs/`. Conventional placement; reversible in
  one edit.
- **S4** — N = 24, K = 4. D1 says start from #404's 20 copies on one core and
  scale to the runner and the budget. Measured on the 4-vCPU runner, 20 on 1
  core and every 2- and 3-core config caught nothing; on 4 cores, 24 copies
  caught #403's test before its fix in 3 of 9 runs and 20 copies in 1 of 3.
  24 is kept for the heavier contention per run; its step took 2:34 to 5:00
  over eighteen runs, 20's 2:06 to 3:54, the measured fallback. K = 4 is every vCPU the runner has,
  so `taskset` there only states the mask; N = 24 > K keeps D1's shape, and a
  larger runner is still pinned to 4. Safe to assume: D1 delegates the scaling,
  and the table is in the doc.

## Acceptance criteria

- [ ] **A1** — `.github/workflows/ci.yml` has a step in the Linux job, after
      `Test`, that runs the helper over the `quantick-app` test binary with N
      copies pinned to K cores (N > K), on every `pull_request` and `push` to
      `main`, and exits non-zero when any copy fails.
      *Evidence:* the step's text in the diff; the throwaway run where it
      failed. → PR body, "Proof". *(R1)*
- [ ] **A2** — The binary is located from the `Test` step's build
      (`cargo test --workspace --no-run --message-format=json`, the `Test`
      step's own invocation, which compiles nothing new), not rebuilt.
      *Evidence:* the step's log shows no `Compiling` line; the step's text.
      → PR body, "Design". *(R2)*
- [ ] **A3** — On failure the log names each failing copy and each failing
      test in it, prints the copy's failure section, and the copies' logs are
      uploaded as an artifact.
      *Evidence:* the failing throwaway run's log excerpt and artifact. → PR
      body, "Proof". *(R3)*
- [ ] **A4** — The step carries `timeout-minutes`, N and K are explained in the
      workflow comment, and the added wall-clock measured on this PR's own CI
      run is stated and is under five minutes.
      *Evidence:* the step's duration from `gh run view` at the final head.
      → PR body, "Cost". *(R4)*
- [ ] **A5** — The step, with the final script and skip list, fails on a
      revision where #403's test is unfixed (`f36cc022`, the parent of #404's
      fix `635f8936`), naming `the_trade_paint_layer_switch_stops_the_marks`,
      and passes at the campaign tip.
      *Evidence:* both runs' URLs and excerpts. → PR body, "Proof". *(R5, R10)*
- [ ] **A6** — The step is documented in `ci.yml` comments and in
      `docs/quality/app-tests-under-contention.md` (the ratchet refused a
      `CLAUDE.md` line; see S3).
      *Evidence:* the diff. → PR body. *(R6)*
- [ ] **A7** — No Rust source or test is changed; a new load-sensitive test
      found at the tip is filed as an issue and reported, not fixed; no
      Windows step is added.
      *Evidence:* `git diff --name-only` against the base; issue URL if any.
      → PR body, handoff. *(R7)*
- [ ] **A10** — Per D14: `tools/ci/contention-known-issues.txt` holds one
      exact test name and one issue per line; the script stops on a line with
      no issue or a name the binary does not have; the `Test` step is
      unchanged; one issue per found test is filed as D14 specifies; the list
      and issue links are in the PR body and the doc.
      *Evidence:* the file; the script's shim exercise (no-issue, stale,
      missing list); `git diff` of the `Test` step (none); issue URLs.
      → PR body. *(R7, R8)*
- [ ] **A8** — Every required check is green at the final PR head, the new
      step included.
      *Evidence:* `gh pr checks` at the head. → PR body, handoff. *(R8, R10)*
- [ ] **A9** — The PR's base is exactly `campaign/lean-a-plus`.
      *Evidence:* `gh pr view --json baseRefName`. → handoff. *(R9)*

## Gates

- [ ] **G1** — Every artifact in English (`CLAUDE.md`). *Evidence:* guards
      `language` check; arch-review dimension 8. → PR body.
- [ ] **G2** — Validation for a workflow/script change: YAML parses;
      `sh .claude/hooks/guardrails_test.sh` green; `cargo test -p
      quantick-guards` green; the four checks at the final head in CI (no Rust
      input changes, so the local fmt/clippy/build/test loop is run once on
      the final tree). *Evidence:* command exit codes. → PR body.
- [ ] **G3** — Performance impact declared: CI-only; no per-trade, per-depth or
      per-frame production path is touched; the cost is CI wall-clock, stated
      in A4. → PR body.
- [ ] **G4** — `arch-review` (step 0 `code-review` at `low`) with every
      Blocker/Should-fix resolved or deferred in the PR body; `ai-review`
      completion with zero unresolved threads. → PR body/comments.

## Closing steps

- **C1** — `delivery-review` completeness pass, inline, returns PASS.
- **C2** — The PR is open, reviewed, green and marked ready (D6).
- **C3** — The coordinator merges into `campaign/lean-a-plus` and reads the
  merge back (A4 of #407); not this child's step.

## Not applicable

- Hot path, user-visible surface, capability, trader action, engine
  determinism: the diff touches no Rust source, no UI and no engine.

## The request as received

The coordinator's dispatch, quoted verbatim (attributed quotation):

> You are executing campaign child mission **Q9** of campaign #367 (https://github.com/milocaetano/quantick/issues/367) for milocaetano/quantick: add a CI step that runs the app test binary under deliberate CPU contention, so a load-sensitive test fails in the PR that introduces it instead of later on an unrelated PR. This earns rubric criterion SE5's next point (4 → 5) by the observable condition an independent assessor named. You are a subagent: you cannot ask the trader; decisions D1..Dn below answer the mission's step-3 questions. A doubt that would need a new decision is reported back, never guessed.
>
> ## Decisions
> - D1: Linux only. Reuse the test binary the existing test step builds (locate it with `cargo test -p quantick-app --no-run --message-format=json` or the path cargo prints); do not add a second full build. Run N concurrent copies of that binary, all restricted with `taskset` to K cores where N is clearly larger than K (start from #404's 20 copies on 1 core; scale to the runner's cores and your time budget), each with `--test-threads` left at the default so the binary itself is parallel. Fail the step if any copy fails; print which copy and which test failed; keep each copy's output as an artifact or in the log on failure.
> - D2: bound the added wall-clock: state the measured added time on the PR's own CI runs; target under 5 minutes; cap the step with a `timeout-minutes`.
> - D3: proof (A2): demonstrate the step catches the class. Preferred: run the same script locally or in a throwaway CI branch against a revision where a known flaky test is still unfixed (for example the parent of #404's fix commit, `git log --oneline 86bbe06a..470e7e47` shows it; or the parent of #382's merge for #361), show it fails there naming the test, and show it passes at the campaign tip. A throwaway branch must be deleted afterwards and must not target `campaign/lean-a-plus` or `main`. Record both outputs in the PR body.
> - D4: if the step finds a NEW load-sensitive test at the campaign tip, do not fix it here: file an issue with the failing copy output, report it in the handoff, and make the step's first landing pass only if the coordinator decides (report as a human_decision-free coordinator question in the handoff).
> - D5: tier `medium`: `arch-review` with `code-review` at `low`, shape pass on the touched dimensions (CI, operability, English); `ai-review` completion; `delivery-review` completeness pass inline. At most three step-0 rounds.
> - D6: `gh pr ready` works with the cd-prefixed form from the Bash tool. Run it once after reviews, markers and green CI; if denied because the campaign tip moved or the PR is DIRTY, stop and report. **Never use the PowerShell tool for `gh pr` commands; never merge; never touch main.**

The task issue #407, quoted verbatim (attributed quotation):

> ## Scope
>
> - Add a CI step (or a separate job) on Linux that runs the `quantick-app` test binary under deliberate CPU contention, for example N parallel copies of the already-built test binary restricted with `taskset` to fewer cores than copies, with a bounded total time, failing the check on any test failure and printing which test failed in which copy.
> - Reuse the binary the main test step builds (no second full build); keep the added wall-clock bounded (state the number, target under 5 minutes) and explain the chosen N and core count.
> - Document the step in `.github/workflows/ci.yml` comments and in `CLAUDE.md`'s CI list only if the context ratchet allows; otherwise in `docs/`.
> - Out of scope: fixing any newly found flaky test in this PR (file each as an issue with the copy/test that failed); Windows.
>
> ## Acceptance criteria
>
> - [ ] A1: the contention step runs on every PR and push, uses the built test binary, and fails the check when a test fails in any copy; its logs name the test.
> - [ ] A2: proof it catches the class: run it against the pre-fix revision of one of the four fixed tests (for example the parent of #404's fix commit) in a throwaway branch or a local reproduction, and show the step fails there and passes at the campaign tip; record both outputs in the PR.
> - [ ] A3: green at the final PR head, with the added CI time stated.
> - [ ] A4: merged into `campaign/lean-a-plus` through a PR whose base is exactly that branch, with the merge read back.
