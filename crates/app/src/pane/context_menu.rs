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

use crate::bands::{self, Band, Bands};
use crate::chart::PriceScale;
use crate::drawings::{self, ChartPoint};
use crate::plot_area::PlotAreas;

use super::{ChartPane, PaneChrome};

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
}

impl ChartPane {
    /// The secondary click on the canvas: what the press resolves (the price,
    /// the tape flag, the drawing, the placing entries), the layer menu it
    /// opens, and the rename an outside click commits when the menu closes.
    ///
    /// One arm of [`ChartPane::handle_navigation`], called once per frame.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_context_menu(
        &mut self,
        chart: &egui::Response,
        areas: &PlotAreas,
        bands: &Bands,
        total: usize,
        price_band: &Band,
        drawing_scale: Option<PriceScale>,
        chrome: &mut PaneChrome<'_>,
    ) {
        // The price under a right-click, remembered before the menu eats
        // the pointer: the trade section places orders at it.
        if chart.secondary_clicked()
            && let Some(position) = chart.interact_pointer_pos()
            && areas.chart.contains(position)
            && let Some(scale) = drawing_scale.as_ref()
        {
            self.context_menu.price = Some(scale.price_at(position.y));
            // The same click, resolved once per placing tool through the
            // projection `drawing_point_at` owns, each with the tool's own
            // snap — the anchored VWAP's candle magnet included.
            let history_right = self.frame.lane_divider_x.unwrap_or(areas.chart.right());
            self.context_menu.on_tape = self.click_on_tape(position.x);
            // The most specific thing under the click: a drawing, resolved
            // on the band the click actually landed in (a CVD line and a
            // price line can share the pixel). Right-click selects like the
            // primary press does, so the menu and the context bar agree on
            // which object is being acted on.
            let clicked = bands::band_at(bands, position)
                .filter(|band| band.drawable())
                .and_then(|band| self.drawing_at(position, band, history_right, total));
            self.context_menu.drawing = clicked.map(|index| {
                self.drawings.select(Some(index));
                let drawing = &self.drawings.items()[index];
                self.context_menu.rename = drawing.name.clone().unwrap_or_default();
                drawing.id
            });
            self.context_menu.places.clear();
            for tool in drawings::DRAWING_TOOLS {
                if tool.context_menu_label().is_none() {
                    continue;
                }
                if let Some(point) = self.drawing_point_at(
                    position,
                    history_right,
                    total,
                    false,
                    tool.anchor_snap(),
                    price_band,
                ) {
                    self.context_menu.places.push((tool, point));
                }
            }
        }
        // Right-click: what is on this canvas, and what is not. Secondary
        // button only, so it shares no gesture with the pan, the zoom or the
        // drawing tools — a pan that ends anywhere never opens it.
        chart.context_menu(|ui| self.draw_layer_menu(ui, chrome));
        // While the menu is open the pointer is reading it, not the chart, so
        // no crosshair chases it across the candles behind it.
        if chart.context_menu_opened() {
            self.hover_pos = None;
        } else if let Some(id) = self.context_menu.drawing.take() {
            // The menu just closed. An in-flight rename commits here too:
            // dismissing the menu with an outside click is the natural
            // blur-to-commit gesture, and the TextEdit's own lost_focus
            // never runs once its closure stops being drawn.
            if let Some(index) = self.drawings.index_of(id) {
                let current = self.drawings.items()[index]
                    .name
                    .clone()
                    .unwrap_or_default();
                if self.context_menu.rename.trim() != current {
                    let name = std::mem::take(&mut self.context_menu.rename);
                    self.drawings.rename_at(index, &name);
                }
            }
        }
    }
}
