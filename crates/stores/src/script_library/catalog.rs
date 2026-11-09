//! Embedded Pine assets and their stable file identities.

/// Embedded starter scripts: (menu name, source). Each doubles as a
/// conformance fixture in the pine crate's corpus.
pub const EMBEDDED_SCRIPTS: &[(&str, &str)] = &[
    ("ema.pine", include_str!("../../../app/scripts/ema.pine")),
    ("cvd.pine", include_str!("../../../app/scripts/cvd.pine")),
    (
        "delta_histogram.pine",
        include_str!("../../../app/scripts/delta_histogram.pine"),
    ),
    (
        "vwap_cumulative.pine",
        include_str!("../../../app/scripts/vwap_cumulative.pine"),
    ),
    (
        "zigzag.pine",
        include_str!("../../../app/scripts/zigzag.pine"),
    ),
    (
        "range_box.pine",
        include_str!("../../../app/scripts/range_box.pine"),
    ),
    (
        "copilot.pine",
        include_str!("../../../app/scripts/copilot.pine"),
    ),
    (
        "force_bar.pine",
        include_str!("../../../app/scripts/force_bar.pine"),
    ),
    (
        "exhaustion_reversal.pine",
        include_str!("../../../app/scripts/exhaustion_reversal.pine"),
    ),
    ("sma.pine", include_str!("../../../app/scripts/sma.pine")),
    ("wma.pine", include_str!("../../../app/scripts/wma.pine")),
    ("vwma.pine", include_str!("../../../app/scripts/vwma.pine")),
    ("hma.pine", include_str!("../../../app/scripts/hma.pine")),
    ("rma.pine", include_str!("../../../app/scripts/rma.pine")),
    ("rsi.pine", include_str!("../../../app/scripts/rsi.pine")),
    (
        "stochastic.pine",
        include_str!("../../../app/scripts/stochastic.pine"),
    ),
    (
        "stochastic_rsi.pine",
        include_str!("../../../app/scripts/stochastic_rsi.pine"),
    ),
    ("macd.pine", include_str!("../../../app/scripts/macd.pine")),
    (
        "bollinger_bands.pine",
        include_str!("../../../app/scripts/bollinger_bands.pine"),
    ),
    ("atr.pine", include_str!("../../../app/scripts/atr.pine")),
    ("adx.pine", include_str!("../../../app/scripts/adx.pine")),
    ("cci.pine", include_str!("../../../app/scripts/cci.pine")),
    ("roc.pine", include_str!("../../../app/scripts/roc.pine")),
    (
        "momentum.pine",
        include_str!("../../../app/scripts/momentum.pine"),
    ),
    (
        "williams_r.pine",
        include_str!("../../../app/scripts/williams_r.pine"),
    ),
    ("obv.pine", include_str!("../../../app/scripts/obv.pine")),
    ("mfi.pine", include_str!("../../../app/scripts/mfi.pine")),
    ("cmf.pine", include_str!("../../../app/scripts/cmf.pine")),
    (
        "donchian_channels.pine",
        include_str!("../../../app/scripts/donchian_channels.pine"),
    ),
    (
        "keltner_channels.pine",
        include_str!("../../../app/scripts/keltner_channels.pine"),
    ),
];

/// Friendly catalog metadata; `id` remains the existing file identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptMetadata {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub description: &'static str,
}

