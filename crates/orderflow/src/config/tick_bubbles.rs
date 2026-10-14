//! Optional tick-chart spheres, independent of the native tape appearance.
use super::BubbleStyle;
use rust_decimal::{Decimal, prelude::ToPrimitive as _};
use serde::{Deserialize, Serialize};

/// Quantity denomination carried by a calibration, including saved custom looks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TickQuantityUnit {
    /// Compatibility default for the original Binance sphere calibrations.
    #[default]
    Btc,
    Contracts,
}

impl TickQuantityUnit {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Btc => "BTC",
            Self::Contracts => "contracts",
        }
    }

    fn is_btc(&self) -> bool {
        *self == Self::Btc
    }
}

impl super::HeatmapConfig {
    /// Whether a change affects the tape or book. Tick painting has its own
    /// cache identity and must not restart their retained factual groups.
    #[must_use]
    pub fn same_tape_settings(&self, other: &Self) -> bool {
        let mut compared = other.clone();
        compared.volume_dots.tick_bubbles = self.volume_dots.tick_bubbles;
        *self == compared
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TickBubbleStyle {
    /// Optional fraction of fill opacity for small volumes. Emphasis reaches
    /// full opacity at one quarter of the fixed reference; area never changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_opacity_floor: Option<f32>,
    /// Optional causal tape grouping for candles; never edits the tape itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tape_bursts: Option<super::TickBurstGrouping>,
    /// Display denomination; absent in previously saved BTC styles.
    #[serde(default, skip_serializing_if = "TickQuantityUnit::is_btc")]
    pub quantity_unit: TickQuantityUnit,
    /// Temporal support of legacy candle regions; causal bursts use their own blocks.
    #[serde(
        default = "default_region_window_ms",
        skip_serializing_if = "default_region_window"
    )]
    pub region_window_ms: i64,
    /// Quantity at the reference radius. Causal bursts keep it fixed; legacy
    /// regions raise it uniformly when a larger ordinary region is visible.
    pub reference_quantity: f64,
    /// Full-size radius in logical pixels.
    #[serde(serialize_with = "super::bubbles::serialize_short_f32")]
    pub max_radius: f32,
    /// Smallest disc eligible for an inside quantity label.
    #[serde(serialize_with = "super::bubbles::serialize_short_f32")]
    pub label_min_radius: f32,
    /// Sphere fill opacity; candle contours are painted afterwards.
    #[serde(serialize_with = "super::bubbles::serialize_short_f32")]
    pub opacity: f32,
}

impl TickBubbleStyle {
    pub fn sanitize(&mut self) {
        self.volume_opacity_floor = self.volume_opacity_floor.map(|value| {
            if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                1.0
            }
        });
        if let Some(bursts) = &mut self.tape_bursts {
            bursts.sanitize();
        }
        self.region_window_ms = (self.region_window_ms / 100 * 100).clamp(100, 10_000);
        if !self.reference_quantity.is_finite() || self.reference_quantity <= 0.0 {
            self.reference_quantity = 40.0;
        }
        let style = self.bubbles();
        self.max_radius = style.max_radius;
        self.label_min_radius = style.label_min_radius;
        self.opacity = style.opacity;
    }

    #[must_use]
    pub fn bubbles(self) -> BubbleStyle {
        let mut style = BubbleStyle {
            max_radius: self.max_radius,
            label_min_radius: self.label_min_radius,
            opacity: self.opacity,
            show_trade_count: false,
            ..BubbleStyle::default()
        };
        style.sanitize();
        style
    }

    #[must_use]
    pub fn fingerprint(self) -> [u64; 11] {
        let bursts = self.tape_bursts.map_or([0; 4], |value| value.fingerprint());
        [
            self.reference_quantity.to_bits(),
            u64::from(self.max_radius.to_bits()),
            u64::from(self.label_min_radius.to_bits()),
            u64::from(self.opacity.to_bits()),
            self.region_window_ms as u64,
            u64::from(self.tape_bursts.is_some()),
            bursts[0],
            bursts[1],
            bursts[2],
            bursts[3],
            self.volume_opacity_floor
                .map_or(u64::MAX, |value| u64::from(value.to_bits())),
        ]
    }

    /// A stable quantity cue, independent of camera, other groups or tape settings.
    #[must_use]
    pub fn fill_opacity(self, quantity: Decimal) -> f32 {
        let Some(floor) = self.volume_opacity_floor else {
            return self.opacity;
        };
        let full = self.reference_quantity
            / f64::from(crate::projection::flow_tape::LARGE_REGION_REFERENCE_DIVISOR);
        if !full.is_finite() || full <= 0.0 {
            return self.opacity;
        }
        let relative = (quantity.to_f64().unwrap_or_default() / full).clamp(0.0, 1.0) as f32;
        self.opacity * (floor + (1.0 - floor) * relative)
    }

    /// Native candle bursts below the existing detail threshold retain both
    /// exact side sectors, without an unreadable inner lighting ring.
    #[must_use]
    pub fn light_offset(self, radius: f32) -> f32 {
        if self.tape_bursts.is_some() && radius < super::bubbles::DEFAULT_DETAIL_MIN_RADIUS {
            0.0
        } else {
            super::dressing::SPHERE_LIGHT_OFFSET
        }
    }

    /// Compact contract labels spend their limited width on the size class.
    /// Exact quantities remain in the source and structured readback.
    #[must_use]
    pub fn quantity_label(self, quantity: Decimal) -> String {
        if self.tape_bursts.is_some()
            && self.quantity_unit == TickQuantityUnit::Contracts
            && quantity >= Decimal::from(1_000)
            && quantity < Decimal::from(10_000)
        {
            let rounded = (quantity / Decimal::from(1_000)).round_dp(1).normalize();
            format!("{rounded}K")
        } else {
            super::labels::format_quantity(quantity)
        }
    }
}

fn default_region_window_ms() -> i64 {
    crate::constants::DEFAULT_TICK_REGION_WINDOW_MS
}

fn default_region_window(value: &i64) -> bool {
    *value == default_region_window_ms()
}
