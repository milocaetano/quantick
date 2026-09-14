# Goal: gate-6 measurements at the campaign tip 950a6440

Re-run the four gate-6 measurements (session-length long variant, tape-growth
stalls, the live-envelope harness and the dense-replay frame timing) on a
release build of exactly the campaign tip
`950a6440bd3497ff37b2abc07b372356bed50b95` — after the main sync of #433
(PR #445), which touched the per-frame path — and commit their raw outputs in
a docs-only PR, so A+ gate 6 can be scored at that SHA. The same task as G6M
(PR #440), reproduced at the new SHA.

**Tier:** small — the coordinator assigned it: docs only, no code changes;
the measurements are run, not tuned, and their raw outputs are committed.
Exempt from `delivery-review` within the small-tier diff ceiling.

Campaign task G6M2 of #367. Base: `campaign/lean-a-plus` at `950a6440`.

## Request ledger

- **R1** — Run `session_length_tests::long` (release, `--ignored`) at
  `950a6440` on a clean tree; counts within +10%, times within +50% between
  18,000 and 3,960,000 prints.
- **R2** — Run `session_length_tests::tape_growth_stalls` (release,
  `--ignored`); slowest ingest at most 4 ms at 2^21 and 2^22, footprint off
  and on.
- **R3** — Run `live_envelope_tests::measure` (release, `--ignored`);
  retained memory at 3,960,000 prints, per-print ingest first against last,
  queue depth at 300/s and 2,000/s with `parked 0`.
- **R4** — The WINV26 2026-08-25 dense replay at speed 60 with book, bubbles,
  footprint and live strip, five 45 s runs, fps min 59, `worker_deferred` 0,
  through G6M's scratch copy of `run_replay.ps1` (every `QUANTICK_*` store in
  scratch, #444), `__COMPAT_LAYER=DPIUNAWARE`, and G6M's single-side summary
  wrapper; both workarounds stated, not committed.
- **R5** — Raw outputs under `docs/quality/gate6-950a6440/` with a `README.md`
  table (command, condition, observed, PASS/FAIL, host, SHA); the "Gate 6 at
  one SHA" pointer in `docs/quality/session-length.md` moves to the new
  folder, keeping `gate6-124cdf0d/` as history. A FAIL is reported and still
  committed; nothing is tuned.
- **R6** — A draft PR against `campaign/lean-a-plus`, reviewed at tier small,
  CI watched, `gh pr ready` once, then `mission_ship_gate.sh ship <n>`; never
  merge, never touch main.
- **R7** — The purpose: gate 6 can be scored at `950a6440` from the committed
  evidence alone.

## Decisions (from the coordinator's brief)

- **D1** — Each run on its own, release build, clean tree at `950a6440`, no
  other cargo build running; the host noted.
- **D2** — No code changes, no tuning; a failing condition is reported.
- **D3** — Reviews: `arch-review` (docs-only: dimension 8 and step 0) and
  `ai-review` completion; markers with the shared campaign key.

## Assumptions

- **S1** — The scratch replay script and summary wrapper are G6M's, with only
  the worktree path in them changed to this worktree. Safe: the diff against
  the committed script is recorded in the raw output's header.

## Acceptance criteria

- [x] **A1** — `long.txt` holds the long variant at `sha: 950a6440…` with no
      "tracked files modified", graded in the README.
      → `docs/quality/gate6-950a6440/long.txt`. *(R1, R7)*
- [x] **A2** — `tape-growth-stalls.txt` holds the stall probe at `950a6440`,
      graded against 4 ms.
      → `docs/quality/gate6-950a6440/tape-growth-stalls.txt`. *(R2, R7)*
- [x] **A3** — `envelope-measure.txt` holds the envelope harness at
      `950a6440` with memory, ingest first/last and queue depth with `parked`.
      → `docs/quality/gate6-950a6440/envelope-measure.txt`. *(R3, R7)*
- [x] **A4** — `frame-timing.txt` holds five 45 s replay runs of a release
      build of `950a6440`, method, host and scratch stores in its header,
      graded against fps min 59 and `worker_deferred` 0.
      → `docs/quality/gate6-950a6440/frame-timing.txt`. *(R4, R7)*
- [x] **A5** — `README.md` carries the table and the deltas against
      `124cdf0d`; "Gate 6 at one SHA" points at the new folder and keeps the
      old one as history. → `docs/quality/gate6-950a6440/README.md`,
      `docs/quality/session-length.md`. *(R5, R7)*
- [ ] **A6** — A draft PR against `campaign/lean-a-plus` with the table in its
      body; CI green at the head; `gh pr ready` requested once; the ship gate
      run and its output reported. → the PR. *(R6)*
- [ ] **G1** — Every artifact in English (`arch-review` dimension 8, the
      language guard). → the arch-review report on the PR.
- [ ] **G2** — `arch-review` over `origin/campaign/lean-a-plus` with step 0;
      every Blocker/Should-fix resolved or deferred in the PR body.
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

A1–A5 met in commit `eb6b6104`: all four conditions PASS at `950a6440`
(`docs/quality/gate6-950a6440/README.md`). G3's local half: `cargo test -p
quantick-guards` green on this branch. A6, G1, G2, G3's CI half and G-AI1–4
land on the PR after this archive; they are not claimed here.

## Closing steps

- **C1** — The PR is open, non-draft, base `campaign/lean-a-plus`.
- **C2** — `mission_ship_gate.sh ship <pr>` run from the worktree, its output
  handed to the coordinator.

## Not applicable

- *Any code change*, *hot path*, *user-visible*, *capability*, *trader
  action*, *engine*: the diff is Markdown and raw text outputs only.
- `delivery-review`: tier small, within the diff ceiling.

## Verbatim request

> The coordinator's brief for campaign task G6M2 (session of 2026-09-13):
>
> You are executing campaign task **G6M2** of campaign #367
> (https://github.com/milocaetano/quantick/issues/367) for
> milocaetano/quantick: re-run the four gate-6 measurements at exactly the
> campaign tip `950a6440bd3497ff37b2abc07b372356bed50b95` (after the main sync
> of #433, PR #445, which touched the per-frame path) and commit their raw
> outputs in a docs-only PR. You change no code. This is the same task as G6M
> (PR #440): read its PR body, `docs/quality/gate6-124cdf0d/README.md` and its
> raw files first, and reproduce them at the new SHA. You are a subagent: you
> cannot ask the trader.
>
> ## Worktree
> - `C:\src\quantick-worktrees\docs-gate6-remeasure-2` (Git Bash
>   `/c/src/quantick-worktrees/docs-gate6-remeasure-2`), branch
>   `docs/gate6-remeasure-2`, at `950a6440`. Every command starts with
>   `cd /c/src/quantick-worktrees/docs-gate6-remeasure-2 &&`. Tier `small`,
>   recorded. Never write to `C:\src\quantick` or other worktrees.
>
> ## The four runs (same commands and pass conditions as G6M)
> On release builds of a clean tree at `950a6440` (headers must read `sha:
> 950a6440...` with no "tracked files modified"), one at a time, with no other
> cargo build running (check before each timed run; no sibling mission is
> building now):
> 1. `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app
>    session_length_tests::long -- --ignored --nocapture --test-threads=1` —
>    counts within +10%, times within +50% between 18,000 and 3,960,000 prints.
> 2. `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app
>    session_length_tests::tape_growth_stalls -- --ignored --nocapture
>    --test-threads=1` — slowest ingest at most 4 ms at 2^21 and 2^22,
>    footprint off and on.
> 3. `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app
>    live_envelope_tests::measure -- --ignored --nocapture --test-threads=1` —
>    retained memory at 3,960,000 prints, per-print ingest first vs last, queue
>    depth at 300/s and 2,000/s with `parked 0`.
> 4. The WINV26 2026-08-25 dense replay at speed 60 with book, bubbles,
>    footprint and live strip, five 45 s runs, fps min 59, `worker_deferred` 0:
>    use the same scratch copy of `tools/live_envelope/run_replay.ps1` that G6M
>    used (it also redirects `QUANTICK_FOOTPRINT_SETTINGS` and
>    `QUANTICK_DEALS_DIR`, see #444; every `QUANTICK_*` store must point at a
>    scratch directory so the trader's files are never touched),
>    `__COMPAT_LAYER=DPIUNAWARE`, and the same single-side summary wrapper for
>    `frame_timing.py`; state both workarounds, do not commit them.
>
> ## Output
> - Raw outputs under `docs/quality/gate6-950a6440/` (`long.txt`,
>   `tape-growth-stalls.txt`, `envelope-measure.txt`, `frame-timing.txt`) plus
>   a `README.md` table (command, condition, observed, PASS/FAIL, host, SHA);
>   update the "Gate 6 at one SHA" pointer in `docs/quality/session-length.md`
>   to the new folder, keeping the `124cdf0d` folder as history. If any
>   condition FAILs, report it plainly and still commit; do not tune.
> - A small-tier goal file archived last (slug `gate6-remeasure-2`).
> - Draft PR: `cd <wt> && gh pr create --draft --base campaign/lean-a-plus
>   --title "docs(quality): gate-6 measurements at the campaign tip 950a6440"
>   --body-file -` (heredoc: tier small, "Campaign task G6M2 of #367", the
>   table, then the Claude Code attribution and the session link).
> - Reviews at tier small: `arch-review` (dimension 8 and step 0),
>   `ai-review` completion; markers with the shared key. CI; `gh pr ready <n>`
>   once via Bash with the cd prefix; then `sh
>   .claude/hooks/mission_ship_gate.sh ship <n>` from the worktree and report
>   its output. **Never use the PowerShell tool for `gh pr` commands; never
>   merge; never touch main.**
>
> Notes: `python`, not `python3`; scratch files under `-g6m2` paths; commit
> trailers `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and
> `Claude-Session: https://claude.ai/code/session_01VdP84bYEGeyCg7KwsNb1X2`.
>
> Return a HANDOFF BLOCK: PR URL; head; the four results with PASS/FAIL and
> deltas against the `124cdf0d` values; host; workarounds; review verdicts;
> markers; CI; ship gate output; the coordinator's next action.
