# Goal: gate-6 measurements at the campaign tip 124cdf0d

Re-run the four gate-6 measurements (session-length long variant, tape-growth
stalls, the live-envelope harness and the dense-replay frame timing) on a
release build of exactly the campaign tip
`124cdf0d926d9af56d1bb6cd64bf9526a0b0c9e6`, and commit their raw outputs in a
docs-only PR, so that the final assessor can score A+ gate 6 — *scalability
claims for supported live workloads have current measurements, stated rates
and bounded-state evidence at the assessed revision* — at that SHA.

**Tier:** small — the coordinator assigned it: docs only, no code changes;
the measurements are run, not tuned, and their raw outputs are committed.
Exempt from `delivery-review` within the small-tier diff ceiling.

Campaign task G6M of #367. Base: `campaign/lean-a-plus` at `124cdf0d`.

## Request ledger

- **R1** — Run `session_length_tests::long` (release, `--ignored`) at
  `124cdf0d` on a clean tree; every path's counts within +10% and times
  within +50% between 18,000 and 3,960,000 prints.
- **R2** — Run `session_length_tests::tape_growth_stalls` (release,
  `--ignored`) at `124cdf0d`; slowest ingest at most 4 ms in both the 2^21 and
  2^22 stretches, footprint off and on.
- **R3** — Run `live_envelope_tests::measure` (release, `--ignored`) at
  `124cdf0d`; record retained memory at 3,960,000 prints, per-print ingest
  first against last, and queue depth at 300/s and 2,000/s with `parked 0`.
- **R4** — Run the WINV26 2026-08-25 dense replay at speed 60 with book,
  bubbles, footprint and live strip, a release build of `124cdf0d`, at least
  five 45 s runs, every `QUANTICK_*` store in a scratch directory and
  `__COMPAT_LAYER=DPIUNAWARE`; fps min 59, `worker_deferred` 0.
- **R5** — Commit the raw outputs under `docs/quality/gate6-124cdf0d/` with a
  `README.md` table (command, pass condition, observed value, PASS/FAIL, host,
  SHA) and point the "Gate 6 at one SHA" section of
  `docs/quality/session-length.md` at it. A FAIL is reported plainly and still
  committed; nothing is tuned.
- **R6** — Deliver it as a draft PR against `campaign/lean-a-plus`, reviewed
  at tier small, CI watched, readiness requested once; never merge, never
  touch main.
- **R7** — The purpose that judges the rest: the assessor can score gate 6 at
  `124cdf0d` from the committed evidence alone.

## Decisions (from the coordinator's brief)

- **D1** — Each run on its own, release build, clean tree at `124cdf0d`;
  nothing committed before all four are done; the machine otherwise idle
  during timed runs; the host noted.
- **D2** — No code changes, no tuning; a failing condition is reported.
- **D3** — Reviews: `arch-review` (docs-only: dimension 8 and step 0 at
  `low`) and `ai-review` completion; markers with the shared campaign key.

## Assumptions

- **S1** — `tools/live_envelope/frame_timing.py` hard-codes ten interleaved
  base/head labels; this task has one side. It is run through a scratch
  wrapper that imports the unchanged script and sets its run order to the
  five `head-*` labels. Safe: the script's parsing and arithmetic are unchanged
  and the wrapper is recorded in the raw output's header, not committed as
  code.
- **S2** — Frame timing is compared against the stated conditions only (fps
  min 59, `worker_deferred` 0), with no base side. Safe: the brief asks for a
  measurement at the SHA, not a before/after comparison.

## Acceptance criteria

- [x] **A1** — `long.txt` holds the long variant's output at `sha: 124cdf0d…`
      with no "tracked files modified", and the README grades its condition.
      *Evidence:* the raw file and the README row.
      → `docs/quality/gate6-124cdf0d/long.txt`. *(R1, R7)*
- [x] **A2** — `tape-growth-stalls.txt` holds the stall test's output at
      `124cdf0d`, graded against 4 ms in both stretches, footprint off and on.
      *Evidence:* the raw file and the README row.
      → `docs/quality/gate6-124cdf0d/tape-growth-stalls.txt`. *(R2, R7)*
