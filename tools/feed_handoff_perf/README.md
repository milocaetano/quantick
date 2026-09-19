# Feed handoff paired measurement

Reusable source export, build and measurement tooling for issue495. This folder
contains inputs and executable tooling, never run evidence. `protocol.md` and
`fixture.json` preserve the original fixed protocol verbatim. No score is produced.

The approved synchronized control is commit
`f217fcf3db65dac0fc1cbb98956d9155000522ac`, replacing protocol control9ff57501.
The eight relevant runtime files are byte-identical between those controls:
feed binance/hyperliquid/continuity, feed-hyperliquid stream/wire, and
feed-binance stream/backfill/continuity. Both exports use synchronized dependency
inputs. The candidate must be the exact clean committed PR head. Original Git
archives, source trees and separate instrumentation hashes identify both sides.

Run the offline tests with:

```sh
python3 -m unittest discover -s tools/feed_handoff_perf -p 'test_*.py'
```

`prepare.py --repo CHECKOUT --output EXTERNAL_DIR --candidate-commit FULL_SHA`
exports and instruments both sides. `build.py --exports EXTERNAL_DIR` builds
their release feed unit-test executables sequentially with jobs1, without running
tests. `run_pairs.py --help` describes the explicit identity and lease arguments.
The hosted job supplies them from the build receipts. These commands never
overwrite an existing export or attempt directory.

Sampling requires the separately authorized one-attempt lease and the approved
Ubuntu24.04 hosted job. Windows functional validation remains separate. The
workflow runs on the initial opening of the exact child PR and rejects a rerun.
Subsequent pushes do not launch a new attempt. The workflow's pending lease
sentinel must be replaced by the coordinator before publication; the source-scope
approval is not itself permission to take samples.

Admission records Linux CPU, image, available power policy and bounded process
observations. It refuses present compilers/linkers, active observed build
descendants, new active processes and material background activity above0.10 of
one core during a3second observation. The process tree is sampled during builds;
short-lived unobserved descendants and future VM scheduler noise remain explicit
limitations. No process is killed. These conservative preconditions do not
replace the unchanged5% CV validity test. Both success and failure receipts are
uploaded, including source/build/admission failures before the first sample.

The common Rust harness and fixed JSON bytes are identical across exports. Only
test-only endpoint/visibility adapters differ. Actual REST/WebSocket source and
feed-host paths are measured, including transport overhead on both sides.
Payload creation, server setup and exclusion prime events are outside measured
segments. Candidate diagnostic processing stays inside each measured segment;
literal correctness assertions and final channel checks remain outside. The
three warmups and15 alternating measured pairs, all sample retention, median,
nearest-rank p95 and sample CV calculations keep the original limits unchanged.

Hosted source amendment:
https://github.com/milocaetano/quantick/issues/495#issuecomment-5708816320.
Any invalid or failing attempt remains evidence; this tooling contains no retry
loop and does not authorize another attempt.


### S2 observed-port instrumentation amendment

Decision: https://github.com/milocaetano/quantick/issues/495#issuecomment-5710534084.
The immutable control remains f217fcf3db65dac0fc1cbb98956d9155000522ac. The
candidate Binance producer remains its actual legacy host, but its timed drain
now receives through the actual ObservedReceiver legacy arm. Hyperliquid uses
the actual static ObservedOutput and observed receiver. Narrow test-only aliases
normalize the different public event types into the common harness; they do not
reimplement source classification. Malformed and stale received rows are counted
separately from legacy skipped IDs and unknown outages. The common timer, usable
trade checksum, prime exclusion, post-join no-extra-event assertion, channel
capacity and literal input bytes remain shared by both exports.

A single fixed 10-second settling interval runs after metadata and before the
unchanged 3-second / 0.10-core admission gate. Its UTC and monotonic boundaries
are retained in settling.json. It never polls admission, exempts a runner process,
retries, discards a sample or alters any ratio/CV limit. The original protocol.md
and fixture.json bytes remain frozen. Authoring this amendment is not permission
to execute another hosted attempt; the coordinator must release a corrected
exact-head candidate and a separate finite attempt.
