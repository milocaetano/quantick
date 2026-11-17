//! How the chrome names the band an object lives on: the manager's chip,
//! the inspector's hint, and the reason an object is not on screen.

use std::sync::Arc;

use quantick_chart::indicator_views::{IndicatorViews, PaneSizing};

use super::{Mark, pane_key};
use crate::DrawingBand;

/// How the chrome names the band an object lives on.
pub enum BandLabel {
    /// The candles. Says nothing anywhere: it is where drawings have always
    /// been, and a chip on every existing object would be noise.
    Price,
    /// A time-only object, which crosses every band.
    AllBands,
    /// One indicator pane, by the name that pane shows.
    Indicator(Arc<str>),
    /// Its pane exists but is not painting right now — collapsed, hidden or
    /// errored. The object is where it was put; the reason it is invisible is
    /// the second field, said in the trader's own words.
    Unpainted(Arc<str>, &'static str),
    /// The indicator it annotates is not on this chart right now. The object
    /// is kept, not deleted, and re-adopted when that indicator comes back.
    Parked(Arc<str>),
}

impl BandLabel {
    /// The short word the manager and the inspector print, if any.
    #[must_use]
    pub fn chip(&self) -> Option<String> {
        match self {
            BandLabel::Price => None,
            BandLabel::AllBands => Some("all bands".to_owned()),
            BandLabel::Indicator(label) => Some(label.to_string()),
            BandLabel::Unpainted(label, _) => Some(format!("{label} - not painted")),
            // Named by kind, because the instance is gone: `native.cvd` reads
            // as "cvd", a script as its file name.
            BandLabel::Parked(kind) => Some(format!(
                "not on chart - {}",
                kind.rsplit('.').next().unwrap_or(kind)
            )),
        }
    }

    /// Why the object is not on screen, when that is the case.
    #[must_use]
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            BandLabel::Parked(_) => Some(
                "The indicator this was drawn on is not on the chart. Add it back and the                  object returns at the same value.",
            ),
            BandLabel::Unpainted(_, reason) => Some(reason),
            _ => None,
        }
    }
}

/// How the chrome names the band `drawing` lives on, given the indicators
/// on its chart right now.
#[must_use]
pub fn label_for(indicators: &IndicatorViews, drawing: &Mark) -> BandLabel {
    match &drawing.band {
        DrawingBand::Price => BandLabel::Price,
        DrawingBand::AllBands => BandLabel::AllBands,
        DrawingBand::Indicator(key) => indicators
            .all()
            .iter()
            .filter(|view| !view.descriptor.overlay)
            .find(|view| &pane_key(view) == key)
            .map_or_else(
                || BandLabel::Parked(Arc::clone(&key.kind)),
                |view| {
                    // A band drawing can stop painting for four different
                    // reasons, and only one of them is "the indicator is
                    // gone". A mark that vanishes unexplained is what
                    // teaches a trader to stop trusting the tool, so each
                    // reason says its own name.
                    match () {
                        () if view.error.is_some() => BandLabel::Unpainted(
                            view.label_shared(),
                            "this indicator is in error, so its pane and everything drawn on                                  it are not being painted",
                        ),
                        () if view.hidden => BandLabel::Unpainted(
                            view.label_shared(),
                            "this indicator is hidden - show it again and the object comes                                  back with it",
                        ),
                        () if view.sizing == PaneSizing::Collapsed => BandLabel::Unpainted(
                            view.label_shared(),
                            "this pane is collapsed - open it with the chevron and the object                                  is there",
                        ),
                        () => BandLabel::Indicator(view.label_shared()),
                    }
                },
            ),
    }
}

#[cfg(test)]
#[path = "band_label_tests.rs"]
mod tests;
