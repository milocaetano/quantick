//! Which bar rule a run uses, as a string a person can type.
//!
//! The vocabulary is deliberately the one the chart already speaks —
//! `tick:100`, `volume:5`, `dollar:500000`, `time:1m`, `imbalance:2500`,
//! `imbalance:volume:2500`, `imbalance:dollar:2500` —
//! so the spec a trader reads off a tab is the spec they hand to a backtest.
//! It is re-implemented here rather than shared because the chart's copy
//! lives in `quantick-app`, and a headless harness that linked the desktop
//! app to parse five words would be exactly the coupling this crate exists to
//! disprove. Folding one `BarSpec` into `quantick-engine` and having both
//! consumers read it is the right eventual home; that is a change to the
//! engine's public surface, not a line item of this harness.

use quantick_engine::{
    BarBuilder, DollarBarBuilder, ImbalanceBarBuilder, ImbalanceUnit, TickBarBuilder,
    TimeBarBuilder, VolumeBarBuilder,
};
use rust_decimal::Decimal;

/// A bar rule and its parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarSpec {
    /// N trades per bar.
    Tick(u64),
    /// N units of quantity per bar.
    Volume(Decimal),
    /// N notional per bar.
    Dollar(Decimal),
    /// N milliseconds per bar.
    Time(i64),
    /// The adaptive imbalance rule: what θ accumulates (trades, volume or
    /// dollar) and the target trades per bar, which counts trades in every
    /// unit.
    Imbalance(ImbalanceUnit, u64),
}

impl BarSpec {
    /// Construct the matching engine builder — the same dispatch the chart
    /// performs, over the same builders.
    #[must_use]
    pub fn build(self) -> Box<dyn BarBuilder> {
        match self {
            Self::Tick(n) => Box::new(TickBarBuilder::new(n)),
            Self::Volume(units) => Box::new(VolumeBarBuilder::new(units)),
            Self::Dollar(notional) => Box::new(DollarBarBuilder::new(notional)),
            Self::Time(ms) => Box::new(TimeBarBuilder::new(ms)),
            Self::Imbalance(unit, target) => Box::new(ImbalanceBarBuilder::with_unit(target, unit)),
        }
    }

    /// Parse `kind:parameter`.
    ///
    /// # Errors
    ///
    /// Returns a message naming what was wrong and what the accepted forms
    /// are — it is printed straight to the operator.
    pub fn parse(text: &str) -> Result<Self, String> {
        let Some((kind, param)) = text.split_once(':') else {
            return Err(format!(
                "'{text}' is not a kind:parameter bar spec, like 'tick:100' or 'time:1m'"
            ));
        };
        let (kind, param) = (kind.trim(), param.trim());
        let positive_count = |what: &str| -> Result<u64, String> {
            match param.parse::<u64>() {
                Ok(n) if n > 0 => Ok(n),
                _ => Err(format!(
                    "{what} bars need a positive whole number, got '{param}'"
                )),
            }
        };
        let positive_decimal = |what: &str| -> Result<Decimal, String> {
            match param.parse::<Decimal>() {
                Ok(d) if d > Decimal::ZERO => Ok(d),
                _ => Err(format!("{what} bars need a positive number, got '{param}'")),
            }
        };
        match kind {
            "tick" => Ok(Self::Tick(positive_count("tick")?)),
            "imbalance" => {
                // The parameter is `target` or `unit:target` — the same two
                // forms the chart accepts. The unit picks what θ accumulates;
                // the target counts trades in every unit.
                let (unit, target) = match param.split_once(':') {
                    None => (ImbalanceUnit::Trades, param),
                    Some((unit, target)) => {
                        let unit = match unit.trim() {
                            "trades" => ImbalanceUnit::Trades,
                            "volume" => ImbalanceUnit::Volume,
                            "dollar" => ImbalanceUnit::Dollar,
                            other => {
                                return Err(format!(
                                    "unknown imbalance unit '{other}'; one of trades, \
                                     volume, dollar"
                                ));
                            }
                        };
                        (unit, target.trim())
                    }
                };
                match target.parse::<u64>() {
                    Ok(n) if n > 0 => Ok(Self::Imbalance(unit, n)),
                    _ => Err(format!(
                        "imbalance bars need a positive whole trade target, got '{target}'"
                    )),
                }
            }
            "volume" => Ok(Self::Volume(positive_decimal("volume")?)),
            "dollar" => Ok(Self::Dollar(positive_decimal("dollar")?)),
            "time" => Ok(Self::Time(parse_interval(param)?)),
            _ => Err(format!(
                "unknown bar kind '{kind}'; one of tick, volume, dollar, time, imbalance"
            )),
        }
    }

