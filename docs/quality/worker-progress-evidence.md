# Worker progress diagnostics

This dossier describes the internal observations added for campaign Q8 / issue
#340. Experiment 1 failed its completion-overhead gates on source `a5dfbb4`.
Experiment 2 also failed completion gates on source `d461e00` after repair 1.
Experiment 3 on `27fc95d` passed all numeric gates except time completion p99.
Repair 3 combines adjacent subset-accounting and Publishing updates. Source
`65e24caa` passed the full ordered validation and every unchanged performance
gate in experiment 4. Earlier failures remain retained below. Independent final
reviews, final-head CI, campaign integration and score reassessment remain due.

## Production path and ownership

`QuantickApp::maybe_emit_summary` keeps its existing two-second cadence and
`APP_HEALTH_SUMMARY` fields. It additionally walks every owned tab and pane,
including off-screen panes, and emits `APP_WORKER_PROGRESS` schema version 1
for each indicator worker and optional book worker. The record carries tab ID,
pane ID, side index (flow 0, context 1 onward), worker kind and process-local
instance ID. `observation` is a JSON-encoded typed internal snapshot. This is
an internal tracing event; no public DTO, schema, permission or capability was
added. There is no new control-plane reachability claim.

The observation reader combines owner-local admission counters with its
fixed-size shared sample slot and consumer ledger. It does not inspect the
command queue or clone indicator columns, ladders or projected frames.
Serialization occurs after both locks are released and only at the health
cadence when INFO logging is enabled. Every summary adds one record per owned
worker, so event volume is proportional to owner count, not session history.

## Accounting and sampling

Each worker keeps fixed-size local admission counters, a shared consumer
ledger and one optional accepted-command sample. Normal clocks report monotonic nanoseconds since that instance's creation. Fixture
clocks are explicitly labeled `fixture_explicit`; their numbers are scripted
observations, not measured wall-clock latencies. Instance IDs are process-local
and never reused; exhausted ID space yields an unavailable ID.

For a valid ledger, accepted commands reconcile as
`accepted = queued + inflight + retired + unfinished`.

- `queued` counts accepted commands not yet admitted to the next batch ledger.
  A command already received into the worker's temporary batch can still count
  here. This is not an instantaneous mpsc channel length.
- `inflight` counts membership in the active batch, including commands that
  will be coalesced and Flush commands. It does not count simultaneous CPU
  operations or promise that every member remains unapplied.
- `retired` advances for the whole original batch after its publication
  boundary. Flush acknowledgement follows this accounting. `cycles` counts
  these completed boundaries, including no-op cycles.
- `unfinished` retains accepted work on worker exit that did not reach a
  completed cycle. Some of that work may already have affected domain state.
  A successful send racing the final receiver destruction is also recorded as
  unfinished if the exit observation has already occurred.
- `failed_sends` counts unsuccessful admissions and is outside `accepted`.
  Existing logging and command channel semantics remain unchanged.
- `inputs_superseded` counts removed SetInputs commands. The survivor remains
  at its original last occurrence. `partials_superseded` counts preview values
  overwritten by a later PartialUpdated before an intervening reset/close;
  their ordered trade suffixes are still applied. `projects_superseded` counts
  replaced Project requests. These are subsets of batch work, not additional
  retirements or dropped data commands. A batch-local RAII owner preserves
  already-counted subsets when domain work unwinds.
  The normal transition commits these subsets and the Publishing phase under
  one ledger lock, then disarms the RAII flush before calling the unlocked
  phase callback. An Applying unwind still flushes once; a Publishing callback
  unwind cannot count the subsets twice. Each output result remains accounted
  immediately after its existing send.
- Indicator `output_attempts = output_successes + output_failures` records
  event-channel sends separately from cycle completion. Success means channel
  delivery, not UI adoption. Book `mailbox_replacements` records replacement
  of its existing latest-value mailbox; intermediate values may never be read.

