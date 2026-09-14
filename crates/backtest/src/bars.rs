//! Which bar rule a run uses, in the vocabulary the chart already speaks.
//!
//! The spec is the engine's [`BarSpec`] — `tick:100`, `volume:5`,
//! `dollar:500000`, `time:1m`, `imbalance:volume:2500` — so the spec a trader
//! reads off a tab is the spec they hand to a backtest, and both cut bars
//! through the same [`BarSpec::build`]. What lives here is only the harness's
//! own boundary: the one kind it cannot run, refused by name.

use std::fmt;

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
    if spec.kind().needs_deal_counter() {
        return Err(SpecError::NeedsDealCounter(spec));
    }
    Ok(spec)
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
