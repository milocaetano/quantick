//! The two values one paint frame passes between its orchestrator and its
//! painters: [`DrawFrame`], what the frame has resolved when the layers go
//! down, and [`AxisChips`], what the axes stand aside for.
//!
//! Their own module so that `draw_chart.rs` and `layer_painters.rs` both read
//! from here and neither reads from the other: two siblings importing each
//! other are one file with two names, and `guards/src/cycle.rs` cannot see a
//! cycle between children of one module.

use eframe::egui;

use crate::chart::PriceScale;
use crate::plot_area::PlotAreas;
use crate::pointer_compass;

use super::PointerCompass;

/// What one paint frame has resolved by the time the layers go down: the
/// geometry, the visible slices of both series and the price scale.
///
/// Built once per frame on the stack by [`ChartPane::draw_chart`], after the
/// viewport is clamped and the scale is known, and lent to every painter in
/// `layer_painters.rs` by reference. Every field is a `Copy` value or a borrow
/// of something the frame already holds — nothing is cloned into it, and no
/// bar is walked to fill it. It is the narrow context the painters read
/// instead of the pane's whole state, and the reason they can be `&self`
/// while the slices of `self.state` stay borrowed across the frame.
pub(super) struct DrawFrame<'a> {
    pub(super) painter: &'a egui::Painter,
    pub(super) areas: &'a PlotAreas,
    pub(super) chart_rect: egui::Rect,
    pub(super) history_rect: egui::Rect,
    pub(super) right: f32,
    pub(super) total: usize,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) closed_start: usize,
    pub(super) closed_total: usize,
    pub(super) scale: PriceScale,
    pub(super) prefix: &'a [quantick_engine::Bar],
    pub(super) closed: &'a [quantick_engine::Bar],
    pub(super) partial: Option<&'a quantick_engine::Bar>,
    pub(super) partial_visible: Option<&'a quantick_engine::Bar>,
    pub(super) visible_prefix: &'a [quantick_engine::Bar],
    pub(super) visible_state: &'a [quantick_engine::Bar],
    pub(super) canvas_background: egui::Color32,
    pub(super) cw: f32,
}

/// What one frame's axes stand aside for, as `axis_claims` decides it.
///
/// Three fields with names rather than a tuple: the two claim lists are the
/// same type, and a tuple would let the price axis's chips and the time
/// strip's be swapped by a `let` that still compiles.
pub(super) struct AxisChips {
    pub(super) compass: Option<PointerCompass>,
    pub(super) price: pointer_compass::AxisClaims,
    pub(super) time: pointer_compass::AxisClaims,
}
