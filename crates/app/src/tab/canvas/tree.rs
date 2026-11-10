//! The canvas painted from the model's pane tree: every layout the column
//! painter does not draw, and any fold or maximize the canvas model holds.
//!
//! The geometry is `quantick_workspace::canvas`'s, unchanged: each docked
//! pane takes its tree rectangle, inset by half a divider on every edge it
//! shares with a neighbour, so the seams read as the column painter's
//! dividers do. Nothing here decides a layout.

use eframe::egui;
use quantick_workspace::canvas::{CanvasRect, CanvasState, PaneKind, PaneMode};
use smallvec::SmallVec;

use super::{CanvasChrome, CanvasFrame, PaneRects, Tab};
use crate::canvas_layout::{CANVAS_DIVIDER_PX, MAX_CANVAS_PANES};
use crate::pane::canvas_split::PaneAreas;
use crate::pane::{PaneSide, split_flow_pane, split_pane_header, split_pane_layout_strip};
use crate::theme;

/// One docked pane of the tree, in window points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TreeBand {
    /// The window's pane this slot shows.
    pub side: PaneSide,
    /// Where it draws: its tree rectangle less half a divider on each seam.
    pub rect: egui::Rect,
    /// Folded to its strip by the canvas model.
    pub minimized: bool,
}

/// Where each docked pane of `state` draws when the canvas is `area`.
///
/// Slots map to the window's panes by kind: the flow slot is the flow pane,
/// and the time slots, in slot order, are the time panes in theirs. A slot
/// the window has no pane for (a second flow) is left out.
#[must_use]
pub(crate) fn tree_bands(
    state: &CanvasState,
    area: egui::Rect,
) -> SmallVec<[TreeBand; MAX_CANVAS_PANES]> {
    let canvas = CanvasRect::new(area.left(), area.top(), area.width(), area.height());
    let mut time = 0;
    let mut flow_taken = false;
    let sides: SmallVec<[_; MAX_CANVAS_PANES]> = (state.panes().into_iter())
        .filter_map(|(slot, kind, _)| match kind {
            PaneKind::Time => {
                time += 1;
                Some((slot, PaneSide::Time(time - 1)))
            }
            _ if flow_taken => None,
            _ => {
                flow_taken = true;
                Some((slot, PaneSide::Flow))
            }
        })
        .collect();
    (state.rects(canvas).into_iter())
        .filter_map(|(slot, rect)| {
            let side = sides.iter().find(|(held, _)| *held == slot)?.1;
            let rect = egui::Rect::from_min_size(
                egui::pos2(rect.x, rect.y),
                egui::vec2(rect.w, rect.h),
            );
            Some(TreeBand {
                side,
                rect: inset_seams(rect, area),
                minimized: state.mode(slot) == Some(PaneMode::Minimized),
            })
        })
        .collect()
}

/// `rect` less half a divider on every edge inside `area`.
#[must_use]
pub(crate) fn inset_seams(rect: egui::Rect, area: egui::Rect) -> egui::Rect {
    const EDGE_SLACK_PX: f32 = 0.5;
    let half = CANVAS_DIVIDER_PX / 2.0;
    let mut inset = rect;
    if rect.left() > area.left() + EDGE_SLACK_PX {
        inset.min.x += half;
    }
    if rect.right() < area.right() - EDGE_SLACK_PX {
        inset.max.x -= half;
    }
    if rect.top() > area.top() + EDGE_SLACK_PX {
        inset.min.y += half;
    }
    if rect.bottom() < area.bottom() - EDGE_SLACK_PX {
        inset.max.y -= half;
    }
    inset
}

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
        let bands = tree_bands(&self.canvas.state, area);
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
            .find(|band| band.side == PaneSide::Flow && !band.minimized)
            .map(|band| band.rect);
        let mut job_slot = flow_band
            .is_none()
            .then(|| self.connection.slot(self.session.history_note()))
            .flatten();
        let mut focus_bands: SmallVec<[egui::Rect; MAX_CANVAS_PANES]> =
            SmallVec::from_elem(egui::Rect::NOTHING, built);
        for band in &bands {
            if band.minimized {
                super::paint_rail(ui.painter(), band.rect, band.rect.left(), false);
                continue;
            }
            let PaneSide::Time(slot) = band.side else {
                continue;
            };
            if slot >= built {
                // Still being built: the canvas, not a stand-in chart.
                ui.painter()
                    .rect_filled(band.rect, egui::Rounding::ZERO, palette.chrome());
                continue;
            }
            rects.pane_bands.push((band.side, band.rect));
            focus_bands[slot] = band.rect;
            let pane_areas = split_pane_layout_strip(band.rect);
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
}
