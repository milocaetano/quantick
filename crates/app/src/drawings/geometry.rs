//! Screen coordinates crossing between egui's types and the headless
//! drawings' ([`Px`], [`PxRect`]). Both sides hold the same `f32`s, so every
//! crossing is exact.

use eframe::egui;
use quantick_drawings::{Px, PxRect};

#[must_use]
pub fn px(pos: egui::Pos2) -> Px {
    Px::new(pos.x, pos.y)
}

#[must_use]
pub fn pos(px: Px) -> egui::Pos2 {
    egui::pos2(px.x, px.y)
}

#[must_use]
pub fn px_rect(rect: egui::Rect) -> PxRect {
    PxRect::from_min_max(px(rect.min), px(rect.max))
}

#[must_use]
pub fn egui_rect(rect: PxRect) -> egui::Rect {
    egui::Rect::from_min_max(pos(rect.min), pos(rect.max))
}

pub fn distance_to_segment(position: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    quantick_drawings::distance_to_segment(px(position), px(start), px(end))
}
