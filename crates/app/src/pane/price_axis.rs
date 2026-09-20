//! The price-axis levels a frame's drawings claim, so the axis can stand
//! aside where one of them lands.

use eframe::egui;

use crate::drawings;
use crate::pointer_compass;

/// One price a drawing declares for the price axis to tag.
///
/// Carries both the pixel and the price because the two answer different
/// questions and are read off one scale: the painter needs the height, and
/// anything reading the trader's levels as data needs the number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceAxisLevel {
    /// The object that declared it.
    pub id: drawings::DrawingId,
    /// Where it sits on the axis, in screen pixels.
    pub y: f32,
    /// What that height reads as on the pane's price scale.
    pub price: f64,
    /// The object's own colour — the tag is the object, said on the axis.
    pub color: egui::Color32,
}

/// What the price axis may not write a round number over this frame.
///
/// Two sources, kept apart because they are *stored* differently and not
/// because they mean different things: the chips the axis draws itself are a
/// pair that fits inline, and the levels are the list already gathered for
/// painting, borrowed rather than copied into a third container once a frame.
pub struct PriceAxisClaims<'a> {
    /// The pointer's tag and the last-price chip.
    pub(super) marks: pointer_compass::AxisClaims,
    /// One per level a drawing declared.
    pub(super) levels: &'a [PriceAxisLevel],
}

impl PriceAxisClaims<'_> {
    /// Every claimed height, from both sources, allocating nothing.
    pub(super) fn heights(&self) -> impl Iterator<Item = f32> + '_ {
        self.marks
            .iter()
            .copied()
            .chain(self.levels.iter().map(|level| level.y))
    }
}
