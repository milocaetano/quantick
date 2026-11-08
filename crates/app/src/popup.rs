//! Where a settings window first opens.
//!
//! One rule, named once, for every settings-style window the app puts on
//! screen: indicator settings, chart settings, exit strategies, the market
//! picker, agent access and the performance report. They open **centred** in
//! the room the panels leave, and stay draggable.
//!
//! The previous rule sent them to the bottom-left corner so they would not
//! cover the chart. In use that read as a window appearing somewhere
//! unrelated to the click, far from where the eye was, and the trader asked
//! for the centre instead. A settings window is a short, deliberate visit, not
//! a live overlay; the centre is where a dialog is looked for. A trader who
//! wants it elsewhere drags it once: the window is placed with `default_pos`,
//! which egui applies only while it remembers no position for that window id,
//! so a dragged window reopens where the hand left it.
//!
//! Drawing settings are the exception and keep their own rule: they open
//! beside the drawing they configure (see the drawing inspector's
//! `placement`).
//!
//! Measured from [`egui::Context::available_rect`] rather than from the window,
//! so the toolbar and the status bar — both panels — are excluded by
//! construction, and no caller has to know how tall either is.

use eframe::egui;

/// Where a settings window should first appear, to be used with
/// [`anchor`] as the window's pivot.
///
/// The pivot matters: anchored by its top-left a window placed at the centre
/// would hang into the bottom-right quarter and, if tall, off the screen.
#[must_use]
pub(crate) fn position(ctx: &egui::Context) -> egui::Pos2 {
    ctx.available_rect().center()
}

/// The pivot [`position`] is expressed in. Always paired with it, so no caller
/// can place a window by the wrong corner and push its controls off screen.
pub(crate) const fn anchor() -> egui::Align2 {
    egui::Align2::CENTER_CENTER
}

/// Give `window` the settings placement: centred on first open, draggable,
/// and reopened wherever it was dragged to.
pub(crate) fn centred<'open>(
    window: egui::Window<'open>,
    ctx: &egui::Context,
) -> egui::Window<'open> {
    window.default_pos(position(ctx)).pivot(anchor())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, size: egui::Vec2, run: impl FnMut(&egui::Context)) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), size)),
            ..egui::RawInput::default()
        };
        let _ = ctx.run(input, run);
    }

    /// The rule, at every window size the app allows: the centre of the
    /// available area, pivoted on the window's own centre. A fixed point would
    /// be the centre of exactly one window.
    #[test]
    fn settings_windows_open_in_the_centre_of_whatever_room_there_is() {
        for size in [
            egui::vec2(1920.0, 1080.0),
            egui::vec2(1500.0, 900.0),
            egui::vec2(900.0, 560.0),
        ] {
            let ctx = egui::Context::default();
            let mut at = egui::Pos2::ZERO;
            frame(&ctx, size, |ctx| at = position(ctx));
            assert!(
                (at.x - size.x / 2.0).abs() < 0.5 && (at.y - size.y / 2.0).abs() < 0.5,
                "{size:?}: centred, not cornered ({at:?})"
            );
            assert_eq!(
                anchor(),
                egui::Align2::CENTER_CENTER,
                "pivots on its centre"
            );
        }
    }

    /// The panels are excluded: a top panel pushes the centre down by half
    /// its height, so the window centres in the room actually left.
    #[test]
    fn the_centre_is_measured_in_the_room_the_panels_leave() {
        let ctx = egui::Context::default();
        let size = egui::vec2(1200.0, 800.0);
        let mut at = egui::Pos2::ZERO;
        frame(&ctx, size, |ctx| {
            egui::TopBottomPanel::top("toolbar")
                .exact_height(100.0)
                .show(ctx, |_| {});
            at = position(ctx);
        });
        assert!(at.y > size.y / 2.0, "below the screen centre: {at:?}");
        assert!((at.x - size.x / 2.0).abs() < 0.5, "{at:?}");
    }

    /// Opened centred, fully on screen, and — once dragged — reopened at the
    /// dragged spot rather than snapped back to the centre.
    #[test]
    fn a_centred_window_is_fully_visible_and_remembers_a_drag() {
        for size in [egui::vec2(1920.0, 1080.0), egui::vec2(900.0, 560.0)] {
            let ctx = egui::Context::default();
            let id = egui::Id::new("settings-under-test");
            // `moved_to` is what a drag leaves behind: a position egui
            // remembers for this window id.
            let show = |ctx: &egui::Context, moved_to: Option<egui::Pos2>| {
                let mut window = centred(egui::Window::new("Settings").id(id), ctx);
                if let Some(at) = moved_to {
                    window = window.current_pos(at);
                }
                window.show(ctx, |ui| {
                    ui.set_min_size(egui::vec2(320.0, 240.0));
                });
            };
            let rect = |ctx: &egui::Context| {
                ctx.memory(|memory| memory.area_rect(id))
                    .expect("the window was laid out")
            };
            for _ in 0..3 {
                frame(&ctx, size, |ctx| show(ctx, None));
            }
            let opened = rect(&ctx);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            assert!(
                screen.contains_rect(opened),
                "{size:?}: on screen {opened:?}"
            );
            assert!(
                opened.center().distance(screen.center()) < 2.0,
                "{size:?}: centred {opened:?}"
            );

            let dragged = egui::pos2(260.0, 200.0);
            frame(&ctx, size, |ctx| show(ctx, Some(dragged)));
            // Closed for a frame, then opened again.
            frame(&ctx, size, |_| {});
            for _ in 0..2 {
                frame(&ctx, size, |ctx| show(ctx, None));
            }
            let reopened = rect(&ctx);
            assert!(
                reopened.center().distance(dragged) < 2.0,
                "{size:?}: reopened where dragged, not re-centred {reopened:?}"
            );
        }
    }
}