A unique `ObservedSender` owns the existing unbounded command sender and local
checked admission counters. It is not Clone, Send or Sync. The read-only
`ProgressObserver` can be cloned on the same UI thread; its `Rc` retains local
counters and identity after the command endpoint is dropped without keeping the
channel open. Only the `Arc<SharedProgress>` consumer ledger and sample state
cross into the worker thread. No atomic counter publication or shared producer
mutex is paid per command.

An unsampled successful send updates local counters and reads the sample's
atomic pending flag. When that flag reports the previous sample acknowledged,
the producer acquires the sample slot and offers the next ticket with its clock
reading. The consumer only tries that slot once during batch admission and
continues immediately on contention. Clearing the slot publishes acknowledgement
with release ordering; the next producer observation uses acquire ordering.
The slot remains protected by its mutex, including a second empty-slot check.

Observation cannot overlap enqueue/bookkeeping because the unique producer and
all read-only observation handles belong to the same thread. The reader takes
the sample slot then consumer ledger; consumer admission only tries the sample
lock, so it never waits in the reverse order. Active backlog is accepted minus
admitted work; terminal unfinished work is accepted minus retired work, including
successful sends accounted after closure. Domain work, channel send/receive,
mailbox access, logging and phase callbacks hold no diagnostic lock. Clock reads
must be bounded and nonblocking; fixture synchronization belongs in phase
callbacks. Fixed storage does not establish a hard wall-clock bound on mutex
acquisition. Shared sample and consumer regions use 64-byte alignment as a fixed
layout choice, not a claim about every host's cache-line size.

The first accepted command after the previous sample's admission was
acknowledged becomes the new sample. Later accepted commands have no auxiliary
timestamps. This is a
biased single-ticket sample, not a distribution or percentile estimator.
`sample_age` is its time since successful admission; `last_sample_residence`
is the duration to batch admission when that transition was observed, attributed
by `last_sampled_ticket`. If sample installation is late or sample-slot contention prevents transfer at
admission, residence is Unknown. A later observation cannot substitute its own
time for that missed transition. The retained slot is acknowledged by a later
uncontended batch; only a subsequent successful send can install a new sample.
An already admitted sample is never reported as waiting, and terminal workers
have no active waiting/processing ages. `oldest_wait` equals sample
age only if its ticket is provably the next to admit; earlier unsampled backlog
makes it Unknown. Idle/empty readings are NotApplicable. `processing_age` is
elapsed time since the current batch entered Applying, including publication;
`since_progress` is time since the most recent completed cycle while work is
pending, and Unknown before the first completion.

Age values distinguish Known (nanoseconds), Unknown, NotApplicable and Invalid
(clock reversal). Counter arithmetic is checked; overflow or a poisoned
ledger sets `valid=false`, after which counters must not be treated as exact.
Monitoring failure does not change domain admission, discard commands, restart
a worker or silently wrap counters.

## Deterministic fixture oracles

The explicit clock/phase port is compiled into normal worker lifecycle,
admission, applying and publication paths. The injected fixture fake pauses
those phases without holding telemetry, channel or publication locks. Tests
require successful bounded acknowledgements; timeout is a failure escape.

Known-age fixtures use test-only prepared constructors that return the ordinary
endpoint and a closure calling the unchanged production consumer. They complete
the original first send and its sample publication before spawning that closure.
The actual-summary fixture still records the original zero-work indicator
snapshot before its first send. This orders the prescribed Known(30) schedule
without a production startup callback, extra command or timing assumption.
The separate S1-S4 schedules deliberately retain late bookkeeping and Unknown.

`indicator_worker::progress_tests` prescribes an Add batch followed by history
`+10`, two input edits, two partial suffixes `+2,-1`, and Flush. It expects
seven accepted/retired commands, two cycles, one superseded input and preview,
committed CVD 10, preview 11, and lane rungs 12 then 11. A disconnected event
receiver separately requires completed domain cycles and failed publications.

