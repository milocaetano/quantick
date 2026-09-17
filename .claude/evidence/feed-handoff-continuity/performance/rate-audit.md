# F2 rate audit

This is a structural audit of the current uncommitted F2 source, not a timing
result or a delivery review. The paired quiet-host measurements remain
pending. The control tree is campaign commit
`9ff57501249f51d8f75c21f52094ec3dc3c39af2`; the candidate commit/tree will be
filled only after the source and fixtures are frozen.

| Path | Rate | Added work and bound |
| --- | --- | --- |
| `feed::BinanceContinuity::after_backfill` | once per feed startup | Copies two scalar fields from the final REST trade; no allocation, lock, clock, scan, or channel send. |
| `feed::BinanceContinuity::observe` | once per live Binance trade | One one-shot boolean branch plus the existing scalar ID/time comparisons. The contiguous path allocates nothing and sends no diagnostic. All counter arithmetic remains checked by comparisons or subtraction after ordering. |
| `feed_hyperliquid::run_trade_session_core` | once per source frame/batch | Parsing and `TradeMapper::map_batch` remain single-owned. One bounded-channel send moves each `MappedBatch` by value; there is no batch clone, second mapper, lock, or per-trade forwarding allocation. Duplicate acknowledgements are one boolean check and do not create another edge. |
| `feed_hyperliquid::run_trade_events_with_reconnect` | once per connection edge | At most one zero-payload `Connected` and one `Disconnected` enum send per acknowledged session. Failed attempts before acknowledgement emit neither edge. Backoff state is scalar and bounded as before. |
| `feed::hyperliquid` host | once per ordered source event | A batch performs two scalar length conversions and at most two bounded continuity sends (malformed first, stale second) plus the existing single trade-batch send. Counts are per category, not per rejected row; `MappedBatch` is moved, not cloned. Empty/overlap-only batches allocate no replacement tape data. |
| app normal drain | once per provider-neutral event | Reuses `FeedIntegrity::observe`; counters saturate. Exclusion continuity has no `FeedGap`, so it cannot grow the bounded gap list. No new lock, thread, clock read, or trade copy is added. |
| observer projection | once per requested snapshot | Reuses one `FeedIntegritySnapshot` constructor. The optional `feed.status` field is serialized only during a control read, not during feed delivery. |

Bounded state remains `DEFAULT_SEEN_TRADE_CAPACITY`, the existing source and
host `tokio::mpsc` capacities, and `MAX_REMEMBERED_GAPS`. No Cargo dependency,
reverse workspace edge, replay state, engine state, or financial path is
introduced.

The structural claim must be rechecked against the frozen diff and guards. It
does not substitute for the paired samples in `protocol.md`.
