# Shared series lifecycle

`SeriesFold` composes an engine-registered `BarBuilder` with an optional forming
footprint. `push` returns a close by value: the fold retains neither a trade tape
nor a closed-bar/ladder history. Capture is disabled by default; that path calls
the builder directly without footprint diagnostics or allocation. Builders may
retain their own required evidence, such as deal-counter readings.

`RetainedSeries` composes the same fold with the existing unbounded trade/deal
evidence, closed outputs, configuration, grid, provenance and revisions. It is
the desktop's domain owner, not an application context. Reads borrow. Backtest
uses the streaming owner only; rendering, scheduling, indicator notifications,
simulator and strategy effects remain with consumers.

## Lifecycle contracts

- Readings at equal milliseconds retain arrival order. Older readings are held
  until rebuild, not used to repeatedly re-cut the live edge. Batch ingestion
  retains/sorts/deduplicates evidence without feeding the current builder.
- Rebuild seeds retained readings before replaying trades. Prepend deliberately
  realigns the complete series. Neither copies the retained tape into a flat vec.
- A footprint close follows the builder's trade count: a boundary print may
  open the next bar instead. An uncounted rollover closes without that print.
- Regroup replaces forming capture, not the live builder's reading state.
  Held readings require a full rebuild; this preserves the original two
  revision increments on that path.
- Live append, including a close, advances timeline revision only. Backfill,
  rebuild, prepend and footprint configuration changes also advance series
  revision. Reset restores fresh revisions/capture defaults while retaining
  venue deal readings; it is a new lifecycle, not a monotonic continuation.
- The reference price is first *seen*, not earliest retained time. Prepend
  refines the grid without replacing that reference. Seed preserves the
  historical/live split and deferred reading semantics.

Retention policy is unchanged. Limiting or evicting history is separate work.
Existing tape chunks, output vectors and enabled footprint maps still allocate;
the streaming boundary introduces no per-print event container or tape copy.

## Extension and ownership checks

`BarBuilder` and engine `BarConfiguration`/`BarRegistry` are the only bar port
and registration point. No series factory or family dispatch exists. The public
`tests/retained_consumer.rs` fixture registers the existing seventh test family
and exercises both owners, seed, prepend, rebuild, regroup and reset without app.
Original domain goldens and lifecycle tests live under `src/tests`.

The extension-boundary guard scans both app and series sources. It protects
RetainedSeries, SeriesFold and FormingFootprint shapes and separate exact caps;
moving an implementation across files does not remove its charge. Its source
grammar and limits remain documented in the original boundary dossier.