`orderflow_worker::progress_tests` prescribes generation 10, snapshot update 10,
contiguous update 11, an ordered buy trade and two Project requests. It expects
eight accepted/retired commands, two cycles and mailbox replacements, one
superseded Project, final bid 99 at quantity 7 and ask 101 at quantity 9,
one depth update and aggression, no gap, and last update ID 11. The selected
projection starts at index 0 and contains one bar slot. Two archived cells
carry generation 10, prices 99/101 and quantities 5/6; the final updated open
runs have zero drawable duration at the latest book timestamp.

For both held workers, applying begins at 0 ns, queued work is accepted at
10 ns, and the 40 ns observation must report processing age 40 ns and oldest
wait 30 ns. The next batch begins at 40 ns and is held at publication at
70 ns, reporting processing age 30 ns and sampled residence 30 ns. Release
drains the same identity. Sender closure, deliberate unwind, failed admission
and deliberate constructor replacement have separate assertions. Replacement
replays the independent domain oracle under a new identity; symbol reset stays
on the same live identity.

`worker_progress::tests` covers a sample behind an unsampled head, idle before
first progress, unavailable clocks, backwards time and counter overflow.
Repair 1 adds independently prescribed ledger protocol schedules in
`worker_progress::tests::protocol` and the real book worker's
`delayed_producer_bookkeeping_does_not_block_real_flush_or_sample_recovery`.
They delay successful-send bookkeeping through actual Flush acknowledgement, require
Unknown residence for the missed admission, and recover a 50 ns residence on
ticket 3 after ticket 2 acknowledges the stale sample. Separate retired,
unwound and unadmitted terminal branches distinguish late successful accounting
from an ordinary failed send. Direct enqueue followed by the same production
accounting helper is explicitly a protocol interleaving fixture, not proof of
the whole ordinary send wrapper or a replacement domain-processing path.
Repair 2 retains the S1-S4 schedules and expected values while removing the old
implementation-specific producer-mutex hold. The new ownership fixture checks
that producer endpoints cannot be cloned/shared across threads and only consumer
state can cross the thread boundary. A separate held-slot fixture requires an
unsampled send to complete while the shared sample slot is locked, preserves
ticket 1, and recovers ticket 3 while ticket 2 remains an unsampled head. Both
real-worker fixtures retain observation clones through actual worker destruction
and still require Closed with no unfinished work.
An additional contention fixture holds the actual shared sample mutex through
production begin/finish acknowledgement at 30 ns for a ticket sampled at 10 ns.
After releasing the lock, the 50 ns observation requires Unknown residence,
never the missed 20 ns or observation-derived 40 ns. Ticket 2 acknowledges the
stale slot at 80 ns; ticket 3 sampled at 100 ns and admitted at 130 ns recovers
a literal Known(30). Actual channel receipt, acknowledgements and thread joins
are required. This complements the unsampled-send fixture, whose lock is
released before admission, rather than claiming that fixture covered contention.
`app::tests::worker_progress_tests` captures an actual JSON tracing subscriber
while calling the existing summary entrypoint with off-screen tab 42 and
context pane 900, asserting owner records and paused/recovered observations.
Existing incremental lane, input-coalescing, indicator and orderflow tests
remain regression evidence; their original assertions and thresholds remain.

## Performance and evidence identity

Touched rates are producer admission per command; batch admission/completion;
indicator event delivery; book mailbox publication; and the two-second
diagnostic reader/serializer. Dense time/dollar/depth benchmark measurements
must include the actual normal instrumentation and report admission separately
from completed worker bursts. They do not certify a different diagnostic
logging rate. Baseline telemetry is explicitly unavailable before Q8.

The prospectively approved gates remain median-of-run-mean completion ratios
at most 1.10, p95/p99 ratios at most 1.15, added admission mean at most 250 ns
per command and added admission p95/p99 at most 500 ns per command. Use the
same exclusive host, exact baseline/candidate source and binary hashes, and
at least B1,A1,A2,B2,B3,A3. Preserve all adverse, failed and inconclusive runs.
Report raw admission percentages even where the absolute gate is satisfied.

