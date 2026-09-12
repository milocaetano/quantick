//! Sanitized runtime configuration for the order-book heatmap.

use rust_decimal::Decimal;

mod bubbles;
mod lane;

pub use bubbles::{
    BubbleRenderMode, BubbleSizeReference, BubbleStyle, ConsumptionMark, DEFAULT_BUBBLE_MAX_RADIUS,
    DEFAULT_BUBBLE_MIN_RADIUS, DEFAULT_BUBBLE_OPACITY, DEFAULT_DETAIL_MIN_RADIUS,
    DEFAULT_FRONT_LENGTH_SCALE, DEFAULT_LABEL_MIN_RADIUS, DEFAULT_LABEL_MIN_RADIUS_SHARE,
    DEFAULT_READABLE_MIN_RADIUS, DEFAULT_SPHERE_HIGHLIGHT, DEFAULT_SPHERE_SHADING, GOLDEN_ANGLE,
    INV_PHI, INV_PHI_2, INV_PHI_3, MAX_BUBBLE_MAX_RADIUS, MAX_BUBBLE_MIN_RADIUS,
    MAX_READABLE_MIN_RADIUS, MIN_BUBBLE_MAX_RADIUS, PHI,
};
pub use lane::{
    DEFAULT_LIVE_LANE_RADIUS_SCALE, DEFAULT_LIVE_LANE_SHARE, DEFAULT_LIVE_LANE_ZOOM,
    LANE_WINDOW_PRESETS_MS, LaneWindow, LiveLaneStyle, MAX_LIVE_LANE_RADIUS_SCALE,
    MAX_LIVE_LANE_SHARE, MAX_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_ZOOM, MIN_LIVE_LANE_RADIUS_SCALE,
    MIN_LIVE_LANE_SHARE, MIN_LIVE_LANE_WIDTH_PX, MIN_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_ZOOM,
    format_window_ms, lane_lag_label, lane_window_label, same_lane_window,
};

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
/// Widest regional fold accepted, in visual price rows per region.
///
/// One row is off — a region the width of a bucket is the bucket. The cap is
/// a sanity bound, not a tuning value: past a few dozen rows a "region"
/// spans more than a screen of prices and the mark stops pointing anywhere.
pub const MAX_BUBBLE_REGION_ROWS: u32 = 64;
/// Default temporal window of one regional fold. Wide enough to gather a
/// sweep walking through a level band, short enough that two separate pushes
/// at the same prices stay two marks.
pub const DEFAULT_BUBBLE_REGION_MS: i64 = 1_000;
/// Largest temporal window accepted for the regional fold.
pub const MAX_BUBBLE_REGION_MS: i64 = 10_000;
/// Default distance accepted when correlating a depth reduction and aggression.
pub const DEFAULT_LIQUIDITY_CORRELATION_MS: i64 = 250;
/// Safe upper bound for depth/aggression correlation.
pub const MAX_LIQUIDITY_CORRELATION_MS: i64 = 10_000;

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

