# Goal: gate-6 measurements at the phase-2 tip ebe462f6

Re-run the four gate-6 measurements (session-length long variant, tape-growth
stalls, the live-envelope harness and the dense-replay frame timing) on a
release build of exactly the campaign tip
`ebe462f60fae65c0811a303ae7cf4dda729680d1` — phase 2: the paper account in
`crates/paper` and `crates/civil` (#453), the control host in
`crates/control-host` (#452), the UI-free ratchet (#450) and the cheap
session-length fast variant (#458) — and commit their raw outputs in a
docs-only PR, so gate 6 is bound by SHA for the second consolidated PR. The
same task as G6M2 (PR #448), reproduced at the new SHA.

**Tier:** small — the coordinator assigned it: docs only, no code changes;
the measurements are run, not tuned, and their raw outputs are committed.
Exempt from `delivery-review` within the small-tier diff ceiling.

Campaign task G6M3 of #367. Base: `campaign/lean-a-plus` at `ebe462f6`.

## Request ledger

- **R1** — Run `session_length_tests::long` (release, `--ignored`) at
  `ebe462f6` on a clean tree; counts within +10%, times within +50% between
  18,000 and 3,960,000 prints.
- **R2** — Run `session_length_tests::tape_growth_stalls` (release,
  `--ignored`); slowest ingest at most 4 ms at 2^21 and 2^22, footprint off
  and on.
- **R3** — Run `live_envelope_tests::measure` (release, `--ignored`);
  `parked 0`; memory, ingest and queue depths reported.
- **R4** — The WINV26 2026-08-25 dense replay at speed 60 with book, bubbles,
  footprint and live strip, five 45 s runs, fps min 59, `worker_deferred` 0,
  through G6M2's scratch replay script copied to a `-g6m3` path (every
  `QUANTICK_*` store in scratch, `QUANTICK_FOOTPRINT_SETTINGS` and
  `QUANTICK_DEALS_DIR` included), `__COMPAT_LAYER=DPIUNAWARE`, and G6M2's
  head-only summary wrapper; both workarounds stated, not committed.
- **R5** — An idle host for each timed run: no `cargo` or `rustc` running;
  poll and wait, never kill a process not ours; after about 90 minutes run
  anyway and label the affected measurement "host not idle", a FAIL under
  load counting as inconclusive.
- **R6** — Raw outputs under `docs/quality/gate6-ebe462f6/` with a
  `README.md` table (command, condition, observed, PASS/FAIL, host, SHA,
  delta against `950a6440`); the "Gate 6 at one SHA" pointer in
  `docs/quality/session-length.md` moves to the new folder, keeping the older
  folders as history. A FAIL on an idle host is reported and still committed;
  nothing is tuned.
- **R7** — A draft PR against `campaign/lean-a-plus`, reviewed at tier small
  (`arch-review` dimension 8 and step 0, `ai-review`), CI watched,
  `gh pr ready` once, then `mission_ship_gate.sh ship <n>`; never merge,
  never touch main.

## Decisions (from the coordinator's brief)

- **D1** — Each run on its own, release build, clean tree at `ebe462f6`; the
  host's idle state recorded per run.
- **D2** — No code changes, no tuning; a failing condition is reported.
- **D3** — Reviews at tier small with the shared campaign key.

## Assumptions

- **S1** — A run that another session's build joined after it started is not
  an idle-host measurement: it is repeated when the host is idle again, and
  the overlapped attempts are listed in the raw output and README rather
  than graded. Safe: every attempt is disclosed with its result.

## Acceptance criteria

- [x] **A1** — `long.txt` holds the long variant at `sha: ebe462f6…` with no
      "tracked files modified", graded in the README.
      → `docs/quality/gate6-ebe462f6/long.txt`. *(R1, R5)*
- [x] **A2** — `tape-growth-stalls.txt` holds the stall probe at `ebe462f6`,
      graded against 4 ms.
      → `docs/quality/gate6-ebe462f6/tape-growth-stalls.txt`. *(R2, R5)*
- [x] **A3** — `envelope-measure.txt` holds the envelope harness at
      `ebe462f6` with memory, ingest and queue depth with `parked`.
      → `docs/quality/gate6-ebe462f6/envelope-measure.txt`. *(R3, R5)*
- [x] **A4** — `frame-timing.txt` holds five 45 s replay runs of a release
      build of `ebe462f6`, method, host and scratch stores in its header,
      graded against fps min 59 and `worker_deferred` 0.
      → `docs/quality/gate6-ebe462f6/frame-timing.txt`. *(R4, R5)*
- [x] **A5** — `README.md` carries the table with host and the deltas against
      `950a6440`; "Gate 6 at one SHA" points at the new folder and keeps the
      older ones as history. → `docs/quality/gate6-ebe462f6/README.md`,
      `docs/quality/session-length.md`. *(R6)*
- [ ] **A6** — A draft PR against `campaign/lean-a-plus` with the table in its
      body; CI green at the head; `gh pr ready` requested once; the ship gate
      run and its output reported. → the PR. *(R7)*
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

A1–A5 met in commit `943af3c0`: all four conditions PASS at `ebe462f6` on an
idle host (`docs/quality/gate6-ebe462f6/README.md`). Runs 1 and 4 each needed
repeats because other sessions' builds joined the earlier attempts; those
attempts are listed, not graded. Cross-SHA absolute frame times rose against
`950a6440` with a host confound (two other sessions' debug apps open); stated
in the README, not a gate condition. G3's local half: `cargo test -p
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

> The coordinator's brief for campaign task G6M3 (session of 2026-09-13),
> condensed to its asks; the full text is in the session:
>
> Re-run the four gate-6 release measurements at exactly the campaign tip
> `ebe462f60fae65c0811a303ae7cf4dda729680d1` and commit their raw outputs in a
> docs-only PR. You change no code. This is the same task as G6M2 (PR #448).
> An idle host is required: before each timed run check for cargo or rustc;
> wait and poll; never kill a process you did not start; after about 90
> minutes run anyway and label every affected measurement "host not idle".
> The four runs with G6M2's commands and pass conditions; the replay through
> G6M2's scratch script copied to a `-g6m3` path, every `QUANTICK_*` store in
> scratch including `QUANTICK_FOOTPRINT_SETTINGS` and `QUANTICK_DEALS_DIR`,
> `__COMPAT_LAYER=DPIUNAWARE`, summarised with G6M2's head-only wrapper; both
> workarounds stated, not committed. Output under
> `docs/quality/gate6-ebe462f6/` with a README table of command, condition,
> observed value, PASS or FAIL, host, SHA and the delta against `950a6440`;
> point "Gate 6 at one SHA" at it and keep older folders as history. If any
> condition FAILs on an idle host, report it plainly and still commit. Archive
> the small-tier goal file last, slug `gate6-remeasure-3`, gates G-AI1 to
> G-AI4. Draft PR against `campaign/lean-a-plus`, titled "docs(quality):
> gate-6 measurements at the phase-2 tip ebe462f6". Reviews: `arch-review`
> (dimension 8 and step 0) and `ai-review` through `review_report.sh`;
> markers via `campaign_context.sh key <wt>`. Wait for CI green, `gh pr
> ready <n>` once, then `mission_ship_gate.sh ship <n>`. Never merge, never
> touch main, never use the PowerShell tool for `gh pr` commands.
