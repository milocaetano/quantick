# Gate 6 at the campaign tip `ebe462f6`

The four gate-6 measurements re-run at exactly
`ebe462f60fae65c0811a303ae7cf4dda729680d1`, the `campaign/lean-a-plus` tip of
phase 2: the paper account moved to `crates/paper` and `crates/civil` (#453),
the control host moved to `crates/control-host` (#452), the UI-free ratchet
landed (#450) and the session-length fast variant became cheap (#458). Every
graded run is a release build of that commit on a clean tree (no tracked file
modified), one run at a time, with no other session's cargo build or `rustc`
on the host during the run. The previous run, at `950a6440`, stays in
[gate6-950a6440/](../gate6-950a6440/README.md) as history.

**Host:** `DESKTOP-BTVJFFR` — 12th Gen Intel Core i5-12400F, 31.8 GB, Windows
11 Pro 10.0.26200. Run 2026-09-13 23:55 to 2026-09-14 01:21. The host was
shared with other sessions; see *Host idle state* below for how each graded
run was kept clear of their builds.

| # | Command | Pass condition | Observed at `ebe462f6` | Result | Host | Delta against `950a6440` | Raw output |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::long -- --ignored --nocapture --test-threads=1` | every path's counts within +10% and times within +50% between 18,000 and 3,960,000 prints | largest count rise +0.10% (`frame.app.tick50` largest copy 381,696 → 382,080 bytes); largest time rise +1.18% (`frame.book` 211,400 → 213,900 ns); `sha: ebe462f6…` with no "tracked files modified"; verdict "within budget" | PASS | idle | largest count rise +0.05% → +0.10%; largest time rise +1.73% → +1.18% | [long.txt](long.txt) |
| 2 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::tape_growth_stalls -- --ignored --nocapture --test-threads=1` | slowest ingest at most 4 ms in both the 2^21 and 2^22 stretches, footprint off and on | footprint off: 0.010 ms (2^21), 0.013 ms (2^22); footprint on: 0.530 ms (2^21), 0.057 ms (2^22); no copy in any stretch | PASS | idle | slowest stretch 0.066 → 0.530 ms (footprint on, 2^21; bound 4 ms); off 0.014 / 0.009 → 0.010 / 0.013 ms | [tape-growth-stalls.txt](tape-growth-stalls.txt) |
| 3 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app live_envelope_tests::measure -- --ignored --nocapture --test-threads=1` | records retained memory at 3,960,000 prints, per-print ingest first against last, and queue depth at 300/s and 2,000/s with `parked 0` | tape 211.5 MiB + bars 9.1 MiB; working set +221.0 MiB (footprint off), +569.8 MiB (on); ingest first/last 100k 108/127 ns (off), 197/181 ns (on); queued max 6/1024 indicator and 23/4096 book at 300/s, 35/1024 and 52/4096 at 2,000/s; `parked 0/0` in every second; settled with deferred 0, lost 0 | PASS | idle | memory and queue depths unchanged to the printed digit except working set off +220.9 → +221.0 MiB; ingest first/last 116/104 → 108/127 ns (off), 179/180 → 197/181 ns (on) | [envelope-measure.txt](envelope-measure.txt) |
| 4 | `tools/live_envelope/run_replay.ps1` ×5, summarised by `tools/live_envelope/frame_timing.py` | WINV26 2026-08-25 replay at speed 60 with book, bubbles, footprint and live strip; at least five 45 s runs; fps min 59; `worker_deferred` 0 | five 45 s runs at ~4,680 prints/s: fps min 59 in every run; frame_avg 16.671 ms; frame_cpu mean 2.408 ms (stdev 0.065); `worker_deferred` 0 and `worker_parked` 0 in all 110 summary lines; `APP_SLOW_FRAMES` 0 | PASS | idle (fourth set; three earlier sets not idle) | fps min 59 → 59; frame_avg 16.667 → 16.671 ms; frame_cpu mean 1.855 → 2.408 ms (+30%); worst steady frame 33.46 → 40.15 ms; worker counters 0 → 0 | [frame-timing.txt](frame-timing.txt) |

## Against `950a6440`

Every condition held at both SHAs. The gate's conditions are within-run
(18,000 against 3,960,000 prints, a bound on the doubling stretches, `parked`,
fps min and `worker_deferred`), and each moved far inside its bound.

Two cross-SHA movements are larger than the within-run ones and are stated
plainly, not graded, because no gate-6 condition compares absolute times
across revisions:

- Run 1's absolute per-unit times at 3,960,000 prints rose: `frame.app.tick50`
  1,619,400 → 1,965,000 ns (+21%), `frame.app.time1d` 718,900 → 933,400 ns
  (+30%), `frame.book` 199,400 → 213,900 ns (+7%); the whole variant took
  188 s against 148 s. Its counts barely moved (`frame.app.tick50`
  allocations per frame 2,902.5 → 2,908.6, +0.2%).
- Run 4's frame_cpu mean rose from 1.855 to 2.408 ms (+30%) with fps and
  frame_avg unchanged.

The host differed from the `950a6440` run: two other sessions' debug
`quantick-app.exe` windows were open during every graded run here
(`verify-copy-paste-latest` from 22:44, `fix-native-drawing-clipboard` from
01:03), which the `950a6440` run does not record. These measurements cannot
separate that from a change in the code; a same-host A/B of both SHAs would.

## Host idle state

Every timed run was started after two consecutive process checks 30 s apart
found no `cargo` or `rustc`, and the host was sampled every 10 s during the
run. Other sessions kept starting builds; the runs they overlapped were
repeated and are not the evidence:

- Run 1: the graded run is the third (01:13–01:17, no `rustc` in any sample).
  The first two (23:55 and 00:31) had other sessions' builds join about 30 s
  in (1 to 8 `rustc.exe` per sample); both also ended "within budget"
  (largest time rise +2.08% and +11.25%).
- Run 2: 00:02:46, 3 s, no other build. Two later repeats under other
  sessions' builds gave 0.016–1.015 ms for the four stretches, all inside the
  bound; not the evidence.
- Run 3: 00:30:15–00:30:44, no `rustc` in any sample.
- Run 4: the graded set is the fourth (01:16:58–01:20:55, no `cargo` or
  `rustc` in any sample). The three earlier sets each started idle and were
  joined by other sessions' builds (1 to 6 `rustc.exe` per sample): fps min
  49, 26 and 16, `worker_deferred` max 0, 335 and 2,385. Host not idle, so
  inconclusive and not graded; their summary lines are at the end of
  [frame-timing.txt](frame-timing.txt).

No process this task did not start was stopped.

## Notes on the method

- Runs 1 and 2 print their own `sha:` line; run 3's harness prints none, so
  its file header states the revision and the clean tree. Run 4's header
  states the build and the method.
- Run 4 used the same scratch copy of `run_replay.ps1` as the `950a6440`
  run: it also points `QUANTICK_FOOTPRINT_SETTINGS` and `QUANTICK_DEALS_DIR`
  at the per-run scratch store (#444; the committed script leaves those two
  store variables unset, which would read the trader's own files), and spells
  the `bubbles.toml` copy source as this worktree's absolute path. Nothing else
  differs. `frame_timing.py` ran unchanged through the same scratch wrapper
  that sets its run order to the five `head-*` labels, because the script
  assumes a base side as well; the wrapper re-derives the head summary line
  with the script's formula. `__COMPAT_LAYER=DPIUNAWARE`. Neither is
  committed: this PR changes no code.
