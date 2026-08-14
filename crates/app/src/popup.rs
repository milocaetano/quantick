//! Where a floating window first opens.
//!
//! One rule, named once, for every popup the chart puts on screen: the
//! settings dialogs, the drawing inspector, the object manager. They used to
//! carry a hardcoded `pos2(90, 120)` each, a few pixels apart, all of them in
//! the top-left — which is to say *over the chart*, and over the drawings and
//! volume profiles whose settings the popup was opened to change. A window
//! that covers the thing it configures makes its own live preview useless.
//!
//! The corner is the bottom-left, and it is the least-bad corner rather than
//! an arbitrary one: the top-left is claimed by the indicator legend, the
//! position HUD and the order-flow key, the top-right by the book status badge,
//! and the right edge by the price axis. What is left is the bottom-left, where
//! the chart carries a time axis and little else.
//!
//! Measured from [`egui::Context::available_rect`] rather than from the window,
//! so the toolbar and the status bar — both panels — are excluded by
//! construction, and no caller has to know how tall either is.

use eframe::egui;

/// Gap between a popup and the edges of the area it opens in, in pixels.
const MARGIN_PX: f32 = 12.0;

/// Where a popup should first appear, to be used with
/// [`Align2::LEFT_BOTTOM`](egui::Align2::LEFT_BOTTOM) as the window's pivot.
///
/// The pivot matters: anchored by its top-left a window placed here would grow
/// *downward*, off the bottom of the screen, taking its buttons with it. See
/// [`anchor`] for the pair.
#[must_use]
pub(crate) fn position(ctx: &egui::Context) -> egui::Pos2 {
    ctx.available_rect().left_bottom() + egui::vec2(MARGIN_PX, -MARGIN_PX)
}

/// The pivot [`position`] is expressed in. Always paired with it, so no caller
/// can place a window by the wrong corner and push its controls off screen.
pub(crate) const fn anchor() -> egui::Align2 {
    egui::Align2::LEFT_BOTTOM
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule, at every window size the app allows: inside the available
    /// area, in its bottom-left, and clear of the edges by the margin. A fixed
    /// `y` would be the bottom of exactly one window.
    #[test]
    fn popups_open_in_the_bottom_left_of_whatever_room_there_is() {
        for size in [
            egui::vec2(1920.0, 1080.0),
            egui::vec2(1500.0, 900.0),
            egui::vec2(900.0, 560.0),
        ] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), size)),
                ..egui::RawInput::default()
            };
            let mut at = egui::Pos2::ZERO;
            let _ = ctx.run(input, |ctx| at = position(ctx));

            assert!(
                (at.x - MARGIN_PX).abs() < f32::EPSILON,
                "{size:?}: a margin off the left edge, not centred ({at:?})"
            );
            assert!(
                at.y > size.y / 2.0 && at.y <= size.y,
                "{size:?}: in the bottom half and on screen ({at:?})"
            );
            assert_eq!(anchor(), egui::Align2::LEFT_BOTTOM, "grows upward");
        }
    }
}
