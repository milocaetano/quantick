# Goal: gate-6 measurements at the synced tip 1baef443

Re-run the four gate-6 measurements (session-length long variant, tape-growth
stalls, the live-envelope harness and the dense-replay frame timing) on a
release build of exactly the campaign tip
`1baef4439a68175d65f8a4fecc1bb1e2d06b403f` — the phase-2 tip plus sync X4
(#466), which brought main's #461 and #462 into the frame path (`pane.rs`,
drawing gestures) — and commit their raw outputs in a docs-only PR, so gate 6
is bound by SHA for the second consolidated PR. The same task as G6M3
(PR #464), reproduced at the new SHA.

**Tier:** small — the coordinator assigned it: docs only, no code changes;
the measurements are run, not tuned, and their raw outputs are committed.
Exempt from `delivery-review` within the small-tier diff ceiling.

Campaign task G6M4 of #367. Base: `campaign/lean-a-plus` at `1baef443`.

## Request ledger

- **R1** — Run `session_length_tests::long` (release, `--ignored`) at
  `1baef443` on a clean tree; counts within +10%, times within +50% between
  18,000 and 3,960,000 prints; header `sha: 1baef443…` with no "tracked
  files modified".
- **R2** — Run `session_length_tests::tape_growth_stalls` (release,
  `--ignored`); slowest ingest at most 4 ms at 2^21 and 2^22, footprint off
  and on.
- **R3** — Run `live_envelope_tests::measure` (release, `--ignored`);
  `parked 0`.
- **R4** — The WINV26 2026-08-25 dense replay at speed 60, five 45 s runs,
  fps min 59, `worker_deferred` 0, through G6M3's scratch replay script
  copied to a `-g6m4` path (every `QUANTICK_*` store in scratch,
  `QUANTICK_FOOTPRINT_SETTINGS` and `QUANTICK_DEALS_DIR` included),
  `__COMPAT_LAYER=DPIUNAWARE`, and the head-only summary wrapper; both
  workarounds stated, not committed.
- **R5** — An idle host for each timed run: two checks 30 s apart with no
  `cargo` or `rustc` before it, a process sample every 10 s during it; a run
  a build joined is repeated; foreign `quantick-app.exe` windows recorded;
  never kill a process not ours; after about 90 minutes run anyway and label
  the affected runs "host not idle, inconclusive".
- **R6** — Raw outputs under `docs/quality/gate6-1baef443/` with a
  `README.md` table (command, condition, observed, PASS/FAIL, host, SHA,
  delta against `ebe462f6`), citing the same-host A/B comment on #367 for
  the absolute-time question; the "Gate 6 at one SHA" pointer in
  `docs/quality/session-length.md` moves to the new folder, keeping the
  older folders as history.
- **R7** — A draft PR against `campaign/lean-a-plus`, reviewed at tier small
  (`arch-review` dimension 8 and step 0, `ai-review`), CI watched,
  `gh pr ready` once, then `mission_ship_gate.sh ship <n>`; never merge,
  never touch main.

## Decisions (from the coordinator's brief)

- **D1** — Each run on its own, release build, clean tree at `1baef443`; the
  host's idle state recorded per run.
- **D2** — No code changes, no tuning; a failing condition is reported.
- **D3** — Reviews at tier small with the shared campaign key.

## Assumptions

- **S1** — A run that another session's build joined after it started is not
  an idle-host measurement: it is repeated when the host is idle again, and
  the overlapped attempts are listed in the raw output and README rather
  than graded. Safe: every attempt is disclosed with its result.

## Acceptance criteria

- [x] **A1** — `long.txt` holds the long variant at `sha: 1baef443…` with no
      "tracked files modified", graded in the README.
      → `docs/quality/gate6-1baef443/long.txt`. *(R1, R5)*
- [x] **A2** — `tape-growth-stalls.txt` holds the stall probe at `1baef443`,
      graded against 4 ms.
      → `docs/quality/gate6-1baef443/tape-growth-stalls.txt`. *(R2, R5)*
- [x] **A3** — `envelope-measure.txt` holds the envelope harness at
      `1baef443` with `parked` per second.
      → `docs/quality/gate6-1baef443/envelope-measure.txt`. *(R3, R5)*
- [x] **A4** — `frame-timing.txt` holds five 45 s replay runs of a release
      build of `1baef443`, method, host and scratch stores in its header,
      graded against fps min 59 and `worker_deferred` 0.
      → `docs/quality/gate6-1baef443/frame-timing.txt`. *(R4, R5)*
- [x] **A5** — `README.md` carries the table with host and the deltas against
      `ebe462f6` and cites the A/B comment; "Gate 6 at one SHA" points at the
      new folder and keeps the older ones as history.
      → `docs/quality/gate6-1baef443/README.md`,
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

A1–A5 met: all four conditions PASS at `1baef443` on an idle host, each on
its first attempt, with no other session's `quantick-app.exe` open
(`docs/quality/gate6-1baef443/README.md`). Against `ebe462f6` every
within-run condition stayed far inside its bound, and the absolute frame
times G6M3 saw rise came back inside the same-host A/B's noise band; stated
in the README, not a gate condition. G3's local half: `cargo test -p
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

> The coordinator's brief for campaign task G6M4 (session of 2026-09-14),
> condensed to its asks; the full text is in the session:
>
> Re-run the four gate-6 release measurements at exactly campaign tip
> `1baef4439a68175d65f8a4fecc1bb1e2d06b403f`, and commit the raw outputs in a
> docs-only PR. You change no code. That tip is the phase-2 tip plus sync X4
> (#466), which brought main's #461 and #462 into the frame path. This
> repeats G6M3 (PR #464); reuse its scratch files under `-g6m4` names.
> Before each timed run, check twice, 30 s apart, that no cargo or rustc is
> running; sample processes every 10 s during the run; repeat any run a
> build joined; record which foreign `quantick-app.exe` windows were open;
> never kill a process you did not start; after about 90 minutes with no
> idle window run anyway and label the affected runs "host not idle,
> inconclusive". The four runs with G6M3's commands and pass conditions on
> release builds of a clean tree, headers `sha: 1baef443...` with no
> "tracked files modified"; the replay with every `QUANTICK_*` store in
> scratch including `QUANTICK_FOOTPRINT_SETTINGS` and `QUANTICK_DEALS_DIR`,
> `__COMPAT_LAYER=DPIUNAWARE`, and the head-only summary wrapper; state both
> workarounds and do not commit them. Raw files in
> `docs/quality/gate6-1baef443/` with a `README.md` table: command,
> condition, observed, PASS/FAIL, host, SHA, and the delta against
> `ebe462f6`; cite the A/B comment
> (https://github.com/milocaetano/quantick/issues/367#issuecomment-5659282485)
> for the absolute-time question. Point "Gate 6 at one SHA" at the new
> folder and keep the older folders as history. Archive the small-tier goal
> last, with gates G-AI1 to G-AI4. Draft PR against `campaign/lean-a-plus`
> titled "docs(quality): gate-6 measurements at the synced tip 1baef443".
> Run arch-review (dimension 8 and step 0) and ai-review, publish through
> `review_report.sh`, record markers via `campaign_context.sh key <wt>`.
> When CI is green, `gh pr ready <n>` once, then `mission_ship_gate.sh ship
> <n>`. Never merge. Never touch main. Never use PowerShell for `gh pr`.
