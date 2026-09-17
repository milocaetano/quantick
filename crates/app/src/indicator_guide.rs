//! Price-hover guides owned by individual non-price indicator panes.

use eframe::egui::{self, Stroke, pos2};

use crate::theme;

pub(crate) const LABEL: &str = "Mouse vertical line";
const DASH_PX: f32 = 4.0;
const GAP_PX: f32 = 4.0;
const WIDTH_PX: f32 = 1.0;

/// Draw the bounded, allocation-free dashed guide at an already-resolved x.
pub(crate) fn paint(painter: &egui::Painter, rect: egui::Rect, x: f32) {
    if !rect.contains(pos2(x, rect.center().y)) {
        return;
    }
    let stroke = Stroke::new(WIDTH_PX, theme::TEXT_MUTED.gamma_multiply(0.42));
    for segment in dash_segments(rect, x) {
        painter.line_segment(segment, stroke);
    }
}

fn dash_segments(rect: egui::Rect, x: f32) -> impl Iterator<Item = [egui::Pos2; 2]> {
    std::iter::successors(Some(rect.top()), move |y| Some(*y + DASH_PX + GAP_PX))
        .take_while(move |y| *y < rect.bottom())
        .map(move |y| [pos2(x, y), pos2(x, (y + DASH_PX).min(rect.bottom()))])
}

/// The menu and automated action both choose an explicit resulting state.
pub(crate) fn menu(response: &egui::Response, enabled: bool) -> Option<bool> {
    let mut next = None;
    response.context_menu(|ui| {
        let mut checked = enabled;
        if ui.checkbox(&mut checked, LABEL).changed() {
            next = Some(checked);
            ui.close_menu();
        }
    });
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_is_vertical_dashed_and_bounded() {
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_max(pos2(10.0, 20.0), pos2(110.0, 80.0));
        let output = ctx.run(Default::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            paint(&painter, rect, 42.0);
        });
        assert!(!output.shapes.is_empty());
        assert!(dash_segments(rect, 42.0).all(|[start, end]| {
            start.x == 42.0
                && end.x == 42.0
                && start.y >= rect.top()
                && end.y <= rect.bottom()
                && end.y > start.y
        }));
    }
}