- [x] **A3** — `envelope-measure.txt` holds the envelope harness's output at
      `124cdf0d`: retained memory at 3,960,000 prints, per-print ingest first
      against last, queue depth at 300/s and 2,000/s with `parked`.
      *Evidence:* the raw file and the README row.
      → `docs/quality/gate6-124cdf0d/envelope-measure.txt`. *(R3, R7)*
- [x] **A4** — `frame-timing.txt` holds at least five 45 s replay runs of a
      release build of `124cdf0d` with the method, host and scratch stores in
      its header, graded against fps min 59 and `worker_deferred` 0 — or an
      exact statement of what could not run.
      *Evidence:* the raw file and the README row.
      → `docs/quality/gate6-124cdf0d/frame-timing.txt`. *(R4, R7)*
- [x] **A5** — `README.md` carries the table (command, condition, observed,
      PASS/FAIL, host, SHA) and "Gate 6 at one SHA" in `session-length.md`
      points at the folder.
      *Evidence:* the two files in the diff.
      → `docs/quality/gate6-124cdf0d/README.md`, `docs/quality/session-length.md`. *(R5, R7)*
- [ ] **A6** — A draft PR against `campaign/lean-a-plus` with the table in its
      body; CI green at the head; `gh pr ready` requested once.
      *Evidence:* the PR and its checks. → the PR. *(R6)*
- [ ] **G1** — Every artifact in English (`arch-review` dimension 8, the
      language guard). → the arch-review report on the PR.
- [ ] **G2** — `arch-review` run over `origin/campaign/lean-a-plus` with step
      0 at `low`; every Blocker/Should-fix resolved or deferred in the PR body.
      → the arch-review report on the PR.
- [ ] **G3** — Proportional local proof for docs: `cargo test -p
      quantick-guards` green; full final-head CI. → the PR's checks.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

AI-review evidence lands on the PR: the durable report comment, the thread
list, and the private `ai-review-complete` projection.

## Status at archive

