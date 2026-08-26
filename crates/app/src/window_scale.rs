//! Correcting a window size the platform reported in the wrong unit.
//!
//! egui is told the window's size in *points* and paints at
//! `pixels_per_point`; the two multiply out to the surface the platform
//! actually gave us. On Windows, at a scale factor other than 1, maximising
//! breaks that identity: the client size arrives in **physical pixels** while
//! the scale factor stays put, so egui lays out a screen 1.5× too large and
//! paints a third of the chart outside the window. What falls off is the right
//! and bottom edge — which on this chart is the toolbar's layer group, the
//! price axis, the live strip and the dock rail.
//!
//! This is an upstream bug, not ours: `emilk/egui#7648` (still open at 0.33.1)
//! and `emilk/egui#7095` are the same family, and the jump from the 0.29 we
//! build against to the current release is 262 compile errors for a version
//! that still has it. So the size is corrected here, on the way in, and the
//! rule is written as one pure function with the arithmetic on show.
//!
//! **The invariant.** A window's client area cannot be larger than the screen
//! it is on: `size_in_points × scale ≤ monitor_in_pixels`. When that fails, the
//! size we were handed is already in pixels, and dividing by the scale recovers
//! the points exactly — 2560 px ÷ 1.5 = 1706.7 pt, which paints back to 2560 px.
//!
//! **The limit, stated rather than hidden.** A window stretched across two
//! side-by-side monitors is legitimately wider than the monitor egui reports,
//! so width alone must never trigger a correction. Requiring *both* axes to
//! overflow is what separates the two: a spanning window is wider without being
//! taller, and a wrongly-scaled one overflows both by the same factor. A window
//! spanning monitors stacked vertically *and* horizontally would fool this, and
//! is left uncorrected on purpose — a rare arrangement, and drawing it small is
//! worse than drawing it as the platform asked.

use eframe::egui;

/// How far past the monitor a size may sit before we call it wrong. Guards the
/// float compare against a window that is legitimately flush with the screen
/// edge, where the two sides land a rounding error apart.
const OVERFLOW_SLACK_PX: f32 = 1.0;

/// The size egui should lay out in, when the one we were handed cannot be
/// points. `None` means the size is already right and nothing should change.
///
/// `size` is what the platform reported, `scale` the pixels-per-point it
/// reported alongside, and `monitor` the screen size — which eframe gives in
/// physical pixels, the unit inconsistency this whole module is downstream of.
#[must_use]
pub(crate) fn corrected_size(
    size: egui::Vec2,
    scale: f32,
    monitor: egui::Vec2,
) -> Option<egui::Vec2> {
    // At scale 1 the two units are the same number and there is nothing to get
    // wrong. A non-finite or non-positive scale is not something to divide by.
    if !(scale.is_finite() && scale > 1.0) {
        return None;
    }
    if !(size.x > 0.0 && size.y > 0.0 && monitor.x > 0.0 && monitor.y > 0.0) {
        return None;
    }
    let overflows = |points: f32, monitor_px: f32| points * scale > monitor_px + OVERFLOW_SLACK_PX;
    // Both axes, never one — see the module's note on spanning windows.
    if !(overflows(size.x, monitor.x) && overflows(size.y, monitor.y)) {
        return None;
    }
    Some(size / scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reported bug, in the numbers it was measured with: a 2560×1369
    /// client on a 2560×1440 screen at scale 1.5, which egui was laying out as
    /// 2560×1369 *points* and painting as 3840×2054 pixels.
    #[test]
    fn a_maximised_window_reported_in_pixels_is_corrected_back_to_points() {
        let corrected =
            corrected_size(egui::vec2(2560.0, 1369.33), 1.5, egui::vec2(2560.0, 1440.0))
                .expect("3840x2054 px of content cannot fit a 2560x1440 screen");
        // Divided by the scale, and back to exactly the surface we have.
        assert!((corrected.x - 1706.67).abs() < 0.01, "{corrected:?}");
        assert!(
            (corrected.x * 1.5 - 2560.0).abs() < 0.01,
            "paints back to px"
        );
    }

    /// The same window before it was maximised, which was never wrong.
    #[test]
    fn a_window_that_fits_its_screen_is_left_alone() {
        assert_eq!(
            corrected_size(egui::vec2(1400.0, 900.0), 1.5, egui::vec2(2560.0, 1440.0)),
            None,
            "1400x1.5 = 2100 px fits a 2560 px screen; nothing to correct"
        );
    }

    /// Scale 1 is every machine that never sees this bug, and the arithmetic
    /// there is a no-op — so the rule must not fire on it whatever the sizes.
    #[test]
    fn scale_one_is_never_corrected() {
        assert_eq!(
            corrected_size(egui::vec2(2560.0, 1369.0), 1.0, egui::vec2(2560.0, 1440.0)),
            None
        );
    }

    /// The false positive this rule is shaped around: a window dragged wide
    /// across two side-by-side monitors is genuinely wider than the monitor
    /// egui names, and shrinking it would be the bug rather than the fix.
    #[test]
    fn a_window_spanning_two_monitors_is_not_mistaken_for_a_bad_scale() {
        assert_eq!(
            corrected_size(egui::vec2(3000.0, 800.0), 1.5, egui::vec2(2560.0, 1440.0)),
            None,
            "wider than one screen but not taller: a spanning window, not a unit mix-up"
        );
    }

    /// A window flush with the screen edge must not be shaved by a rounding
    /// error — the reason the compare carries slack at all.
    #[test]
    fn a_window_exactly_filling_the_screen_survives_the_float_compare() {
        assert_eq!(
            corrected_size(
                egui::vec2(2560.0 / 1.5, 1440.0 / 1.5),
                1.5,
                egui::vec2(2560.0, 1440.0)
            ),
            None,
            "a correctly-reported full-screen window is not an overflow"
        );
    }

    /// Nonsense in, nothing out: a zero monitor or a garbage scale must not
    /// produce a zero-sized or infinite screen for the chart to lay out in.
    #[test]
    fn degenerate_input_changes_nothing() {
        let screen = egui::vec2(2560.0, 1369.0);
        let monitor = egui::vec2(2560.0, 1440.0);
        assert_eq!(corrected_size(screen, f32::NAN, monitor), None);
        assert_eq!(corrected_size(screen, f32::INFINITY, monitor), None);
        assert_eq!(corrected_size(screen, 0.0, monitor), None);
        assert_eq!(corrected_size(screen, -1.5, monitor), None);
        assert_eq!(corrected_size(screen, 1.5, egui::Vec2::ZERO), None);
        assert_eq!(corrected_size(egui::Vec2::ZERO, 1.5, monitor), None);
    }
}
