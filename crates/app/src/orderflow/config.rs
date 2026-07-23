//! Sanitized runtime configuration for the order-book heatmap.

use rust_decimal::Decimal;

/// Shortest history window accepted by the UI.
pub const MIN_RETENTION_MS: i64 = 1_000;
/// Longest in-memory history window accepted by the UI.
pub const MAX_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
/// Default history window: thirty minutes.
pub const DEFAULT_RETENTION_MS: i64 = 30 * 60 * 1_000;

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
    pub price_grouping: Decimal,
    /// Maximum alpha contributed by the heatmap.
    pub opacity: f32,
    /// Colour-curve exponent. Values below one make quieter liquidity visible.
    pub gamma: f32,
    /// Whether aggressive executions are retained and projected.
    pub show_aggressions: bool,
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
            opacity: 0.72,
            gamma: 0.75,
            show_aggressions: true,
            max_history_runs: 500_000,
            max_history_bytes: 64 * 1024 * 1024,
            max_aggressions: 100_000,
            max_visible_cells: 50_000,
            max_aggression_primitives: 2_000,
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
        if !self.opacity.is_finite() {
            self.opacity = 0.72;
        }
        self.opacity = self.opacity.clamp(0.0, 1.0);
        if !self.gamma.is_finite() || self.gamma <= 0.0 {
            self.gamma = 1.0;
        }
        self.gamma = self.gamma.clamp(0.1, 5.0);
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
        assert!((0.0..=1.0).contains(&config.opacity));
        assert!(config.gamma > 0.0);
        assert!(config.show_aggressions);
        assert_eq!(config.max_visible_cells, 50_000);
        assert_eq!(config.max_aggression_primitives, 2_000);
    }

    #[test]
    fn sanitizes_invalid_values_without_enabling_the_feature() {
        let mut config = HeatmapConfig {
            enabled: false,
            retention_ms: i64::MAX,
            price_grouping: Decimal::ZERO,
            opacity: f32::NAN,
            gamma: -1.0,
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
        assert_eq!(config.opacity, 0.72);
        assert_eq!(config.gamma, 1.0);
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
}
