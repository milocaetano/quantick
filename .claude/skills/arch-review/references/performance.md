# Dimension 2 — performance impact

Read before writing a performance finding, or before classifying a touched path.

No change ships without an answer to: **how often does this run?**

Classify each touched path before judging it:

| Path | Rate | Bar |
| --- | --- | --- |
| Aggregator, tick ingest | per trade | zero allocation, no locks |
| Book state, depth projection | per depth update | bounded work, no full rescans |
| Renderer, per-frame view | ~60 Hz | no per-frame recompute of stable data |
| Config load, startup, panel edits | rare | clarity wins freely |

Hot-path findings to hunt:

- Allocation per event or per frame: `to_string`, `format!`, `collect()`,
  `Vec` built and dropped every tick, `clone()` of a container.
- `HashMap` where iteration order can reach the output — a determinism bug and
  a cache-locality loss at once. Prefer `BTreeMap` / `Vec`.
- Work that repeats per frame but only changes per event: recomputed
  projections, re-sorted levels, re-parsed config. Cache and invalidate.
- Locks or channel waits on the render thread; unbounded queues that grow
  under a fast tape.
- Per-element draw calls where the repo already batches into one mesh.
- Growth that is worse than the data: an O(n²) pass over book levels is fine
  on ten levels and fatal on a dense book.

Rules for reporting performance:

- Never assert "this is slow" without the call rate and the concrete cost
  (allocations per call, extra passes, added lock). A guess stated as a
  measurement is itself a finding against the reviewer.
- If the cost is real but the magnitude is unclear, say so and name the
  measurement that would settle it — the perf HUD frame time, a bench over a
  fixture, a dense-book capture.
- Cold-path micro-optimisation that costs clarity is a finding in reverse:
  call it out and ask for the readable version back.

