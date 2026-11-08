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
//! for the centre instead (2026-11-08). A settings window is a short,
//! deliberate visit, not a live overlay; the centre is where a dialog is
//! looked for. A trader who wants it elsewhere drags it once and it reopens
//! there.
//!
//! The window is centred **once**, from its drawn size, and then held by its
//! top-left corner like any window. Pivoting on its centre instead would make
//! it grow up and down at once whenever its content changes height — a tab
//! switch would move the tab strip out from under the pointer. egui's first
//! frame of a window is an invisible sizing pass, so the centred spot is the
//! first one seen.
//!
//! Drawing settings are the exception and keep their own rule: they open
//! beside the drawing they configure (`quantick_chart_interaction::panel_placement`).
//!
//! Measured from [`egui::Context::available_rect`] rather than from the window,
//! so the toolbar and the status bar — both panels — are excluded by
//! construction, and no caller has to know how tall either is.

use eframe::egui;

/// Give `window` the settings placement under `id`: centred on first open,
/// draggable, and reopened wherever it was left.
pub(crate) fn centred<'open>(
    window: egui::Window<'open>,
    id: egui::Id,
    ctx: &egui::Context,
) -> egui::Window<'open> {
    let window = window.id(id).pivot(egui::Align2::LEFT_TOP);
    let centred_flag = id.with("centred");
    let done = ctx.data(|data| data.get_temp::<bool>(centred_flag).unwrap_or(false));
    match ctx.memory(|memory| memory.area_rect(id)) {
        Some(drawn) if !done => {
            ctx.data_mut(|data| data.insert_temp(centred_flag, true));
            window.current_pos(ctx.available_rect().center() - drawn.size() / 2.0)
        }
        _ => window,
    }
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

    const ID: &str = "settings-under-test";

    /// A settings window `height` tall, centred, with an optional top panel
    /// and an optional position a drag left behind.
    fn show(ctx: &egui::Context, height: f32, toolbar: bool, dragged: Option<egui::Pos2>) {
        if toolbar {
            egui::TopBottomPanel::top("toolbar")
                .exact_height(100.0)
                .show(ctx, |_| {});
        }
        let mut window = centred(egui::Window::new("Settings"), egui::Id::new(ID), ctx);
        if let Some(at) = dragged {
            window = window.current_pos(at);
        }
        window.show(ctx, |ui| {
            ui.set_min_size(egui::vec2(320.0, height));
        });
    }

    fn rect(ctx: &egui::Context) -> egui::Rect {
        ctx.memory(|memory| memory.area_rect(egui::Id::new(ID)))
            .expect("the window was laid out")
    }

    /// The rule, at every window size the app allows: centred and fully on
    /// screen. A fixed point would be the centre of exactly one window.
    #[test]
    fn settings_windows_open_centred_and_on_screen() {
        for size in [
            egui::vec2(1920.0, 1080.0),
            egui::vec2(1500.0, 900.0),
            egui::vec2(900.0, 560.0),
        ] {
            let ctx = egui::Context::default();
            for _ in 0..3 {
                frame(&ctx, size, |ctx| show(ctx, 240.0, false, None));
            }
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let opened = rect(&ctx);
            assert!(screen.contains_rect(opened), "{size:?}: {opened:?}");
            assert!(
                opened.center().distance(screen.center()) < 2.0,
                "{size:?}: centred, not cornered {opened:?}"
            );
        }
    }

    /// The panels are excluded: a top panel pushes the centre down by half
    /// its height, so the window centres in the room actually left.
    #[test]
    fn the_centre_is_measured_in_the_room_the_panels_leave() {
        let ctx = egui::Context::default();
        let size = egui::vec2(1200.0, 800.0);
        for _ in 0..3 {
            frame(&ctx, size, |ctx| show(ctx, 240.0, true, None));
        }
        let room = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), size.to_pos2());
        assert!(
            rect(&ctx).center().distance(room.center()) < 2.0,
            "{:?}",
            rect(&ctx)
        );
    }

    /// Content that grows — a taller tab — extends the window downward: the
    /// title bar and tab strip stay under the pointer.
    #[test]
    fn a_taller_tab_grows_the_window_downward_without_moving_its_top() {
        let ctx = egui::Context::default();
        let size = egui::vec2(1500.0, 900.0);
        for _ in 0..3 {
            frame(&ctx, size, |ctx| show(ctx, 200.0, false, None));
        }
        let before = rect(&ctx);
        for _ in 0..2 {
            frame(&ctx, size, |ctx| show(ctx, 400.0, false, None));
        }
        let after = rect(&ctx);
        assert!(
            after.height() > before.height() + 150.0,
            "{before:?} {after:?}"
        );
        assert!(
            (after.top() - before.top()).abs() < 0.5,
            "{before:?} {after:?}"
        );
    }

    /// Once dragged, closed and opened again, it is where the hand left it,
    /// not snapped back to the centre.
    #[test]
    fn a_dragged_window_reopens_where_it_was_left() {
        let ctx = egui::Context::default();
        let size = egui::vec2(1500.0, 900.0);
        for _ in 0..3 {
            frame(&ctx, size, |ctx| show(ctx, 240.0, false, None));
        }
        let dragged = egui::pos2(40.0, 60.0);
        frame(&ctx, size, |ctx| show(ctx, 240.0, false, Some(dragged)));
        // Closed for a frame, then opened again.
        frame(&ctx, size, |_| {});
        for _ in 0..2 {
            frame(&ctx, size, |ctx| show(ctx, 240.0, false, None));
        }
        assert!(rect(&ctx).min.distance(dragged) < 0.5, "{:?}", rect(&ctx));
    }
}
