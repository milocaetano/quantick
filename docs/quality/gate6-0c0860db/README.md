# Gate 6 at the final phase-2 tip `0c0860db`

The four gate-6 measurements re-run at exactly
`0c0860dbea02c40125097e7c5fbeca009d6eabb2`, the final `campaign/lean-a-plus`
tip of phase 2: the synced tip `1baef443` plus #469, which reorders the frame
path in `crates/app/src/app/frame.rs` so `settle_paper_panels(now)` runs before
`draw_report_window` (the same two calls, the settle moved ahead of the
paint). Every graded run is a release build of that commit on a clean tree (no
tracked file modified), one run at a time, with no other session's cargo
build or `rustc` on the host during the run. The previous run, at `1baef443`,
stays in [gate6-1baef443/](../gate6-1baef443/README.md) as history.

**Host:** `DESKTOP-BTVJFFR` — 12th Gen Intel Core i5-12400F, 31.8 GB, Windows
11 Pro 10.0.26200. Run 2026-09-14 05:05 to 05:15. The host was idle for all
four runs, each on its first attempt, and no other session's
`quantick-app.exe` window was open during any of them; see *Host idle state*
below.

| # | Command | Pass condition | Observed at `0c0860db` | Result | Host | Delta against `1baef443` | Raw output |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::long -- --ignored --nocapture --test-threads=1` | every path's counts within +10% (plus the path's absolute slack) and times within +50% between 18,000 and 3,960,000 prints | largest count rise +0.10% (`frame.app.tick50` largest copy 381,696 → 382,080 bytes), apart from one sub-slack cell: `frame.worker.time1d` copy bytes per unit 2.9 → 4.2 (+1.3 bytes, allowed 2.9 × 1.1 + 8,192); largest time rise +2.24% (`frame.book` 183,300 → 187,400 ns); `sha: 0c0860db…` with no "tracked files modified"; verdict "within budget" | PASS | idle | largest count rise +0.10% → +0.10%; `frame.worker.time1d` copy 1.9 → 3.1 became 2.9 → 4.2 bytes per unit; largest time rise +0.21% → +2.24% | [long.txt](long.txt) |
| 2 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::tape_growth_stalls -- --ignored --nocapture --test-threads=1` | slowest ingest at most 4 ms in both the 2^21 and 2^22 stretches, footprint off and on | footprint off: 0.006 ms (2^21), 0.010 ms (2^22); footprint on: 0.008 ms (2^21), 0.049 ms (2^22); no copy in any stretch | PASS | idle | slowest stretch 0.050 → 0.049 ms (bound 4 ms); off 0.005 / 0.009 → 0.006 / 0.010 ms; on 0.010 / 0.050 → 0.008 / 0.049 ms | [tape-growth-stalls.txt](tape-growth-stalls.txt) |
| 3 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app live_envelope_tests::measure -- --ignored --nocapture --test-threads=1` | records retained memory at 3,960,000 prints, per-print ingest first against last, and queue depth at 300/s and 2,000/s with `parked 0` | tape 211.5 MiB + bars 9.1 MiB; working set +221.0 MiB (footprint off), +569.8 MiB (on); ingest first/last 100k 101/98 ns (off), 183/167 ns (on); queued max 6/1024 indicator and 23/4096 book at 300/s, 35/1024 and 52/4096 at 2,000/s; `parked 0/0` in all 26 lines; settled with deferred 0, lost 0 | PASS | idle | memory and queue maxima unchanged to the printed digit except working set +220.9 → +221.0 MiB (off), +569.9 → +569.8 MiB (on); ingest first/last 101/104 → 101/98 ns (off), 178/171 → 183/167 ns (on); peak frame book queued max 2,176 → 1,190, UI-side send 2.28 → 2.07 ms | [envelope-measure.txt](envelope-measure.txt) |
| 4 | `tools/live_envelope/run_replay.ps1` ×5, summarised by `tools/live_envelope/frame_timing.py` | WINV26 2026-08-25 replay at speed 60 with book, bubbles, footprint and live strip; at least five 45 s runs; fps min 59; `worker_deferred` 0 | five 45 s runs at ~4,674 prints/s: fps min 59 in every run; frame_avg 16.667 ms; frame_cpu mean 1.662 ms (stdev 0.045); `worker_deferred` 0 and `worker_parked` 0 in all 110 summary lines; `APP_SLOW_FRAMES` 0 | PASS | idle (first set) | fps min 59 → 59; frame_avg 16.667 → 16.667 ms; frame_cpu mean 1.680 → 1.662 ms (−1%); worst steady frame 33.54 → 33.85 ms; worker counters 0 → 0 | [frame-timing.txt](frame-timing.txt) |

