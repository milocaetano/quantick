# Gate 6 at the campaign tip `950a6440`

The four gate-6 measurements re-run at exactly
`950a6440bd3497ff37b2abc07b372356bed50b95`, the `campaign/lean-a-plus` tip
after main was synchronized in again (#433, PR #445, which touched the
per-frame path). Every run is a release build of that commit on a clean tree
(no tracked file modified), one run at a time, with no other cargo build on
the host. The previous run, at `124cdf0d`, stays in
[gate6-124cdf0d/](../gate6-124cdf0d/README.md) as history.

**Host:** `DESKTOP-BTVJFFR` — 12th Gen Intel Core i5-12400F, 31.8 GB, Windows
11 Pro 10.0.26200. Run on 2026-09-13.

| # | Command | Pass condition | Observed at `950a6440` | Result | Raw output |
| --- | --- | --- | --- | --- | --- |
| 1 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::long -- --ignored --nocapture --test-threads=1` | every path's counts within +10% and times within +50% between 18,000 and 3,960,000 prints | largest count rise +0.05% (`frame.app.tick50` largest copy 381,696 → 381,888 bytes); largest time rise +1.73% (`frame.book` 196,000 → 199,400 ns); `sha: 950a6440…` with no "tracked files modified"; verdict "within budget" | PASS | [long.txt](long.txt) |
| 2 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::tape_growth_stalls -- --ignored --nocapture --test-threads=1` | slowest ingest at most 4 ms in both the 2^21 and 2^22 stretches, footprint off and on | footprint off: 0.014 ms (2^21), 0.009 ms (2^22); footprint on: 0.019 ms (2^21), 0.066 ms (2^22); no copy in any stretch | PASS | [tape-growth-stalls.txt](tape-growth-stalls.txt) |
| 3 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app live_envelope_tests::measure -- --ignored --nocapture --test-threads=1` | records retained memory at 3,960,000 prints, per-print ingest first against last, and queue depth at 300/s and 2,000/s with `parked 0` | tape 211.5 MiB + bars 9.1 MiB; working set +220.9 MiB (footprint off), +569.8 MiB (on); ingest first/last 100k 116/104 ns (off), 179/180 ns (on); queued max 6/1024 indicator and 23/4096 book at 300/s, 35/1024 and 52/4096 at 2,000/s; `parked 0/0` in every second; settled with deferred 0, lost 0 | PASS | [envelope-measure.txt](envelope-measure.txt) |
| 4 | `tools/live_envelope/run_replay.ps1` ×5, summarised by `tools/live_envelope/frame_timing.py` | WINV26 2026-08-25 replay at speed 60 with book, bubbles, footprint and live strip; at least five 45 s runs; fps min 59; `worker_deferred` 0 | five 45 s runs at ~4,680 prints/s: fps min 59 in every run; frame_avg 16.667 ms; frame_cpu mean 1.855 ms (stdev 0.150); `worker_deferred` 0 and `worker_parked` 0 in all 110 summary lines; `APP_SLOW_FRAMES` 0 | PASS | [frame-timing.txt](frame-timing.txt) |

## Against `124cdf0d`

Every condition held at both SHAs. The largest movements, all far inside
their bounds: run 1's largest time rise went from +1.45% to +1.73% and its
largest count rise from +0.41% to +0.05%; run 2's slowest stretch ingest from
0.056 to 0.066 ms (bound 4 ms); run 3's memory and queue depths are unchanged
to the printed digit, with ingest first/last 111/124 → 116/104 ns (off) and
187/241 → 179/180 ns (on); run 4's frame_cpu mean rose from 1.766 to 1.855 ms
(per-run spread 1.71 to 2.02 ms) with fps min, frame_avg and both worker
counters unchanged.

## Notes on the method

- Runs 1 and 2 print their own `sha:` line; run 3's harness prints none, so
  its file header states the revision and the clean tree. Run 4's header
  states the build and the method.
- Run 4 used the same scratch copy of `run_replay.ps1` as the `124cdf0d`
  run: it also points `QUANTICK_FOOTPRINT_SETTINGS` and `QUANTICK_DEALS_DIR`
  at the per-run scratch store (#444; the committed script leaves those two
  store variables unset, which would read the trader's own files), and spells
  the `bubbles.toml` copy source as this worktree's absolute path. Nothing else
  differs. `frame_timing.py` ran unchanged through the same scratch wrapper
  that sets its run order to the five `head-*` labels, because the script
  assumes a base side as well; the wrapper re-derives the head summary line
  with the script's formula. Neither is committed: this PR changes no code.
- The trader's MetaTrader terminal was open on the host during the runs; the
  replay reads a recorded file and does not use it.
