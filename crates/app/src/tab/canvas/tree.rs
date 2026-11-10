//! The canvas painted from the model's pane tree: every layout the column
//! painter does not draw, and any fold or maximize the canvas model holds.
//!
//! The geometry is `quantick_workspace::canvas`'s, unchanged: each docked
//! pane takes its tree rectangle, inset by half a divider on every edge it
//! shares with a neighbour, so the seams read as the column painter's
//! dividers do. Nothing here decides a layout.

use eframe::egui;
use quantick_chart::pane_header::{PaneHeader, PaneIdentity};
use quantick_workspace::canvas::{CanvasRect, PaneKind, strip_label, tree_bands};
use smallvec::SmallVec;

use super::{CanvasChrome, CanvasFrame, PaneRects, Tab};
use crate::canvas_layout::{CANVAS_DIVIDER_PX, MAX_CANVAS_PANES};
use crate::pane::canvas_split::PaneAreas;
use crate::pane::{PaneSide, split_flow_pane, split_pane_header, split_pane_layout_strip};
use crate::theme;

impl Tab {
    /// Lay the canvas out from the model's tree and spend each band's header,
    /// as the column painter does for its own bands.
    pub(super) fn draw_tree(
        &mut self,
        ui: &mut egui::Ui,
        area: egui::Rect,
        chrome: &mut CanvasChrome<'_>,
    ) -> (CanvasFrame, PaneRects) {
        self.canvas.last_canvas = area;
        let palette = theme::get(ui.ctx());
        let bar_inputs = crate::toolbar::available_bar_inputs(
            chrome.capabilities,
            self.deal_recording_view().as_ref(),
        );
        let canvas = CanvasRect::new(area.left(), area.top(), area.width(), area.height());
        let bands = tree_bands(
            &self.canvas.state,
            &mut self.canvas.seats,
            canvas,
            CANVAS_DIVIDER_PX,
        );
        let bands = bands.into_iter().map(|band| {
            let at = egui::Rect::from_min_size(
                egui::pos2(band.rect.x, band.rect.y),
                egui::vec2(band.rect.w, band.rect.h),
            );
            (
                band.seat.map_or(PaneSide::Flow, PaneSide::Time),
                at,
                band.minimized,
                band.slot,
            )
        });
        let bands: SmallVec<[_; MAX_CANVAS_PANES]> = bands.collect();
        let built = self.time_panes.len();
        let heights = &mut self.canvas.context_stack.heights;
        if heights.len() < built {
            heights.resize(built, crate::canvas_layout::PaneWidth::Auto);
        }
        self.canvas.context_stack.frame = None;
        #[cfg(test)]
        self.context_dividers.clear();
        // The seams: the canvas shows through between the bands.
        ui.painter()
            .rect_filled(area, egui::Rounding::ZERO, palette.border());
        let mut rects = PaneRects {
            context_charts: SmallVec::from_elem(None, built),
            context_dividers: SmallVec::new(),
            flow_areas: None,
            pane_bands: SmallVec::new(),
            flow_side_row: None,
        };
        let flow_band = (bands.iter())
            .find(|(side, _, minimized, _)| *side == PaneSide::Flow && !minimized)
            .map(|(_, rect, ..)| *rect);
        let mut job_slot = flow_band
            .is_none()
            .then(|| self.connection.slot(self.session.history_note()))
            .flatten();
        let mut focus_bands: SmallVec<[egui::Rect; MAX_CANVAS_PANES]> =
            SmallVec::from_elem(egui::Rect::NOTHING, built);
        for &(side, rect, minimized, slot) in &bands {
            if minimized {
                let name = self.strip_name(side);
                crate::pane_menu::strip(ui, rect, &name, slot, &mut self.canvas.menus);
                continue;
            }
            let PaneSide::Time(slot) = side else {
                continue;
            };
            if slot >= built {
                // Still being built: the canvas, not a stand-in chart.
                ui.painter()
                    .rect_filled(rect, egui::Rounding::ZERO, palette.chrome());
                continue;
            }
            rects.pane_bands.push((side, rect));
            focus_bands[slot] = rect;
            let pane_areas = split_pane_layout_strip(rect);
            let areas = split_pane_header(
                pane_areas.body,
                palette.layout(crate::time_header::HEIGHT_PX),
            );
            self.draw_time_header(ui, slot, areas.header, bar_inputs, job_slot.take());
            rects.context_charts[slot] = Some((areas.chart, pane_areas.layout_strip));
        }
        if rects.pane_bands.len() + usize::from(flow_band.is_some()) > 1 {
            self.focus_from_pointer(ui, &focus_bands, flow_band.unwrap_or(egui::Rect::NOTHING));
        }
        if let Some(band) = flow_band {
            rects.pane_bands.push((PaneSide::Flow, band));
            let pane_areas = split_pane_layout_strip(band);
            let areas = split_flow_pane(pane_areas.body, palette.layout(1.0));
            ui.painter().rect_filled(
                pane_areas.body,
                egui::Rounding::ZERO,
                palette.flow_surface(),
            );
            self.draw_flow_header(ui, areas.band, chrome);
            rects.flow_side_row = Some((areas.side_row, bar_inputs));
            self.flow_pane.frame.side_row = Some(areas.side_row);
            rects.flow_areas = Some(PaneAreas {
                body: crate::orderflow_panel::draw(self, ui, areas.chart, chrome.panel_folds),
                layout_strip: pane_areas.layout_strip,
            });
        }
        let frame = CanvasFrame {
            area,
            context_shown: built,
            show_flow: flow_band.is_some(),
            split: false,
            time_area: None,
            divider: None,
            flow_band: flow_band.unwrap_or(egui::Rect::NOTHING),
            collapsed_rail: None,
            flow_rail: None,
            tree: true,
        };
        (frame, rects)
    }

    /// What a folded chart's strip calls it: "Time · 1m · BTCUSDT".
    fn strip_name(&self, side: PaneSide) -> String {
        let (kind, bars) = match side {
            PaneSide::Time(seat) => (
                PaneKind::Time,
                self.time_panes
                    .get(seat)
                    .map(|pane| pane.spec.spec().parameter_summary()),
            ),
            PaneSide::Flow => (
                PaneKind::Flow,
                Some(PaneHeader::of(PaneIdentity::OrderFlow, self.flow_pane.spec.spec()).bars_text),
            ),
        };
        strip_label(kind, &bars.unwrap_or_default(), &self.symbol)
    }
}
