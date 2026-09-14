# Gate 6 at the campaign tip `124cdf0d`

The four gate-6 measurements re-run at exactly
`124cdf0d926d9af56d1bb6cd64bf9526a0b0c9e6`, the `campaign/lean-a-plus` tip
after main was synchronized in (#437, which rewrote `state.rs`). Every run is
a release build of that commit on a clean tree (no tracked file modified), one
run at a time, with no other cargo build on the host.

**Host:** `DESKTOP-BTVJFFR` — 12th Gen Intel Core i5-12400F, 31.8 GB, Windows
11 Pro 10.0.26200. Run on 2026-09-13.

| # | Command | Pass condition | Observed at `124cdf0d` | Result | Raw output |
| --- | --- | --- | --- | --- | --- |
| 1 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::long -- --ignored --nocapture --test-threads=1` | every path's counts within +10% and times within +50% between 18,000 and 3,960,000 prints | largest count rise +0.41% (`frame.app.time1d` copy bytes/unit 806,574.6 → 809,870.2); largest time rise +1.45% (`depth.book` 6,900 → 7,000 ns); `sha: 124cdf0d…` with no "tracked files modified"; verdict "within budget" | PASS | [long.txt](long.txt) |
| 2 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app session_length_tests::tape_growth_stalls -- --ignored --nocapture --test-threads=1` | slowest ingest at most 4 ms in both the 2^21 and 2^22 stretches, footprint off and on | footprint off: 0.014 ms (2^21), 0.007 ms (2^22); footprint on: 0.006 ms (2^21), 0.056 ms (2^22); no copy in any stretch | PASS | [tape-growth-stalls.txt](tape-growth-stalls.txt) |
| 3 | `env -u QUANTICK_BUBBLES cargo test --release -p quantick-app live_envelope_tests::measure -- --ignored --nocapture --test-threads=1` | records retained memory at 3,960,000 prints, per-print ingest first against last, and queue depth at 300/s and 2,000/s with `parked 0` | tape 211.5 MiB + bars 9.1 MiB; working set +221.0 MiB (footprint off), +569.9 MiB (on); ingest first/last 100k 111/124 ns (off), 187/241 ns (on); queued max 6/1024 indicator and 23/4096 book at 300/s, 35/1024 and 52/4096 at 2,000/s; `parked 0/0` in every second; settled with deferred 0, lost 0 | PASS | [envelope-measure.txt](envelope-measure.txt) |
| 4 | `tools/live_envelope/run_replay.ps1` ×5, summarised by `tools/live_envelope/frame_timing.py` | WINV26 2026-08-25 replay at speed 60 with book, bubbles, footprint and live strip; at least five 45 s runs; fps min 59; `worker_deferred` 0 | five 45 s runs at ~4,680 prints/s: fps min 59 in every run; frame_avg 16.667 ms; frame_cpu mean 1.766 ms (stdev 0.083); `worker_deferred` 0 and `worker_parked` 0 in all 110 summary lines; `APP_SLOW_FRAMES` 0 | PASS | [frame-timing.txt](frame-timing.txt) |

## What each run proves

1. **Per-unit work does not grow with session length.** Every live path —
   chart ingest (backfilled and live), book trade and depth, the book frame,
   the app frame at tick:50 and time:1d, and both indicator-worker folds —
   costs the same per unit after 3,960,000 prints as after 18,000.
2. **The tape's former reallocation stall is gone.** The chunked tape never
   copies at the old doubling points; the slowest ingest there is under
   0.06 ms against a 4 ms bound.
3. **State is bounded at the stated envelope.** One pane's retained memory at
   the envelope's edge (3,960,000 prints, `RETAINED_TRADES_PER_PANE`) is
   measured, ingest cost is flat from the first to the last 100k prints, the
   heatmap history stays under its caps, and the bounded worker queues stay
   far below capacity with nothing parked at the sustained (300/s) and burst
   (2,000/s) rates.
4. **The real application keeps its frame rate on the densest recorded
   session.** On the 1.70 M-print WINV26 day replayed at about 4,680 prints/s
   — about 2.3 times the burst rate — the app never fell below 59 fps and no
   worker command was deferred or parked.

## Notes on the method

- Runs 1 and 2 print their own `sha:` line; run 3's harness prints none, so
  its file header states the revision and the clean tree. Run 4's header
  states the build and the method.
- Run 4 used a scratch copy of `run_replay.ps1` that also points
  `QUANTICK_FOOTPRINT_SETTINGS` and `QUANTICK_DEALS_DIR` at the per-run scratch
  store; the committed script leaves those two store variables unset, which
  would read the trader's own files. Nothing else differs. `frame_timing.py`
  ran unchanged through a scratch wrapper that sets its run order to the five
  `head-*` labels, because the script assumes a base side as well; the
  wrapper re-derives the head summary line with the script's formula. Neither
  change is committed: this PR changes no code.
- The trader's MetaTrader terminal was open on the host during the runs; the
  replay reads a recorded file and does not use it.
