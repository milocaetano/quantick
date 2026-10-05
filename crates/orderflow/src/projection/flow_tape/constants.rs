//! Tunable limits of the FLOW tape: worker bounds, merge reach, region
//! support and streaming cadence. One place for later config wiring.

/// Retained worker cache bound, in executions, independent of per-frame
/// source admission. The readback reports missing coverage if a larger
/// source exhausts it.
pub const MAX_FLOW_EXECUTIONS: usize = 2_000_000;

// Merges may pool a local time/price region, but may not walk along an entire swing.
// An indivisible native cell wider than this remains alone; it is never split or lost.

/// Widest hull, in logical pixels, a merged FLOW region may span in time.
pub(crate) const FLOW_MERGE_WIDTH_PX: f64 = 48.0;
/// Tallest hull, in logical pixels, a merged FLOW region may span in price.
pub(crate) const FLOW_MERGE_HEIGHT_PX: f64 = 32.0;

/// Fixed time support, in exchange milliseconds, prevents compressed tick
/// coordinates from pooling minutes of routine flow into peers of a brief
/// large execution. These are regions, not reconstructed orders. A multiple
/// of the native 100 ms cell keeps facts whole.
pub const FLOW_REGION_WINDOW_MS: i64 = 1_000;

/// Start ordinary-region placement and gradual emphasis at a quarter of the
/// reference: the reference quantity is divided by this.
pub const LARGE_REGION_REFERENCE_DIVISOR: u32 = 4;

/// Candle slots kept beyond each edge of the view, so small pans reuse whole
/// candles while the retained source stays bounded.
pub(crate) const KEEP_MARGIN_SLOTS: usize = 32;

/// Most source executions sent to the runner in one packet, and the least
/// growth a cold fill waits for before its first partial publication.
pub(crate) const SOURCE_CHUNK: usize = 2048;

/// A fresh partial publication starts once the loaded source falls under
/// the last publication's count divided by this: substantial eviction, not
/// a small live pan.
pub(crate) const EVICTION_RESET_DIVISOR: usize = 2;

/// Cold-fill publications happen at geometric milestones: the loaded source
/// must reach the last published count times this. Bounds total cold-fill
/// projection work.
pub(crate) const PUBLICATION_GROWTH_FACTOR: usize = 2;
