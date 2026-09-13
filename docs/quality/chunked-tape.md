# The chunked trade tape

Each pane's retained trade tape is now stored in fixed-size chunks, so that
appending a print never copies the tape already held. That removes the
UI-thread stall the contiguous `Vec<Trade>` put on the chart each time it
doubled — 25 ms at print 2,097,152 and 52 ms at 4,194,304 on this host — and
every consumer's output stays byte for byte what it was. It is the evidence
for rubric criterion SE6 beside [the session-length page](session-length.md),
the mission Q12 of campaign #367 (issue #423, the trader's decision D21: "no
change visible to the trader").

Measured on `DESKTOP-BTVJFFR` (Intel Core i5-12400F, 31.8 GB, Windows 11 Pro).
The base is `eb60ed41` (`campaign/lean-a-plus` when the mission began); the
head is `3cc6a7db`, the last commit that changed code. Each raw file under
[chunked-tape/](chunked-tape/) names its command and the commit it ran at.

The branch was then rebased onto `a3c962fa`, whose only changes are
`crates/mcp/*`, a control-plane contract page and a mission archive — nothing
the app's binary is built from (`quantick-mcp` is a leaf). The raw files name
the commits as they were measured; each is the same tree as its rebased
commit outside those files (`git diff --name-only <measured> <rebased>` lists
exactly them):

| Measured as | Rebased as | What it is |
| --- | --- | --- |
| `eb60ed41` | `a3c962fa` | the base |
| `eef07e5f` | `dafcaa90` | the tape's API over a contiguous `Vec`, tests red |
| `b9cbff74` | `33a1d75f` | the consumers on the tape, still contiguous |
| `fca1e190` | `bcffcf2a` | the chunked storage |
| `13b3f51b` | `d11d1b98` | push and readers inlined across the crate boundary |
| `349d04ae` | `9b953e2f` | a test's printed line |
| `d2030cc0` | `3cc6a7db` | the newest chunk kept beside the directory — the head |

## The design

`quantick_engine::trade_tape::TradeTape` (`crates/engine/src/trade_tape.rs`,
one additive file in the headless engine, beside `Trade`):

- **Chunks of `CHUNK_TRADES` = 65,536 prints** (3.5 MiB of 56-byte trades),
  each allocated once at full size and never grown. A power of two, so a
  position splits into chunk and offset with a shift and a mask. Under 2 % of
  the envelope's 222 MB tape, which bounds the reserved-but-unused tail of the
  last chunk; a pane opens one every 3.6 minutes at the envelope's sustained
  300 prints/s, 61 of them at its edge.
- **An append** writes one print into the newest chunk, which the tape keeps
  beside its directory of full ones, so a push touches nothing the directory
  points at; when that chunk is full it moves into the directory and one new
  chunk is allocated. Nothing already held moves.
- **Reads**: `len`, `is_empty`, `get` and indexing, `first`, `last`, `iter`
  and `range` (double-ended, exact-size), `since(start)` (the suffix from a
  position), `last_n(n)`, `slices(range)` (the stretch as at most one
  contiguous slice per chunk, for bulk readers), `partition_point` (a binary
  search over the chunks, then inside one). `TradeSeq` is the positional read
  trait — `len` and `get`, with a default binary search — that `[Trade]`,
  `Vec<Trade>` and the tape implement, for readers that walk by index.
- **Prepending** older history (`prepend`, rare: once per history page, then a
  full rebuild that is O(tape) anyway) copies into a fresh tape, freeing each
  old chunk as soon as it is copied: the peak is the tape plus one chunk,
  where it was two whole tapes.
- The small readers, the iterators' `next`/`next_back` and `push` are
  `#[inline]`: without it a non-generic function is not inlined across the
  crate boundary (the workspace builds without LTO), and the chart paid a call
  per print that `Vec::push` never cost it (`d11d1b98`).

## The consumers

Every reader of `ChartState::trades()`, which now returns `&TradeTape`
(`grep` for `trades()` across the workspace at `eb60ed41`):

