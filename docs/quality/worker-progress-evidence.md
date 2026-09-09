# Worker progress diagnostics

This dossier describes the internal observations added for campaign Q8 / issue
#340. Validation and paired performance results are pending; this file does not
claim a score increment, benchmark PASS, or final source identity yet.

## Production path and ownership

`QuantickApp::maybe_emit_summary` keeps its existing two-second cadence and
`APP_HEALTH_SUMMARY` fields. It additionally walks every owned tab and pane,
including off-screen panes, and emits `APP_WORKER_PROGRESS` schema version 1
for each indicator worker and optional book worker. The record carries tab ID,
pane ID, side index (flow 0, context 1 onward), worker kind and process-local
instance ID. `observation` is a JSON-encoded typed internal snapshot. This is
an internal tracing event; no public DTO, schema, permission or capability was
added. There is no new control-plane reachability claim.

The observation reader locks only its fixed-size ledger. It does not inspect
the command queue or clone indicator columns, ladders or projected frames.
Serialization occurs after the ledger lock is released and only at the health
cadence when INFO logging is enabled. Every summary adds one record per owned
worker, so event volume is proportional to owner count, not session history.

## Accounting and sampling

Each worker keeps one ledger and one optional accepted-command sample. Normal
clocks report monotonic nanoseconds since that instance's creation. Fixture
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
- Indicator `output_attempts = output_successes + output_failures` records
  event-channel sends separately from cycle completion. Success means channel
  delivery, not UI adoption. Book `mailbox_replacements` records replacement
  of its existing latest-value mailbox; intermediate values may never be read.

Admission serializes the existing unbounded send and its accounting under a
short ledger lock, preventing a batch from retiring work before acceptance is
recorded. Domain work, channel receive, mailbox access, logging and phase
callbacks hold no ledger lock. Clock reads must be bounded and nonblocking;
fixture synchronization belongs in phase callbacks.

The first accepted command after the previous sample was admitted becomes the
new sample. Later accepted commands have no auxiliary timestamps. This is a
biased single-ticket sample, not a distribution or percentile estimator.
`sample_age` is its time since successful admission; `last_sample_residence`
is the last measured duration to batch admission, attributed by
`last_sampled_ticket`. `oldest_wait` equals sample
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

External evidence is retained under the coordinator's `Q8-validation/`
directory. `04-development-check-failure-excerpts.md` preserves the original
development compile failure with tool-output provenance, and
`05-independent-worker-schedules.md` preserves pre-execution expected schedules.
The frozen harness, source/binary manifests, raw test transcripts, measurements
and ordered verification receipts must be associated with the delivered source
commit before this dossier can claim completion.

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
A fresh complete loop is required after that repair. No measurements have run.
