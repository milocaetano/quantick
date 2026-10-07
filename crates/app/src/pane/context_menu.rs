//! What the right-click that opened the layer menu resolved.
//!
//! A context menu stays open across frames while the chart under it keeps
//! moving, so every question the menu asks is answered once — at press time —
//! and held until the menu closes. That is the whole reason these five exist
//! rather than being re-derived per menu frame: a price re-read from the
//! pointer would follow the cursor mid-reach, and an index re-hit-tested would
//! go stale under a menu the trader is still reading.
//!
//! The rename buffer is here for the same reason from the other direction: it
//! is seeded on the press and edited across the frames the menu is open.

use eframe::egui;

use crate::bands::{self, Bands};
use crate::drawings::{self, ChartPoint, Drawings};
use crate::plot_area::PlotAreas;
#[cfg(test)]
use quantick_layers::ChartLayer;

use super::PaneFrame;
use super::drawing_projection::DrawingProjection;
use super::menus::PaneMenuIntent;

/// The last right-click, as the press resolved it. See the module docs.
#[derive(Default)]
pub struct PaneContextMenu {
    /// Whether the right-click that opened the menu landed on the tape rather
    /// than on the candles. The two panes are configured apart, so the menu has
    /// to know which one was asked.
    pub(super) on_tape: bool,
    /// Price under the right-click that opened the layer menu — the trade
    /// section's anchor. Refreshed by every secondary click on the canvas.
    pub(super) price: Option<f64>,
    /// The placing entries of the last right-click: each registry tool that
    /// declares a `context_menu_label`, with the chart point *its own*
    /// `anchor_snap` resolved for that click — so the menu never re-derives
    /// a projection and a new tool's snap rule needs no edit here.
    pub(super) places: Vec<(drawings::DrawingTool, ChartPoint)>,
    /// The drawing under the last right-click, resolved at press time like
    /// the price and the tape flag. Held as an id, not an index: the menu
    /// stays open across frames, and an index can go stale under it.
    /// `pub(crate)` so the menu tests can stage the click's outcome.
    pub(crate) drawing: Option<drawings::DrawingId>,
    /// Rename buffer for the layer menu's drawing section, seeded from the
    /// clicked object's current name on the press that opened the menu.
    pub(super) rename: String,
    /// Test-only trace of the drawing section's widgets, the
    /// `layer_menu_rects` idiom: label → rect, rebuilt per menu frame.
    ///
    /// Here rather than with the gestures because it is rebuilt and read on
    /// the menu's clock, not a press's: it is the menu's own drawing, traced.
    #[cfg(test)]
    pub menu_rects: Vec<(&'static str, egui::Rect)>,
    /// The chart-layer submenu's latest painted rectangle. A launch hook uses
    /// the same button geometry to expand it for visual validation.
    pub(crate) chart_layers_rect: Option<egui::Rect>,
    /// "clear objects…" was clicked: the confirmation shows until answered.
    pub(crate) confirm_clear: bool,
    /// Test-only trace of the objects submenu's row buttons: index, label, rect.
    #[cfg(test)]
    pub object_rects: Vec<(usize, &'static str, egui::Rect)>,
    /// Test-only: where "clear objects…" was painted.
    #[cfg(test)]
    pub clear_objects_rect: Option<egui::Rect>,
    /// Where each layer's switch landed in the last menu frame, so a test can
    /// click the real widget instead of calling the setter behind it.
    #[cfg(test)]
    pub layer_menu_rects: Vec<(ChartLayer, egui::Rect)>,
}

impl PaneContextMenu {
    /// Aim the next menu at one pane or the other, as a right-click would.
    #[cfg(test)]
    pub(crate) fn aim_at_tape(&mut self, on_tape: bool) {
        self.on_tape = on_tape;
    }

    /// Open on a resolved press: hold its answers for the menu's whole life,
    /// and seed the rename buffer from the drawing it landed on. Returns the
    /// selection the press makes — a right-click selects like the primary
    /// press does, so the menu and the context bar agree on the object.
    pub(super) fn open_at(
        &mut self,
        press: ContextPress,
        drawings: &Drawings,
    ) -> Option<PaneMenuIntent> {
        self.price = Some(press.price);
        self.on_tape = press.on_tape;
        self.drawing = press.drawing.map(|index| {
            let drawing = &drawings.items()[index];
            self.rename = drawing.name.clone().unwrap_or_default();
            drawing.id
        });
        self.places = press.places;
        self.chart_layers_rect = None;
        self.drawing.map(PaneMenuIntent::SelectDrawing)
    }

    /// The menu just closed. An in-flight rename commits here too:
    /// dismissing the menu with an outside click is the natural
    /// blur-to-commit gesture, and the TextEdit's own lost_focus never runs
    /// once its closure stops being drawn.
    /// The buffer is emptied either way, so a drawing deleted under the
    /// menu leaves no name behind for the next one.
    pub(super) fn close(&mut self, drawings: &Drawings) -> Option<PaneMenuIntent> {
        let name = std::mem::take(&mut self.rename);
        let id = self.drawing.take()?;
        let index = drawings.index_of(id)?;
        let current = drawings.items()[index].name.clone().unwrap_or_default();
        (name.trim() != current).then_some(PaneMenuIntent::RenameDrawing { id, name })
    }

    /// Where the chart-layer submenu button was painted, for the scripted
    /// pointer event that opens the real egui menu during capture.
    #[cfg(any(feature = "scenario-harness", test))]
    pub(crate) fn chart_layers_center(&self) -> Option<egui::Pos2> {
        self.chart_layers_rect.map(|rect| rect.center())
    }
}

/// What a secondary click on the canvas resolved, once, at press time.
pub(super) struct ContextPress {
    price: f64,
    on_tape: bool,
    /// Index of the drawing under the click, on the band it landed in.
    drawing: Option<usize>,
    places: Vec<(drawings::DrawingTool, ChartPoint)>,
}

impl ContextPress {
    /// Resolve a right-click at `position`: the price under it, the tape
    /// flag, the drawing, and one placing point per declaring tool. `None`
    /// when the click is off the candles or they have no scale yet — the
    /// paper lines and the right-click price live on the candles only: an
    /// order is a price, not a value on someone's oscillator.
    pub(super) fn resolve(
        position: egui::Pos2,
        areas: &PlotAreas,
        bands: &Bands,
        projection: &DrawingProjection<'_>,
        drawings: &Drawings,
        frame: &PaneFrame,
        total: usize,
    ) -> Option<Self> {
        let price_band = &bands[0];
        let scale = price_band.scale.as_ref()?;
        if !areas.chart.contains(position) {
            return None;
        }
        let history_right = frame.lane_divider_x.unwrap_or(areas.chart.right());
        // The most specific thing under the click: a drawing, resolved on the
        // band the click actually landed in (a CVD line and a price line can
        // share the pixel).
        let drawing = bands::band_at(bands, position)
            .filter(|band| band.drawable())
            .and_then(|band| projection.drawing_at(drawings, position, band, history_right, total));
        // The same click, resolved once per placing tool through the
        // projection `drawing_point_at` owns, each with the tool's own snap —
        // the anchored VWAP's candle magnet included.
        let places = drawings::DRAWING_TOOLS
            .into_iter()
            .filter(|tool| tool.context_menu_label().is_some())
            .filter_map(|tool| {
                projection
                    .drawing_point_at(
                        position,
                        history_right,
                        total,
                        false,
                        tool.anchor_snap(),
                        price_band,
                    )
                    .map(|point| (tool, point))
            })
            .collect();
        Some(Self {
            price: scale.price_at(position.y),
            on_tape: frame.click_on_tape(position.x),
            drawing,
            places,
        })
    }
}
