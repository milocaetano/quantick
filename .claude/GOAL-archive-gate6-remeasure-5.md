# Goal: gate-6 measurements at the final phase-2 tip 0c0860db

Re-run the four gate-6 measurements (session-length long variant, tape-growth
stalls, the live-envelope harness and the dense-replay frame timing) on a
release build of exactly the campaign tip
`0c0860dbea02c40125097e7c5fbeca009d6eabb2` — the synced tip `1baef443` plus
#469, which moved `settle_paper_panels(now)` ahead of `draw_report_window`
in `crates/app/src/app/frame.rs` — and commit their raw outputs in a
docs-only PR, so gate 6 is bound by SHA for the second consolidated PR. The same task as G6M4
(PR #467), reproduced at the new SHA.

**Tier:** small — the coordinator assigned it: docs only, no code changes;
the measurements are run, not tuned, and their raw outputs are committed.
Exempt from `delivery-review` within the small-tier diff ceiling.

Campaign task G6M5 of #367. Base: `campaign/lean-a-plus` at `0c0860db`.

## Request ledger

- **R1** — Run `session_length_tests::long` (release, `--ignored`) at
  `0c0860db` on a clean tree; counts within +10%, times within +50% between
  18,000 and 3,960,000 prints; header `sha: 0c0860db…` with no "tracked
  files modified".
- **R2** — Run `session_length_tests::tape_growth_stalls` (release,
  `--ignored`); slowest ingest at most 4 ms at 2^21 and 2^22, footprint off
  and on.
- **R3** — Run `live_envelope_tests::measure` (release, `--ignored`);
  `parked 0`.
- **R4** — The WINV26 2026-08-25 dense replay at speed 60, five 45 s runs,
  fps min 59, `worker_deferred` 0, through G6M4's scratch replay script
  copied to a `-g6m5` path (every `QUANTICK_*` store in scratch,
  `QUANTICK_FOOTPRINT_SETTINGS` and `QUANTICK_DEALS_DIR` included),
  `__COMPAT_LAYER=DPIUNAWARE`, and the head-only summary wrapper; both
  workarounds stated, not committed.
- **R5** — An idle host for each timed run: two checks 30 s apart with no
  `cargo` or `rustc` before it, a process sample every 10 s during it; a run
  a build joined is repeated; foreign `quantick-app.exe` windows recorded;
  never kill a process not ours; after about 90 minutes run anyway and label
  the affected runs "host not idle, inconclusive".
- **R6** — Raw outputs under `docs/quality/gate6-0c0860db/` with a
  `README.md` table (command, condition, observed, PASS/FAIL, host, SHA,
  delta against `1baef443`), citing the same-host A/B comment on #367 for
  the absolute-time question; the "Gate 6 at one SHA" pointer in
  `docs/quality/session-length.md` moves to the new folder, keeping the
  older folders as history.
- **R7** — A draft PR against `campaign/lean-a-plus`, reviewed at tier small
  (`arch-review` dimension 8 and step 0, `ai-review`), CI watched,
  `gh pr ready` once, then `mission_ship_gate.sh ship <n>`; never merge,
  never touch main.

## Decisions (from the coordinator's brief)

- **D1** — Each run on its own, release build, clean tree at `0c0860db`; the
  host's idle state recorded per run.
- **D2** — No code changes, no tuning; a failing condition is reported.
- **D3** — Reviews at tier small with the shared campaign key.

## Assumptions

- **S1** — A run that another session's build joined after it started is not
  an idle-host measurement: it is repeated when the host is idle again, and
  the overlapped attempts are listed in the raw output and README rather
  than graded. Safe: every attempt is disclosed with its result.

## Acceptance criteria

- [x] **A1** — `long.txt` holds the long variant at `sha: 0c0860db…` with no
      "tracked files modified", graded in the README.
      → `docs/quality/gate6-0c0860db/long.txt`. *(R1, R5)*
- [x] **A2** — `tape-growth-stalls.txt` holds the stall probe at `0c0860db`,
      graded against 4 ms.
      → `docs/quality/gate6-0c0860db/tape-growth-stalls.txt`. *(R2, R5)*
- [x] **A3** — `envelope-measure.txt` holds the envelope harness at
      `0c0860db` with `parked` per second.
      → `docs/quality/gate6-0c0860db/envelope-measure.txt`. *(R3, R5)*
- [x] **A4** — `frame-timing.txt` holds five 45 s replay runs of a release
      build of `0c0860db`, method, host and scratch stores in its header,
      graded against fps min 59 and `worker_deferred` 0.
      → `docs/quality/gate6-0c0860db/frame-timing.txt`. *(R4, R5)*
- [x] **A5** — `README.md` carries the table with host and the deltas against
      `1baef443` and cites the A/B comment; "Gate 6 at one SHA" points at the
      new folder and keeps the older ones as history.
      → `docs/quality/gate6-0c0860db/README.md`,
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

A1–A5 met: all four conditions PASS at `0c0860db` on an idle host, each on
its first attempt, with no other session's `quantick-app.exe` open
(`docs/quality/gate6-0c0860db/README.md`). Against `1baef443` every
within-run condition stayed far inside its bound and the absolute frame
times moved by about 1%, inside the same-host A/B's noise band; stated in
the README, not a gate condition. G3's local half: `cargo test -p
quantick-guards` green on this branch. A6, G1, G2, G3's CI half and
G-AI1–4 land on the PR after this archive; they are not claimed here.

## Closing steps

- **C1** — The PR is open, non-draft, base `campaign/lean-a-plus`.
- **C2** — `mission_ship_gate.sh ship <pr>` run from the worktree, its output
  handed to the coordinator.

## Not applicable

- *Any code change*, *hot path*, *user-visible*, *capability*, *trader
  action*, *engine*: the diff is Markdown and raw text outputs only.
- `delivery-review`: tier small, within the diff ceiling.

## Verbatim request

> The coordinator's message for campaign task G6M5 (session of 2026-09-14),
> condensed to its asks; the full text is in the session:
>
> Please do one more gate-6 run, G6M5. It is the same task as G6M4 and uses
> the same rules, scripts and idle-host discipline. The target is the final
> campaign tip `0c0860dbea02c40125097e7c5fbeca009d6eabb2`. Why another run:
> #469 merged a frame-path change. `crates/app/src/app/frame.rs` now runs
> `settle_paper_panels(now)` before `draw_report_window`. So the `1baef443`
> measurements no longer bind to the tip by SHA. Worktree
> `docs-gate6-remeasure-5`, branch `docs/gate6-remeasure-5` at `0c0860db`.
> Output: `docs/quality/gate6-0c0860db/` with the README table and deltas
> against `1baef443`; point `session-length.md` at it. Reuse the `-g6m4`
> scratch scripts under `-g6m5` names. PR with `--base campaign/lean-a-plus
> --head docs/gate6-remeasure-5` and the title "docs(quality): gate-6
> measurements at the final phase-2 tip 0c0860db". Delivery: same as G6M4 —
> small-tier goal archive, arch-review and ai-review through
> `review_report.sh`, CI green, `gh pr ready` once, then the ship gate.
> Never merge and never touch main.
