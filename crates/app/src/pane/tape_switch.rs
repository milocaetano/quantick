//! The tape switch: the chip in the canvas's top-right corner that takes the
//! live lane off the canvas and puts it back.
//!
//! The chip's geometry, its click in the input pass and its paint in the draw
//! pass live together so the pixel a press lands on is the pixel the chip is
//! drawn in. `click_on_tape` sits beside them because it reads the same divider
//! the chip's state moves.

use eframe::egui;

use crate::chart_layers::ChartLayer;
use crate::theme;

use super::{ChartPane, PaneChrome};

/// The tape switch's chip, in logical pixels.
///
/// A fixed size rather than one measured off its own text: the hit rect is
/// registered in the input pass and painted in the pass after it, and two
/// measurements of one chip are two chances for the button to be somewhere the
/// click is not.
const TAPE_SWITCH_SIZE: egui::Vec2 = egui::vec2(54.0, 18.0);
/// Inset of that chip from the canvas's top-right corner.
const TAPE_SWITCH_INSET: egui::Vec2 = egui::vec2(8.0, 4.0);
/// Gap between the switch and whatever sits to its left.
const TAPE_SWITCH_GAP_PX: f32 = 6.0;
/// Room the switch takes off the right edge, for anything else that wants the
/// same corner — the book status badge is the one thing that does.
pub(super) const TAPE_SWITCH_RESERVED_PX: f32 =
    TAPE_SWITCH_SIZE.x + TAPE_SWITCH_INSET.x + TAPE_SWITCH_GAP_PX;
/// Corner radius of the chip, matching the status badge it sits beside.
const TAPE_SWITCH_ROUNDING_PX: f32 = 3.0;
/// Chip background opacity over the canvas, resting and hovered. The resting
/// value is the status badge's, so the two read as one family of chrome.
const TAPE_SWITCH_FILL_ALPHA: u8 = 165;
const TAPE_SWITCH_HOVER_FILL_ALPHA: u8 = 210;
/// Opacity of the hover outline, relative to the chip's own accent.
const TAPE_SWITCH_HOVER_STROKE_ALPHA: f32 = 0.7;
/// Width of every line the chip draws.
const TAPE_SWITCH_STROKE_PX: f32 = 1.0;
/// State dot: how far its centre sits from the chip's left edge, and its
/// radius. Filled means the tape is on the canvas, hollow means it is not.
const TAPE_SWITCH_DOT_X_PX: f32 = 9.0;
const TAPE_SWITCH_DOT_RADIUS_PX: f32 = 3.0;
/// Where the label starts, measured from the same edge as the dot.
const TAPE_SWITCH_LABEL_X_PX: f32 = 17.0;
/// Label size, matching the status badge's.
const TAPE_SWITCH_FONT_PX: f32 = 11.0;
/// The label itself. Short by necessity: the chip sits over market data.
const TAPE_SWITCH_LABEL: &str = "tape";

/// Where the tape switch sits on a canvas this size.
///
/// The canvas's top-right corner: the tape's own corner, so the switch that
/// puts it there and takes it away is on it. One function, read by the input
/// pass and the paint pass alike.
#[must_use]
pub(crate) fn tape_switch_rect(chart_rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(
            chart_rect.right() - TAPE_SWITCH_INSET.x - TAPE_SWITCH_SIZE.x,
            chart_rect.top() + TAPE_SWITCH_INSET.y,
        ),
        TAPE_SWITCH_SIZE,
    )
}

impl ChartPane {
    /// The canvas right-click menu: one entry per chart layer, then one per
    /// indicator on this pane.
    ///
    /// The indicator entries drive `IndicatorViews::toggle_hidden` — the same
    /// state the toolbar's eye writes — so an indicator hidden here shows as
    /// hidden there, and the indicator state file remains its single home.
    /// Aim the next menu at one pane or the other, as a right-click would.
    #[cfg(test)]
    pub(crate) fn aim_context_menu_at_tape(&mut self, on_tape: bool) {
        self.context_menu.on_tape = on_tape;
    }

