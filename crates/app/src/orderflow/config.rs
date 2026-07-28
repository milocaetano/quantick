//! Sanitized runtime configuration for the order-book heatmap.

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use serde::{Deserialize, Serialize};

/// Shortest history window accepted by the UI.
pub const MIN_RETENTION_MS: i64 = 1_000;
/// Longest in-memory history window accepted by the UI.
pub const MAX_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
/// Default history window: thirty minutes, so the visible past keeps its
/// story — where a wall lived, when it was eaten and when it was pulled.
/// Affordable because projection only sweeps runs intersecting the visible
/// window (on its own thread) and the run/byte caps below still bound memory;
/// the adaptive capture bucket keeps dense books at ~10^4 runs per hour.
pub const DEFAULT_RETENTION_MS: i64 = 30 * 60 * 1_000;
/// Default number of price rows requested by adaptive visual grouping.
/// Thin rows are the Bookmap look: aggregating too much sums liquidity until
/// every band saturates into one yellow wall. Legibility comes from the
/// default gamma contrast (quiet rows sink into the dark canvas), and the
/// off-thread projection absorbs the extra cell cost.
pub const DEFAULT_ADAPTIVE_ROWS: u32 = 128;
/// Smallest useful adaptive row target.
pub const MIN_ADAPTIVE_ROWS: u32 = 16;
/// Largest adaptive row target accepted from configuration.
pub const MAX_ADAPTIVE_ROWS: u32 = 2_000;
/// Largest explicit multiple accepted for visual grouping.
pub const MAX_DISPLAY_GROUP_MULTIPLE: u32 = 1_000_000;
/// Default temporal window used to cluster aggressive prints.
pub const DEFAULT_BUBBLE_CLUSTER_MS: i64 = 200;
/// Largest temporal window used to cluster aggressive prints.
pub const MAX_BUBBLE_CLUSTER_MS: i64 = 2_000;
/// Default window over which prints too small to read are folded together.
/// Long enough to gather the dust of a quiet price range, short enough that
/// the merged bubble still points at a moment rather than at the whole bar.
pub const DEFAULT_BUBBLE_DUST_MERGE_MS: i64 = 1_500;
/// Largest window accepted for folding unreadable prints together.
pub const MAX_BUBBLE_DUST_MERGE_MS: i64 = 30_000;
/// Default distance accepted when correlating a depth reduction and aggression.
pub const DEFAULT_LIQUIDITY_CORRELATION_MS: i64 = 250;
/// Safe upper bound for depth/aggression correlation.
pub const MAX_LIQUIDITY_CORRELATION_MS: i64 = 10_000;
/// Default alpha applied to aggression bubbles.
pub const DEFAULT_BUBBLE_OPACITY: f32 = 0.78;
/// Default on-screen radius, in points, of the largest aggression bubble.
pub const DEFAULT_BUBBLE_MAX_RADIUS: f32 = 15.0;
/// Smallest accepted maximum bubble radius.
pub const MIN_BUBBLE_MAX_RADIUS: f32 = 4.0;
/// Largest accepted maximum bubble radius.
pub const MAX_BUBBLE_MAX_RADIUS: f32 = 48.0;
/// Largest accepted radius for the smallest drawn bubble.
pub const MAX_BUBBLE_MIN_RADIUS: f32 = 12.0;
/// Default radius, in pixels, below which a bubble is treated as unreadable
/// on its own: small enough to be a routine print, large enough that side
/// colour and the side nudge actually register at a glance.
pub const DEFAULT_READABLE_MIN_RADIUS: f32 = 6.0;
/// Largest accepted readability floor.
pub const MAX_READABLE_MIN_RADIUS: f32 = 32.0;
/// Default edge darkening of a sphere-rendered bubble.
pub const DEFAULT_SPHERE_SHADING: f32 = 0.55;
/// Default highlight strength of a sphere-rendered bubble.
pub const DEFAULT_SPHERE_HIGHLIGHT: f32 = 0.35;
/// Default width of the live lane, in candle widths. The chart already capped
/// the forming bar's tail at eighteen candle widths including the candle's own
/// slot, so the reserved lane opens exactly as wide as the growing one used to
/// reach.
pub const DEFAULT_LIVE_LANE_CANDLES: f32 = 17.0;
/// Narrowest live lane accepted. Below a few candle widths the band stops
/// being a lane and becomes a second candle.
pub const MIN_LIVE_LANE_CANDLES: f32 = 4.0;
/// Widest live lane accepted. Past this the history it is supposed to be read
/// against has no room left.
pub const MAX_LIVE_LANE_CANDLES: f32 = 80.0;
/// Share of the visible chart the lane may occupy when the window is too
/// narrow to grant its configured width. Not a setting: it is the guard that
/// keeps a small window from becoming all lane and no history.
pub const LIVE_LANE_CHART_SHARE: f32 = 0.55;
/// Default multiplier applied to the bubble radii inside the live lane.
pub const DEFAULT_LIVE_LANE_RADIUS_SCALE: f32 = 1.0;
/// Bounds accepted for the live lane's radius multiplier.
pub const MIN_LIVE_LANE_RADIUS_SCALE: f32 = 0.25;
/// See [`MIN_LIVE_LANE_RADIUS_SCALE`].
pub const MAX_LIVE_LANE_RADIUS_SCALE: f32 = 4.0;

/// Renderer-only price grouping layered over the exact capture buckets.
///
/// Changing this setting never asks [`LiquidityHistory`](super::history::LiquidityHistory)
/// to reinterpret or reset retained history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayGrouping {
    /// Render each captured base bucket independently.
    Native,
    /// Combine this many adjacent base buckets.
    Multiple(u32),
    /// Choose an integer base-bucket multiple near the requested row count.
    Adaptive {
        /// Approximate number of rows across the visible price window.
        target_rows: u32,
    },
}

impl Default for DisplayGrouping {
    fn default() -> Self {
        Self::Adaptive {
            target_rows: DEFAULT_ADAPTIVE_ROWS,
        }
    }
}

impl DisplayGrouping {
    fn sanitized(self) -> Self {
        match self {
            Self::Native => Self::Native,
            Self::Multiple(multiple) => {
                Self::Multiple(multiple.clamp(1, MAX_DISPLAY_GROUP_MULTIPLE))
            }
            Self::Adaptive { target_rows: 0 } => Self::default(),
            Self::Adaptive { target_rows } => Self::Adaptive {
                target_rows: target_rows.clamp(MIN_ADAPTIVE_ROWS, MAX_ADAPTIVE_ROWS),
            },
        }
    }
}

