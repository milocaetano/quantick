# Performance classification

- Per trade: counter observation and aggregation retain the pull request's constant-time path through an optional `DealCounterInput`; the lookup and observation are constant-time and add no allocation.
- Per feed drain: recorder sampling retains the bounded write path.
- Per frame: the toolbar edits `SpecSelector` instead of synchronizing a second kind/count pair.
- Rare: schema generation, restoration, control invocation, recorder setup, `.deals` scanning/loading, and recutting run on explicit operations. Rebuild seeding is linear in retained counter samples, as before, and is centralized in one helper.

Ownership and compatibility changed, not hot-path semantics, so no new benchmark comparison was required. Deterministic fixtures cover the semantic paths.

Touched-path inventory: `engine/deals.rs` and builder dispatch are per trade; `feed-mt5/deals.rs`, mapping, protocol, and stream are per received batch/drain; `state.rs`, `tab.rs`, and pane ingestion are per UI drain; toolbar/status/chrome are per frame; recording file discovery, workspace/config serialization, schema generation, recovery, menu dialogs, and control calls are rare or explicit. Documentation, tests, hooks, schemas, and guard baselines have no production execution rate.