A1–A5 met in commit `0bd7ef09`: all four conditions PASS at `124cdf0d`
(`docs/quality/gate6-124cdf0d/README.md`). G3's local half: `cargo test -p
quantick-guards` green on this branch. A6, G1, G2, G3's CI half and G-AI1–4
land on the PR after this archive; they are not claimed here.

## Closing steps

- **C1** — The PR is open, non-draft, base `campaign/lean-a-plus`.
- **C2** — `mission_ship_gate.sh mission <pr>` passes, or the coordinator is
  told why not.

## Not applicable

- *Any code change*, *hot path*, *user-visible*, *capability*, *trader
  action*, *engine*: the diff is Markdown and raw text outputs only.
- `delivery-review`: tier small, within the diff ceiling.

## Verbatim request

> The coordinator's brief for campaign task G6M (session of 2026-09-13):
>
> You are executing campaign task **G6M** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for
> milocaetano/quantick: re-run the four gate-6 measurements at exactly the
> campaign tip `124cdf0d926d9af56d1bb6cd64bf9526a0b0c9e6` and commit their raw
> outputs in a docs-only PR, so the final assessor can score A+ gate 6
> ("scalability claims for supported live workloads have current
> measurements, stated rates and bounded-state evidence at the assessed
> revision") at that SHA. You change no code. You are a subagent: you cannot
> ask the trader; the decisions below answer the questions.
>
> ## Why
> The final candidate assessment
> (https://github.com/milocaetano/quantick/issues/367#issuecomment-5652264747)
> scored 96/100 but blocked gate 6 only because the measurements were taken at
> child commits; since then main was synchronized in (PR #437, which rewrote
> `state.rs`), so the measurement must be at `124cdf0d`.
>
> ## Worktree
> - `C:\src\quantick-worktrees\docs-gate6-remeasure` (Git Bash
>   `/c/src/quantick-worktrees/docs-gate6-remeasure`), branch
>   `docs/gate6-remeasure`, at `124cdf0d`. Every command starts with
>   `cd /c/src/quantick-worktrees/docs-gate6-remeasure &&`. Never write to
>   `C:\src\quantick` or other worktrees. Mission tier `small` (docs-only),
>   recorded already.
>
> ## The four runs (from the assessor's report, exact commands and pass conditions)
> Run each on its own, on a release build, on a clean tree at `124cdf0d` (the
> output headers must read `sha: 124cdf0d...` without "tracked files
> modified"); do not commit anything before all four are done:
> 1. `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app
>    session_length_tests::long -- --ignored --nocapture --test-threads=1`.
>    Every path's counts must stay within +10% and times within +50% between
>    18,000 and 3,960,000 prints.
> 2. `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app
>    session_length_tests::tape_growth_stalls -- --ignored --nocapture
>    --test-threads=1`. The slowest ingest must be at most 4 ms in both the
>    2^21 and 2^22 stretches, footprint off and on.
> 3. `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app
>    live_envelope_tests::measure -- --ignored --nocapture --test-threads=1`.
>    It records retained memory at 3,960,000 prints, per-print ingest first
>    against last, and queue depth at 300/s and 2,000/s with `parked 0`.
> 4. `tools/live_envelope/run_replay.ps1` plus `frame_timing.py` (read
>    `docs/quality/live-envelope.md` and `docs/quality/session-length.md` for
>    how Q3/Q4/Q12 ran them): the WINV26 2026-08-25 replay at speed 60, with
>    book, bubbles, footprint and live strip, a release build of `124cdf0d`,
>    at least five 45 s runs, fps min 59, `worker_deferred` 0. Point every
>    `QUANTICK_*` store variable at a scratch directory first (see
>    `.claude/skills/ui-harness/SKILL.md`), clear them between runs, and use
>    `__COMPAT_LAYER=DPIUNAWARE`, so the trader's live workspace is never
>    touched. If the replay data or the MT5 terminal is unavailable on this
>    host, say so exactly and run whatever part can run; never fake a number.
> Keep the machine otherwise idle during the timed runs (no other cargo
> builds); note the host.
>
> ## Output
> - Commit the raw outputs under `docs/quality/gate6-124cdf0d/` (`long.txt`,
>   `tape-growth-stalls.txt`, `envelope-measure.txt`, `frame-timing.txt`) plus
>   a short `README.md` table: command, pass condition, observed value,
>   PASS/FAIL, host, SHA `124cdf0d`. Update the "Gate 6 at one SHA" section of
>   `docs/quality/session-length.md` to point at this folder (docs only). If
>   any condition FAILs, report it plainly and still commit the raw outputs;
>   do not tune anything.
> - A goal file per the mission skill's small tier (objective, tier line,
>   ledger, criteria, verbatim request = this brief), archived as the last
>   commit (slug `gate6-remeasure`).
> - Open a **draft** PR: `cd /c/src/quantick-worktrees/docs-gate6-remeasure &&
>   gh pr create --draft --base campaign/lean-a-plus --title "docs(quality):
>   gate-6 measurements at the campaign tip 124cdf0d" --body-file -` with a
>   heredoc body: tier small, "Campaign task G6M of #367", the table, what
>   each run proves, then the Claude Code attribution and the session link.
> - Reviews at tier small: `arch-review` (docs-only: dimension 8 and step 0
>   apply), `ai-review` completion; record `arch-review-ok` and
>   `ai-review-complete` with the shared key per `docs/campaign/integration.md`.
>   Watch CI; `gh pr ready <n>` once via Bash with the cd prefix. **Never use
>   the PowerShell tool for `gh pr` commands; never merge; never touch main.**
>
> ## Notes
> - Use `python`, not `python3`. Keep scratch files under a `-g6m` path.
>   Commit trailers: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`
>   and `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
> - If writing review markers into the git dir is denied by the permission
>   classifier, put the exact `printf` lines in your handoff.
>
> Return a HANDOFF BLOCK: branch; PR URL; head SHA; the four results with
> PASS/FAIL against their conditions; host; anything that could not run and
> why; review verdicts; markers; CI; ready accepted or denied; the
> coordinator's next action.
