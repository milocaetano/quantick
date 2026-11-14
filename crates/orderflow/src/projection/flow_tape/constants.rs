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

// Peak spheres (`peaks.rs`): a zoom region divided at the valleys of its
// volume-by-price profile, one sphere per peak.

/// The standard deviation, in native ticks, of the Gaussian that smooths a
/// region's volume-by-price profile before its peaks are read.
pub const PEAK_SMOOTHING_SIGMA_TICKS: f64 = 1.0;

/// The Gaussian of [`PEAK_SMOOTHING_SIGMA_TICKS`] as exact integer weights,
/// from the centre row out: `round(1e6 * exp(-d^2 / 2 sigma^2))` for each
/// distance `d` in ticks, cut at four sigma. Integer weights keep the smoothed
/// profile exact, so no float summation order can move a peak.
pub const PEAK_SMOOTHING_KERNEL: [u32; 5] = [1_000_000, 606_531, 135_335, 11_109, 335];

/// A peak counts when it rises above its higher saddle by at least this
/// percentage of the region's highest smoothed row (its point of control).
pub const PEAK_PROMINENCE_PERCENT: u32 = 8;

/// The most rows a region's profile holds. A region whose prints span more
/// native ticks than this reads them in whole multiples of the tick, so a
/// stray print far away never makes the profile unbounded.
pub const PEAK_PROFILE_MAX_ROWS: usize = 4_096;

/// The smoothing sigma, in pixels, as a share of the look's full-size
/// radius. The profile's rows are the smallest power-of-two multiple of the
/// tape's tick at least this tall at the held price scale, and two equal
/// peaks stay two only past two sigma apart. At the mini index's default
/// scale (about a point a pixel, an 18 px radius) a row stays the native
/// 5-point tick, as the approved prototype read it; zooming price out
/// coarsens the rows, so peaks the screen would pile become one mass.
pub const PEAK_SIGMA_RADIUS_SHARE: f64 = 0.2;

/// The most times the tick doubles into a profile row: a row of 2^40 ticks
/// is far coarser than any chart draws, and the bound keeps a degenerate
/// price scale from looping.
pub const PEAK_ROW_MAX_DOUBLINGS: u32 = 40;

/// Rank fractions (the share of the masses in view at or below a mass) at
/// which [`PEAK_RADIUS_SHARES`] are reached; linear between them.
pub const PEAK_RANK_STOPS: [f32; 5] = [0.0, 0.5, 0.8, 0.95, 1.0];

/// A mass's radius at each of [`PEAK_RANK_STOPS`], as a share of the range
/// from [`PEAK_MIN_RADIUS_PX`] to the look's full-size radius: the median
/// small, the top twentieth large.
pub const PEAK_RADIUS_SHARES: [f32; 5] = [0.08, 0.19, 0.38, 0.66, 1.0];

/// The largest masses' count divided by this carry their quantity: the top
/// fifth, at least one when any mass is in view.
pub const PEAK_LABEL_SHARE_DIVISOR: usize = 5;

/// The smallest radius, in logical pixels, a ranked mass is drawn at: the
/// bubble style's minimum, so the least mass is a dot, never hidden. Tick
/// styles carry no minimum of their own.
pub const PEAK_MIN_RADIUS_PX: f32 = crate::config::DEFAULT_BUBBLE_MIN_RADIUS;

/// The fill strength of a perfectly balanced mass; a one-sided mass is drawn
/// at full strength, and strength is linear in `|buy - sell| / (buy + sell)`.
pub const PEAK_BALANCED_STRENGTH: f32 = 0.45;