/// Visual palette selected by the renderer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum HeatmapTheme {
    /// Dark Bookmap-inspired palette.
    #[default]
    Bookmap,
    /// Higher luminance separation for difficult displays.
    HighContrast,
    /// Palette that avoids relying on red/green discrimination.
    ColorBlind,
}

/// How displayed liquidity is normalized before applying the colour ramp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntensityMode {
    /// Use the 99th percentile of positive quantities visible in this frame.
    ///
    /// A percentile prevents a single unusually large wall from making every
    /// other level effectively invisible.
    VisibleP99,
    /// Use an explicit quantity as full intensity.
    Fixed(Decimal),
}

impl IntensityMode {
    fn sanitized(self) -> Self {
        match self {
            Self::Fixed(maximum) if maximum > Decimal::ZERO => Self::Fixed(maximum),
            Self::Fixed(_) | Self::VisibleP99 => Self::VisibleP99,
        }
    }
}

/// How the quantity that maps to a full-size bubble is chosen.
///
/// `VisibleP99` keeps one outlier sweep from shrinking every other print to a
/// dot, at the cost of making the top 1% all render at the maximum radius.
/// `VisibleMax` restores a strict ordering (the biggest print is the only one
/// at full size); `Fixed` pins the scale so bubble size means the same thing
/// across sessions and symbols.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BubbleSizeReference {
    /// 99th percentile of the visible clustered prints.
    #[default]
    VisibleP99,
    /// The largest visible clustered print.
    VisibleMax,
    /// An explicit quantity, [`BubbleStyle::size_reference_quantity`].
    Fixed,
}

impl BubbleSizeReference {
    /// Whether the reference is derived from what is on screen.
    ///
    /// The opposite is [`Fixed`](Self::Fixed), pinned so a bubble means the
    /// same quantity all session — which is exactly why nothing the renderer
    /// decides is allowed to rescale it.
    #[must_use]
    pub fn is_visible(self) -> bool {
        !matches!(self, Self::Fixed)
    }
}

/// How a bubble's fill is painted.
///
/// `Flat` is the classic solid disc. `Sphere` shades each bubble like a ball
/// lit from the upper left — an offset highlight over a darkened rim — so
/// overlapping prints on a dense tape keep a visible boundary instead of
/// merging into one solid blob. Purely visual: geometry, clustering and
/// liquidity association are identical in both modes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BubbleRenderMode {
    /// Solid 2D disc.
    #[default]
    Flat,
    /// Shaded 3D-looking sphere.
    Sphere,
}

/// Decimal places kept when a bubble float is written to the presets file.
///
/// Four is past the precision any pixel radius or alpha carries, and well
/// inside what `f32` represents exactly enough to survive a round trip.
const SERIALIZED_FLOAT_PLACES: i32 = 4;

/// Serialize an `f32` as a short decimal.
///
/// TOML floats are `f64`, so an `f32` promoted straight through prints its
/// exact binary expansion: `0.78` is written back as `0.7799999713897705`.
/// Presets are tracked in git precisely so a look can be reviewed and rolled
/// back like code, and a diff of noise defeats that — this is the write path
/// for every visual float, so rounding here fixes the file for all of them.
fn serialize_short_f32<S>(value: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let scale = 10_f64.powi(SERIALIZED_FLOAT_PLACES);
    let rounded = (f64::from(*value) * scale).round() / scale;
    serializer.serialize_f64(rounded)
}

/// Everything the "aggression bubbles" panel owns: bubble geometry, colour,
/// labels and the two marks a bubble draws when it ate resting liquidity — the
/// vertical consumption front (the "risco") and the trail that leaks into the
/// consumed side (the "rastro").
///
/// Every field here is display-only. [`min_quantity`](Self::min_quantity) and
/// the size reference reach the projection, but only to decide which bubbles
/// are drawn and how large — never what is captured, retained, or associated
/// with an L2 reduction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BubbleStyle {
    /// Radius of the smallest drawn print, in pixels.
    #[serde(serialize_with = "serialize_short_f32")]
    pub min_radius: f32,
    /// Radius of a full-size print, in pixels. Area stays proportional to
    /// quantity between the two.
    #[serde(serialize_with = "serialize_short_f32")]
    pub max_radius: f32,
    /// Alpha of the bubble fill.
    #[serde(serialize_with = "serialize_short_f32")]
    pub opacity: f32,
    /// Rim stroke width; zero draws no rim.
    #[serde(serialize_with = "serialize_short_f32")]
    pub outline_width: f32,
    /// Soft halo drawn behind the fill, as a fraction of full alpha.
    #[serde(serialize_with = "serialize_short_f32")]
    pub halo_strength: f32,
    /// Bubbles below this radius are drawn as a plain dot (no halo, rim or
    /// impact ring). Raising it trades detail for frame time on a fast tape.
    #[serde(serialize_with = "serialize_short_f32")]
    pub detail_min_radius: f32,
    /// Radius, in pixels, below which a bubble cannot be read on its own.
    ///
    /// The readability floor both the dust merge and
    /// [`hollow_small_buys`](Self::hollow_small_buys) gate on. Deliberately
    /// *not* [`detail_min_radius`](Self::detail_min_radius): that one decides
    /// how much dressing a bubble can afford, and a sphere-heavy look sets it
    /// low on purpose — the "3d spheres" and "dense tape btc" presets put it
    /// at or below `min_radius`, which would leave nothing to fold.
    #[serde(serialize_with = "serialize_short_f32")]
    pub readable_min_radius: f32,
    /// Whether buy prints below [`readable_min_radius`](Self::readable_min_radius)
    /// are drawn as an open ring instead of a solid dot.
    ///
    /// Shape survives where colour does not: at that size a green speck and a
    /// red speck are the same speck, but a ring and a disc still read as two
    /// different things. Bubbles above the floor keep their fill, so a sphere
    /// look is untouched.
    pub hollow_small_buys: bool,
    /// Whether the fill is a flat disc or a shaded sphere.
    pub render_mode: BubbleRenderMode,
    /// How much a sphere-rendered bubble darkens toward its rim. Zero shades
    /// nothing (the fill reads flat again); one pushes the rim to black.
    #[serde(serialize_with = "serialize_short_f32")]
    pub sphere_shading: f32,
    /// Strength of the off-center light spot on a sphere-rendered bubble.
    #[serde(serialize_with = "serialize_short_f32")]
    pub sphere_highlight: f32,
    /// How the full-size reference quantity is chosen.
    pub size_reference: BubbleSizeReference,
    /// Quantity mapped to [`max_radius`](Self::max_radius) when the reference
    /// is [`BubbleSizeReference::Fixed`].
    pub size_reference_quantity: f64,
    /// Prints below this exact quantity are not drawn. Zero draws everything.
    ///
    /// Display-only and applied *after* liquidity association, so hiding small
    /// prints never turns an aggression-aligned reduction into an
    /// unattributed one.
    pub min_quantity: f64,
    /// Vertical separation, in pixels, between the two sides: buy bubbles are
    /// nudged up (they lift the ask), sell bubbles down (they hit the bid).
    ///
    /// Deliberately unfaithful to the exact price — with a one-tick spread both
    /// sides land on the same row and stack into an unreadable line. Zero
    /// restores exact price placement.
    #[serde(serialize_with = "serialize_short_f32")]
    pub side_offset: f32,
    /// Whether a bubble that ate resting liquidity draws its vertical
    /// consumption front.
    pub show_consumption_front: bool,
    /// Width of that front, in pixels.
    #[serde(serialize_with = "serialize_short_f32")]
    pub front_width: f32,
    /// Half-length of the front as a multiple of the bubble radius.
    #[serde(serialize_with = "serialize_short_f32")]
    pub front_length_scale: f32,
    /// Whether a consuming bubble draws a ring around its rim.
    pub show_impact_ring: bool,
    /// Width of that ring, in pixels.
    #[serde(serialize_with = "serialize_short_f32")]
    pub impact_ring_width: f32,
    /// Length of the consumption trail leaking to the right, in pixels. Zero
    /// draws no trail.
    #[serde(serialize_with = "serialize_short_f32")]
    pub trail_length: f32,
    /// Alpha of the trail at the bubble's edge; it fades to nothing.
    #[serde(serialize_with = "serialize_short_f32")]
    pub trail_opacity: f32,
    /// Whether large bubbles print their quantity.
    pub show_quantity_labels: bool,
    /// Whether large bubbles print how many trades they cluster.
    pub show_trade_count: bool,
    /// Smallest radius that gets a label at all.
    #[serde(serialize_with = "serialize_short_f32")]
    pub label_min_radius: f32,
    /// Buy-side colour override; absent follows the theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_color: Option<[u8; 3]>,
    /// Sell-side colour override; absent follows the theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sell_color: Option<[u8; 3]>,
    /// Consumption-front colour override; absent follows the theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front_color: Option<[u8; 3]>,
    /// Trail colour override; absent follows the front colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trail_color: Option<[u8; 3]>,
    /// Label colour override; absent follows the theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_color: Option<[u8; 3]>,
}

