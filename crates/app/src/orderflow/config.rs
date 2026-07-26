//! Sanitized runtime configuration for the order-book heatmap.

use rust_decimal::Decimal;
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
/// Default edge darkening of a sphere-rendered bubble.
pub const DEFAULT_SPHERE_SHADING: f32 = 0.55;
/// Default highlight strength of a sphere-rendered bubble.
pub const DEFAULT_SPHERE_HIGHLIGHT: f32 = 0.35;

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

/// Settings shared by history retention and the pure projection layer.
///
/// The two visual layers are independent switches: [`enabled`](Self::enabled)
/// owns the L2 depth map, [`show_aggressions`](Self::show_aggressions) owns the
/// aggression bubbles. Bubbles are built from the aggregate-trade stream the
/// chart already consumes, so they never need the depth pipeline.
///
/// Both default to disabled: merely adding the feature cannot change feed load,
/// memory use or rendering behaviour of the existing chart.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatmapConfig {
    /// Whether L2 depth capture/projection is enabled.
    ///
    /// Only the depth map depends on this: the aggression layer keeps working
    /// with capture off.
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
    /// Everything else the aggression-bubble panel owns: geometry (including
    /// the alpha and largest radius this used to carry as two flat fields),
    /// colour, consumption marks and labels.
    pub bubbles: BubbleStyle,
    /// Whether factual displayed-liquidity reductions are projected.
    pub show_liquidity_events: bool,
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
            bubbles: BubbleStyle::default(),
            show_liquidity_events: true,
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
    /// Whether any order-flow layer asks for capture and projection.
    ///
    /// The depth map and the aggression bubbles are independent: either one
    /// alone keeps the pipeline alive, and neither can switch the other off.
    #[must_use]
    pub fn any_layer_enabled(&self) -> bool {
        self.enabled || self.show_aggressions
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
        self.bubbles.sanitize();
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
        assert_eq!(config.bubbles.opacity, DEFAULT_BUBBLE_OPACITY);
        assert_eq!(config.bubbles.max_radius, DEFAULT_BUBBLE_MAX_RADIUS);
        assert!(config.show_liquidity_events);
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
            bubbles: BubbleStyle {
                opacity: f32::NAN,
                max_radius: 900.0,
                ..BubbleStyle::default()
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
        assert_eq!(config.bubbles.opacity, DEFAULT_BUBBLE_OPACITY);
        assert_eq!(config.bubbles.max_radius, MAX_BUBBLE_MAX_RADIUS);
        assert_eq!(config.liquidity_correlation_ms, 0);
        assert_eq!(config.max_history_runs, 1);
        assert_eq!(config.max_history_bytes, 1_024);
        assert_eq!(config.max_aggressions, 1);
        assert_eq!(config.max_visible_cells, 1);
        assert_eq!(config.max_aggression_primitives, 1);
        assert_eq!(config.intensity_mode, IntensityMode::VisibleP99);
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
            liquidity_correlation_ms: i64::MAX,
            ..HeatmapConfig::default()
        }
        .sanitized();
        assert_eq!(zero_rows.display_grouping, DisplayGrouping::default());
        assert_eq!(zero_rows.bubble_cluster_ms, 0);
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
