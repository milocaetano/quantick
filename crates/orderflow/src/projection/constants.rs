//! Tunable sizes, ladders, bands and budgets of the projection: volume dots,
//! candle marks, the tape's merge, geometry and work pricing. One place for
//! later config wiring; the FLOW tape keeps its own in `flow_tape/constants.rs`.

// Volume dots (`dots.rs`).

/// The windows of market time a tape dot may cover, in exchange
/// milliseconds, narrowest first.
pub const DOT_WINDOW_LADDER_MS: [i64; 10] = [
    crate::constants::NATIVE_TAPE_WINDOW_MS,
    250,
    500,
    1_000,
    2_000,
    5_000,
    10_000,
    30_000,
    60_000,
    300_000,
];

/// The heights, in native ticks, a dot's price level may span, narrowest
/// first.
pub const DOT_LEVEL_LADDER_TICKS: [i64; 12] =
    [1, 2, 5, 10, 20, 50, 100, 200, 500, 1_000, 2_000, 5_000];

/// The least screen width, in pixels, of a tape dot's window of market time:
/// a thin column, not the biggest dot, so a dot sits near the moment its
/// prints traded and only squeezing the time axis widens it.
pub const DOT_WINDOW_CELL_PX: f64 = 8.0;

/// A held rung moves down once the dot would be under this share of the next
/// smaller cell.
pub(crate) const HOLD_BELOW: f64 = 0.7;

/// A held rung moves up once the dot would be over this share of its cell.
pub(crate) const HOLD_ABOVE: f64 = 1.5;

/// The smallest radius a shrunk dot keeps, in pixels.
pub const MIN_DOT_RADIUS_PX: f32 = 1.0;

/// How many lane windows, counted back from the series' newest bar, the
/// lane's bars reach ([`super::lane_bars`]). Two because the view and the
/// engine each resolve the tape's window.
pub(crate) const LANE_BAR_WINDOWS: i64 = 2;

// Candle marks (`candle_dots.rs`).

/// The largest radius a mark reaches, in pixels; half its group's width may
/// reduce it. 6 rather than 4, so the heaviest group of a zoomed-out chart
/// reads as clearly larger than a quiet one while staying a quiet overlay.
pub const CANDLE_MARK_MAX_RADIUS_PX: f32 = 6.0;

/// The narrowest a group of candles is held at, in pixels. A mark's cap is
/// half its group's width, so it never falls under 3 px and a quiet group
/// and a heavy one still differ in size.
pub const CANDLE_GROUP_MIN_WIDTH_PX: f32 = 6.0;

/// The hysteresis band: a held group halves only once the half would be this
/// many times [`CANDLE_GROUP_MIN_WIDTH_PX`] wide (7.5 px). Between the two
/// widths a steady zoom keeps its rung, and at the chart's default 8 px per
/// bar every rung returns to one candle per mark.
pub const CANDLE_GROUP_HOLD_BAND: f32 = 1.25;

/// Candles per mark on offer: powers of two, the widest when none fits.
pub(crate) const CANDLE_GROUP_LADDER: [usize; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

// Tape merge and geometry (`tape.rs`, `tape_geometry.rs`).

/// Allowed intersection depth as a share of the smaller disc's radius.
pub(crate) const SMALLER_DOT_OVERLAP_SHARE: f32 = 0.1;

/// Decoration may consume at most this fraction of the pane at each edge.
/// The actual disc radius takes precedence when its diameter nearly fills it.
pub(crate) const MAX_DECORATED_INSET_SHARE: f32 = 0.45;
/// Leave room between the extreme prices even in a very short pane: the
/// largest radius as a share of the pane's height.
pub(crate) const MAX_VERTICAL_RADIUS_SHARE: f32 = 0.4;

// Past tape memory (`tape_past_memory.rs`).

/// Most frozen blocks held at once; the farthest from the window go first.
pub const MAX_PAST_BLOCKS: usize = 64;

/// How far the drawn price span may drift from the span the blocks were
/// merged at, as a factor either way, before they are merged again. Inside
/// it an axis change never regroups the past; outside it the old merges
/// would overlap or scatter on the new axis.
pub const PAST_PRICE_SPAN_BAND: f64 = 2.0;

// Tape work pricing (`tape_work.rs`).

/// The work, in [`super::TapeWork::units`], a frame reconciles itself: past
/// it the reconciliation runs beside the frame. A unit is about a microsecond
/// of a release build on the recorded WIN tape, so this keeps the tape's
/// share of a frame near 4 ms whatever the zoom or the worker's lag.
pub const FRAME_WORK_BUDGET: usize = 4_000;

/// Units a sealed frame spends on each cell it reads: it turns the cell into
/// a mark and looks it up among the groups (measured at about 3 us).
pub(crate) const SEALED_CELL_UNITS: usize = 3;
/// Units the complete path spends on each mark in the window: only a look-up
/// (measured at about 0.3 us, priced high).
pub(crate) const COMPLETE_CELL_UNITS: usize = 1;
/// Units one closed window costs to merge, beside one per frontier group.
pub(crate) const WINDOW_UNITS: usize = 16;
/// The frontier, in groups, a reread is priced at: it rebuilds its own as it
/// goes.
pub(crate) const REREAD_FRONTIER: usize = 32;

// Tier geometry (`tiers.rs`).

/// Normalized error a native screen position may carry (1e-12: under one
/// millionth of a pixel at 16K) before the exact Decimal division is used at
/// a clip boundary.
pub(crate) const SCREEN_FRACTION_BOUNDARY_TOLERANCE: f64 = 1e-12;
/// Lowest normalized screen position the approximate arithmetic is trusted
/// for; further off-axis values take the exact Decimal division.
pub(crate) const SCREEN_FRACTION_APPROXIMATE_MIN: f64 = -2.0;
/// Highest normalized screen position the approximate arithmetic is trusted
/// for; further off-axis values take the exact Decimal division.
pub(crate) const SCREEN_FRACTION_APPROXIMATE_MAX: f64 = 3.0;

// Aggression folds (`fold.rs`).

/// Ids a single fold keeps, before it stops recording which prints it stands
/// for and lets [`super::AggressionPrimitive::trade_count`] speak for them.
///
/// A fold is unbounded in principle — a quiet budget on a busy session can put
/// a whole minute of prints under one mark — and the id lists are cloned into
/// every published frame. The exact count is never lost, only the roll of
/// individual ids past this point, and the truncation is declared by
/// `trade_count` exceeding `agg_ids.len()`.
pub(crate) const MAX_FOLD_IDS: usize = 256;