impl Default for BubbleStyle {
    fn default() -> Self {
        Self {
            // Area stays quantity-proportional: the minimum keeps the smallest
            // print a subtle dot, the maximum lets a real sweep dominate.
            min_radius: 2.0,
            max_radius: DEFAULT_BUBBLE_MAX_RADIUS,
            opacity: DEFAULT_BUBBLE_OPACITY,
            outline_width: 1.0,
            halo_strength: 0.12,
            detail_min_radius: 4.0,
            readable_min_radius: DEFAULT_READABLE_MIN_RADIUS,
            hollow_small_buys: true,
            // Flat, so merely gaining the sphere option changes no pixels.
            render_mode: BubbleRenderMode::Flat,
            sphere_shading: DEFAULT_SPHERE_SHADING,
            sphere_highlight: DEFAULT_SPHERE_HIGHLIGHT,
            size_reference: BubbleSizeReference::VisibleP99,
            size_reference_quantity: 100.0,
            min_quantity: 0.0,
            side_offset: 3.5,
            show_consumption_front: true,
            front_width: 3.0,
            front_length_scale: 2.1,
            show_impact_ring: true,
            impact_ring_width: 2.2,
            trail_length: 18.0,
            trail_opacity: 0.62,
            show_quantity_labels: true,
            show_trade_count: true,
            // Only the largest bubbles get a (text-layout-costly) label.
            label_min_radius: 16.0,
            buy_color: None,
            sell_color: None,
            front_color: None,
            trail_color: None,
            label_color: None,
        }
    }
}

impl BubbleStyle {
    /// Clamp every numeric field to a value that is safe for geometry and math.
    pub fn sanitize(&mut self) {
        self.min_radius = finite_clamp(self.min_radius, 0.5, MAX_BUBBLE_MIN_RADIUS, 2.0);
        self.max_radius = finite_clamp(
            self.max_radius,
            self.min_radius,
            MAX_BUBBLE_MAX_RADIUS,
            DEFAULT_BUBBLE_MAX_RADIUS,
        );
        self.opacity = finite_clamp(self.opacity, 0.05, 1.0, DEFAULT_BUBBLE_OPACITY);
        self.outline_width = finite_clamp(self.outline_width, 0.0, 6.0, 1.0);
        self.halo_strength = finite_clamp(self.halo_strength, 0.0, 1.0, 0.12);
        self.detail_min_radius = finite_clamp(self.detail_min_radius, 0.0, 32.0, 4.0);
        self.readable_min_radius = finite_clamp(
            self.readable_min_radius,
            0.0,
            MAX_READABLE_MIN_RADIUS,
            DEFAULT_READABLE_MIN_RADIUS,
        );
        self.sphere_shading = finite_clamp(self.sphere_shading, 0.0, 1.0, DEFAULT_SPHERE_SHADING);
        self.sphere_highlight =
            finite_clamp(self.sphere_highlight, 0.0, 1.0, DEFAULT_SPHERE_HIGHLIGHT);
        if !self.size_reference_quantity.is_finite() || self.size_reference_quantity <= 0.0 {
            self.size_reference_quantity = 100.0;
        }
        if !self.min_quantity.is_finite() || self.min_quantity < 0.0 {
            self.min_quantity = 0.0;
        }
        self.side_offset = finite_clamp(self.side_offset, 0.0, 40.0, 3.5);
        self.front_width = finite_clamp(self.front_width, 0.5, 12.0, 3.0);
        self.front_length_scale = finite_clamp(self.front_length_scale, 0.2, 8.0, 2.1);
        self.impact_ring_width = finite_clamp(self.impact_ring_width, 0.5, 8.0, 2.2);
        self.trail_length = finite_clamp(self.trail_length, 0.0, 120.0, 18.0);
        self.trail_opacity = finite_clamp(self.trail_opacity, 0.0, 1.0, 0.62);
        self.label_min_radius = finite_clamp(self.label_min_radius, 4.0, 64.0, 16.0);
    }