| Reader | Rate | Before | After |
| --- | --- | --- | --- |
| `ChartState::ingest_live` | per print | `Vec::push`, doubling | `TradeTape::push`, one chunk per 65,536 |
| `LaneTransport::command` (`indicator_worker.rs`) | per frame | `trades[start..].to_vec()` | the same unsent suffix, copied chunk slice by chunk slice into one exact allocation — the only contiguous copy, of the requested range only |
| `ChartState::ingest_backfill` | per load | `&[Trade]`, then reserve twice what is held | any cloneable run of prints (a slice, or another tape's `range`) |
| `ChartPane::seed_from` (`pane/series.rs`), from `tab/layout.rs` | per split | `&trades[..split]`, `&trades[split..]` | `range(..split)`, `since(split)` |
| `prepend_history` | per history page | a fresh `Vec` of twice the joined size | `TradeTape::prepend` |
| `rebuild`, `refold_footprints` | per spec change / footprint toggle | slice iteration | tape iteration |
| `Campaign::advance`, `traded_span_before`, `last_close_before` (`quantick-feed`) | per history reply | `&[Trade]` | any `TradeSeq`: the chart's tape, or a slice |
| `Tab::oldest_retained_trade_ms`, `empty_page_verdict`, `load_older` count (`tab/history.rs`) | per history action | `first()`, `is_empty()`, `len()` | the same on the tape |
| health envelope retained count | per publish | `len()` | `len()` |

The indicator worker and the footprint ladders never read the tape: the
worker is handed the lane transport's copy, and the ladders are fed each print
as it is ingested (and by the rebuild and refold walks above). The control
plane reads only the envelope's retained count. No replay export reads it —
a replay is a feed source, upstream of the chart. No consumer clones the whole
tape.

`ChartState`'s field type is a reviewed amendment of its declared shape in
`crates/guards/extension-shapes-baseline.txt`; its root production lines fell
from 429 to 426 and the cap was tightened with them.

## Output identity

- **Every chart path, against a `Vec` oracle** —
  `state::tape_identity_tests::every_way_in_shows_what_a_contiguous_tape_showed`:
  every golden trade tape in `crates/engine/tests/fixtures/` through six bar
  specs (tick 3 and 50, volume, dollar, time, imbalance), and all of them
  joined and repeated past three chunk boundaries (more than 196,608 prints)
  through tick:50 and time:1s, with the footprint off and on, gives the same
  closed bars, forming bar, backfill boundary,
  footprint ladders and retained prints — compared as `Debug` text, so a
  `Decimal`'s scale counts — as a plain fold of the `Vec` through a fresh
  builder and footprint series. Paths: a backfill, live prints, backfill then
  live, a page prepended between, a second chart seeded from the first's tape,
  and on the long tape a spec switch then a footprint refold.
- **The lane transport, against itself before** —
  `pane::lane_transport_tests::the_chunked_tape_sends_the_worker_what_the_slice_sent`
  keeps the slice-based `LaneTransport` of `eb60ed41` verbatim and requires
  the same run on every frame, on tick:50 and time:1d, through bar closes, lane
  switch-offs and bursts, with runs that straddle chunk boundaries.
- **The history reach** —
  `history_reach::tests::the_reach_reads_a_chunked_tape_as_it_read_a_slice`:
  the same answers over a tape and a slice for anchors either side of chunk
  boundaries, and the same campaign steps for every reach.
- **The tape type** — `crates/engine/tests/trade_tape.rs`: after every one of
  a sequence of irregular pushes, batch appends and prepends across several
  chunk boundaries, and on the golden tapes repeated, every read answers as
  the same read over a `Vec`.
- **The contiguous stage.** `33a1d75f` moved every consumer onto the tape's
  API while it still kept one `Vec` (with the chart's own sizing); every
  identity test passed there before the storage changed under them in
  `bcffcf2a`, where they pass unchanged.
- **Q4's harness** reads the same prints copied to the worker (lane entries
  per unit 0.90 / 4.50 / 5.00) and the same folds (26.10 on tick:50;
  1,929.91 and 1,889.48 on time:1d at 18,000 and 180,000 prints) before and
  after — [fast-base.txt](chunked-tape/fast-base.txt),
  [fast-head.txt](chunked-tape/fast-head.txt).

Test-first: `dafcaa90` committed the tape's API over a contiguous `Vec`, the
oracle tests (green) and four tests ignored and red against it — no print
moves when the tape grows, at most one chunk held unused, one slice per chunk,
and building the fast variant's session live never copies more than one chunk
(7,340,032 bytes there at the base). `bcffcf2a` chunked the storage and
removed the ignores.

## The stall at the former doubling points

[The stall probe](../../crates/app/src/app/tests/session_length_tests.rs)
(`tape_growth_stalls`, ignored) builds one tick:50 pane live, print by print,
to 2^22 + 65,536 = 4,259,840 prints — past the envelope's 3,960,000 so both
former doubling points are inside — timing every ingest and counting its
largest reallocation. The bound is **4 ms**, a quarter of a 60 fps frame.

| Footprint | Stretch | Base: slowest ingest | Base: largest copy | Head: slowest ingest | Head: largest copy |
| --- | --- | ---: | ---: | ---: | ---: |
| off | 2^21 ± 1,024 prints | 25.444 ms | 117,440,512 B | 0.007 ms | 0 |
| off | 2^22 ± 1,024 prints | 52.461 ms | 234,881,024 B | 0.012 ms | 0 |
| on | 2^21 ± 1,024 prints | 21.952 ms | 117,440,512 B | 0.011 ms | 0 |
| on | 2^22 ± 1,024 prints | 53.217 ms | 234,881,024 B | 0.064 ms | 0 |
| off | whole run | 52.461 ms | 234,881,024 B | 1.427 ms | 7,864,320 B |
| on | whole run | 53.217 ms | 234,881,024 B | 2.321 ms | 7,864,320 B |

From [stall-base.txt](chunked-tape/stall-base.txt) and
[stall-head.txt](chunked-tape/stall-head.txt); the interleaved runs below
repeat it seven times a side (base 20.5–29.4 ms at 2^21 and 41.8–58.3 ms at
2^22; head at most 0.119 ms at either). **No ingest near either former doubling point comes near
the bound; the largest is 0.11 ms.**

What is left, and named: the largest copy anywhere in the head's run is
7,864,320 bytes at print 3,276,849 — the chart's *bar* vector doubling at
65,536 bars (120 B each), not the tape — 1.4–2.9 ms in every head run, under
the bound. Twice in fourteen interleaved head runs the slowest ingest of the
whole run fell elsewhere (4.2 ms at print 428,908, 3.7 ms at 3,344,292) with no
reallocation there: preemption on a host shared with sibling agents' builds,
which a wall-clock maximum cannot tell from work. The footprint ladders' vector doubles at the same bar count.
The next such doubling is at 131,072 bars, 6.55 million prints on tick:50,
outside the envelope. They are outside this mission's scope (the tape); their
size is reported, not hidden.

## Per-trade cost

The same probe, base and head builds interleaved, five runs each of the base
(`dafcaa90`'s test binary, the base's `Vec`), the all-in-directory chunked
form (`d11d1b98`) and the final form, in one sequence
([probe-interleaved-final.txt](chunked-tape/probe-interleaved-final.txt);
an earlier set of seven pairs against `d11d1b98` under a loaded host is in
[probe-interleaved.txt](chunked-tape/probe-interleaved.txt)):

| Footprint | Measure (ns per print) | Base | Head | Difference |
| --- | --- | ---: | ---: | ---: |
| off | median 100k-print block (steady state, probe timer included) | 107.6 (sd 3.4) | 106.6 (sd 3.0) | −1.0 (t −0.5) |
| off | ingest alone, mean over the whole run, doublings included | 95.4 (sd 3.1) | 76.2 (sd 3.4) | −19.2 (−20 %) |
| on | median 100k-print block | 196.8 (sd 13.1) | 191.8 (sd 6.5) | −5.0 (t −0.8) |
| on | ingest alone, mean over the whole run | 184.9 (sd 10.0) | 164.4 (sd 4.2) | −20.4 (−11 %) |

Steady-state per-print cost is unchanged within noise; over a whole session
the head is cheaper, because it no longer copies the tape at every doubling
nor touches twice its memory doing it. The long variant's `trade.chart.live`
median lap reads 140 ns per print at both ends and on both sides.

## Counts: Q4's session-length harness

| Harness | Base | Head |
| --- | --- | --- |
| fast: largest single copy building 180,000 prints live | 7,340,032 B (the tape, at 131,072) | 245,760 B (the bar vector) |
| fast: copy bytes per print over that session | 85.4 | 3.8 |
| long: largest single copy building 3,960,000 prints live | 117,440,512 B (the tape, at 2^21) | 7,864,320 B (the bar vector) |
| long: copy bytes per print over that session | 64.9 | 5.6 |
| prints folded and copied to the worker, every path, both lengths | 26.10 / 1,923–1,930 folds; 0.90 / 4.50 / 5.00 entries | identical |
| heap counts per unit, both lengths | within budget | equal on the ingest, book and worker paths; within the base's own run-to-run spread on the two whole-frame paths (timer-driven UI work); within budget |

The fast variant now also asserts the one-chunk bound —
`building_the_tape_live_never_copies_more_than_one_chunk`, counts only, part
of `cargo test --workspace` — and `the_growth_check_fails_a_contiguous_tape`
shows the bound fails a `Vec` grown to the same length. Q4's growth budgets
on every read path are unchanged and hold at both lengths
([long-base.txt](chunked-tape/long-base.txt),
[long-head.txt](chunked-tape/long-head.txt)).

The first long run at the head failed its **time** bound on `depth.book` —
7,800 against 12,200 ns per depth update, allowed 11,700 — with every count
identical; `depth.book` is the book worker's depth path, which never reads the
tape, and the run shared the host with sibling agents' builds. The rerun at the
same commit passed; both outputs are committed
([long-head-run1.txt](chunked-tape/long-head-run1.txt)).

## Memory

The tape at the envelope's edge, 3,960,000 prints (221,760,000 bytes of
trades), from the probe's memory table; the base's figures are measured on
the contiguous stage `33a1d75f`, whose `Vec` grows and reserves exactly as the
base's did ([stall-contiguous.txt](chunked-tape/stall-contiguous.txt)):