/// The exact 22 distinct market indicators; no presets or aliases.
pub const POPULAR_INDICATORS: &[ScriptMetadata] = &[
    ScriptMetadata {
        id: "sma.pine",
        title: "Simple Moving Average",
        category: "Moving averages",
        description: "Arithmetic average of closing prices.",
    },
    ScriptMetadata {
        id: "ema.pine",
        title: "Exponential Moving Average",
        category: "Moving averages",
        description: "Exponential smoothing with an SMA seed.",
    },
    ScriptMetadata {
        id: "wma.pine",
        title: "Weighted Moving Average",
        category: "Moving averages",
        description: "Linear weights favor the most recent price.",
    },
    ScriptMetadata {
        id: "vwma.pine",
        title: "Volume-Weighted Moving Average",
        category: "Moving averages",
        description: "Average price weighted by traded volume.",
    },
    ScriptMetadata {
        id: "hma.pine",
        title: "Hull Moving Average",
        category: "Moving averages",
        description: "Weighted averages reduce smoothing lag.",
    },
    ScriptMetadata {
        id: "rma.pine",
        title: "Wilder Moving Average",
        category: "Moving averages",
        description: "Wilder smoothing with an SMA seed.",
    },
    ScriptMetadata {
        id: "rsi.pine",
        title: "Relative Strength Index",
        category: "Momentum",
        description: "Relative Wilder-smoothed gains and losses.",
    },
    ScriptMetadata {
        id: "stochastic.pine",
        title: "Stochastic Oscillator",
        category: "Momentum",
        description: "Smoothed close position in the high-low range.",
    },
    ScriptMetadata {
        id: "stochastic_rsi.pine",
        title: "Stochastic RSI",
        category: "Momentum",
        description: "Smoothed RSI position in its recent range.",
    },
    ScriptMetadata {
        id: "macd.pine",
        title: "MACD",
        category: "Momentum",
        description: "Fast and slow EMA difference, signal and histogram.",
    },
    ScriptMetadata {
        id: "bollinger_bands.pine",
        title: "Bollinger Bands",
        category: "Volatility",
        description: "SMA with population standard deviation bands.",
    },
    ScriptMetadata {
        id: "atr.pine",
        title: "Average True Range",
        category: "Volatility",
        description: "Wilder-smoothed true range including price gaps.",
    },
    ScriptMetadata {
        id: "adx.pine",
        title: "Directional Movement / ADX",
        category: "Trend",
        description: "Directional movement and smoothed trend strength.",
    },
    ScriptMetadata {
        id: "cci.pine",
        title: "Commodity Channel Index",
        category: "Momentum",
        description: "Typical price distance measured in mean deviations.",
    },
    ScriptMetadata {
        id: "roc.pine",
        title: "Rate of Change",
        category: "Momentum",
        description: "Percentage price change over the selected lookback.",
    },
    ScriptMetadata {
        id: "momentum.pine",
        title: "Momentum",
        category: "Momentum",
        description: "Absolute price change over the selected lookback.",
    },
    ScriptMetadata {
        id: "williams_r.pine",
        title: "Williams %R",
        category: "Momentum",
        description: "Negative close distance from the window high.",
    },
    ScriptMetadata {
        id: "obv.pine",
        title: "On-Balance Volume",
        category: "Volume",
        description: "Volume accumulated by closing price direction.",
    },
    ScriptMetadata {
        id: "mfi.pine",
        title: "Money Flow Index",
        category: "Volume",
        description: "Relative positive and negative typical-price money flow.",
    },
    ScriptMetadata {
        id: "cmf.pine",
        title: "Chaikin Money Flow",
        category: "Volume",
        description: "Volume-weighted close position within each bar.",
    },
    ScriptMetadata {
        id: "donchian_channels.pine",
        title: "Donchian Channels",
        category: "Channels",
        description: "Rolling high and low with their midpoint.",
    },
    ScriptMetadata {
        id: "keltner_channels.pine",
        title: "Keltner Channels",
        category: "Channels",
        description: "EMA center with configurable ATR bands.",
    },
];

/// Display title for a stable identity, also valid for user files.
pub fn script_title(id: &str) -> &str {
    POPULAR_INDICATORS
        .iter()
        .find(|entry| entry.id == id)
        .map_or(id, |entry| entry.title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popular_catalog_has_exactly_twenty_two_distinct_embedded_scripts() {
        assert_eq!(POPULAR_INDICATORS.len(), 22);
        let mut ids = std::collections::BTreeSet::new();
        for metadata in POPULAR_INDICATORS {
            assert!(ids.insert(metadata.id), "duplicate {}", metadata.id);
            assert!(EMBEDDED_SCRIPTS.iter().any(|(id, _)| *id == metadata.id));
            assert!(!metadata.description.is_empty());
            assert_eq!(script_title(metadata.id), metadata.title);
        }
        assert_eq!(
            EMBEDDED_SCRIPTS.len(),
            30,
            "nine originals plus 21 new assets"
        );
        assert_eq!(script_title("my-indicator.pine"), "my-indicator.pine");
    }
}