    /// Quantity at which a print stops being dust: the exact quantity whose
    /// bubble lands on [`readable_min_radius`](Self::readable_min_radius).
    ///
    /// Inverts the renderer's area mapping — `radius = sqrt(min² + size² ·
    /// (max² − min²))` at `size² = quantity / reference` — so the threshold
    /// follows whatever radius range is configured instead of pinning a second
    /// magic number beside it. `None` means nothing can be dust: no reference
    /// to size against, or a floor already at the smallest drawn radius.
    #[must_use]
    pub fn dust_quantity(&self, reference: Decimal) -> Option<Decimal> {
        if reference <= Decimal::ZERO || self.readable_min_radius <= self.min_radius {
            return None;
        }
        let span = self.max_radius.powi(2) - self.min_radius.powi(2);
        if span <= 0.0 {
            return None;
        }
        let size_sq = (self.readable_min_radius.powi(2) - self.min_radius.powi(2)) / span;
        Decimal::from_f32(size_sq.clamp(0.0, 1.0)).map(|share| reference * share)
    }

    /// Exact quantity below which a print is not drawn, if any.
    #[must_use]
    pub fn min_quantity_decimal(&self) -> Option<Decimal> {
        (self.min_quantity > 0.0)
            .then(|| Decimal::from_f64_retain(self.min_quantity))
            .flatten()
    }

    /// Explicit full-size quantity, when the reference is fixed.
    #[must_use]
    pub fn fixed_reference_decimal(&self) -> Option<Decimal> {
        Decimal::from_f64_retain(self.size_reference_quantity)
            .filter(|value| *value > Decimal::ZERO)
    }
}

fn finite_clamp(value: f32, low: f32, high: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback.clamp(low, high)
    }
}

/// How the reserved band right of the forming bar is drawn.
///
/// The live lane is where prints arrive in real time, and it is the one region
/// of the chart with room to spare: history is compressed into equal-width bar
/// slots, the lane is a fixed band a single bar fills over its whole life. That
/// room is what earns it settings of its own — a wider radius range reads as
/// detail here and as overlap anywhere else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveLaneStyle {
    /// Width of the lane, in candle widths.
    ///
    /// Fixed: it is reserved when the bar opens and does not shrink when the
    /// bar closes, so the chart never steps sideways and the eye always finds
    /// the present in the same place.
    pub width_candles: f32,
    /// Clustering window applied to prints inside the lane.
    ///
    /// `None` inherits [`HeatmapConfig::bubble_cluster_ms`]. A shorter window
    /// than history's buys detail where there is room for it; a longer one
    /// gathers a fast tape into readable marks.
    pub cluster_ms: Option<i64>,
    /// Multiplier applied to both bubble radii inside the lane.
    pub radius_scale: f32,
    /// Whether the lane's boundary and its live-time line are drawn.
    pub show_marks: bool,
}

impl Default for LiveLaneStyle {
    fn default() -> Self {
        Self {
            width_candles: DEFAULT_LIVE_LANE_CANDLES,
            cluster_ms: None,
            radius_scale: DEFAULT_LIVE_LANE_RADIUS_SCALE,
            show_marks: true,
        }
    }
}

impl LiveLaneStyle {
    /// Clamp every value into a range the renderer can use.
    pub fn sanitize(&mut self) {
        self.width_candles = finite_clamp(
            self.width_candles,
            MIN_LIVE_LANE_CANDLES,
            MAX_LIVE_LANE_CANDLES,
            DEFAULT_LIVE_LANE_CANDLES,
        );
        self.radius_scale = finite_clamp(
            self.radius_scale,
            MIN_LIVE_LANE_RADIUS_SCALE,
            MAX_LIVE_LANE_RADIUS_SCALE,
            DEFAULT_LIVE_LANE_RADIUS_SCALE,
        );
        self.cluster_ms = self
            .cluster_ms
            .map(|window| window.clamp(0, MAX_BUBBLE_CLUSTER_MS));
    }

    /// Lane width, in candle widths, on a chart this wide.
    ///
    /// The configured width is granted whenever the window can spare it. On a
    /// narrow window the lane gives way instead of swallowing the history it
    /// exists to be read against. The share is measured over the whole forming
    /// region, so the candle's own slot comes out of it before the lane does.
    #[must_use]
    pub fn resolved_width(&self, chart_width: f32, candle_width: f32) -> f32 {
        let cap = if self.width_candles.is_finite() {
            self.width_candles
                .clamp(MIN_LIVE_LANE_CANDLES, MAX_LIVE_LANE_CANDLES)
        } else {
            DEFAULT_LIVE_LANE_CANDLES
        };
        if !chart_width.is_finite() || !candle_width.is_finite() || candle_width <= 0.0 {
            return cap;
        }
        let available = (chart_width / candle_width) * LIVE_LANE_CHART_SHARE - 1.0;
        if !available.is_finite() {
            return cap;
        }
        available.clamp(MIN_LIVE_LANE_CANDLES.min(cap), cap)
    }

    /// Clustering window for prints inside the lane, given history's own.
    #[must_use]
    pub fn effective_cluster_ms(&self, history_cluster_ms: i64) -> i64 {
        self.cluster_ms.unwrap_or(history_cluster_ms).max(0)
    }

    /// Bubble radius range inside the lane, given the shared bubble style.
    #[must_use]
    pub fn scaled_radii(&self, bubbles: &BubbleStyle) -> (f32, f32) {
        let scale = if self.radius_scale.is_finite() {
            self.radius_scale
                .clamp(MIN_LIVE_LANE_RADIUS_SCALE, MAX_LIVE_LANE_RADIUS_SCALE)
        } else {
            DEFAULT_LIVE_LANE_RADIUS_SCALE
        };
        let min = (bubbles.min_radius * scale).clamp(0.0, MAX_BUBBLE_MIN_RADIUS);
        let max = (bubbles.max_radius * scale).clamp(MIN_BUBBLE_MAX_RADIUS, MAX_BUBBLE_MAX_RADIUS);
        (min.min(max), max)
    }
}