fn finite_clamp(value: f32, low: f32, high: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback.clamp(low, high)
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
    /// Whether a surface that is not the depth map or the bubbles is reading
    /// the projection right now: the live strip beside the price axis, which
    /// draws the same clusters, and the live lane's marks, which need the
    /// frame's live edge.
    ///
    /// Not a user setting and never written to disk: the pane owns those
    /// layers' visibility and states their demand here every frame, so this
    /// field can have no second owner to disagree with. It exists because a
    /// frame nobody builds is a surface nobody draws — hiding the bubbles used
    /// to blank the strip, and switching every other flow layer off left the
    /// lane reserved but unmarked while its menu entry still read as on.
    pub projection_demand: bool,
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
    /// Height of one aggression region, in adjacent visual price rows.
    ///
    /// One (the default) keeps today's per-row marks. Above one, clusters that
    /// share a side land in regions `bubble_region_rows` rows tall and fold
    /// into one bubble per region within
    /// [`bubble_region_ms`](Self::bubble_region_ms), placed at the fold's
    /// volume-weighted price. A dense tape then reads as pressure on a price
    /// *area* — the Bookmap read — instead of a bead necklace of per-tick
    /// marks. Folding happens after evidence association and sums exact
    /// quantities, so nothing is dropped or re-attributed; with the candle
    /// summary on, closed bars show one buy/sell pie per region.
    pub bubble_region_rows: u32,
    /// Temporal window of one regional fold, anchored at its first cluster.
    ///
    /// Only read when [`bubble_region_rows`](Self::bubble_region_rows) is
    /// above one.
    pub bubble_region_ms: i64,
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
    /// Whether the book's status badge is drawn in the canvas's top-right
    /// corner. Chrome about the capture, not about the market: switching it
    /// off silences the label and changes nothing about the recording.
    pub show_status_badge: bool,
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
            projection_demand: false,
            bubble_cluster_ms: DEFAULT_BUBBLE_CLUSTER_MS,
            bubble_dust_merge_ms: DEFAULT_BUBBLE_DUST_MERGE_MS,
            bubble_region_rows: 1,
            bubble_region_ms: DEFAULT_BUBBLE_REGION_MS,
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
            show_status_badge: true,
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

    /// Whether the tape is on the canvas at all
    /// ([`LiveLaneStyle::enabled`]).
    #[must_use]
    pub fn lane_enabled(&self) -> bool {
        self.live_lane.enabled
    }

    /// Whether the depth map is *switched on* for the tape.
    ///
    /// Capture still gates it — a map with nothing recorded behind it is not a
    /// map on either pane — but the display choice is the lane's own, never the
    /// chart's ([`LiveLaneStyle::show_depth`]).
    ///
    /// Deliberately blind to [`lane_enabled`](Self::lane_enabled): this answers
    /// what the tape draws *when there is a tape*, so switching the whole tape
    /// off and on again returns the tape that was switched off. What actually
    /// reaches the canvas is [`lane_depth_drawn`](Self::lane_depth_drawn).
    #[must_use]
    pub fn lane_depth_visible(&self) -> bool {
        self.enabled && self.live_lane.show_depth
    }

    /// Whether aggression bubbles are *switched on* for the tape. No capture
    /// gate: the bubbles are built from the trade stream the chart already
    /// consumes. Same blindness to the tape's own switch as
    /// [`lane_depth_visible`](Self::lane_depth_visible).
    #[must_use]
    pub fn lane_aggressions_visible(&self) -> bool {
        self.live_lane.show_aggressions
    }

    /// Whether the depth map actually reaches the tape: switched on, and there
    /// is a tape to reach.
    #[must_use]
    pub fn lane_depth_drawn(&self) -> bool {
        self.lane_enabled() && self.lane_depth_visible()
    }

    /// Whether the bubbles actually reach the tape. See
    /// [`lane_depth_drawn`](Self::lane_depth_drawn).
    #[must_use]
    pub fn lane_aggressions_drawn(&self) -> bool {
        self.lane_enabled() && self.lane_aggressions_visible()
    }

    /// Whether any pane still draws the depth map.
    ///
    /// What decides that the projection has to keep building depth primitives:
    /// hiding the map on the candles while the tape still shows it is a change
    /// of view, never a reason to stop producing what the tape is reading. A
    /// tape that is switched off reads nothing, so it asks for nothing.
    #[must_use]
    pub fn depth_visible_anywhere(&self) -> bool {
        self.depth_visible() || self.lane_depth_drawn()
    }

    /// Whether any pane still draws the aggression bubbles. Same rule as
    /// [`depth_visible_anywhere`](Self::depth_visible_anywhere).
    #[must_use]
    pub fn aggressions_visible_anywhere(&self) -> bool {
        self.show_aggressions || self.lane_aggressions_drawn()
    }

    /// Whether any order-flow layer asks for a projection.
    ///
    /// The depth map and the aggression bubbles are independent: either one
    /// alone keeps the pipeline alive, and neither can switch the other off.
    /// Capture is not part of this question — the recorder runs on its own, so
    /// a hidden map stops costing projections without stopping the recording.
    ///
    /// A surface that is not a layer of its own can ask too
    /// ([`projection_demand`](Self::projection_demand)): the live strip draws
    /// the same clusters the bubbles do, and it stays alive when they are
    /// hidden.
    #[must_use]
    pub fn any_layer_enabled(&self) -> bool {
        self.depth_visible_anywhere()
            || self.aggressions_visible_anywhere()
            || self.projection_demand
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
        self.bubble_region_rows = self.bubble_region_rows.clamp(1, MAX_BUBBLE_REGION_ROWS);
        self.bubble_region_ms = self.bubble_region_ms.clamp(0, MAX_BUBBLE_REGION_MS);
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
        // The candles open clean; the tape does not. Its whole job is the book
        // and the prints landing into it, so it opens with both — and that is
        // what holds the aggression pipeline open from the first frame, where
        // a fresh config used to be wholly inert.
        assert!(config.lane_enabled());
        assert!(config.lane_aggressions_drawn());
        assert!(config.any_layer_enabled());
        assert!(
            !HeatmapConfig {
                live_lane: LiveLaneStyle {
                    enabled: false,
                    ..LiveLaneStyle::default()
                },
                ..HeatmapConfig::default()
            }
            .any_layer_enabled(),
            "and with the tape off there is nothing left asking for a projection"
        );
        assert_eq!(config.bubble_cluster_ms, DEFAULT_BUBBLE_CLUSTER_MS);
        assert_eq!(config.bubble_dust_merge_ms, DEFAULT_BUBBLE_DUST_MERGE_MS);
        // Summarizing a closed bar throws away the intra-bar detail on
        // purpose, so nobody gets it without asking.
        assert!(!config.bubble_candle_summary);
        // The lane still inherits every *bubble* decision history makes —
        // clustering, radii, regions — and only its own visibility is its own.
        assert_eq!(config.live_lane, LiveLaneStyle::default());
        assert_eq!(config.live_lane.cluster_ms, None);
        assert_eq!(
            config
                .live_lane
                .effective_cluster_ms(config.bubble_cluster_ms, 8_000),
            DEFAULT_BUBBLE_CLUSTER_MS
        );
        assert_eq!(
            config.live_lane.scaled_radii(&config.bubbles),
            (config.bubbles.min_radius, config.bubbles.max_radius)
        );
        assert!(
            !config.bubbles.hollow_small_buys,
            "a small print keeps its fill; side is carried by luminance and the nudge"
        );
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
        assert!(config.show_status_badge);
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
                width_share: f32::NAN,
                window: LaneWindow::Auto { zoom: 900.0 },
                cluster_ms: Some(i64::MIN),
                radius_scale: 900.0,
                show_marks: true,
                enabled: true,
                show_depth: true,
                show_aggressions: true,
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
        assert_eq!(config.live_lane.width_share, DEFAULT_LIVE_LANE_SHARE);
        assert_eq!(
            config.live_lane.window,
            LaneWindow::Auto {
                zoom: MAX_LIVE_LANE_ZOOM
            }
        );
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

    /// The two panes are switched apart, and neither switch may starve the
    /// other pane of the projection that feeds it.
    #[test]
    fn hiding_a_layer_on_one_pane_leaves_the_other_drawing_and_fed() {
        let mut config = HeatmapConfig {
            enabled: true,
            show_depth: true,
            show_aggressions: true,
            ..HeatmapConfig::default()
        };
        // Two switches, two panes, and the tape's are on out of the box.
        assert!(config.depth_visible() && config.lane_depth_drawn());
        assert!(config.show_aggressions && config.lane_aggressions_drawn());

        // The candles go quiet; the tape was never asked anything and keeps
        // both layers.
        config.show_depth = false;
        config.show_aggressions = false;
        assert!(!config.depth_visible(), "the candles are clear");
        assert!(config.lane_depth_drawn(), "the tape still has the book");
        assert!(config.lane_aggressions_drawn(), "and the prints");
        assert!(
            config.any_layer_enabled(),
            "a tape nobody switched off may not lose the projection that feeds it"
        );

        // The other way round: the tape is cleared, the candles keep drawing.
        config.show_depth = true;
        config.live_lane.show_depth = false;
        config.live_lane.show_aggressions = false;
        assert!(config.depth_visible() && !config.lane_depth_drawn());
        assert!(config.any_layer_enabled());

        // Only with every pane's every layer off does the pipeline stand down
        // — and a surface that is not a layer can still hold it open.
        config.show_depth = false;
        assert!(!config.any_layer_enabled());
        config.projection_demand = true;
        assert!(config.any_layer_enabled());

        // Capture still gates the map on both panes: a map with nothing
        // recorded behind it is not a map anywhere.
        config.projection_demand = false;
        config.enabled = false;
        config.show_depth = true;
        config.live_lane.show_depth = true;
        assert!(!config.depth_visible() && !config.lane_depth_drawn());
    }

    /// The tape's own switch is a gate over what it draws, never an eraser of
    /// what it was set to draw.
    #[test]
    fn taking_the_tape_off_the_canvas_reserves_nothing_and_forgets_nothing() {
        let mut config = HeatmapConfig {
            enabled: true,
            show_depth: false,
            show_aggressions: false,
            ..HeatmapConfig::default()
        };
        assert!(config.lane_enabled(), "a fresh tape is on the canvas");
        assert!(
            config.lane_depth_drawn() && config.lane_aggressions_drawn(),
            "with the book and the prints, whatever the candles are showing"
        );
        assert!(
            config.any_layer_enabled(),
            "so the projection runs for the tape alone"
        );
        assert!(config.live_lane.resolved_width_px(1_000.0) > 0.0);

        config.live_lane.enabled = false;
        assert!(
            !config.lane_depth_drawn() && !config.lane_aggressions_drawn(),
            "nothing reaches a canvas with no tape on it"
        );
        assert!(
            !config.any_layer_enabled(),
            "and nothing is projected on its account"
        );
        assert_eq!(
            config.live_lane.resolved_width_px(1_000.0),
            0.0,
            "the band is not reserved: the candles take the whole canvas"
        );
        assert!(
            config.lane_depth_visible() && config.lane_aggressions_visible(),
            "the switches themselves are untouched"
        );

        config.live_lane.enabled = true;
        assert!(
            config.lane_depth_drawn() && config.lane_aggressions_drawn(),
            "so switching the tape back on returns the tape that was switched off"
        );
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
    fn sanitizing_the_heatmap_config_also_sanitizes_its_bubbles() {
        let config = HeatmapConfig {
            bubbles: BubbleStyle {
                opacity: f32::NAN,
                ..BubbleStyle::default()
            },
            ..HeatmapConfig::default()
        }
        .sanitized();
        assert_eq!(config.bubbles.opacity, DEFAULT_BUBBLE_OPACITY);
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
