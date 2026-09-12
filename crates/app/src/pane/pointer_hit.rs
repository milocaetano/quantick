//! What is under the pointer, answered off the geometry the last frame
//! painted: the control plane's cursor scope, and the overlay plot a double
//! click opens the settings of.
//!
//! Both are pull operations — nothing here runs on the frame loop. They read
//! the projection `draw_chart` cached (`last_projection`) rather than
//! re-measuring a world a gesture may have moved (§D8). A pure move out of
//! `pane.rs`.

use eframe::egui;

use crate::bands;
use crate::chart::PriceScale;
use crate::drawings::{self, DrawingBand};
use crate::indicator_worker::SlotId;

use super::{ChartPane, PLOT_PICK_TOLERANCE_PX};

/// One drawing resolved under the pointer for an on-demand control capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlDrawingHit {
    pub id: drawings::DrawingId,
    pub tool_id: &'static str,
    pub label: String,
    pub user_label_present: bool,
    pub handle_index: Option<usize>,
    pub selected: bool,
    pub locked: bool,
}

/// Semantic meaning of the pointer over one chart pane.
///
/// Coordinates are kept internal here. `app::control` owns the transport DTO
/// and converts every float into a canonical decimal string, preventing UI
/// types and non-canonical JSON numbers from leaking onto the wire.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ControlPointerHit {
    pub screen_x_px: f32,
    pub screen_y_px: f32,
    pub band: String,
    pub axis_value: Option<f64>,
    pub axis_unit: String,
    pub slot: Option<usize>,
    pub bar: Option<quantick_engine::Bar>,
    pub flow_cell: Option<crate::orderflow_view::FlowCellHit>,
    pub drawing: Option<ControlDrawingHit>,
}

impl ChartPane {
    /// This pane's own projection, rebuilt from the geometry the last
    /// [`Self::draw_chart`] cached. `None` before the pane has drawn once, or
    /// while it has no price range to project against.
    pub(super) fn last_projection(&self) -> Option<(egui::Rect, f32, usize, PriceScale)> {
        let chart = self.frame.chart_area?;
        let auto = self.frame.auto_range?;
        let scale = self.price_view.scale(
            auto,
            self.frame.chart_top,
            self.frame.chart_top + self.frame.chart_height,
        );
        let history_right = self.frame.lane_divider_x.unwrap_or(chart.right());
        Some((self.drawing_area(chart), history_right, self.slots(), scale))
    }

    /// Resolve the pointer against the exact geometry the last frame painted.
    ///
    /// This is deliberately a pull operation. The normal frame loop only
    /// records the position it already needs for the crosshair; bar, price,
    /// L2 cell, and drawing hit-testing happen here only when a control client
    /// requests the cursor scope.
    #[must_use]
    pub(crate) fn control_pointer_hit(&self) -> Option<ControlPointerHit> {
        let position = self.hover_pos?;
        let chart = self.frame.chart_area?;
        if !chart.contains(position) {
            return None;
        }
        let band = bands::band_at(&self.frame.bands, position)?;
        let history_right = self.frame.lane_divider_x.unwrap_or_else(|| chart.right());
        let total = self.slots();

        // The same question the axis compass paints the answer to, asked
        // through the same owner: a client reading the cursor and a trader
        // reading the axis may not be told two different bars.
        let slot = (total > 0 && position.x <= history_right)
            .then(|| self.viewport.slot_at_x(position.x, history_right, total))
            .flatten();
        let axis_value = band.scale.as_ref().map(|scale| scale.price_at(position.y));
        // What the pointer's y means on this band. A time-only band has no
        // value axis of its own, so y is read on the pane's price axis, which
        // is what `axis_value` below is computed from.
        let axis_unit = match &band.key {
            DrawingBand::Price | DrawingBand::AllBands => "price".to_owned(),
            DrawingBand::Indicator(_) => "indicator_value".to_owned(),
        };

        let drawing_pick = self
            .drawing_handle_at(position, band, history_right, total)
            .map(|(index, handle)| (index, Some(handle)))
            .or_else(|| {
                self.drawing_at(position, band, history_right, total)
                    .map(|index| (index, None))
            });
        let drawing = drawing_pick.and_then(|(index, handle_index)| {
            let drawing = self.drawings.items().get(index)?;
            Some(ControlDrawingHit {
                id: drawing.id,
                tool_id: drawing.tool.id(),
                label: format!("{} {}", drawing.tool.name(), index + 1),
                user_label_present: drawing.name.is_some(),
                handle_index,
                selected: self.drawings.selected() == Some(index),
                locked: drawing.locked,
            })
        });

        let lane_width_px = (chart.right() - history_right).max(0.0);
        let flow_cell = self.orderflow.as_ref().and_then(|orderflow| {
            orderflow.control_flow_cell_at(
                chart,
                &self.viewport,
                total,
                lane_width_px,
                self.price_view.is_inverted(),
                position,
            )
        });
        let band_name = crate::control::drawing_band_name(&band.key);
        Some(ControlPointerHit {
            screen_x_px: position.x,
            screen_y_px: position.y,
            band: band_name.to_owned(),
            axis_value,
            axis_unit,
            slot,
            bar: slot.and_then(|slot| self.candle_at_slot(slot).cloned()),
            flow_cell,
            drawing,
        })
    }

    /// Which overlay indicator's plotted line a pointer at `pos` is sitting
    /// on, if any — the fourth place a double click opens settings from, and
    /// the most direct one: the thing the trader wants to change is the line
    /// they are looking at.
    ///
    /// Measured against the projection the *last* frame drew (§D8: no gesture
    /// re-measures a world it moved), and against the resolved style rather
    /// than the declared one, so a plot the trader switched off in the dialog
    /// cannot be picked where it is no longer drawn.
    ///
    /// Cost: nothing per frame — this runs on a double click only, and is then
    /// bounded by the visible bars of each overlay's plots, the same span the
    /// renderer already walks every frame.
    pub(super) fn overlay_plot_at(&self, pos: egui::Pos2) -> Option<SlotId> {
        let (chart, right, total, scale) = self.last_projection()?;
        let (start, end) = self.viewport.visible_range(chart.width(), total);
        let mut best: Option<(f32, SlotId)> = None;
        for view in self.indicators.visible_overlays() {
            for index in 0..view.descriptor.plots.len() {
                let Some(resolved) = view.plot_style(index) else {
                    continue;
                };
                if !resolved.visible {
                    continue;
                }
                let Some(column) = view.columns.get(index) else {
                    continue;
                };
                // The segments the renderer joins, tested as segments: at a
                // wide zoom a fast series climbs further between two bars than
                // any point tolerance would forgive, and a line you can only
                // grab directly over a bar is a line that ignores most clicks.
                let stop = end.min(column.len());
                let mut previous: Option<egui::Pos2> = None;
                for (row, value) in column[start..stop].iter().copied().enumerate() {
                    let row = row + start;
                    if value.is_nan() {
                        previous = None;
                        continue;
                    }
                    let point =
                        egui::pos2(self.viewport.x_center(row, right, total), scale.y(value));
                    if let Some(from) = previous {
                        let distance = drawings::distance_to_segment(pos, from, point);
                        if distance <= PLOT_PICK_TOLERANCE_PX
                            && best.is_none_or(|(closest, _)| distance < closest)
                        {
                            best = Some((distance, view.slot));
                        }
                    }
                    previous = Some(point);
                }
            }
        }
        best.map(|(_, slot)| slot)
    }
}