/// Settings shared by history retention and the pure projection layer.
///
/// The two visual layers are independent switches:
/// [`show_depth`](Self::show_depth) owns the L2 depth map,
/// [`show_aggressions`](Self::show_aggressions) owns the aggression bubbles.
/// Bubbles are built from the aggregate-trade stream the chart already
/// consumes, so they never need the depth pipeline.
///
/// Recording and drawing are separate concerns. [`enabled`](Self::enabled) is
/// the recorder and defaults to disabled — merely adding the feature cannot
/// change feed load or memory use. `show_depth` is the display switch, and the
/// projection only builds depth primitives when both are on.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatmapConfig {
    /// Whether L2 depth capture is running.
    ///
    /// This is a data concern, not a visual one: the app keeps it on for every
    /// feed that can stream depth, so hiding the map never punches a hole in
    /// the recording. Only the depth map depends on it — the aggression layer
    /// keeps working with capture off.
    pub enabled: bool,
    /// Maximum age retained in memory, measured in exchange milliseconds.
    pub retention_ms: i64,
    /// Exact price width of one displayed bucket.
    ///
    /// This is the capture/RLE base resolution. Visual grouping is configured
    /// independently with [`display_grouping`](Self::display_grouping).
    pub price_grouping: Decimal,
    /// Renderer-only grouping of captured base buckets.
    pub display_grouping: DisplayGrouping,
    /// Maximum alpha contributed by the heatmap.
    pub opacity: f32,
    /// Colour-curve exponent. Values below one make quieter liquidity visible.
    pub gamma: f32,
    /// Whether aggressive executions are retained and projected.
    ///
    /// This is the aggression layer's own switch: it needs the trade stream
    /// only, so it turns on and off without touching L2 depth capture. Hiding
    /// bubbles is a visual choice and never discards factual history.
    pub show_aggressions: bool,
    /// Temporal window used to cluster compatible aggressive prints.
    ///
    /// Zero keeps raw, one-trade-per-bubble projection.
    pub bubble_cluster_ms: i64,
    /// Temporal window over which prints too small to read are folded into one
    /// bubble per visual price range.
    ///
    /// Zero draws every cluster, however small. The threshold itself is not a
    /// setting: it is whatever quantity lands on
    /// [`BubbleStyle::readable_min_radius`], so widening the radius range
    /// moves the floor with it.
    pub bubble_dust_merge_ms: i64,
    /// Whether the prints of a closed bar are summarized into one bubble per
    /// visual price range, with buy and sell shown as sectors of a pie.
    ///
    /// Off by default. The live lane draws every print as it lands; once the
    /// bar closes that detail is compressed into a single slot, where the two
    /// sides stack on top of each other and read as a smear. The summary is
    /// the alternative: one mark per price range per bar, carrying the summed
    /// quantity of both sides and showing their proportion instead of hiding
    /// one behind the other. Prints in the live lane are never summarized —
    /// they have not finished happening.
    pub bubble_candle_summary: bool,
    /// Everything else the aggression-bubble panel owns: geometry (including
    /// the alpha and largest radius this used to carry as two flat fields),
    /// colour, consumption marks and labels.
    pub bubbles: BubbleStyle,
    /// How the reserved band right of the forming bar is drawn and clustered.
    pub live_lane: LiveLaneStyle,
    /// Whether the depth map is drawn at all — the toolbar's book-heatmap
    /// switch, and the master of the `show_*` flags under it.
    ///
    /// Display-only, like every `show_*` flag in this block: L2 capture keeps
    /// running and retained history keeps accumulating, so switching the map
    /// back on repaints the past it kept recording instead of opening a new
    /// hole. With it off the projection builds no depth primitives at all, so
    /// a hidden map costs nothing beyond the capture the recorder was doing
    /// anyway.
    pub show_depth: bool,
    /// Whether the resting-liquidity heat cells are drawn. Refines
    /// [`show_depth`](Self::show_depth), which stays the depth layer's master
    /// switch.
    ///
    /// Display-only, like every `show_*` flag in this block: L2 capture keeps
    /// running and retained history keeps accumulating, so switching a layer
    /// back on repaints the past it kept recording. Each flag matches one
    /// legend entry, and the legend lists only the layers that are on.
    pub show_liquidity: bool,
    /// Whether buy-side aggression bubbles are drawn. Refines
    /// [`show_aggressions`](Self::show_aggressions), which stays the layer's
    /// master switch (it is what starts trade retention at all).
    pub show_buy_aggressions: bool,
    /// Whether sell-side aggression bubbles are drawn. See
    /// [`show_buy_aggressions`](Self::show_buy_aggressions).
    pub show_sell_aggressions: bool,
    /// Whether reductions with compatible aggression evidence draw their
    /// depletion markers.
    pub show_aligned_depletion: bool,
    /// Whether depth-only (unattributed) reductions draw their markers and
    /// fading withdrawal tails.
    pub show_unattributed_reductions: bool,
    /// Whether L2 coverage gaps draw their boundary marks.
    pub show_gaps: bool,
    /// Smallest reduction fraction whose *unattributed* (depth-only) marker is
    /// displayed. A busy book shrinks buckets by >10% constantly; drawing every
    /// one is violet drizzle. Aggression-aligned reductions always display —
    /// consumption is the feature's heart. Display-only: the underlying runs
    /// and transitions stay factual and complete.
    pub min_unattributed_reduction: f32,
    /// Smallest unattributed pull as a share of the visible full-intensity
    /// liquidity reference (P99). A 50% pull of a tiny level is noise; a 50%
    /// pull of a wall is the story. In a panic thousands of levels shrink at
    /// once, and without a size gate the map turns violet. Display-only.
    pub min_unattributed_pull_share: f32,
    /// Maximum temporal distance for compatible aggression evidence.
    pub liquidity_correlation_ms: i64,
    /// Whether the renderer should show its visual legend.
    pub show_legend: bool,
    /// Renderer palette.
    pub theme: HeatmapTheme,
    /// Maximum number of closed RLE runs retained. Active levels are separate.
    pub max_history_runs: usize,
    /// Approximate byte budget for closed runs and aggressions.
    pub max_history_bytes: usize,
    /// Maximum number of aggressive executions retained.
    pub max_aggressions: usize,
    /// Maximum number of renderable heatmap cells returned by one projection.
    /// Also caps the liquidity-event primitives (a shared safety budget).
    pub max_visible_cells: usize,
    /// Maximum number of aggression primitives returned by one projection.
    pub max_aggression_primitives: usize,
    /// Quantity normalization policy.
    pub intensity_mode: IntensityMode,
}

