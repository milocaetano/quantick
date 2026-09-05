//! What the last draw measured — the geometry the passes outside it ask for.
//!
//! A pane's input pass runs *before* its draw pass, and several things that are
//! neither pass at all need a point on this pane: the scripted right-clicks of
//! `QUANTICK_CONTEXT_MENU`, the inspector's placement, the tab's shared-drawing
//! projection. All of them want the rect the draw actually used, and computing
//! it a second time is how two answers start to disagree.
//!
//! So the draw publishes what it measured here and everything else reads it.
//! Grouped rather than spread across [`super::ChartPane`] because they share
//! one lifetime — written by one pass, read until the next — and one meaning:
//! every field is *where something was last frame*, never what it should be.
//! The `last_` prefix each of them used to carry is this struct's name now.

use eframe::egui;

use crate::bands::Bands;

/// The geometry the last draw published. See the module docs.
///
/// Not `Debug` or `Clone`, for the same reason [`super::ChartPane`] is
/// neither: a pane is a place, not a value, and the bands it last painted
/// hold no cheaper copy than the draw that produced them.
pub struct PaneFrame {
    /// Where the history pane ended last frame — the lane's divider, and the
    /// handle that resizes it. The input pass runs before the draw computes it.
    pub lane_divider_x: Option<f32>,
    /// The canvas the last draw used. Published for the same reason the divider
    /// is: something outside the draw needs a point on this pane — the scripted
    /// right-click of `QUANTICK_CONTEXT_MENU` — and computing the geometry a
    /// second time is how two answers start to disagree.
    pub chart_rect: Option<egui::Rect>,
    /// The whole rect this pane was last painted into, gutters and time strip
    /// included — and set whether or not the pane had anything to draw, which
    /// is what separates it from [`Self::chart_rect`]. The feed's one-line
    /// offline note is placed against it: an explanation belongs on the pane
    /// with nothing in it, which is precisely the pane that has room for one.
    pub area: Option<egui::Rect>,
    /// The price gutter of the last draw, published for the same reason: the
    /// scripted right-click of `QUANTICK_CONTEXT_MENU=axis` needs a point that
    /// is really on the axis, not a guess about where the gutter probably is.
    pub price_gutter: Option<egui::Rect>,
    /// The candles' segment of the bottom time strip, published for the same
    /// reason again: `QUANTICK_CONTEXT_MENU=time` needs a point that is really
    /// on the time axis, and past the lane divider the strip belongs to the
    /// tape's own window rather than to this menu.
    pub time_strip: Option<egui::Rect>,
    /// The automatic tape window at the last draw — the recent bars' typical
    /// duration. Only the menu reads it, and only to state what "follows the
    /// bars" currently amounts to; the drawing itself is handed the resolved
    /// window, never this.
    pub(super) lane_reference_ms: Option<i64>,
    /// Last frame's auto-fit price range, for pixel↔price maths in the input
    /// handler (which runs before the draw computes it).
    pub auto_range: Option<(f64, f64)>,
    /// Last frame's chart height. See [`Self::auto_range`].
    pub chart_height: f32,
    /// Last frame's chart top. See [`Self::auto_range`].
    pub chart_top: f32,
    /// The chart pane from the last frame (excludes axes and the live lane),
    /// for inspector placement and manager centring.
    pub chart_area: Option<egui::Rect>,
    /// The bands the last [`super::ChartPane::draw_chart`] painted, kept for
    /// the passes that run outside it: the tab's shared-drawing projection and
    /// the inspector's "which band is this on". Reused every frame rather than
    /// rebuilt, so the draw pass allocates no container.
    pub(super) bands: Bands,
    /// The raw canvas area the last frame split into chart, panes and gutters.
    /// Kept so a caller that needs a band it does not otherwise see — the pane
    /// axis tests aiming a drag at a pane's own gutter — asks `plot_split` for
    /// it rather than re-deriving the layout and drifting from it.
    pub plot_area: Option<egui::Rect>,
}

impl Default for PaneFrame {
    /// Nothing measured yet. The two lengths are not zero: a pane is asked for
    /// pixel↔price maths on the very first input pass, before any draw has
    /// published a height, and a zero height there is a division by zero.
    fn default() -> Self {
        Self {
            lane_divider_x: None,
            chart_rect: None,
            area: None,
            price_gutter: None,
            time_strip: None,
            lane_reference_ms: None,
            auto_range: None,
            chart_height: 1.0,
            chart_top: 0.0,
            chart_area: None,
            bands: Bands::new(),
            plot_area: None,
        }
    }
}
