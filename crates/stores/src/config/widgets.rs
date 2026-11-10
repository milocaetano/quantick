//! What the widget panes need to know about an asset that only the config
//! knows (`symbol_widgets`): its tick, what price times quantity is
//! counted in, and the Big Trades thresholds. A points-quoted future and a
//! dollar pair differ here, and no symbol in code says which is which.

use rust_decimal::Decimal;
use serde::Deserialize;

use super::{AppConfig, FeedConfig, declared};

/// The Big Trades thresholds an asset gets when its config names none.
pub const DEFAULT_BIG_TRADE_PRESETS: [u32; 3] = [50_000, 250_000, 1_000_000];

/// What price times quantity is counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotionalKind {
    /// Dollars or a dollar stablecoin: written `$250k`.
    Usd,
    /// Whatever the price is quoted in, points included: never called
    /// money. The default, because it claims nothing.
    #[default]
    Quote,
}

/// One asset's widget setup, keyed like `symbol_bubble_presets`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WidgetSetup {
    /// One price row of the Depth ladder, as a decimal string (`"0.01"`).
    /// Absent, the pane uses the order-flow engine's own grouping.
    #[serde(default)]
    pub tick: Option<String>,
    #[serde(default)]
    pub notional_unit: NotionalKind,
    /// The threshold chips Big Trades offers, in that unit.
    #[serde(default = "default_presets")]
    pub big_trade_presets: Vec<u32>,
    /// The threshold in force; absent, the first preset.
    #[serde(default)]
    pub big_trade_min: Option<u32>,
    /// Fold same-millisecond, same-side fills into one row.
    #[serde(default = "merge_by_default")]
    pub merge_fills: bool,
}

fn default_presets() -> Vec<u32> {
    DEFAULT_BIG_TRADE_PRESETS.to_vec()
}

fn merge_by_default() -> bool {
    true
}

impl Default for WidgetSetup {
    fn default() -> Self {
        Self {
            tick: None,
            notional_unit: NotionalKind::default(),
            big_trade_presets: default_presets(),
            big_trade_min: None,
            merge_fills: merge_by_default(),
        }
    }
}

impl WidgetSetup {
    /// The configured tick, when it is a positive decimal.
    #[must_use]
    pub fn tick(&self) -> Option<Decimal> {
        let tick: Decimal = self.tick.as_deref()?.trim().parse().ok()?;
        (tick > Decimal::ZERO).then_some(tick)
    }

    /// The Big Trades threshold in force.
    #[must_use]
    pub fn big_trade_min(&self) -> u32 {
        self.big_trade_min
            .or_else(|| self.big_trade_presets.first().copied())
            .unwrap_or(DEFAULT_BIG_TRADE_PRESETS[0])
    }
}

impl FeedConfig {
    /// `symbol`'s widget setup on this feed: the exact key, else the longest
    /// family pattern, else the defaults.
    #[must_use]
    pub fn widget_setup(&self, symbol: &str) -> WidgetSetup {
        declared(&self.symbol_widgets, symbol)
            .map(|(_, setup)| setup.clone())
            .unwrap_or_default()
    }

    pub(super) fn validate_widgets(&self) -> Result<(), String> {
        for (symbol, setup) in &self.symbol_widgets {
            if !self
                .symbols
                .iter()
                .any(|offered| super::key_matches(symbol, offered))
            {
                return Err(format!(
                    "feed '{}' sets widgets up for symbol '{symbol}', which matches none of \
                     the symbols it offers",
                    self.id
                ));
            }
            if setup.tick.is_some() && setup.tick().is_none() {
                return Err(format!(
                    "feed '{}' names a widget tick for '{symbol}' that is not a positive decimal",
                    self.id
                ));
            }
            if setup.big_trade_presets.is_empty() {
                return Err(format!(
                    "feed '{}' offers no Big Trades preset for '{symbol}'",
                    self.id
                ));
            }
        }
        Ok(())
    }
}

impl AppConfig {
    /// `symbol`'s widget setup on `feed_id`, and what a quantity on it
    /// counts (its asset mode's unit; empty without one).
    #[must_use]
    pub fn widget_setup(&self, feed_id: &str, symbol: &str) -> (WidgetSetup, String) {
        let feed = self.feed(feed_id);
        let setup = feed
            .map(|feed| feed.widget_setup(symbol))
            .unwrap_or_default();
        let mode = feed.and_then(|feed| feed.bubble_asset(symbol).mode);
        (
            setup,
            mode.map(|mode| mode.quantity_unit).unwrap_or_default(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(widgets: &str) -> FeedConfig {
        toml::from_str(&format!(
            "id = \"b3\"\nname = \"B3\"\nprovider = \"metatrader\"\n\
             symbols = [\"WINZ26\", \"WDOZ26\"]\nsymbol_widgets = {widgets}\n"
        ))
        .expect("fixture feed")
    }

    #[test]
    fn a_family_key_sets_up_its_contracts_and_others_take_the_defaults() {
        let feed = feed(
            "{ \"WIN*\" = { tick = \"5\", notional_unit = \"quote\", \
             big_trade_presets = [500000, 2000000] } }",
        );
        let win = feed.widget_setup("WINZ26");
        assert_eq!(win.tick(), Some(Decimal::from(5)));
        assert_eq!(win.notional_unit, NotionalKind::Quote);
        assert_eq!(win.big_trade_min(), 500_000);
        assert!(win.merge_fills);
        assert_eq!(feed.widget_setup("WDOZ26"), WidgetSetup::default());
        assert!(feed.validate_widgets().is_ok());
    }

    #[test]
    fn a_bad_tick_or_a_key_matching_nothing_is_refused() {
        assert!(
            feed("{ \"WIN*\" = { tick = \"-1\" } }")
                .validate_widgets()
                .is_err()
        );
        assert!(feed("{ \"BTC\" = { } }").validate_widgets().is_err());
        assert!(
            feed("{ \"WIN*\" = { big_trade_presets = [] } }")
                .validate_widgets()
                .is_err()
        );
    }
}
