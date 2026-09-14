# Mission: session-length-test-cheap

Make the session-length fast variant cheap enough that it no longer dominates
the `quantick-app` test binary or times out the contention harness, while it
still proves that hot-path work per unit is independent of session length.
Why it matters: the test stretched the binary from about 8 s to 35 s on CI,
every PR pays that, and the contention step (#414) cannot land while all 24
copies time out.

**Tier:** `small` — set by the campaign coordinator. One test module and one
quality doc change; no production code, no CI workflow change (the preferred
fix held, so the `#[ignore]` fallback and its CI step were not needed).

Campaign child of #367 (Q14). Base: `campaign/lean-a-plus`. Issue: #435.

## Request ledger

- **R1** — Keep the proof in CI but stop it dominating the default test run.
  *(coordinator decision, bullet 1)*
- **R2** — Preferred fix: a cheap fast variant whose counts stay independent of
  session length "with enough size contrast to mean something" (at least 10x,
  #435 coordinator note); the heavy sizes stay in the `--ignored` `long` test.
  *(coordinator decision, bullet 2; #435 comment)*
- **R3** — Fallback only if the cheap variant cannot stay honest: `#[ignore]`
  plus a dedicated release CI step with `--ignored --exact`. *(bullet 3)*
- **R4** — No skip line in the contention known-issues list (D25). *(bullet 4)*
- **R5** — Measure the `quantick-app` test binary's wall time before and after
  on this host; target near 8–10 s, no single test above about 10 s.
  *(coordinator, Measure)*
- **R6** — Purpose: #414 can land and pass its 10 consecutive green contention
  rounds. *(coordinator, Problem)*

## Decisions (from the coordinator)

- **D1** — Preferred fix first; `#[ignore]` + dedicated CI step only as fallback.
- **D2** — D25: no known-issues skip line.
- **D3** — Tier `small`; `gh pr ready` once; never merge.

## Assumptions

- **S1** — The fast variant's short session may drop below one minute (to
  20 s, 6,000 prints). Safe: the growth check reads the contrast, not the
  absolute size; 6,000 prints is 120 tick:50 bars, the book projection's whole
  window, so both sessions project the same visible work; every per-unit
  count in the new table is flat between the two sessions, and the planted
  per-frame tape clone still fails the check at the new lengths.
- **S2** — The long variant (`long`) keeps its sizes and window unchanged
  (18,000 vs 3,960,000 prints, 1,800 frames), so the committed gate-6 evidence
  still describes it. Safe: only the fast pair moved; `measure` now takes both
  sessions explicitly.
- **S3** — The one-chunk growth checks keep a ten-minute tape
  (`GROWTH_SESSION`, 180,000 prints). Safe: they must pass print 131,072,
  where a contiguous tape first copies more than one chunk, and they build
  only the chart's ingest (0.04 s).
- **S4** — "This host" is the 12-core Windows worktree host, loaded by other
  campaign agents building in parallel; the rest of the binary alone takes
  about 14 s here, so "near 8–10 s" is read as "the binary no longer waits on
  this test", judged against the binary with the test skipped. Safe: the CI
  run on this PR reports the 4-core figure.
- **S5** — The contention harness (`tools/ci/contention.sh`) is not on this
  base; Q9 (#414) reruns its rounds. Safe: the coordinator scheduled that. The
  local stand-in is 24 copies of this one test pinned to processor mask `0xF`
  (4 of 12 cores), the Q10 (#418) reproduction shape.
- **S6** — A necessary detour, tied to R6: under load the application rig's
  "the fixture draws a lane" assertion failed in 7 of 36 concurrent copies on
  the *base* binary (2 of 36 after the resize alone). The book worker
  publishes asynchronously and the rig flushed only the indicator worker. The
  rig now also flushes the book worker after every frame (`AppRig::settle`,
  outside the lap, so nothing it does is counted). Without that, #414's rounds
  would fail on this test's assertion once they stop timing out. The settle
  applies to the `long` variant's rig too, where it only removes the same
  load dependence. Safe: test-only; the counted work is unchanged.

## Acceptance criteria

- [x] **A1** — `hot_path_work_is_independent_of_session_length` runs at a
      tenfold contrast (6,000 vs 60,000 prints) over a 60-frame window, every
      path's counts flat, and passes.
      *Evidence:* the test's table from `cargo test -p quantick-app
      session_length_tests -- --nocapture`. → PR body. *(R1, R2)*
- [x] **A2** — The check still has teeth at the new lengths:
      `the_check_fails_a_path_whose_work_grows_with_the_session` passes (a
      planted per-frame tape clone breaks the growth budget).
      *Evidence:* test result. → PR body. *(R2)*
- [x] **A3** — The `long` variant's sessions and window are unchanged.
      *Evidence:* the diff (`LONG_SESSIONS = [SHORT_SESSION, LONG_SESSION]`,
      `LONG_WINDOW` untouched). → PR body. *(R2)*
- [x] **A4** — Before/after wall time of the `quantick-app` test binary and the
      test's own time on this host; the test is no longer the binary's
      longest and is below about 10 s.
      *Evidence:* timing table. → PR body. *(R5, R6)*
- [x] **A5** — No skip line added; no `#[ignore]` added; no CI workflow change.
      *Evidence:* the diff's file list. → PR body. *(R3, R4)*
- [x] **A6** — Under contention the test passes with no lane-assertion failure:
      36 concurrent copies on 12 cores and 24 copies pinned to 4 cores, all ok.
      *Evidence:* stress-run counts, before and after. → PR body. *(R6)*

## Gates

- **G1** — English everywhere (`CLAUDE.md`), graded by `arch-review` dimension 8
  and the language guard.
- **G2** — Four checks green, each run on its own, plus `cargo test -p
  quantick-guards`; performance impact declared: test-only change, no
  production path touched (no per-trade / per-depth / per-frame code).
- **G3** — `arch-review` (step 0 `code-review` at `low`, shape dimensions the
  diff touches, 8 always) with every Blocker/Should-fix resolved or deferred in
  the PR body.
- **G4** — Final-head CI green.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

Evidence destinations: G-AI1 and G-AI2 — the ai-review report comment on the
PR; G-AI3 — the `ai_review_threads.sh list` output in the ship-gate
reconciliation; G-AI4 — the private `ai-review-complete` projection checked by
`mission_ship_gate.sh`.

## Not applicable

- Hot-path evidence: no production path changes; the test's own table is the
  measurement.
- User-visible, capability, trader-action and engine rows: none touched.
- `delivery-review`: exempt at `small` within the diff-size ceiling.

## Closing steps

- **C1** — Open PR against `campaign/lean-a-plus`, ready after green CI.
- **C2** — `mission_ship_gate.sh ship <n>` PASS.

## Verbatim request

> Execute campaign task **Q14**, issue https://github.com/milocaetano/quantick/issues/435 (read it in full with `gh issue view 435`), for campaign #367 (https://github.com/milocaetano/quantick/issues/367). Mission tier **small**, through this repo's `/mission` workflow. You are a subagent and cannot ask the trader. Make routine calls and record them in the goal file and PR body.
>
> **Problem.** `hot_path_work_is_independent_of_session_length` (`crates/app/src/app/tests/session_length_tests.rs`, from #431) stretches the `quantick-app` test binary from about 8 s to 35 s. The contention harness from #414 / Q9 (`tools/ci/contention.sh`, 24 copies on 4 cores, 360 s per copy) therefore times out every copy. That blocks #414 from landing, and #414 has to pass 10 consecutive green contention rounds.
>
> **Decision, made by the coordinator:**
> - Keep the proof in CI, but make it stop dominating the default test run.
> - Preferred fix: make the fast variant cheap while it still proves what it claims. That means the counts stay independent of session length, with enough size contrast to mean something; the heavy sizes live in the `--ignored` `long` test that gate 6 already runs in release.
> - If a cheap variant cannot keep the proof honest, fall back to `#[ignore]` on the test plus a dedicated step in `.github/workflows/ci.yml` that runs it alone, in release, with `--ignored --exact`. That keeps it running on every PR, outside the contention copies.
> - Do not add a skip line to the contention known-issues list; decision D25 forbids that.
>
> **Measure** the `quantick-app` test binary's wall time before and after on this host and report it. Target: back near 8–10 s, with no single test above about 10 s.
>
> (The worktree, checks and delivery instructions that follow in the coordinator's prompt map to G2–G4, the G-AI gates and C1–C2.)
