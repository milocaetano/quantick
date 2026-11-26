//! What a simulated fill costs on an asset (`symbol_costs`): the paper
//! account's commission, slippage and limit-fill rule. Measured in the
//! asset's tick from `symbol_widgets`, so one tick is declared once.

use quantick_trading::{Commission, FillCosts};
use rust_decimal::Decimal;
use serde::Deserialize;

use super::{AppConfig, FeedConfig, declared};

/// What one commission figure is counted against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommissionBasis {
    /// Points per contract, per side.
    #[default]
    Contract,
    /// A fraction of the fill's notional, per side.
    Notional,
}

/// One asset's fill costs, keyed like `symbol_bubble_presets`. Every field
/// is optional and an absent table is free.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CostSetup {
    /// The commission per side, as a decimal string: points per contract,
    /// or a fraction of notional (`"0.0004"`), as `commission_basis` says.
    #[serde(default)]
    pub commission: Option<String>,
    #[serde(default)]
    pub commission_basis: CommissionBasis,
    /// Ticks a market, stop or close fill lands against the trader.
    #[serde(default)]
    pub slippage_ticks: u32,
    /// A limit or take profit fills only on a print one tick through.
    #[serde(default)]
    pub conservative_limits: bool,
}

impl CostSetup {
    /// These costs measured in `tick`.
    fn fill_costs(&self, tick: Option<Decimal>) -> Result<FillCosts, String> {
        let commission = match self.commission.as_deref() {
            None => Commission::Free,
            Some(text) => {
                let rate: Decimal = text
                    .trim()
                    .parse()
                    .map_err(|_| format!("commission `{text}` is not a decimal"))?;
                match self.commission_basis {
                    CommissionBasis::Contract => Commission::PerContract(rate),
                    CommissionBasis::Notional => Commission::PerNotional(rate),
                }
            }
        };
        FillCosts::new(
            commission,
            self.slippage_ticks,
            tick,
            self.conservative_limits,
        )
        .map_err(|error| error.to_string())
    }
}

impl FeedConfig {
    /// What a paper fill of `symbol` costs on this feed: the exact key, else
    /// the longest family pattern, else free. The tick is the one
    /// `symbol_widgets` declares for the costs key itself, the same lookup
    /// [`Self::validate_costs`] proves at load, so a declared table never
    /// degrades to free for a symbol the key matches.
    #[must_use]
    pub fn fill_costs(&self, symbol: &str) -> FillCosts {
        declared(&self.symbol_costs, symbol)
            .and_then(|(key, setup)| setup.fill_costs(self.widget_tick(key)).ok())
            .unwrap_or_default()
    }

    pub(super) fn validate_costs(&self) -> Result<(), String> {
        for (symbol, setup) in &self.symbol_costs {
            if !self
                .symbols
                .iter()
                .any(|offered| super::key_matches(symbol, offered))
            {
                return Err(format!(
                    "feed '{}' prices fills for symbol '{symbol}', which matches none of the \
                     symbols it offers",
                    self.id
                ));
            }
            setup
                .fill_costs(self.widget_tick(symbol))
                .map_err(|error| {
                    format!(
                        "feed '{}' symbol_costs for '{symbol}': {error} (the tick is \
                     symbol_widgets' `tick` for the same key)",
                        self.id
                    )
                })?;
        }
        Ok(())
    }
}

impl AppConfig {
    /// What a paper fill of `symbol` costs on `feed_id`; free on a feed the
    /// config does not know.
    #[must_use]
    pub fn fill_costs(&self, feed_id: &str, symbol: &str) -> FillCosts {
        self.feed(feed_id)
            .map(|feed| feed.fill_costs(symbol))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(tables: &str) -> FeedConfig {
        toml::from_str(&format!(
            "id = \"b3\"\nname = \"B3\"\nprovider = \"metatrader\"\n\
             symbols = [\"WINZ26\", \"WDOZ26\"]\n{tables}"
        ))
        .expect("test feed parses")
    }

    fn dec(text: &str) -> Decimal {
        text.parse().expect("test decimal")
    }

    #[test]
    fn a_family_key_prices_its_contracts_in_the_widget_tick() {
        let feed = feed(
            "symbol_widgets = { \"WIN*\" = { tick = \"5\" } }\n\
             symbol_costs = { \"WIN*\" = { commission = \"0.25\", slippage_ticks = 1, \
             conservative_limits = true } }",
        );
        feed.validate_costs().expect("valid costs");
        assert_eq!(
            feed.fill_costs("WINZ26"),
            FillCosts::new(
                Commission::PerContract(dec("0.25")),
                1,
                Some(dec("5")),
                true
            )
            .expect("valid costs")
        );
        assert!(
            feed.fill_costs("WDOZ26") == FillCosts::default(),
            "an undeclared asset is free"
        );
    }

    #[test]
    fn a_notional_commission_needs_no_tick() {
        let feed = feed(
            "symbol_costs = { \"WDOZ26\" = { commission = \"0.0004\", \
             commission_basis = \"notional\" } }",
        );
        feed.validate_costs().expect("valid costs");
        assert_eq!(
            feed.fill_costs("WDOZ26"),
            FillCosts::new(Commission::PerNotional(dec("0.0004")), 0, None, false)
                .expect("valid costs")
        );
    }

    #[test]
    fn a_family_key_takes_its_own_tick_even_where_an_exact_widget_entry_has_none() {
        let feed = feed(
            "symbol_widgets = { \"WIN*\" = { tick = \"5\" }, \"WINZ26\" = { big_trade_min = 50000 } }\n\
             symbol_costs = { \"WIN*\" = { slippage_ticks = 1 } }",
        );
        feed.validate_costs().expect("valid costs");
        assert_eq!(
            feed.fill_costs("WINZ26"),
            FillCosts::new(Commission::Free, 1, Some(dec("5")), false).expect("valid costs"),
            "never silently free"
        );
    }

    #[test]
    fn slippage_without_a_widget_tick_is_refused_at_load() {
        let error = feed("symbol_costs = { \"WINZ26\" = { slippage_ticks = 2 } }")
            .validate_costs()
            .expect_err("slippage needs a tick");
        assert!(error.contains("symbol_widgets"), "{error}");
    }

    #[test]
    fn a_bad_commission_and_an_unknown_key_are_refused_at_load() {
        assert!(
            feed("symbol_costs = { \"WINZ26\" = { commission = \"-1\" } }")
                .validate_costs()
                .is_err()
        );
        assert!(
            feed("symbol_costs = { \"WINZ26\" = { commission = \"cheap\" } }")
                .validate_costs()
                .is_err()
        );
        assert!(
            feed("symbol_costs = { \"PETR4\" = { commission = \"1\" } }")
                .validate_costs()
                .is_err()
        );
    }
}
