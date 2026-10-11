//! A band: one value axis of one chart pane, as the gestures read it.
//!
//! The window carves a pane into bands each frame (the candles' price band,
//! plus one per expanded indicator pane); drawings are placed, projected,
//! picked and painted band by band, so a CVD level is a CVD level everywhere
//! it is touched. A band's drawings and its curve project through the *same*
//! `PriceScale`, built from the range the renderer actually drew.

use std::sync::Arc;

use quantick_chart::indicator_views::{IndicatorView, MAX_PANES};
use smallvec::SmallVec;

use super::Mark;
use crate::chart::PriceScale;
use crate::{DrawingBand, PaneKey, Px, PxRect, ValueUnit};

/// Most bands a chart pane can carry: the candles plus every indicator pane.
pub const MAX_BANDS: usize = 1 + MAX_PANES;

/// The carve's own container. Inline for every real chart, so a per-frame
/// carve never touches the heap for the bands themselves.
pub type Bands = SmallVec<[Band; MAX_BANDS]>;

/// One value axis of one chart pane.
pub struct Band {
    /// The identity an object anchored here stores.
    pub key: DrawingBand,
    /// The drawable rows: the band's own height, x-clipped at the live lane.
    pub rect: PxRect,
    /// The scale this band's curve was drawn with. `None` while the band has
    /// nothing to project against, which is exactly when it refuses.
    pub scale: Option<PriceScale>,
    /// What the band is called, for readouts and the inspector title.
    ///
    /// Shared, not cloned: the label is stable data and the carve runs twice
    /// per pane per frame (input pass and draw pass).
    pub label: Arc<str>,
    /// Why the band refuses an anchor right now, if it does.
    pub refusal: Option<&'static str>,
}

impl Band {
    /// Whether an anchor may be placed here at all.
    #[must_use]
    pub fn drawable(&self) -> bool {
        self.refusal.is_none() && self.scale.is_some()
    }

    /// What this band's second coordinate measures — the difference between
    /// a ruler that says `pts` and one that says `CVD`.
    #[must_use]
    pub fn unit(&self) -> ValueUnit<'_> {
        match self.key {
            DrawingBand::Price => ValueUnit::Price,
            // A time-only object carries no value, so the unit it is painted
            // with is never read; naming the band it is passing through is
            // the honest answer either way.
            DrawingBand::Indicator(_) | DrawingBand::AllBands => ValueUnit::Indicator(&self.label),
        }
    }
}

/// The durable identity of the pane `view` draws in: the band key an
/// object anchored there stores.
///
/// Both halves are fixed at birth, so nothing a trader does to the *other*
/// indicators — hiding one, collapsing one, removing one — can move a
/// drawing from the pane it was placed on.
#[must_use]
pub fn pane_key(view: &IndicatorView) -> PaneKey {
    PaneKey {
        kind: Arc::clone(&view.kind),
        ordinal: view.ordinal,
    }
}

/// The band under a pointer, drawable or not — a refusing band still owns its
/// pixels, which is what lets the cursor explain the refusal.
#[must_use]
pub fn band_at(bands: &[Band], pos: Px) -> Option<&Band> {
    bands.iter().find(|band| band.rect.contains(pos))
}

/// The band an existing object is anchored to, when it is on the chart right
/// now. `None` is the parked case: the indicator it annotates is not on this
/// chart, so the object neither paints nor hit-tests — and it is not deleted
/// either (the manager still lists it).
#[must_use]
pub fn band_of<'a>(bands: &'a [Band], drawing: &Mark) -> Option<&'a Band> {
    match &drawing.band {
        // A time-only object has no value axis, so it has no home band;
        // callers that need one take the first, and painters take them all.
        DrawingBand::AllBands => bands.first(),
        key => bands.iter().find(|band| &band.key == key),
    }
}

/// Whether `drawing` takes part in `band`: its own band, or any band at all
/// when it is time-only. Hit-testing never crosses bands — a price trend line
/// and a CVD trend line can be one pixel apart on screen and mean unrelated
/// things.
#[must_use]
pub fn drawing_in_band(drawing: &Mark, band: &Band) -> bool {
    drawing.band == DrawingBand::AllBands || drawing.band == band.key
}

/// The magnet's candidates on an indicator band: every value the pane plots
/// at this bar, plus zero.
///
/// Zero earns its place because so many pane series are signed — a CVD zero
/// line is the single most-drawn level on the band, and eyeballing it against
/// the pane's own zero rule is exactly the imprecision the magnet exists to
/// remove. Out-of-reach candidates snap nothing, as on the candles.
#[must_use]
pub fn magnet_value_of(
    view: &IndicatorView,
    row: usize,
    pointer_y: f32,
    scale: &PriceScale,
    reach_px: f32,
) -> Option<f64> {
    view.columns
        .iter()
        .filter_map(|column| column.get(row).copied())
        .chain(std::iter::once(0.0))
        .filter(|value| value.is_finite())
        .filter_map(|value| {
            let distance = (scale.y(value) - pointer_y).abs();
            (distance <= reach_px).then_some((distance, value))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, value)| value)
}
