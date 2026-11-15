//! Native replay browser painting and transport input. Semantic state lives in ui-model.
#[cfg(test)]
use crate::surfaces::replay_browser::BrowserPaint;
use crate::theme;
use eframe::egui;
use quantick_feed::{ReplayControl, ReplayRequest};
#[cfg(test)]
use quantick_replay::format;
#[cfg(test)]
use quantick_ui_model::replay_browser::Intent;
pub const TRANSPORT_HEIGHT: f32 = 30.0;
#[cfg(any(feature = "scenario-harness", test))]
pub const GET_DATA_ENV: &str = "QUANTICK_REPLAY_GET_DATA";
const TRACK_HEIGHT: f32 = 6.0;
const HANDLE_RADIUS: f32 = 4.5;
const HANDLE_RADIUS_HOVERED: f32 = 6.0;
pub struct MarketMenu<'a> {
    pub current: Option<&'a str>,
    pub catalogue: Vec<&'a str>,
}
pub enum ReplayAction {
    Open(Box<ReplayRequest>),
    Control(ReplayControl),
    Close,
}
/// A small speed button, e.g. `10×`.
pub(crate) fn speed_chip(ui: &mut egui::Ui, speed: f32, selected: bool) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let text = if speed.fract() == 0.0 {
        format!("{speed:.0}×")
    } else {
        format!("{speed}×")
    };
    let mut rich = egui::RichText::new(text).monospace().small();
    if selected {
        rich = rich.color(palette.replay_plate_ink()).strong();
    }
    let mut button = egui::Button::new(rich).min_size(egui::vec2(30.0, 18.0));
    if selected {
        button = button.fill(palette.replay_plate());
    }
    ui.add(button)
}

/// The `REPLAY` tag on its replay plate: the one loud thing on the bar.
pub(crate) fn badge(ui: &mut egui::Ui, text: &str) {
    let palette = theme::get(ui.ctx());
    egui::Frame::none()
        .fill(palette.replay_plate())
        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
        .rounding(3.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .color(palette.replay_plate_ink())
                    .small()
                    .strong(),
            );
        });
}

/// The seek track. Returns the fraction to jump to, once per gesture.
///
/// A held handle only moves `scrub`, which is what the track draws; the jump is
/// emitted when the handle is let go, or immediately on a click. That bound is
/// not cosmetic: every seek rebuilds the chart from the session's history, so
/// emitting one per dragged frame would re-ingest the whole recording at frame
/// rate — hundreds of megabytes a second on a full trading day.
pub(crate) fn seek_track(ui: &mut egui::Ui, progress: f32, scrub: &mut Option<f32>) -> Option<f32> {
    let palette = theme::get(ui.ctx());
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, TRACK_HEIGHT + 8.0),
        egui::Sense::click_and_drag(),
    );
    let track = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - TRACK_HEIGHT / 2.0),
        egui::vec2(width, TRACK_HEIGHT),
    );

    // Keep the last position the pointer held: on the frame the button is
    // released egui may already report no interaction position, and that frame
    // is exactly the one carrying the seek.
    if let Some(pointer) = response.interact_pointer_pos() {
        *scrub = Some(((pointer.x - track.left()) / width.max(1.0)).clamp(0.0, 1.0));
    }
    let held = response.dragged() || response.is_pointer_button_down_on();
    let shown = match *scrub {
        Some(fraction) if held => fraction,
        _ => progress.clamp(0.0, 1.0),
    };

    let painter = ui.painter();
    painter.rect_filled(track, 3.0, palette.control());
    let filled = egui::Rect::from_min_size(track.min, egui::vec2(width * shown, TRACK_HEIGHT));
    painter.rect_filled(filled, 3.0, palette.replay_plate());
    painter.circle_filled(
        egui::pos2(filled.right(), track.center().y),
        if response.hovered() || held {
            HANDLE_RADIUS_HOVERED
        } else {
            HANDLE_RADIUS
        },
        palette.replay_plate(),
    );

    if response.drag_stopped() || response.clicked() {
        return scrub.take();
    }
    if !held {
        *scrub = None;
    }
    None
}

pub(crate) use quantick_replay::deals::thousands;

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_REPLAY_GET_DATA"];

#[cfg(test)]
mod tests;