| Tape | Built live | Loaded at once |
| --- | ---: | ---: |
| `Vec` (base) | 4,194,304 prints reserved: 234,881,024 B | 7,920,000 reserved (twice what is held, so the first live print copies nothing): 443,520,000 B |
| chunked (head) | 61 chunks: 223,870,976 B + 61 chunk headers (1,464 B) = 223,872,440 B | the same, 223,872,440 B |
| difference | −11,008,584 B (−4.7 %) | −219,647,560 B (−49.5 %) |

The chunked form reserves at most one chunk it does not use (here 2,112,440
bytes); the bound D4 set — no more than the `Vec` plus one chunk plus
bookkeeping — holds with room: it is below the `Vec` in both cases. Q3's
envelope harness reads the working set the same (+221.0 MiB against +220.8
at the base, footprint off; +569.9 against +569.5 on)
([envelope-measure-head.txt](chunked-tape/envelope-measure-head.txt)).

## Frame timing

`APP_HEALTH_SUMMARY` on the WINV26 2026-08-25 replay at speed 60 with the
book, bubbles, footprint and live strip on, release builds of the base and of
the head, five 45 s runs a side, interleaved, every store in a scratch
directory (`tools/live_envelope/run_replay.ps1`, `frame_timing.py`; PR #388's
method); raw tables in [frame-timing.txt](chunked-tape/frame-timing.txt):

