//! Input model: what an indicator lets the user configure.
//!
//! Inputs are *declared* ([`InputSpec`], extracted once at load time) and
//! *valued* ([`InputValue`]). The app builds its settings UI from the specs
//! alone — this crate never renders anything. Changing a value means a full
//! recompute of that indicator: scripts never observe an input changing
//! mid-stream, which keeps every run a pure function of (bars, inputs).

use crate::bar::IndicatorBar;
use crate::output::Rgba8;

/// A selectable price/flow series — the value set of a `source` input.
///
/// This is a closed enum rather than a string so a persisted state file or a
/// script that names an unknown source fails at load, not at bar one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceId {
    /// First trade price of the bar.
    Open,
    /// Highest trade price of the bar.
    High,
    /// Lowest trade price of the bar.
    Low,
    /// Last trade price of the bar.
    Close,
    /// `(high + low) / 2`.
    Hl2,
    /// `(high + low + close) / 3`.
    Hlc3,
    /// `(open + high + low + close) / 4`.
    Ohlc4,
    /// `(high + low + close + close) / 4`.
    Hlcc4,
    /// Total traded quantity.
    Volume,
    /// Buy volume minus sell volume.
    Delta,
    /// Taker-buy quantity.
    BuyVolume,
    /// Taker-sell quantity.
    SellVolume,
    /// Trades aggregated in the bar.
    TradeCount,
    /// Cumulative volume delta (host-maintained running sum of `delta`).
    Cvd,
}

impl SourceId {
    /// Every selectable source, for building a settings dropdown.
    pub const ALL: [SourceId; 14] = [
        SourceId::Open,
        SourceId::High,
        SourceId::Low,
        SourceId::Close,
        SourceId::Hl2,
        SourceId::Hlc3,
        SourceId::Ohlc4,
        SourceId::Hlcc4,
        SourceId::Volume,
        SourceId::Delta,
        SourceId::BuyVolume,
        SourceId::SellVolume,
        SourceId::TradeCount,
        SourceId::Cvd,
    ];

    /// Resolve this source against one bar. `cvd` is passed in because it is
    /// the one cross-bar series — the host maintains it, the bar cannot.
    #[must_use]
    pub fn value(self, bar: &IndicatorBar, cvd: f64) -> f64 {
        match self {
            SourceId::Open => bar.open,
            SourceId::High => bar.high,
            SourceId::Low => bar.low,
            SourceId::Close => bar.close,
            SourceId::Hl2 => bar.hl2(),
            SourceId::Hlc3 => bar.hlc3(),
            SourceId::Ohlc4 => bar.ohlc4(),
            SourceId::Hlcc4 => bar.hlcc4(),
            SourceId::Volume => bar.volume(),
            SourceId::Delta => bar.delta(),
            SourceId::BuyVolume => bar.buy_volume,
            SourceId::SellVolume => bar.sell_volume,
            SourceId::TradeCount => bar.trade_count,
            SourceId::Cvd => cvd,
        }
    }

    /// True when this series lives on the price scale — an overlay of it
    /// belongs on the price chart. Flow series (volume, delta, cvd, …) need
    /// their own pane; drawing them over price would be a scale lie.
    #[must_use]
    pub fn is_price_scaled(self) -> bool {
        matches!(
            self,
            SourceId::Open
                | SourceId::High
                | SourceId::Low
                | SourceId::Close
                | SourceId::Hl2
                | SourceId::Hlc3
                | SourceId::Ohlc4
                | SourceId::Hlcc4
        )
    }

    /// The script-facing / persistence name of this source.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SourceId::Open => "open",
            SourceId::High => "high",
            SourceId::Low => "low",
            SourceId::Close => "close",
            SourceId::Hl2 => "hl2",
            SourceId::Hlc3 => "hlc3",
            SourceId::Ohlc4 => "ohlc4",
            SourceId::Hlcc4 => "hlcc4",
            SourceId::Volume => "volume",
            SourceId::Delta => "delta",
            SourceId::BuyVolume => "buy_volume",
            SourceId::SellVolume => "sell_volume",
            SourceId::TradeCount => "trade_count",
            SourceId::Cvd => "cvd",
        }
    }
}

