# Gate 6 at the synced campaign tip `1baef443`

The four gate-6 measurements re-run at exactly
`1baef4439a68175d65f8a4fecc1bb1e2d06b403f`, the `campaign/lean-a-plus` tip
after sync X4 (#466): the phase-2 tip `ebe462f6` plus main's #461 and #462,
which touch the frame path (`pane.rs`, drawing gestures). Every graded run is
a release build of that commit on a clean tree (no tracked file modified),
one run at a time, with no other session's cargo build or `rustc` on the host
during the run. The previous run, at `ebe462f6`, stays in
[gate6-ebe462f6/](../gate6-ebe462f6/README.md) as history.

**Host:** `DESKTOP-BTVJFFR` — 12th Gen Intel Core i5-12400F, 31.8 GB, Windows
11 Pro 10.0.26200. Run 2026-09-14 03:24 to 03:34. The host was idle for all
four runs, each on its first attempt, and no other session's
`quantick-app.exe` window was open during any of them; see *Host idle state*
below.

| # | Command | Pass condition | Observed at `1baef443` | Result | Host | Delta against `ebe462f6` | Raw output |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::long -- --ignored --nocapture --test-threads=1` | every path's counts within +10% (plus the path's absolute slack) and times within +50% between 18,000 and 3,960,000 prints | largest count rise +0.10% (`frame.app.tick50` largest copy 381,696 → 382,080 bytes), apart from one sub-slack cell: `frame.worker.time1d` copy bytes per unit 1.9 → 3.1 (+1.2 bytes, allowed 1.9 × 1.1 + 8,192); largest time rise +0.21% (`frame.app.tick50` 1,494,100 → 1,497,300 ns); `sha: 1baef443…` with no "tracked files modified"; verdict "within budget" | PASS | idle | largest count rise +0.10% → +0.10%; `frame.worker.time1d` copy 0.3 → 0.1 became 1.9 → 3.1 bytes per unit; largest time rise +1.18% → +0.21% | [long.txt](long.txt) |
| 2 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::tape_growth_stalls -- --ignored --nocapture --test-threads=1` | slowest ingest at most 4 ms in both the 2^21 and 2^22 stretches, footprint off and on | footprint off: 0.005 ms (2^21), 0.009 ms (2^22); footprint on: 0.010 ms (2^21), 0.050 ms (2^22); no copy in any stretch | PASS | idle | slowest stretch 0.530 → 0.050 ms (bound 4 ms); off 0.010 / 0.013 → 0.005 / 0.009 ms; on 0.530 / 0.057 → 0.010 / 0.050 ms | [tape-growth-stalls.txt](tape-growth-stalls.txt) |
| 3 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app live_envelope_tests::measure -- --ignored --nocapture --test-threads=1` | records retained memory at 3,960,000 prints, per-print ingest first against last, and queue depth at 300/s and 2,000/s with `parked 0` | tape 211.5 MiB + bars 9.1 MiB; working set +220.9 MiB (footprint off), +569.9 MiB (on); ingest first/last 100k 101/104 ns (off), 178/171 ns (on); queued max 6/1024 indicator and 23/4096 book at 300/s, 35/1024 and 52/4096 at 2,000/s; `parked 0/0` in all 26 lines; settled with deferred 0, lost 0 | PASS | idle | memory and queue maxima unchanged to the printed digit except working set +221.0 → +220.9 MiB (off), +569.8 → +569.9 MiB (on); ingest first/last 108/127 → 101/104 ns (off), 197/181 → 178/171 ns (on); peak frame indicator queued max 94 → 1, UI-side send 1.47 → 2.28 ms | [envelope-measure.txt](envelope-measure.txt) |
| 4 | `tools/live_envelope/run_replay.ps1` ×5, summarised by `tools/live_envelope/frame_timing.py` | WINV26 2026-08-25 replay at speed 60 with book, bubbles, footprint and live strip; at least five 45 s runs; fps min 59; `worker_deferred` 0 | five 45 s runs at ~4,677 prints/s: fps min 59 in every run; frame_avg 16.667 ms; frame_cpu mean 1.680 ms (stdev 0.049); `worker_deferred` 0 and `worker_parked` 0 in all 110 summary lines; `APP_SLOW_FRAMES` 0 | PASS | idle (first set) | fps min 59 → 59; frame_avg 16.671 → 16.667 ms; frame_cpu mean 2.408 → 1.680 ms (−30%); worst steady frame 40.15 → 33.54 ms; worker counters 0 → 0 | [frame-timing.txt](frame-timing.txt) |

## Against `ebe462f6`

Every condition held at both SHAs, each far inside its bound. The gate's
conditions are within-run (18,000 against 3,960,000 prints, a bound on the
doubling stretches, `parked`, fps min and `worker_deferred`).

One within-run cell moved more than the others in relative terms and is stated
plainly: run 1's `frame.worker.time1d` copy bytes per unit went 1.9 → 3.1
between the two session lengths (0.3 → 0.1 at `ebe462f6`). It is a byte-scale
value against an allowance of 1.9 × 1.1 + 8,192 bytes per unit (the
`FRAME_WORKER` slack in `session_length_tests.rs`); its largest single copy is
160 bytes at both lengths and at both SHAs, and the harness's verdict is
"within budget".

The absolute times that G6M3 saw rise against `950a6440` came back down on an
idle host with no other session's app open. At 3,960,000 prints:
`frame.app.tick50` 1,965,000 → 1,497,300 ns (1,619,400 at `950a6440`),
`frame.app.time1d` 933,400 → 708,100 ns (718,900), `frame.book` 213,900 →
188,900 ns (199,400); the whole variant took 152 s against 188 s (148 s).
Run 4's frame_cpu mean is 1.680 ms against 2.408 ms (1.855 ms). No gate-6
condition compares absolute times across revisions, so these are not graded.
The same-host A/B of `950a6440` against `ebe462f6`
([#367 comment](https://github.com/milocaetano/quantick/issues/367#issuecomment-5659282485))
had already attributed G6M3's rise to host load — other sessions' debug apps
were open — and found the frame paths equal within run-to-run noise
(`frame.app.tick50` 1.48–1.66 ms across its four runs). The numbers here sit
inside that band, so #461 and #462 add no measurable frame-path cost; this
run is not itself an A/B and makes no finer claim.

## Host idle state

Every timed run was started after two consecutive process checks 30 s apart
found no `cargo` or `rustc`, and the host was sampled every 10 s during the
run (`tasklist`, filtered to `cargo`, `rustc`, `cargo-clippy` and
`quantick`). Only this task's own processes appear in the samples: its
`cargo` pair and test binary, and in run 4 the five runs' own
`quantick-app.exe`. No run needed a repeat.

- Run 2: 03:24:12–03:24:15.
- Run 3: 03:25:01–03:25:28, 3 samples.
- Run 1: 03:26:08–03:28:40, 15 samples.
- Run 4: 03:29:47–03:33:44, 23 samples.

No other session's `quantick-app.exe` window was open during any graded run
(G6M3 had two throughout). No process this task did not start was stopped.

## Notes on the method

- Runs 1 and 2 print their own `sha:` line; run 3's harness prints none, so
  its file header states the revision and the clean tree. Run 4's header
  states the build and the method.
- Run 4 used G6M3's scratch copy of `run_replay.ps1`, re-pointed at this
  worktree: it also points `QUANTICK_FOOTPRINT_SETTINGS` and
  `QUANTICK_DEALS_DIR` at the per-run scratch store (#444; the committed
  script leaves those two store variables unset, which would read the
  trader's own files), and spells the `bubbles.toml` copy source as this
  worktree's absolute path. Nothing else differs, and no store variable was
  added between `ebe462f6` and `1baef443`. `frame_timing.py` ran unchanged
  through the same scratch wrapper that sets its run order to the five
  `head-*` labels, because the script assumes a base side as well; the
  wrapper re-derives the head summary line with the script's formula.
  `__COMPAT_LAYER=DPIUNAWARE`. Neither is committed: this PR changes no code.
