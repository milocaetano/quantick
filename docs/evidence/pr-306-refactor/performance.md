# Performance classification

- Per trade: counter observation and aggregation retain the pull request's constant-time path; this integration adds no loop or allocation.
- Per feed drain: recorder sampling retains the bounded write path.
- Per frame: the toolbar edits `SpecSelector` instead of synchronizing a second kind/count pair.
- Rare: schema generation, restoration, control invocation, recorder setup, and recutting run on explicit operations.

Ownership and compatibility changed, not hot-path semantics, so no new benchmark comparison was required. Deterministic fixtures cover the semantic paths.
