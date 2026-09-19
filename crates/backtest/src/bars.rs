//! Which bar rule a run uses, in the vocabulary the chart already speaks.
//!
//! The configuration is the engine's [`BarConfiguration`] — `tick:100`, `volume:5`,
//! `dollar:500000`, `time:1m`, `imbalance:volume:2500` — so the spec a trader
//! reads off a tab is the spec they hand to a backtest, and both cut bars
//! through the same registered factory. [`parse_runnable`] retains the closed
//! legacy enum API; the CLI uses [`parse_configuration`]. This module owns only
//! the harness boundary: missing deal-counter data is refused, never invented.

use std::fmt;

use quantick_engine::bar_registry::{
    BUILTIN_BARS, BarConfiguration, BarConfigurationError, BarRegistry,
};
use quantick_engine::bar_selection::BarInputAvailability;
pub use quantick_engine::{BarSpec, BarSpecError};

/// Why a spec the vocabulary accepts cannot be run here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// Not a spec at all; the vocabulary's own reason, whose `Display` names
    /// what was expected.
    Unparsable(BarSpecError),
    /// A deal-count (`trades:N`) spec. It cuts on the venue's deal counter,
    /// which no exported session or replay tape carries yet, so there is
    /// nothing to cut it on — refused for that reason, never approximated by a
    /// tick count.
    NeedsDealCounter(BarSpec),
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unparsable(reason) => reason.fmt(f),
            Self::NeedsDealCounter(spec) => write!(
                f,
                "{} counts the venue's deal counter, which no exported session carries \
                 yet; the chart cuts it live and from its own recording",
                spec.to_config_string()
            ),
        }
    }
}

impl std::error::Error for SpecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unparsable(reason) => Some(reason),
            Self::NeedsDealCounter(_) => None,
        }
    }
}

/// Parse a `kind:parameter` spec this harness can run.
///
/// # Errors
///
/// [`SpecError::Unparsable`] for text the vocabulary rejects, carrying the
/// vocabulary's own [`BarSpecError`]; [`SpecError::NeedsDealCounter`] for a
/// spec that counts deals.
pub fn parse_runnable(text: &str) -> Result<BarSpec, SpecError> {
    let spec = BarSpec::parse(text).map_err(SpecError::Unparsable)?;
    if BarInputAvailability::PRINTS
        .refusal(BarConfiguration::from(spec).requirements())
        .is_some()
    {
        return Err(SpecError::NeedsDealCounter(spec));
    }
    Ok(spec)
}

/// Refusals for the open registry API. The closed legacy error remains unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnableConfigurationError {
    Invalid(BarConfigurationError),
    NeedsDealCounter(BarConfiguration),
}
impl fmt::Display for RunnableConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(error) => error.fmt(f),
            Self::NeedsDealCounter(config) => write!(
                f,
                "{} counts the venue's deal counter, which no exported session carries yet; the chart cuts it live and from its own recording",
                config.to_config_string()
            ),
        }
    }
}
impl std::error::Error for RunnableConfigurationError {}

/// Resolve a runnable definition without passing through the legacy enum.
pub fn parse_with_registry(
    registry: &BarRegistry,
    text: &str,
) -> Result<BarConfiguration, RunnableConfigurationError> {
    let config = registry
        .parse(text)
        .map_err(RunnableConfigurationError::Invalid)?;
    if BarInputAvailability::PRINTS
        .refusal(config.requirements())
        .is_some()
    {
        return Err(RunnableConfigurationError::NeedsDealCounter(config));
    }
    Ok(config)
}

/// The CLI keeps the resolved configuration through the actual runner.
pub fn parse_configuration(text: &str) -> Result<BarConfiguration, RunnableConfigurationError> {
    parse_with_registry(&BUILTIN_BARS, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trades_is_refused_for_the_reading_it_lacks_not_as_unknown() {
        let refusal = parse_runnable("trades:2000").unwrap_err();
        assert_eq!(refusal, SpecError::NeedsDealCounter(BarSpec::Trades(2000)));
        let message = refusal.to_string();
        assert!(message.contains("deal counter"), "{message}");
        assert!(message.starts_with("trades:2000"), "{message}");
        assert!(!message.contains("unknown"), "{message}");
    }

    #[test]
    fn every_other_kind_runs_as_the_chart_parses_it() {
        for text in [
            "tick:100",
            "volume:5",
            "dollar:500000",
            "time:1m",
            "imbalance:2500",
            "imbalance:volume:500",
        ] {
            assert_eq!(
                parse_runnable(text),
                Ok(BarSpec::parse(text).unwrap()),
                "{text}"
            );
        }
    }

    #[test]
    fn runnable_boundary_preserves_decimal_scale_and_chart_time_bounds() {
        for (text, summary, config) in [
            ("volume:5.50", "volume(5.50)", "volume:5.50"),
            ("dollar:125.00", "dollar(125.00)", "dollar:125.00"),
        ] {
            let spec = parse_runnable(text).expect(text);
            assert_eq!(spec.summary(), summary, "{text}");
            assert_eq!(spec.to_config_string(), config, "{text}");
        }

        for boundary in ["time:100ms", "time:24h"] {
            let spec = parse_runnable(boundary).expect(boundary);
            assert_eq!(spec.to_config_string(), boundary, "{boundary}");
        }

        for (text, ms) in [("time:99ms", 99), ("time:86400001ms", 86_400_001)] {
            assert_eq!(
                parse_runnable(text),
                Err(SpecError::Unparsable(BarSpecError::IntervalOutOfRange {
                    ms,
                    param: text.trim_start_matches("time:").to_owned(),
                })),
                "{text}"
            );
        }
    }

    #[test]
    fn a_nonsense_spec_carries_the_vocabulary_message() {
        let refusal = parse_runnable("candles:5").unwrap_err();
        assert_eq!(
            refusal,
            SpecError::Unparsable(BarSpecError::UnknownKind {
                kind: "candles".to_owned()
            })
        );
        assert!(refusal.to_string().contains("tick"), "{refusal}");
    }
}