/// One declared input: its identity, label, default and constraints. The
/// settings UI is generated from this — every widget the user sees maps to
/// exactly one variant here.
#[derive(Debug, Clone, PartialEq)]
pub enum InputSpec {
    /// Whole-number input (`input.int`).
    Int {
        /// Stable identifier (persistence key, script argument name).
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: i64,
        /// Inclusive lower bound, if constrained.
        min: Option<i64>,
        /// Inclusive upper bound, if constrained.
        max: Option<i64>,
        /// UI increment, if constrained.
        step: Option<i64>,
        /// When non-empty, the value must be one of these (rendered as a
        /// dropdown instead of a numeric field).
        options: Vec<i64>,
    },
    /// Floating-point input (`input.float`).
    Float {
        /// Stable identifier (persistence key, script argument name).
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: f64,
        /// Inclusive lower bound, if constrained.
        min: Option<f64>,
        /// Inclusive upper bound, if constrained.
        max: Option<f64>,
        /// UI increment, if constrained.
        step: Option<f64>,
        /// When non-empty, the value must be one of these.
        options: Vec<f64>,
    },
    /// Checkbox (`input.bool`).
    Bool {
        /// Stable identifier.
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: bool,
    },
    /// Color picker (`input.color`).
    Color {
        /// Stable identifier.
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: Rgba8,
    },
    /// Text input, optionally constrained to fixed options (`input.string`).
    Str {
        /// Stable identifier.
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Value used until the user changes it.
        default: String,
        /// When non-empty, the value must be one of these.
        options: Vec<String>,
    },
    /// Series selector (`input.source`), including the flow series Pine
    /// doesn't have (`delta`, `cvd`, …).
    Source {
        /// Stable identifier.
        name: String,
        /// Label shown in the settings UI.
        title: String,
        /// Series used until the user changes it.
        default: SourceId,
    },
}

impl InputSpec {
    /// The stable identifier of this input.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            InputSpec::Int { name, .. }
            | InputSpec::Float { name, .. }
            | InputSpec::Bool { name, .. }
            | InputSpec::Color { name, .. }
            | InputSpec::Str { name, .. }
            | InputSpec::Source { name, .. } => name,
        }
    }

    /// The default as a value, for seeding a fresh instance's input set.
    #[must_use]
    pub fn default_value(&self) -> InputValue {
        match self {
            InputSpec::Int { default, .. } => InputValue::Int(*default),
            InputSpec::Float { default, .. } => InputValue::Float(*default),
            InputSpec::Bool { default, .. } => InputValue::Bool(*default),
            InputSpec::Color { default, .. } => InputValue::Color(*default),
            InputSpec::Str { default, .. } => InputValue::Str(default.clone()),
            InputSpec::Source { default, .. } => InputValue::Source(*default),
        }
    }
}

/// A concrete value for one declared input, same variant as its spec.
#[derive(Debug, Clone, PartialEq)]
pub enum InputValue {
    /// Value of an [`InputSpec::Int`].
    Int(i64),
    /// Value of an [`InputSpec::Float`].
    Float(f64),
    /// Value of an [`InputSpec::Bool`].
    Bool(bool),
    /// Value of an [`InputSpec::Color`].
    Color(Rgba8),
    /// Value of an [`InputSpec::Str`].
    Str(String),
    /// Value of an [`InputSpec::Source`].
    Source(SourceId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Bar;
    use rust_decimal::Decimal;

    fn bar() -> IndicatorBar {
        IndicatorBar::from(&Bar {
            open_time: 0,
            close_time: 1,
            open: Decimal::from(10),
            high: Decimal::from(20),
            low: Decimal::from(5),
            close: Decimal::from(15),
            buy_volume: Decimal::from(3),
            sell_volume: Decimal::from(1),
            trade_count: 4,
        })
    }

    #[test]
    fn every_source_resolves() {
        let b = bar();
        assert_eq!(SourceId::Open.value(&b, 0.0), 10.0);
        assert_eq!(SourceId::High.value(&b, 0.0), 20.0);
        assert_eq!(SourceId::Low.value(&b, 0.0), 5.0);
        assert_eq!(SourceId::Close.value(&b, 0.0), 15.0);
        assert_eq!(SourceId::Hl2.value(&b, 0.0), 12.5);
        assert_eq!(SourceId::Hlc3.value(&b, 0.0), 40.0 / 3.0);
        assert_eq!(SourceId::Ohlc4.value(&b, 0.0), 12.5);
        assert_eq!(SourceId::Hlcc4.value(&b, 0.0), 13.75);
        assert_eq!(SourceId::Volume.value(&b, 0.0), 4.0);
        assert_eq!(SourceId::Delta.value(&b, 0.0), 2.0);
        assert_eq!(SourceId::BuyVolume.value(&b, 0.0), 3.0);
        assert_eq!(SourceId::SellVolume.value(&b, 0.0), 1.0);
        assert_eq!(SourceId::TradeCount.value(&b, 0.0), 4.0);
        assert_eq!(SourceId::Cvd.value(&b, -7.5), -7.5);
    }

    #[test]
    fn defaults_seed_matching_values() {
        let spec = InputSpec::Int {
            name: "len".into(),
            title: "Length".into(),
            default: 9,
            min: Some(1),
            max: None,
            step: None,
            options: Vec::new(),
        };
        assert_eq!(spec.name(), "len");
        assert_eq!(spec.default_value(), InputValue::Int(9));

        let spec = InputSpec::Source {
            name: "src".into(),
            title: "Source".into(),
            default: SourceId::Close,
        };
        assert_eq!(spec.default_value(), InputValue::Source(SourceId::Close));
    }
}
