//! The order-flow renderer's tunables: sizes, gaps, alphas and colours its
//! painters share. One file, so retuning a look touches one place.

use eframe::egui;

/// How far a laid-out label may spill past the radius before it is dropped.
/// Wider than tall: a bubble is a circle, and text is a horizontal band across
/// its middle, where there is more room.
pub(super) const LABEL_MAX_WIDTH_SCALE: f32 = 1.78;
/// See [`LABEL_MAX_WIDTH_SCALE`].
pub(super) const LABEL_MAX_HEIGHT_SCALE: f32 = 1.45;
/// Drop shadow that keeps a label legible over any fill colour.
pub(super) const LABEL_SHADOW_OFFSET_PX: egui::Vec2 = egui::vec2(1.0, 1.0);
/// See [`LABEL_SHADOW_OFFSET_PX`].
pub(super) const LABEL_SHADOW_ALPHA: u8 = 190;

/// FLOW volume uses its own palette, distinct from candle direction and native Tape.
pub(super) const FLOW_BUY: egui::Color32 = egui::Color32::from_rgb(112, 185, 244);
pub(super) const FLOW_SELL: egui::Color32 = egui::Color32::from_rgb(232, 175, 99);

/// Text size of the first-contour quantity label, in points.
pub(super) const FLOW_OPENING_LABEL_FONT_PX: f32 = 11.0;
/// Drop-shadow offset behind the first-contour quantity label, in pixels.
pub(super) const FLOW_OPENING_LABEL_SHADOW_OFFSET: egui::Vec2 = egui::vec2(1.0, 1.0);

/// Canvas colour the settings preview dresses its sample against.
pub(super) const PREVIEW_CANVAS_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(19, 23, 34);

/// Text size of the held-tape and retained-edge labels, in points.
pub(super) const PAST_TAPE_LABEL_FONT_PX: f32 = 11.0;
/// Where the held-tape label starts inside the lane, in pixels.
pub(super) const PAST_TAPE_LABEL_INSET: egui::Vec2 = egui::vec2(6.0, 4.0);
/// Alpha of the shade over the stretch before the retained tape begins (0-255).
pub(super) const PAST_TAPE_SHADE_ALPHA: u8 = 90;
pub(crate) use quantick_orderflow::config::painting::*;
