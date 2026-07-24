//! Sanitized runtime configuration for the order-book heatmap.

use rust_decimal::Decimal;

/// Shortest history window accepted by the UI.
pub const MIN_RETENTION_MS: i64 = 1_000;
/// Longest in-memory history window accepted by the UI.
pub const MAX_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
/// Default history window: ten minutes. Dense books (e.g. BTC) accumulate tens
/// of thousands of RLE runs, and every visible run is re-swept on projection, so
/// a long default window makes the heatmap unaffordable. Users can raise it.
pub const DEFAULT_RETENTION_MS: i64 = 5 * 60 * 1_000;
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

/// Settings shared by history retention and the pure projection layer.
///
/// The default is deliberately disabled: merely adding the feature cannot
/// change feed load, memory use or rendering behaviour of the existing chart.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatmapConfig {
    /// Whether capture/projection is enabled.
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
    /// Whether retained aggressive executions are projected.
    ///
    /// Hiding bubbles is a visual choice and never discards factual history.
    pub show_aggressions: bool,
    /// Temporal window used to cluster compatible aggressive prints.
    ///
    /// Zero keeps raw, one-trade-per-bubble projection.
    pub bubble_cluster_ms: i64,
    /// Whether factual displayed-liquidity reductions are projected.
    pub show_liquidity_events: bool,
    /// Smallest reduction fraction whose *unattributed* (depth-only) marker is
    /// displayed. A busy book shrinks buckets by >10% constantly; drawing every
    /// one is violet drizzle. Full removals and aggression-aligned reductions
    /// always display — consumption is the feature's heart. Display-only: the
    /// underlying runs and transitions stay factual and complete.
    pub min_unattributed_reduction: f32,
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
            show_aggressions: true,
            bubble_cluster_ms: DEFAULT_BUBBLE_CLUSTER_MS,
            show_liquidity_events: true,
            min_unattributed_reduction: 0.5,
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
            self.opacity = 0.72;
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
        self.bubble_cluster_ms = self.bubble_cluster_ms.clamp(0, MAX_BUBBLE_CLUSTER_MS);
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
        assert!(config.show_aggressions);
        assert_eq!(config.bubble_cluster_ms, DEFAULT_BUBBLE_CLUSTER_MS);
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
        assert_eq!(config.opacity, 0.72);
        assert_eq!(config.gamma, 1.0);
        assert_eq!(config.bubble_cluster_ms, MAX_BUBBLE_CLUSTER_MS);
        assert_eq!(config.liquidity_correlation_ms, 0);
        assert_eq!(config.max_history_runs, 1);
        assert_eq!(config.max_history_bytes, 1_024);
        assert_eq!(config.max_aggressions, 1);
        assert_eq!(config.max_visible_cells, 1);
        assert_eq!(config.max_aggression_primitives, 1);
        assert_eq!(config.intensity_mode, IntensityMode::VisibleP99);
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