impl Default for HeatmapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            retention_ms: DEFAULT_RETENTION_MS,
            price_grouping: Decimal::new(1, 2),
            display_grouping: DisplayGrouping::default(),
            opacity: 0.9,
            // Above one so quiet liquidity sinks into the dark canvas and only
            // real walls glow — the Bookmap contrast. Below one paints a dense
            // book edge-to-edge (no walls stand out).
            gamma: 1.8,
            show_aggressions: false,
            bubble_cluster_ms: DEFAULT_BUBBLE_CLUSTER_MS,
            bubble_dust_merge_ms: DEFAULT_BUBBLE_DUST_MERGE_MS,
            bubble_candle_summary: false,
            bubbles: BubbleStyle::default(),
            live_lane: LiveLaneStyle::default(),
            show_depth: true,
            show_liquidity: true,
            show_buy_aggressions: true,
            show_sell_aggressions: true,
            show_aligned_depletion: true,
            show_unattributed_reductions: true,
            show_gaps: true,
            min_unattributed_reduction: 0.5,
            min_unattributed_pull_share: 0.25,
            liquidity_correlation_ms: DEFAULT_LIQUIDITY_CORRELATION_MS,
            show_legend: true,
            theme: HeatmapTheme::Bookmap,
            max_history_runs: 500_000,
            max_history_bytes: 64 * 1024 * 1024,
            max_aggressions: 100_000,
            max_visible_cells: 12_000,
            max_aggression_primitives: 700,
            intensity_mode: IntensityMode::VisibleP99,
        }
    }
}

impl HeatmapConfig {
    /// Whether the depth map has both something recorded and permission to
    /// draw it. Everything the depth layer projects hangs off this.
    #[must_use]
    pub fn depth_visible(&self) -> bool {
        self.enabled && self.show_depth
    }

    /// Whether any order-flow layer asks for a projection.
    ///
    /// The depth map and the aggression bubbles are independent: either one
    /// alone keeps the pipeline alive, and neither can switch the other off.
    /// Capture is not part of this question — the recorder runs on its own, so
    /// a hidden map stops costing projections without stopping the recording.
    #[must_use]
    pub fn any_layer_enabled(&self) -> bool {
        self.depth_visible() || self.show_aggressions
    }

    /// Whether displayed-liquidity reductions need to be computed at all.
    ///
    /// The two depletion layers share one computation; either alone keeps it
    /// on. With both off, aggression bubbles also lose their consumption
    /// marks — matched evidence comes from that same correlation.
    #[must_use]
    pub fn liquidity_events_enabled(&self) -> bool {
        self.show_aligned_depletion || self.show_unattributed_reductions
    }