Experiment 1 retains all six original runs under
`Q8-validation/experiment-1-workers/`. All 18 domain oracles and stage-counter
contracts passed, as did admission gates. Completion mean ratios failed:
time 1.1138, dollar 1.1919, depth 1.1446; time p99, dollar p95 and depth p95
also failed their unchanged ceilings. The separate actual-summary experiment
passed its record/ownership contracts; mean synchronous caller cost increased
from 7.797 to 30.754 microseconds per synthetic two-second summary opportunity.
That result is descriptive and excludes asynchronous counter-reset completion,
disk I/O and GUI work.

Experiment 2 retains all six runs under the coordinator's
`quantick-campaign-score9/experiment-2-workers/`. All 18 domain/telemetry contracts
and admission absolute gates passed. Completion mean ratios were time 1.0804,
dollar 1.1678 and depth 1.1518; dollar/depth means failed. Time p99 1.3045 and
dollar p95 1.3223 also failed. Candidate workload cycle counts, including warmup
but excluding setup, were time 721/775/771, dollar 878/910/906 and depth
818/782/800 for 630 bursts each. Baseline cycle/rebuild counters remain
unavailable, so those counts do not prove amplification relative to baseline.
The separately executed summary experiment passed its descriptive ownership and
record contracts; it does not waive the failed worker completion gates.

Repair 2 removes the shared producer mutex paid on every command by making
admission explicitly single-producer and owner-local. Added mean admission
cost in experiment 2 was about 1.90, 2.28 and 5.86 microseconds per time/dollar/
depth burst, versus completion increases of about 31.59, 36.18 and 4588.16
microseconds. Direct admission cost alone does not explain those differences.
Natural batch timing can repeat retained-lane or projection/publication work,
but that is a repair hypothesis, not an established cause or a predicted PASS.
Natural channel draining, domain work, projection cadence, shared harness bytes,
independent domain oracles and acceptance ceilings remain fixed.

