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

/// The candles a zoom region may span, narrowest first: powers of two, so a
/// wider rung's regions are whole unions of a narrower rung's.
pub(crate) const FLOW_REGION_CANDLE_LADDER: [usize; 12] =
    [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1_024, 2_048];

/// The least screen width, in logical pixels, of a zoom region: the widest
/// hull a merged region may span, so the region's bounds, anchored in candles,
/// cut no more merges than the hull already does.
pub const FLOW_REGION_MIN_WIDTH_PX: f64 = FLOW_MERGE_WIDTH_PX;

/// A held zoom region is kept until its half would be this many times
/// [`FLOW_REGION_MIN_WIDTH_PX`] wide, so a steady zoom keeps its rung.
pub const FLOW_REGION_HOLD_BAND: f64 = 1.25;

/// Tick spheres merge at a held price scale (pixels per price unit): the
/// held scale is kept while the live one stays within this factor of it, so
/// a pan or an axis fit never regroups a sphere; leaving the band is a zoom
/// step, which does.
pub const FLOW_SCALE_HOLD: f64 = 1.5;

/// Start ordinary-region placement and gradual emphasis at a quarter of the
/// reference: the reference quantity is divided by this.
pub const LARGE_REGION_REFERENCE_DIVISOR: u32 = 4;

/// Candle slots kept beyond each edge of the view, so small pans reuse whole
/// candles while the retained source stays bounded.
pub(crate) const KEEP_MARGIN_SLOTS: usize = 32;

/// Executions in the first source packet, captured newest first, and the
/// least growth of older history a cold fill waits for before its next
/// partial publication.
pub(crate) const SOURCE_CHUNK: usize = 2048;

/// Cold admission grows after its first useful partial, capped at eight base
/// packets per frame. Source capture stays bounded even for a whole-day view.
pub(crate) const MAX_SOURCE_CHUNK: usize = 8 * SOURCE_CHUNK;

/// Source packets captured a frame: the newest gap — on a busy tape the
/// prints since the last frame — and the next one back, so a print before
/// every frame never starves the history behind it. With packets growing to
/// [`MAX_SOURCE_CHUNK`], history of `n` executions is captured within about
/// `n / MAX_SOURCE_CHUNK + 4` frames however busy the tape stays.
pub(crate) const CAPTURES_PER_FRAME: usize = 2;

/// A fresh partial publication starts once the loaded source falls under
/// the last publication's count divided by this: substantial eviction, not
/// a small live pan.
pub(crate) const EVICTION_RESET_DIVISOR: usize = 2;

/// Older history a cold fill adds is published at geometric milestones: the
/// loaded source must reach the last published count times this. Bounds
/// total cold-fill projection work; a newer print never waits for one.
pub(crate) const PUBLICATION_GROWTH_FACTOR: usize = 2;