    /// Return a copy whose numeric values are safe for allocation and math.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.sanitize();
        self
    }

    /// Sanitize in place, returning whether any field was changed.
    pub fn sanitize(&mut self) -> bool {
        let before = self.clone();
        self.retention_ms = self.retention_ms.clamp(MIN_RETENTION_MS, MAX_RETENTION_MS);
        if self.price_grouping <= Decimal::ZERO {
            self.price_grouping = Decimal::new(1, 2);
        }
        self.display_grouping = self.display_grouping.sanitized();
        if !self.opacity.is_finite() {
            self.opacity = 0.9;
        }
        self.opacity = self.opacity.clamp(0.0, 1.0);
        if !self.gamma.is_finite() || self.gamma <= 0.0 {
            self.gamma = 1.0;
        }
        self.gamma = self.gamma.clamp(0.1, 5.0);
        if !self.min_unattributed_reduction.is_finite() {
            self.min_unattributed_reduction = 0.5;
        }
        self.min_unattributed_reduction = self.min_unattributed_reduction.clamp(0.0, 1.0);
        if !self.min_unattributed_pull_share.is_finite() {
            self.min_unattributed_pull_share = 0.25;
        }
        self.min_unattributed_pull_share = self.min_unattributed_pull_share.clamp(0.0, 1.0);
        self.bubble_cluster_ms = self.bubble_cluster_ms.clamp(0, MAX_BUBBLE_CLUSTER_MS);
        self.bubble_dust_merge_ms = self.bubble_dust_merge_ms.clamp(0, MAX_BUBBLE_DUST_MERGE_MS);
        self.bubbles.sanitize();
        self.live_lane.sanitize();
        self.liquidity_correlation_ms = self
            .liquidity_correlation_ms
            .clamp(0, MAX_LIQUIDITY_CORRELATION_MS);
        self.max_history_runs = self.max_history_runs.clamp(1, 10_000_000);
        self.max_history_bytes = self.max_history_bytes.clamp(1_024, 2 * 1024 * 1024 * 1024);
        self.max_aggressions = self.max_aggressions.clamp(1, 5_000_000);
        self.max_visible_cells = self.max_visible_cells.clamp(1, 4_000_000);
        self.max_aggression_primitives = self.max_aggression_primitives.clamp(1, 1_000_000);
        self.intensity_mode = self.intensity_mode.clone().sanitized();
        *self != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_and_bounded() {
        let config = HeatmapConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.retention_ms, DEFAULT_RETENTION_MS);
        assert!(config.price_grouping > Decimal::ZERO);
        assert_eq!(
            config.display_grouping,
            DisplayGrouping::Adaptive {
                target_rows: DEFAULT_ADAPTIVE_ROWS
            }
        );
        assert!((0.0..=1.0).contains(&config.opacity));
        assert!(config.gamma > 0.0);
        assert!(!config.show_aggressions);
        assert!(!config.any_layer_enabled());
        assert_eq!(config.bubble_cluster_ms, DEFAULT_BUBBLE_CLUSTER_MS);
        assert_eq!(config.bubble_dust_merge_ms, DEFAULT_BUBBLE_DUST_MERGE_MS);
        // Summarizing a closed bar throws away the intra-bar detail on
        // purpose, so nobody gets it without asking.
        assert!(!config.bubble_candle_summary);
        // The lane defaults to inheriting every bubble decision history makes:
        // gaining a region of its own must change no pixels on its own.
        assert_eq!(config.live_lane, LiveLaneStyle::default());
        assert_eq!(config.live_lane.cluster_ms, None);
        assert_eq!(
            config
                .live_lane
                .effective_cluster_ms(config.bubble_cluster_ms),
            DEFAULT_BUBBLE_CLUSTER_MS
        );
        assert_eq!(
            config.live_lane.scaled_radii(&config.bubbles),
            (config.bubbles.min_radius, config.bubbles.max_radius)
        );
        assert!(config.bubbles.hollow_small_buys);
        assert_eq!(config.bubbles.opacity, DEFAULT_BUBBLE_OPACITY);
        assert_eq!(config.bubbles.max_radius, DEFAULT_BUBBLE_MAX_RADIUS);
        // Every visual layer defaults to on: gaining per-layer switches must
        // change no pixels until someone actually flips one. The depth map
        // still draws nothing here, because nothing is being recorded yet —
        // recording and drawing are separate questions.
        assert!(config.show_depth);
        assert!(!config.depth_visible());
        assert!(config.show_liquidity);
        assert!(config.show_buy_aggressions);
        assert!(config.show_sell_aggressions);
        assert!(config.show_aligned_depletion);
        assert!(config.show_unattributed_reductions);
        assert!(config.show_gaps);
        assert!(config.liquidity_events_enabled());
        assert_eq!(
            config.liquidity_correlation_ms,
            DEFAULT_LIQUIDITY_CORRELATION_MS
        );
        assert!(config.show_legend);
        assert_eq!(config.theme, HeatmapTheme::Bookmap);
        assert_eq!(config.max_visible_cells, 12_000);
        assert_eq!(config.max_aggression_primitives, 700);
    }

    #[test]
    fn sanitizes_invalid_values_without_enabling_the_feature() {
        let mut config = HeatmapConfig {
            enabled: false,
            retention_ms: i64::MAX,
            price_grouping: Decimal::ZERO,
            display_grouping: DisplayGrouping::Multiple(0),
            opacity: f32::NAN,
            gamma: -1.0,
            bubble_cluster_ms: i64::MAX,
            bubble_dust_merge_ms: i64::MAX,
            bubbles: BubbleStyle {
                opacity: f32::NAN,
                max_radius: 900.0,
                ..BubbleStyle::default()
            },
            live_lane: LiveLaneStyle {
                width_candles: f32::NAN,
                cluster_ms: Some(i64::MIN),
                radius_scale: 900.0,
                show_marks: true,
            },
            liquidity_correlation_ms: i64::MIN,
            max_history_runs: 0,
            max_history_bytes: 0,
            max_aggressions: 0,
            max_visible_cells: 0,
            max_aggression_primitives: 0,
            intensity_mode: IntensityMode::Fixed(Decimal::ZERO),
            ..HeatmapConfig::default()
        };

        assert!(config.sanitize());
        assert!(!config.enabled);
        assert_eq!(config.retention_ms, MAX_RETENTION_MS);
        assert_eq!(config.price_grouping, Decimal::new(1, 2));
        assert_eq!(config.display_grouping, DisplayGrouping::Multiple(1));
        assert_eq!(config.opacity, 0.9);
        assert_eq!(config.gamma, 1.0);
        assert_eq!(config.bubble_cluster_ms, MAX_BUBBLE_CLUSTER_MS);
        assert_eq!(config.bubble_dust_merge_ms, MAX_BUBBLE_DUST_MERGE_MS);
        assert_eq!(config.bubbles.opacity, DEFAULT_BUBBLE_OPACITY);
        assert_eq!(config.bubbles.max_radius, MAX_BUBBLE_MAX_RADIUS);
        assert_eq!(config.live_lane.width_candles, DEFAULT_LIVE_LANE_CANDLES);
        assert_eq!(config.live_lane.cluster_ms, Some(0));
        assert_eq!(config.live_lane.radius_scale, MAX_LIVE_LANE_RADIUS_SCALE);
        assert_eq!(config.liquidity_correlation_ms, 0);
        assert_eq!(config.max_history_runs, 1);
        assert_eq!(config.max_history_bytes, 1_024);
        assert_eq!(config.max_aggressions, 1);
        assert_eq!(config.max_visible_cells, 1);
        assert_eq!(config.max_aggression_primitives, 1);
        assert_eq!(config.intensity_mode, IntensityMode::VisibleP99);
    }

    #[test]
    fn the_lane_keeps_its_width_until_the_window_cannot_spare_it() {
        let lane = LiveLaneStyle::default();
        // A roomy chart grants the configured width exactly — the lane is
        // fixed, not a fraction that drifts with every zoom step.
        assert_eq!(
            lane.resolved_width(1_600.0, 10.0),
            DEFAULT_LIVE_LANE_CANDLES
        );
        assert_eq!(lane.resolved_width(900.0, 5.0), DEFAULT_LIVE_LANE_CANDLES);
        // A narrow window gives way instead of becoming all lane.
        assert!((lane.resolved_width(400.0, 20.0) - 10.0).abs() < 1e-4);
        // Degenerate viewports fall back to the configured width.
        assert_eq!(
            lane.resolved_width(f32::NAN, 10.0),
            DEFAULT_LIVE_LANE_CANDLES
        );
        assert_eq!(lane.resolved_width(1_600.0, 0.0), DEFAULT_LIVE_LANE_CANDLES);

        let wide = LiveLaneStyle {
            width_candles: 40.0,
            ..LiveLaneStyle::default()
        };
        assert_eq!(wide.resolved_width(1_600.0, 10.0), 40.0);
    }

    #[test]
    fn the_lane_overrides_only_what_it_was_given() {
        let bubbles = BubbleStyle::default();
        let tuned = LiveLaneStyle {
            cluster_ms: Some(50),
            radius_scale: 2.0,
            ..LiveLaneStyle::default()
        };
        assert_eq!(tuned.effective_cluster_ms(DEFAULT_BUBBLE_CLUSTER_MS), 50);
        let (min, max) = tuned.scaled_radii(&bubbles);
        assert!((min - bubbles.min_radius * 2.0).abs() < 1e-4);
        assert!((max - bubbles.max_radius * 2.0).abs() < 1e-4);
        // Even an absurd scale stays inside the radius bounds the renderer
        // was built for, and never inverts the range.
        let huge = LiveLaneStyle {
            radius_scale: MAX_LIVE_LANE_RADIUS_SCALE,
            ..LiveLaneStyle::default()
        };
        let (min, max) = huge.scaled_radii(&BubbleStyle {
            min_radius: MAX_BUBBLE_MIN_RADIUS,
            max_radius: MAX_BUBBLE_MAX_RADIUS,
            ..BubbleStyle::default()
        });
        assert!(min <= max);
        assert!(max <= MAX_BUBBLE_MAX_RADIUS);
        assert!(min <= MAX_BUBBLE_MIN_RADIUS);
    }

    #[test]
    fn each_visual_layer_switches_on_its_own() {
        let bubbles_only = HeatmapConfig {
            show_aggressions: true,
            ..HeatmapConfig::default()
        };
        assert!(!bubbles_only.enabled);
        assert!(bubbles_only.any_layer_enabled());

        let depth_only = HeatmapConfig {
            enabled: true,
            ..HeatmapConfig::default()
        };
        assert!(!depth_only.show_aggressions);
        assert!(depth_only.any_layer_enabled());
    }

    #[test]
    fn clamps_opacity_gamma_and_retention() {
        let high = HeatmapConfig {
            retention_ms: i64::MIN,
            opacity: 7.0,
            gamma: 99.0,
            ..HeatmapConfig::default()
        }
        .sanitized();
        assert_eq!(high.retention_ms, MIN_RETENTION_MS);
        assert_eq!(high.opacity, 1.0);
        assert_eq!(high.gamma, 5.0);

        let low = HeatmapConfig {
            gamma: 0.001,
            ..HeatmapConfig::default()
        }
        .sanitized();
        assert_eq!(low.gamma, 0.1);
    }

    #[test]
    fn a_positive_fixed_scale_survives_sanitization() {
        let config = HeatmapConfig {
            intensity_mode: IntensityMode::Fixed(Decimal::from(25)),
            ..HeatmapConfig::default()
        }
        .sanitized();
        assert_eq!(
            config.intensity_mode,
            IntensityMode::Fixed(Decimal::from(25))
        );
    }

    #[test]
    fn bubble_defaults_separate_the_two_sides_and_stay_bounded() {
        let bubbles = HeatmapConfig::default().bubbles;
        assert!(bubbles.max_radius > bubbles.min_radius);
        assert!(
            bubbles.side_offset > 0.0,
            "buy and sell must not stack on the same row by default"
        );
        assert!(bubbles.show_consumption_front);
        assert_eq!(bubbles.size_reference, BubbleSizeReference::VisibleP99);
        assert_eq!(bubbles.min_quantity, 0.0, "nothing is hidden by default");
        assert_eq!(bubbles.min_quantity_decimal(), None);
        assert_eq!(
            bubbles.render_mode,
            BubbleRenderMode::Flat,
            "gaining the sphere option must not change existing charts"
        );
        assert!((0.0..=1.0).contains(&bubbles.sphere_shading));
        assert!((0.0..=1.0).contains(&bubbles.sphere_highlight));
    }

    #[test]
    fn sphere_fields_sanitize_and_round_trip_through_toml() {
        let mut style = BubbleStyle {
            render_mode: BubbleRenderMode::Sphere,
            sphere_shading: f32::NAN,
            sphere_highlight: 7.0,
            ..BubbleStyle::default()
        };
        style.sanitize();
        assert_eq!(style.render_mode, BubbleRenderMode::Sphere);
        assert_eq!(style.sphere_shading, DEFAULT_SPHERE_SHADING);
        assert_eq!(style.sphere_highlight, 1.0);

        let text = toml::to_string(&style).expect("serialize");
        assert!(text.contains("render_mode = \"sphere\""));
        let parsed: BubbleStyle = toml::from_str(&text).expect("parse");
        assert_eq!(parsed, style);

        // A presets file written before the mode existed keeps rendering flat.
        let old: BubbleStyle = toml::from_str("max_radius = 30.0").expect("parse old file");
        assert_eq!(old.render_mode, BubbleRenderMode::Flat);
    }

    #[test]
    fn bubble_style_sanitizes_invalid_geometry() {
        let mut style = BubbleStyle {
            min_radius: f32::NAN,
            max_radius: -3.0,
            opacity: 12.0,
            front_width: 0.0,
            trail_length: f32::INFINITY,
            side_offset: -5.0,
            min_quantity: -1.0,
            size_reference_quantity: 0.0,
            ..BubbleStyle::default()
        };
        style.sanitize();
        assert_eq!(style.min_radius, 2.0);
        assert!(style.max_radius >= style.min_radius);
        assert_eq!(style.opacity, 1.0);
        assert_eq!(style.front_width, 0.5);
        assert_eq!(style.trail_length, 18.0);
        assert_eq!(style.side_offset, 0.0);
        assert_eq!(style.min_quantity, 0.0);
        assert_eq!(style.size_reference_quantity, 100.0);
    }

    #[test]
    fn sanitizing_the_heatmap_config_also_sanitizes_its_bubbles() {
        let config = HeatmapConfig {
            bubbles: BubbleStyle {
                opacity: f32::NAN,
                ..BubbleStyle::default()
            },
            ..HeatmapConfig::default()
        }
        .sanitized();
        assert_eq!(config.bubbles.opacity, 0.78);
    }

    #[test]
    fn bubble_style_round_trips_through_toml() {
        // Presets are stored as TOML, so an unserializable field (a bare
        // `None`, say) would break saving at runtime instead of at build time.
        let style = BubbleStyle {
            buy_color: Some([1, 2, 3]),
            size_reference: BubbleSizeReference::Fixed,
            min_quantity: 2.5,
            ..BubbleStyle::default()
        };
        let text = toml::to_string(&style).expect("serialize");
        let parsed: BubbleStyle = toml::from_str(&text).expect("parse");
        assert_eq!(parsed, style);
        assert!(!text.contains("sell_color"), "absent overrides stay absent");

        // A partial document keeps every untouched field at its default.
        let partial: BubbleStyle = toml::from_str("max_radius = 30.0").expect("parse partial");
        assert_eq!(partial.max_radius, 30.0);
        assert_eq!(partial.min_radius, BubbleStyle::default().min_radius);
    }

    #[test]
    fn sanitizes_adaptive_rows_and_temporal_windows() {
        let zero_rows = HeatmapConfig {
            display_grouping: DisplayGrouping::Adaptive { target_rows: 0 },
            bubble_cluster_ms: -5,
            bubble_dust_merge_ms: -1,
            liquidity_correlation_ms: i64::MAX,
            ..HeatmapConfig::default()
        }
        .sanitized();
        assert_eq!(zero_rows.display_grouping, DisplayGrouping::default());
        assert_eq!(zero_rows.bubble_cluster_ms, 0);
        assert_eq!(zero_rows.bubble_dust_merge_ms, 0);
        assert_eq!(
            zero_rows.liquidity_correlation_ms,
            MAX_LIQUIDITY_CORRELATION_MS
        );

        let bounded_rows = HeatmapConfig {
            display_grouping: DisplayGrouping::Adaptive {
                target_rows: u32::MAX,
            },
            ..HeatmapConfig::default()
        }
        .sanitized();
        assert_eq!(
            bounded_rows.display_grouping,
            DisplayGrouping::Adaptive {
                target_rows: MAX_ADAPTIVE_ROWS
            }
        );
    }
}