    /// A human-readable summary, e.g. `tick(100)`.
    #[must_use]
    pub fn summary(self) -> String {
        match self {
            Self::Tick(n) => format!("tick({n})"),
            Self::Volume(u) => format!("volume({})", u.normalize()),
            Self::Dollar(d) => format!("dollar({})", d.normalize()),
            Self::Time(ms) => format!("time({})", fmt_interval(ms)),
            Self::Imbalance(ImbalanceUnit::Trades, target) => format!("imbalance({target})"),
            Self::Imbalance(ImbalanceUnit::Volume, target) => format!("imbalance(volume {target})"),
            Self::Imbalance(ImbalanceUnit::Dollar, target) => format!("imbalance(dollar {target})"),
        }
    }
}

/// `1h`, `5m`, `90s`, `1500ms`, or a bare millisecond count.
fn parse_interval(text: &str) -> Result<i64, String> {
    let scaled = |digits: &str, scale: i64| -> Result<i64, String> {
        digits
            .parse::<i64>()
            .ok()
            .and_then(|n| n.checked_mul(scale))
            .filter(|ms| *ms > 0)
            .ok_or_else(|| format!("time interval '{text}' is not a positive duration"))
    };
    if let Some(digits) = text.strip_suffix("ms") {
        scaled(digits, 1)
    } else if let Some(digits) = text.strip_suffix('s') {
        scaled(digits, 1_000)
    } else if let Some(digits) = text.strip_suffix('m') {
        scaled(digits, 60_000)
    } else if let Some(digits) = text.strip_suffix('h') {
        scaled(digits, 3_600_000)
    } else {
        scaled(text, 1)
    }
}

/// The inverse of [`parse_interval`] for the coarsest unit that divides.
fn fmt_interval(ms: i64) -> String {
    match ms {
        ms if ms % 3_600_000 == 0 => format!("{}h", ms / 3_600_000),
        ms if ms % 60_000 == 0 => format!("{}m", ms / 60_000),
        ms if ms % 1_000 == 0 => format!("{}s", ms / 1_000),
        ms => format!("{ms}ms"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_its_summary() {
        let cases = [
            ("tick:100", BarSpec::Tick(100)),
            (
                "imbalance:2500",
                BarSpec::Imbalance(ImbalanceUnit::Trades, 2500),
            ),
            (
                "imbalance:trades:2500",
                BarSpec::Imbalance(ImbalanceUnit::Trades, 2500),
            ),
            (
                "imbalance:volume:500",
                BarSpec::Imbalance(ImbalanceUnit::Volume, 500),
            ),
            (
                "imbalance:dollar:2500",
                BarSpec::Imbalance(ImbalanceUnit::Dollar, 2500),
            ),
            ("volume:5", BarSpec::Volume(Decimal::from(5u64))),
            ("dollar:500000", BarSpec::Dollar(Decimal::from(500_000u64))),
            ("time:1m", BarSpec::Time(60_000)),
            ("time:90s", BarSpec::Time(90_000)),
            ("time:1500ms", BarSpec::Time(1_500)),
            ("time:1h", BarSpec::Time(3_600_000)),
        ];
        for (text, expected) in cases {
            assert_eq!(BarSpec::parse(text), Ok(expected), "parsing {text}");
        }
        assert_eq!(BarSpec::Time(90_000).summary(), "time(90s)");
        assert_eq!(BarSpec::Time(60_000).summary(), "time(1m)");
        assert_eq!(BarSpec::Tick(100).summary(), "tick(100)");
    }

    #[test]
    fn a_nonsense_spec_says_what_was_expected() {
        // The message is the whole point: an operator who typed it wrong
        // gets the vocabulary back, not "invalid input".
        for text in ["tick", "tick:0", "tick:-1", "candles:5", "time:0", "time:x"] {
            let message = BarSpec::parse(text).expect_err("rejected");
            assert!(!message.is_empty(), "{text} produced an empty complaint");
        }
        assert!(
            BarSpec::parse("candles:5")
                .expect_err("rejected")
                .contains("tick")
        );
    }
}
