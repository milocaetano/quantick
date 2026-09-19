# F2 paired performance protocol

No timings have been run. This protocol fixes inputs and analysis before the
quiet-host slot so thresholds cannot be tuned after samples are observed.
The common literal input specification is `fixture.json`, SHA-256
`b7e798913b7ddd7269949c60c4035da39f3d212b0656ebf0bcd76878dd268d65`;
it must match in the control and candidate harnesses.

## Identities and execution

- Control product source: exact campaign commit
  `9ff57501249f51d8f75c21f52094ec3dc3c39af2`, built in an isolated control
  worktree/target.
- Candidate product source: the eventual frozen F2 commit, built in its own
  target. Its commit, tree, changed-input hashes, binary hashes, rustc/Cargo
  versions, host CPU, power state, and relevant environment are recorded before
  sampling.
- No Cargo/rustc/linker, benchmark, virus scan, or other known heavy writer may
  overlap the samples. A process inventory is captured immediately before and
  after. The two sides use identical build profile, job count, fixture data,
  event count, channel capacity, and consumer behavior.
- Run three warmups per side, retained and labelled but excluded from the
  verdict statistics. Then run 15 measured samples per side. Within every
  warmup and measured pair, alternate which side runs first by pair index.
  Retain every sample in execution order; do not discard an outlier.

## Fixed Binance fixture

Use the literal aggregate-trade frame already retained in
`continuity::tests::benchmark_contiguous_binance_delivery`, with one REST seed
trade followed by sequential live aggregate IDs and timestamps. Both sides
decode the same frame count, run the unchanged lower continuity tracker, pass
the same bounded channel, and drain every event. The control uses the exact
campaign host handoff; the candidate uses the seeded handoff. Record total
nanoseconds and nanoseconds per live trade. Assert outside the timed interval
that both sides publish the same trades and no continuity anomaly for the
contiguous fixture.

## Fixed Hyperliquid fixtures

Use two literal JSON batch streams derived from the retained local WebSocket
fixture, with stable coin, IDs, times, decimal strings, and order:

1. dense-valid batches, all new and chronological;
2. exclusion-bearing batches containing valid new rows, one individually
   malformed row, one previously unseen stale row, and one already-seen
   overlap duplicate.

Each side parses and maps the same batch stream with the same
`DEFAULT_SEEN_TRADE_CAPACITY`, forwards over the same bounded capacity, and
drains synchronously. The control uses the exact campaign legacy trade/watch
path; the candidate uses the ordered event path and provider-neutral host
classification. Record total nanoseconds, nanoseconds per input batch, and
nanoseconds per input row. Outside the timed interval assert identical usable
trades; additionally assert the candidate emits exactly one batch message,
malformed-before-stale continuity, no duplicate-as-loss, and no invented gap.

The control harness must be a test-only patch over the exact control commit,
and the candidate harness a test-only patch over the frozen candidate. The
literal input JSON and common measurement loop must have the same SHA-256 in
both harnesses; branch-specific adapters are outside the timed fixture setup
and their hashes are recorded. Neither harness changes product source.

## Fixed verdict

- Candidate median cost per event must be at most `1.05x` control.
- Candidate p95 must be at most `1.10x` control.
- Median ratio `0.95..=1.05` is labelled flat/noise; below `0.95` is better;
  above `1.05` is a regression. A p95 ratio above `1.10` also fails.
- Compute coefficient of variation separately for each side and fixture. If
  either exceeds `5%`, the pair is invalid host noise, not a pass; reschedule
  unchanged.
- Preserve raw samples and any invalid or failed attempt. Do not tune fixture,
  sample count, statistic, or threshold after observing results.
