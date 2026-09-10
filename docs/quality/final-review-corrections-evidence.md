# Final review corrections: coordinator policy and retired lane allocation

Task: [Q13 / #357](https://github.com/milocaetano/quantick/issues/357), under
[campaign #330](https://github.com/milocaetano/quantick/issues/330).
Baseline: `a78842ec852eacf7aecf9af7613e887c6bff144b`.
Status: focused tests and scoped six-run CPU comparison passed. Full ordered
workspace validation, final independent reviews, current-head CI and integration
remain closing work. No delivery approval or numeric score is presumed.

## Maintained behavior and source provenance

ARC-FINAL-HC-001 names three existing coordinator policies:
`DEFAULT_LEASE_MINUTES = 15`, `ACTIVE_TASK_PARTITION_THRESHOLD = 50` and
`COMMENT_PAGE_SIZE = 5`. Query construction preserves the existing GraphQL
request bytes, including `comments(last:5,before:$before)`. Lease policy still
uses the existing fixed renewal duration; no retry-policy interpretation changes.
The initial source and test hashes remain historical provenance in the
[helper README](../../tools/campaign/README.md).

| Source | SHA256 at implementation handoff |
| --- | --- |
| Maintained coordinator | `7e73236894f4a64f32b725f232c6a7cfaa0487779f29e2c2161ad60e1811f0c7` |
| Original 53-test file, unchanged | `2c9a432068d02891f6245621f28daed50ca3731ba06d86e53421151e60c8d3b8` |
| Six composed recovery regressions, unchanged | `74a8d8e720ba6518ea48dd4d8c0d2599633c89ba4c4139614515a6fd39e05c28` |
| Control-plane test file containing dense benchmark, unchanged | `6ce96160da0c905aeb248a56c2ff0557785bf01ce8ed14681ca43034e51e4222` |

FAI-LANE-PEAK-001 changes only the three lane-run reset assignments on BarClosed,
Backfilled and Rebuild from clearing length to replacing the vector with an
empty vector. Retired allocation is released at those boundaries, as already
happens for disabled lanes and vanished partials. The active forming run still
retains every valid trade. Suffix transport, event order, indicator evaluation
and full-run folding are unchanged. Existing command queues and market history
remain unbounded; this change does not establish a global memory or live-rate
bound. Historical measurements/disclosures remain historical records.

## Executed focused behavioral proof

The appended actual-worker test
`retired_epoch_releases_peak_capacity_and_preserves_small_run_outputs` uses the
existing `retained_lane_for_test` observation command. For each reset it sends
4096 alternating signed prints across suffix drains, checks full retained length
and independently specified CVD output, then requires zero length and capacity
immediately after reset. Two later small epochs retain their complete inputs,
produce specified CVD columns/previews/rungs, retain less capacity than the large
epoch and release it again when closed. A no-data update preserves the active
run and output. All existing reset/order/disable/vanish/progress tests and their
assertions remain unchanged. The original 53 coordinator tests and all six
composed regressions, including exact three-page query and lease/refusal paths,
are unchanged.

Root executed repository guards, the original53 tests, all6 composed
regressions, Ruff F and the focused lane suite including the new actual-worker
fixture; all passed on unchanged source. Receipts and full source manifest are
in `quantick-campaign-score9/q13-focused`. Review-progress/shared hooks and the
full ordered workspace loop follow this archive before code commit; their
actual receipts belong in `q13-validation-full1` and its hooks directory.

## Prospective performance protocol

Root must freeze current baseline/candidate sources and binaries before running
the unchanged `incremental_lane_dense_frame_benchmark`. Required serialized order
is B1,A1,A2,B2,B3,A3. For both time and dollar cases, median-of-three mean ratios
must be <=1.10 and p95/p99 ratios <=1.15. Preserve the prospective manifest,
source/binary hashes, all six logs, every failure, capacity/output observations
and comparison JSON. No rerun follows a failure until it is classified and a
finite repair budget is reserved. These are finite CPU fixture results; they
cannot establish GPU, desktop or supported live throughput.

Pending root-owned closing evidence: original source/mission archive,
authority/ownership/arming and attempt reservations, exact validation receipts,
prospective benchmark package, current-head CI, inspectable review-input package,
independent architecture/bug and AI reports, source-first delivery verdict and
campaign-only integration readback. Q12 live adoption, final numeric assessment
and user-only main integration remain separate pending obligations.

## Actual CPU experiment1

[Prospective plan](https://github.com/milocaetano/quantick/issues/357#issuecomment-5611224474) preceded every sample;
[actual complete result](https://github.com/milocaetano/quantick/issues/357#issuecomment-5611225380) retains all original outputs.
Order B1,A1,A2,B2,B3,A3 executed once; all processes exited0.
All12 deterministic fixture parity checks and all6 timing gates passed.

| Fixture | Metric | Baseline median ms | Candidate median ms | Ratio | Ceiling |
| --- | --- | --- | --- | --- | --- |
| time(1m) | mean_ms | 0.320056 | 0.321156 | 1.003437 | 1.10 |
| time(1m) | p95_ms | 0.378800 | 0.393500 | 1.038807 | 1.15 |
| time(1m) | p99_ms | 0.543900 | 0.515100 | 0.947049 | 1.15 |
| dollar(12000000) | mean_ms | 0.527354 | 0.528574 | 1.002313 | 1.10 |
| dollar(12000000) | p95_ms | 0.746400 | 0.785100 | 1.051849 | 1.15 |
| dollar(12000000) | p99_ms | 0.849100 | 0.884900 | 1.042162 | 1.15 |

Frozen baseline source is `a78842ec852eacf7aecf9af7613e887c6bff144b`;
candidate was explicitly uncommitted, identified by the complete focused input
manifest. Binary identities and all six log/receipt pairs are in
`quantick-campaign-score9/q13-measurements`, with comparison JSON SHA256
`25cfb75f4dcc2e4b80a66a66d0450bf6ace7aa658bd807c93750bb08a1372c5d`.

Baseline binary SHA256: `c88148baff115a83743192459e30acee34db4132e475e02f8298608e45498dc6`.

Candidate binary SHA256: `9dac891e230f3571358f06e3d2aa5def47c2d168bc59c33ff0e79e8056de2900`.

The CPU fixture's final retained capacities were identical between versions
(64512 entries for time,16384 for dollar); it is not claimed as a memory
reduction measurement. The separate actual-worker large-to-small regression
proves release to zero capacity at each retired epoch and correct subsequent
outputs. Active forming storage, queues/history and full-run folding retain
their stated limits; no global, desktop, GPU or live scalability gate is earned.

The corrected source preflight closed Q13-PREFLIGHT-MAP-001 before edits; its
v0 artifacts remain retained. Consolidated review batch1 and both original
finding attempt1 remain reserved. Current architecture/AI/full delivery and CI
will be published on the child PR after this exact source is committed. Final
consolidated findings require actual verification, and Q12 must adopt the
reviewed corrected maintained source before its live evidence exercise.
