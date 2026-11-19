//! [`FootprintBar`] — an engine [`Bar`] plus the summary of its footprint
//! ladder that scripts read, and [`HostBar`], what the host accepts.
//!
//! The summary is taken from a ladder the caller captured over the bar's own
//! trades. A bar that arrives without one — a candle chart with footprints
//! off, a backtest, a lane rung — carries no summary and scripts read `na`;
//! [`BarFootprint::approximated`] is never a source here, since a spread
//! inferred from the bar's summary is not a point of control that traded.

use quantick_engine::{Bar, BarFootprint};
use rust_decimal::Decimal;

use crate::bar::{IndicatorBar, to_f64};

/// An engine bar with its footprint summary.
#[derive(Debug, Clone, PartialEq)]
pub struct FootprintBar {
    /// The bar itself.
    pub bar: Bar,
    /// Price of the ladder's point of control (`poc` in scripts): the lower
    /// edge of its heaviest row, at the ladder's row width. `None` when the
    /// bar has no footprint or its ladder holds no volume.
    pub poc: Option<Decimal>,
}

impl FootprintBar {
    /// Summarise `footprint`, the ladder of exactly the trades in `bar`.
    #[must_use]
    pub fn new(bar: Bar, footprint: Option<&BarFootprint>) -> Self {
        let poc = footprint.and_then(|ladder| ladder.poc().map(|row| ladder.bucket_price(row)));
        Self { bar, poc }
    }
}

impl From<Bar> for FootprintBar {
    /// A bar with no footprint: every summary field reads `na`.
    fn from(bar: Bar) -> Self {
        Self { bar, poc: None }
    }
}

impl From<&FootprintBar> for IndicatorBar {
    fn from(bar: &FootprintBar) -> Self {
        Self {
            poc: bar.poc.map(to_f64),
            ..Self::from(&bar.bar)
        }
    }
}

/// A bar the [`IndicatorHost`](crate::IndicatorHost) can commit or preview:
/// projected once, at the crate boundary, into an [`IndicatorBar`].
pub trait HostBar {
    /// The f64 projection scripts and natives read.
    fn project(&self) -> IndicatorBar;
}

impl HostBar for Bar {
    fn project(&self) -> IndicatorBar {
        IndicatorBar::from(self)
    }
}

impl HostBar for FootprintBar {
    fn project(&self) -> IndicatorBar {
        IndicatorBar::from(self)
    }
}

#[cfg(test)]
mod tests;