| Side | fps min | frame_avg ms | frame_cpu ms mean (per run) | stdev | `APP_SLOW_FRAMES` |
| --- | ---: | ---: | --- | ---: | ---: |
| base | 59 | 16.667 | 2.175 (2.21, 2.19, 2.25, 2.02, 2.21) | 0.092 | 0 |
| head | 59 | 16.666 | 2.202 (2.38, 2.27, 2.19, 2.12, 2.06) | 0.126 | 0 |

frame_cpu differs by +0.027 ms (+1.2 %) against a standard error of the
difference of 0.070 ms (t ≈ 0.39): within noise. fps never fell below 59 and
no slow-frame line appeared on either side. A replay at this speed covers a
session's prints, not 2 million of them, so it measures the steady state —
the removed stall is the probe's to show. An earlier set, taken while sibling
agents' builds held the CPU, read 3.153 against 2.912 ms (t ≈ −0.48) and is in
the same file.

## Reproduce

```sh
# The tape and its identity tests:
cargo test -p quantick-engine --test trade_tape
env -u QUANTICK_BUBBLES cargo test -p quantick-app -- tape_identity_tests the_chunked_tape_sends
cargo test -p quantick-feed the_reach_reads_a_chunked_tape

# Q4's harness with the one-chunk bound (part of cargo test --workspace):
env -u QUANTICK_BUBBLES cargo test -p quantick-app -- session_length_tests --nocapture --test-threads=1

# The stall probe and the memory table (release, a few seconds):
env -u QUANTICK_BUBBLES cargo test --release -p quantick-app \
    session_length_tests::tape_growth_stalls -- --ignored --nocapture --test-threads=1

# The long variant (about 3 minutes):
env -u QUANTICK_BUBBLES cargo test --release -p quantick-app \
    session_length_tests::long -- --ignored --nocapture --test-threads=1
```
