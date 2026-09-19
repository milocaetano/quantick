# Anchored studies

Two computational owners over borrowed market evidence: `RangeProfile` retains
its closed-bar fold and coverage; `AnchoredAverage` retains its indicator kernel
and rows. Their associated `refresh` methods handle the optional lifetime held
by a caller, including replacement and an empty average. The desktop uses them
for existing finalized drawings and profile drafts; `tests/consumer.rs` exercises
them without app. No new drawing registry or mathematical kernel is introduced.

Inputs borrow bars, ladders and the existing snapshot and revision identities.
Results are read through an immutable borrow for painting. Drawing configuration,
anchors, persistence, undo, heat-boundary presentation, clocks, environment and
repaint requests remain in app. Derived computation stays out of payload equality
and presets.

The profile preserves incremental right-edge extension and partial-on-copy
merging. Its caller supplies the work budget: a whole ladder can exceed the
remaining budget, exactly as in the desktop. The average preserves its actual
timeline revision key; desktop live input advances that revision and may replay
the anchored span. Equal-key preview and compatible append paths reuse the
retained kernel. This extraction makes no new timing or constant-time claim.