## Against `1baef443`

Every condition held at both SHAs, each far inside its bound. The gate's
conditions are within-run (18,000 against 3,960,000 prints, a bound on the
doubling stretches, `parked`, fps min and `worker_deferred`).

Run 1's `frame.worker.time1d` copy bytes per unit moved 2.9 → 4.2 between the
two session lengths, as it moved 1.9 → 3.1 at `1baef443`. It is a byte-scale
value against an allowance of 2.9 × 1.1 + 8,192 bytes per unit (the
`FRAME_WORKER` slack in `session_length_tests.rs`); its largest single copy is
160 bytes at both lengths and at both SHAs, and the harness's verdict is
"within budget".

Absolute times at 3,960,000 prints, not graded because no gate-6 condition
compares them across revisions: `frame.app.tick50` 1,497,300 → 1,516,600 ns
(+1.3%), `frame.app.time1d` 708,100 → 716,700 ns (+1.2%), `frame.book`
188,900 → 187,400 ns (−0.8%); the whole variant took 153 s against 152 s.
Run 4's frame_cpu mean is 1.662 ms against 1.680 ms. All sit inside the
run-to-run band the same-host A/B measured
([#367 comment](https://github.com/milocaetano/quantick/issues/367#issuecomment-5659282485):
`frame.app.tick50` 1.48–1.66 ms across four runs), so #469's reorder shows no
measurable frame-path cost; this run is not itself an A/B and makes no finer
claim.

## Host idle state

Every timed run was started after two consecutive process checks 30 s apart
found no `cargo` or `rustc`, and the host was sampled every 10 s during the
run (`tasklist`, filtered to `cargo`, `rustc`, `cargo-clippy` and
`quantick`). Only this task's own processes appear in the samples: its
`cargo` pair and test binary, and in run 4 the five runs' own
`quantick-app.exe`. No run needed a repeat.

- Run 2: 05:05:36–05:05:39.
- Run 3: 05:06:18–05:06:46, 3 samples.
- Run 1: 05:07:25–05:09:58, 15 samples.
- Run 4: 05:10:38–05:14:35, 23 samples.

No other session's `quantick-app.exe` window was open during any graded run.
No process this task did not start was stopped.

## Notes on the method

- Runs 1 and 2 print their own `sha:` line; run 3's harness prints none, so
  its file header states the revision and the clean tree. Run 4's header
  states the build and the method.
- Run 4 used the same scratch copy of `run_replay.ps1` as the `1baef443`
  run, re-pointed at this worktree: it also points
  `QUANTICK_FOOTPRINT_SETTINGS` and `QUANTICK_DEALS_DIR` at the per-run
  scratch store (#444; the committed script leaves those two store variables
  unset, which would read the trader's own files), and spells the
  `bubbles.toml` copy source as this worktree's absolute path. Nothing else
  differs, and no store variable was added between `1baef443` and
  `0c0860db`. `frame_timing.py` ran unchanged through the same scratch
  wrapper that sets its run order to the five `head-*` labels, because the
  script assumes a base side as well; the wrapper re-derives the head summary
  line with the script's formula. `__COMPAT_LAYER=DPIUNAWARE`. Neither is
  committed: this PR changes no code.
