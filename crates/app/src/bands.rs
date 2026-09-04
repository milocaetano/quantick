//! Bands: the value axes of one chart pane, carved once per frame.
//!
//! A *band* is a region of a chart pane that owns a value axis — the candles'
//! price band, plus one per expanded indicator pane. Drawings are placed,
//! projected, picked and painted band by band, so a CVD level is a CVD level
//! everywhere it is touched.
//!
//! This module is the docking point: a future band that is neither the
//! candles nor an indicator pane (a volume profile, a second price band in a
//! comparison layout) is a new arm in [`carve`] and a new variant beside it,
//! and `pane.rs` keeps only consuming what it is handed.
//!
//! The invariant the whole feature rests on lives here: a band's drawings and
//! a band's curve project through the *same* `PriceScale`, built from the
//! range the renderer actually drew. A level that has drifted off the curve
//! it annotates is a lie, and the only way to make the drift impossible
//! rather than unlikely is for both to read one object.

use std::sync::Arc;

use eframe::egui;
use smallvec::SmallVec;

use crate::chart::PriceScale;
use crate::drawings::{Drawing, DrawingBand, ValueUnit};
use crate::indicators::{IndicatorView, IndicatorViews};
use crate::plot_area::PlotAreas;

/// What the price band is called where a band has to name itself.
pub const PRICE_BAND_LABEL: &str = "price";
/// Said by the cursor over a collapsed pane, which has 20 px of strip and no
/// axis to place an anchor against.
pub const COLLAPSED_BAND_REFUSAL: &str = "collapsed - click the chevron to draw here";
/// Said by a band that has no range yet. An indicator pane already writes
/// "warming up" across its own centre, so the cursor only has to stop
/// pretending; the candles say the same thing in their empty-chart notice.
pub const WARMING_BAND_REFUSAL: &str = "no values yet - still warming up";

/// Half-width of the marker that says an object is off the top or the bottom
/// of its band.
const OFF_BAND_CARET_HALF_PX: f32 = 3.0;

/// Most bands a chart pane can carry: the candles plus every indicator pane.
pub const MAX_BANDS: usize = 1 + crate::indicators::MAX_PANES;

/// The carve's own container. Inline for every real chart, so a per-frame
/// carve never touches the heap for the bands themselves.
pub type Bands = SmallVec<[Band; MAX_BANDS]>;