Experiment 3 retains all six worker and summary runs under
`quantick-campaign-score9/experiment-3-workers/` and `experiment-3-summary/`.
The [prospective record](https://github.com/milocaetano/quantick/issues/340#issuecomment-5609287684)
binds source `27fc95d97d2af33c3e2d9d22350a616a9136d5a7`, the actual release
binary and complete manifests to the unchanged recipe. All 18 worker
domain/counter contracts and every absolute admission gate passed. The
[complete result](https://github.com/milocaetano/quantick/issues/340#issuecomment-5609378223)
retains the failed time p99 gate:

| Completion ratio, candidate/base | Mean (limit 1.10) | p95 (limit 1.15) | p99 (limit 1.15) |
| --- | --- | --- | --- |
| Time | 1.034928 | 1.029565 | **1.263822 FAIL** |
| Dollar | 1.042041 | 1.073533 | 1.032802 |
| Depth | 0.985278 | 0.957698 | 0.976308 |

The six summary contracts passed; median mean synchronous caller cost was
7.7065 microseconds for the baseline and 32.7853 microseconds for the candidate.
Summary cost remains descriptive and excludes disk I/O, GUI work and
asynchronous resets. It does not substitute for the failed worker gate.
The exact failed experiment 3 binary is retained as
`candidate-release-3/frozen-build-3/original-test-binary.exe`, SHA256
`ff7e8dde2664ccd0fb71c811518a153e0f1dc989d10e445fe9ea31ed6029388f`.

The [repair 3 reservation and independent diagnosis](https://github.com/milocaetano/quantick/issues/340#issuecomment-5609490772)
retain all distributions and reject treating the failed p99 as disposable noise.
Fusing the adjacent subset and Publishing ledger updates saves one acquisition
per completed transition, or 701/710/703 in the observed candidate time runs.
The source and aggregate counters do not prove that this will close the
remaining 82.145 microseconds above the p99 ceiling. No domain scheduling,
retained-lane folding, clock callback order, immediate output accounting or
benchmark input is changed. At that reservation, repair attempts were 3 of 3 and experiment attempts
remained 3. The separately frozen fourth experiment below advances only the
experiment count to 4; no repair budget or failed result is reset.

External evidence is retained under the coordinator's `Q8-validation/`
directory. `04-development-check-failure-excerpts.md` preserves the original
development compile failure with tool-output provenance, and
`05-independent-worker-schedules.md` preserves pre-execution expected schedules.
The frozen harness, source/binary manifests, raw test transcripts, measurements
and ordered verification receipts must be associated with the delivered source
commit before this dossier can claim completion.

## Experiment 4 and delivered-source evidence

The [prospective experiment](https://github.com/milocaetano/quantick/issues/340#issuecomment-5609794967)
and [complete checkpoint 953](https://github.com/milocaetano/quantick/issues/330#issuecomment-5609826793)
precede first B1 execution at 2026-09-09T22:52:45Z. The
[complete result](https://github.com/milocaetano/quantick/issues/340#issuecomment-5609861970) retains all six alternating worker runs and
six separate summary runs. Every one of the 18 domain/counter contracts and
all original performance gates passed. All summary output contracts passed.
There was no selected rerun, trimming, threshold change or discarded prior
failure. This finite evidence supports the repaired candidate; it does not
establish a causal explanation for every earlier timing distribution.

| Completion candidate/base | Mean (limit 1.10) | p95 (limit 1.15) | p99 (limit 1.15) |
| --- | --- | --- | --- |
| Time | 1.022367 | 1.012538 | 1.032396 |
| Dollar | 1.051347 | 1.048045 | 1.014022 |
| Depth | 0.993455 | 0.983488 | 0.984447 |

Admission gates use added nanoseconds per command, not relative percentages.
Adverse relative changes are retained in this table and the complete result.

| Kind / metric | Baseline ns | Candidate ns | Added ns | Relative change | Limit added ns |
| --- | --- | --- | --- | --- | --- |
| time / mean | 83.546875 | 87.216146 | 3.669271 | +4.391871% | 250 |
| time / p95 | 200.000000 | 200.000000 | 0.000000 | +0.000000% | 500 |
| time / p99 | 1200.000000 | 1100.000000 | -100.000000 | -8.333333% | 500 |
| dollar / mean | 88.612572 | 93.294620 | 4.682048 | +5.283729% | 250 |
| dollar / p95 | 300.000000 | 300.000000 | 0.000000 | +0.000000% | 500 |
| dollar / p99 | 1600.000000 | 1700.000000 | 100.000000 | +6.250000% | 500 |
| depth / mean | 83.725641 | 91.580769 | 7.855128 | +9.381986% | 250 |
| depth / p95 | 200.000000 | 100.000000 | -100.000000 | -50.000000% | 500 |
| depth / p99 | 1300.000000 | 1400.000000 | 100.000000 | +7.692308% | 500 |

The separate summary fixture's median mean synchronous caller cost was
7.832500 microseconds baseline and
31.281667 microseconds candidate
(+299.382913%). It emits five additional worker records
per opportunity. Its synthetic two-second cadence yields arithmetic costs,
not measured background CPU, disk throughput or GUI frame time; no numerical
summary gate was introduced. See the complete result for tail/byte/event values.

| Frozen input | Identity |
| --- | --- |
| Candidate source | `65e24caa23b35381471b84eb69ae69db09ac794f` |
| Candidate tree | `a20943228cf0691d566d2bd66933fff652e13caf` |
| Candidate release test binary SHA256 | `43877d47bf50b9d27b3bb14e308139bc1b725c165c4c91e2e8c1220bd2199b3d` |
| Complete source manifest SHA256 | `74764bfdc9c2c1cae42d06d8299c91a990868e7b2d22b07ba6745beb4fd329e1` |
| Actual release build log SHA256 | `c65835d0326ecd0ff258b6f0b97f88ab873fdd839909603a519dce91ee2c3f36` |
| Full validation source manifest SHA256 | `eede4b969675a4f8004476cadbbf0cc420e9c5799baf741701fd7c30b75678bd` |
| Independent experiment audit SHA256 | `68360820b2fe50d4532b901a8274f02d3873602009ef0d51a3f7913aa9b72057` |

All exact manifests, raw samples and output/exit hashes remain in
`quantick-campaign-score9/experiment-4-inputs`, `experiment-4-workers` and
`experiment-4-summary`; the actual executable is also archived under
`candidate-release-4/frozen-build-4/original-test-binary.exe`. Baseline is the
unchanged instrumented `98c1955` control; its manifest and binary identity are
in the prospective record. The two pre-execution host-policy/missing-host
preparation failures are retained explicitly there and in
`experiment-4-preparation-launch-failures.json`. No executable ran during those
failures; the unchanged manifest helper and both dry-run validators then passed.

The source-bound final repair validation passed fmt, guards, 29 focused tests,
workspace Clippy/build and 3544 workspace tests (zero failures, 15 ignored).
Its six unchanged-source receipts are in `validation-repair3-final` and bound
by [reviewed checkpoint 952](https://github.com/milocaetano/quantick/issues/330#issuecomment-5609766090).
[Committed transcript excerpts](worker-progress-transcripts.md) retain actual
normal/degraded/recovered application rows and both workers' degraded/recovered/
terminal observations with the exact log hash and independent fixture oracles.

Only evidence and the mission archive are added after this measured source.
Q8's explicit G2 still requires a new full ordered validation before that archive
commit; its actual receipts and final committed identity belong on issue340/PR.
Performance evidence is attributed to the frozen source above, never relabeled
as a new measurement of the archive commit. No runtime, benchmark, validator,
dependency, configuration, clock or domain-fixture input is intentionally changed.
Final reviews must inspect the complete delta and confirm that attribution.
Campaign SE9 credit and the overall9.0 goal remain pending integrated assessment.

## Limits

Both command channels remain unbounded. Existing retained market history and
forming-bar storage policies remain. A bounded diagnostic ledger is not a queue
cap, a session retention bound, supported-live scalability proof or SE7
completion. The change neither repairs a failed worker automatically nor proves
that an idle UI consumed every published event or mailbox value. No engine,
financial, data-retention or public contract rule is changed.

## Standalone reproduction

From the source worktree associated with the retained manifest, use:

```text
cargo test -p quantick-app worker_progress::tests -- --nocapture
cargo test -p quantick-app indicator_worker::progress_tests -- --nocapture
cargo test -p quantick-app orderflow_worker::progress_tests -- --nocapture
cargo test -p quantick-app app::tests::worker_progress_tests -- --nocapture
cargo test -p quantick-app incremental_lane_tests
cargo test -p quantick-app indicator_worker
cargo test -p quantick-app orderflow
```

The owner fixtures print `Q8_INDICATOR_DEGRADED`, `Q8_INDICATOR_RECOVERED`,
`Q8_INDICATOR_TERMINAL`, `Q8_BOOK_DEGRADED`, `Q8_BOOK_RECOVERED`, and
`Q8_BOOK_TERMINAL` typed JSON excerpts. The app fixture prints
`Q8_APP_DIAGNOSTIC_TRANSCRIPT` followed by the actual tracing JSON lines.
These are deterministic fixture observations with explicit clock provenance.
The normal constructors used in explicit replacement fixtures exercise the
production monotonic clock. Baseline clock/counter telemetry remains unavailable.

Ignored measurements require the separately frozen exclusive-host protocol.
Do not run them concurrently with compilation or other campaign workloads:

```text
<exact test binary> --exact worker_progress_bench::dense_workers --ignored --nocapture --test-threads=1
<exact test binary> --exact app::tests::worker_summary_bench_tests::worker_summary_cadence_cost --ignored --nocapture --test-threads=1
```

The cadence experiment uses synthetic two-second timestamps in back-to-back
calls, a counting JSON sink and one bounded untimed correctness probe. Its
measurements exclude asynchronous ResetSummaryCounters completion, disk IO
and GUI rendering. The probe is checked independently for exactly one legacy
summary and five attributed candidate worker rows (baseline: one legacy row).

## Focused validation record

The first focused runtime suite passed 18 tests with zero failures and one
ignored benchmark (`14-progress-tests.log`, source hashes in
`14-source-manifest.json`). It includes both held-worker fixtures, terminal
replacement and failed event delivery, the real tracing entrypoint and disabled
logging cadence. It does not include any performance measurement or full
workspace validation. Later unwind-subset, delivered-error and last-sample
identity refinements require a subsequent source-bound run; those results are
pending here. Earlier compile/guard failures remain retained in the external
development record, with no guard baseline or existing assertion weakened.

The subsequent source-bound focused run (`18-progress.log/.json`) passed 21
checks with zero failures and one ignored benchmark, including the unwind and
last-sample refinements. Indicator regressions passed 33 tests and orderflow
regressions passed 92 (`19-indicators` and `20-orderflow`). The first complete
validation loop stopped at a test-fixture type-complexity Clippy finding; its
repair names the phase-hold fields and preserves the synchronization oracle.
At that historical source boundary, a fresh complete loop was required and no
measurements had yet run. Experiments 1 and 2 above record the later failures.


Repair 2 focused validation is retained under the coordinator's
`quantick-campaign-score9/repair2/`, with full source manifests in each JSON
receipt. `01-guards` passed 206 tests; `02-check` passed the app all-targets
check; `03-progress` passed 27 tests with zero failures and one ignored
benchmark. After formatting, `05-indicators` passed 33 tests and `06-orderflow`
passed 93 tests, both with zero failures. The original S1-S4 protocol and
real-worker domain expectations remain unchanged. These development checks are not the final ordered workspace
loop, independent reviews, release build or paired performance experiment.


Independent preflight findings Q8-PREFLIGHT2-001 and Q8-PREFLIGHT2-002 led to
the test-only setup ordering and explicit contended-admission fixture above.
Their development checks are retained in
`quantick-campaign-score9/repair2-preflight-fixes/`: app all-target Clippy
(`03-clippy`) passed and the focused progress suite (`04-progress`) passed
28 tests with zero failures and one ignored benchmark. The original Known-age,
S1-S4, zero-work snapshot and domain literal oracles remain. Production worker
and telemetry source bytes and all benchmark inputs are unchanged by these
fixture corrections. The independent follow-up closed both findings, and
`validation-repair2-final` then passed the full ordered workspace loop with
3543 tests, zero failures and 15 ignored tests. That frozen source became
`27fc95d` and was measured in experiment 3 above.

Repair 3 focused evidence is in `quantick-campaign-score9/repair3/`, with full
source manifests and checksums in each receipt and `09-handoff.json`.
Guards passed 206 tests; app check and Clippy passed all targets; progress
passed 29 tests with zero failures and one ignored benchmark; indicator and
orderflow suites passed 33 and 93 tests. Formatting and diff checks passed.
The added `publishing_unwind_records_nonzero_subsets_once_and_keeps_delivered_output`
fixture holds a real phase callback after three accepted/admitted commands,
two input supersessions and delivery of output 17. Literal full-count snapshots
prove the Publishing callback unwind leaves two supersessions, one successful
output, three unfinished commands and zero retirements/cycles/mailbox updates.
Existing Applying-unwind, S1-S4, ownership and real-domain fixtures are unchanged.
These focused checks do not replace final full validation, independent reviews
or paired performance evidence for the repaired source.