    /// Whether a click at this x belongs to the tape rather than the candles.
    ///
    /// Read off the divider the draw already published, never a second copy of
    /// the lane's geometry — the two could then disagree, and the menu would
    /// configure a pane the trader did not click. A canvas with no lane has no
    /// divider, and every click on it is the candles'.
    #[must_use]
    pub(super) fn click_on_tape(&self, x: f32) -> bool {
        self.frame
            .lane_divider_x
            .is_some_and(|divider| x >= divider)
    }

    /// The tape switch's click, in the input pass.
    ///
    /// Drawn by [`Self::draw_tape_switch`] in the pass after this one, off the
    /// same [`tape_switch_rect`]. Nothing is registered on a pane with no tape
    /// machinery (§11: a time pane has none), so no chip appears there to
    /// promise a band that canvas will never draw.
    pub(super) fn handle_tape_switch(
        &mut self,
        ui: &egui::Ui,
        chart_rect: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) {
        self.tape_switch_hovered = false;
        if self.orderflow.is_none() {
            return;
        }
        let on = self.layer_visible(ChartLayer::TapeChart, chrome.style);
        let response = ui.interact(
            tape_switch_rect(chart_rect),
            self.interaction_id("tape_switch"),
            egui::Sense::click(),
        );
        self.tape_switch_hovered = response.hovered();
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            // The chip is chrome on top of the canvas. A crosshair chasing the
            // pointer underneath it would say the chart is being hovered while
            // the pointer is reading a button.
            self.hover_pos = None;
        }
        let clicked = response.clicked();
        // `on_hover_ui` over `on_hover_text`: the closure runs only while the
        // pointer is actually on the chip, so the wording costs nothing on the
        // frames nobody is hovering — and this is a per-frame path.
        response.on_hover_ui(|ui| {
            ui.label(if on {
                "the tape is on — click to take it off the canvas"
            } else {
                "the tape is off — click to put it back"
            });
            ui.label(
                egui::RichText::new(ChartLayer::TapeChart.hint())
                    .size(11.0)
                    .color(theme::TEXT_MUTED),
            );
        });
        if clicked {
            self.set_layer_visible(ChartLayer::TapeChart, !on, chrome.layers);
        }
    }

    /// Paint the tape switch: a chip in the canvas's top-right corner, lit
    /// while the tape is on the canvas and muted while it is not.
    pub(super) fn draw_tape_switch(&self, painter: &egui::Painter, chart_rect: egui::Rect) {
        let Some(tape) = self.orderflow.as_ref() else {
            return;
        };
        let on = tape.lane_enabled();
        let rect = tape_switch_rect(chart_rect);
        let accent = if on { theme::ACCENT } else { theme::TEXT_MUTED };
        let rounding = egui::Rounding::same(TAPE_SWITCH_ROUNDING_PX);
        painter.rect_filled(
            rect,
            rounding,
            egui::Color32::from_black_alpha(if self.tape_switch_hovered {
                TAPE_SWITCH_HOVER_FILL_ALPHA
            } else {
                TAPE_SWITCH_FILL_ALPHA
            }),
        );
        if self.tape_switch_hovered {
            painter.rect_stroke(
                rect,
                rounding,
                egui::Stroke::new(
                    TAPE_SWITCH_STROKE_PX,
                    accent.gamma_multiply(TAPE_SWITCH_HOVER_STROKE_ALPHA),
                ),
            );
        }
        // A filled dot for on, a ring for off: the state survives a screenshot
        // read in greyscale, which colour alone would not.
        let dot = egui::pos2(rect.left() + TAPE_SWITCH_DOT_X_PX, rect.center().y);
        if on {
            painter.circle_filled(dot, TAPE_SWITCH_DOT_RADIUS_PX, accent);
        } else {
            painter.circle_stroke(
                dot,
                TAPE_SWITCH_DOT_RADIUS_PX,
                egui::Stroke::new(TAPE_SWITCH_STROKE_PX, accent),
            );
        }
        painter.text(
            egui::pos2(rect.left() + TAPE_SWITCH_LABEL_X_PX, rect.center().y),
            egui::Align2::LEFT_CENTER,
            TAPE_SWITCH_LABEL,
            egui::FontId::proportional(TAPE_SWITCH_FONT_PX),
            accent,
        );
    }
}