/// One value axis of one chart pane.
pub struct Band {
    /// The identity an object anchored here stores.
    pub key: DrawingBand,
    /// The drawable rows: the band's own height, x-clipped at the live lane.
    pub rect: egui::Rect,
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

/// What the price band needs from the pane that owns it.
pub struct PriceBand {
    /// The drawable rows of the candles: their rect minus the live lane.
    pub rect: egui::Rect,
    /// The range the candles were drawn with, after the trader's own zoom.
    pub range: Option<(f64, f64)>,
    /// Top and bottom of the candles' plot, which the scale maps onto.
    pub top: f32,
    pub bottom: f32,
    /// Whether the candles are drawn upside down. The band's scale has to
    /// carry it, or a drawing would anchor at the mirror of the price the
    /// click named.
    pub inverted: bool,
}

/// Carve one chart pane into its bands.
///
/// The indicator scale is built from `view.scale.resolve(last_auto)` — the
/// range the renderer actually drew — and never from `last_auto` alone, which
/// is only the auto-fit before the trader's own zoom.
///
/// Per-frame cost: one pass over at most [`crate::indicators::MAX_PANES`]
/// panes; a handful of `f32`, one `Arc` refcount bump per band, and no heap
/// allocation at all when `out` is reused. Nothing here is on the per-trade
/// or per-depth path.
pub fn carve(
    out: &mut Bands,
    price: &PriceBand,
    indicators: &IndicatorViews,
    areas: &PlotAreas,
    price_label: &Arc<str>,
) {
    out.clear();
    out.push(Band {
        key: DrawingBand::Price,
        rect: price.rect,
        scale: price.range.map(|(lo, hi)| {
            PriceScale::from_range(lo, hi, price.top, price.bottom).with_inverted(price.inverted)
        }),
        label: Arc::clone(price_label),
        refusal: (price.range.is_none()).then_some(WARMING_BAND_REFUSAL),
    });
    for (view, slot) in indicators.visible_panes().zip(&areas.indicator_panes) {
        let rect = egui::Rect::from_min_max(
            egui::pos2(price.rect.left(), slot.rect.top()),
            egui::pos2(price.rect.right(), slot.rect.bottom()),
        );
        let range = view.last_auto.map(|auto| view.scale.resolve(auto));
        out.push(Band {
            key: DrawingBand::Indicator(indicators.pane_key(view)),
            rect,
            scale: range
                .filter(|_| !slot.collapsed)
                .map(|(lo, hi)| PriceScale::from_range(lo, hi, rect.top(), rect.bottom())),
            label: view.label_shared(),
            // A band that cannot honestly place an anchor says so before the
            // press, through the cursor — never by swallowing a click.
            refusal: if slot.collapsed {
                Some(COLLAPSED_BAND_REFUSAL)
            } else if range.is_none() {
                Some(WARMING_BAND_REFUSAL)
            } else {
                None
            },
        });
    }
}

/// The band under a pointer, drawable or not — a refusing band still owns its
/// pixels, which is what lets the cursor explain the refusal.
#[must_use]
pub fn band_at(bands: &[Band], pos: egui::Pos2) -> Option<&Band> {
    bands.iter().find(|band| band.rect.contains(pos))
}

/// The band an existing object is anchored to, when it is on the chart right
/// now. `None` is the parked case: the indicator it annotates is not on this
/// chart, so the object neither paints nor hit-tests — and it is not deleted
/// either (the manager still lists it).
#[must_use]
pub fn band_of<'a>(bands: &'a [Band], drawing: &Drawing) -> Option<&'a Band> {
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
pub fn drawing_in_band(drawing: &Drawing, band: &Band) -> bool {
    drawing.band == DrawingBand::AllBands || drawing.band == band.key
}

/// Mark an object whose every anchor sits outside the band's current range.
///
/// Without it, "the drawing keeps its value and the band re-fits around it"
/// is indistinguishable from "my line vanished". The caret is the paper HUD's
/// own directional language, in the object's own colour, at the object's own
/// x — so the trader reads *where* it went, not merely that something did.
pub fn paint_off_band_caret(
    painter: &egui::Painter,
    band: egui::Rect,
    points: &[egui::Pos2],
    drawing: &Drawing,
) {
    // A time-only object has no value to be off the band by.
    if points.is_empty() || drawing.band == DrawingBand::AllBands {
        return;
    }
    let above = points.iter().all(|point| point.y < band.top());
    let below = points.iter().all(|point| point.y > band.bottom());
    if !above && !below {
        return;
    }
    #[allow(clippy::cast_precision_loss)]
    let x = points.iter().map(|point| point.x).sum::<f32>() / points.len() as f32;
    // Inset by the caret's own half-width, not to the raw edge: an object
    // whose anchor is off the left of the window clamps here, and clamping to
    // the edge itself put half the triangle outside the band's clip — the
    // mark then read as a smudge instead of a direction (seen in the visual
    // QA pass of 2026-08-07).
    let x = x.clamp(
        band.left() + OFF_BAND_CARET_HALF_PX,
        band.right() - OFF_BAND_CARET_HALF_PX,
    );
    let (tip, base) = if above {
        (
            band.top() + 1.0,
            band.top() + 1.0 + OFF_BAND_CARET_HALF_PX * 2.0,
        )
    } else {
        (
            band.bottom() - 1.0,
            band.bottom() - 1.0 - OFF_BAND_CARET_HALF_PX * 2.0,
        )
    };
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(x, tip),
            egui::pos2(x - OFF_BAND_CARET_HALF_PX, base),
            egui::pos2(x + OFF_BAND_CARET_HALF_PX, base),
        ],
        drawing.style.color,
        egui::Stroke::NONE,
    ));
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
