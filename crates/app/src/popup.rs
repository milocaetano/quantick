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
//! The window is centred from its drawn size while that settles, then held by its
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
    let Some(drawn) = ctx.memory(|memory| memory.area_rect(id)) else {
        return window;
    };
    // (frames of settling left, where it was last put). A window's size can
    // take a few frames to settle, so it is re-centred while it does — and
    // left alone the moment a hand has moved it.
    let state = id.with("centring");
    let (left, placed) = ctx
        .data(|data| data.get_temp(state))
        .unwrap_or((SETTLE_FRAMES, drawn.min));
    if left == 0 || drawn.min.distance(placed) > 0.5 {
        ctx.data_mut(|data| data.insert_temp(state, (0_u8, placed)));
        return window;
    }
    let at = ctx.available_rect().center() - drawn.size() / 2.0;
    ctx.data_mut(|data| data.insert_temp(state, (left - 1, at)));
    window.current_pos(at)
}

/// How many frames a settings window is re-centred for while its size
/// settles after it first appears.
const SETTLE_FRAMES: u8 = 4;

#[cfg(test)]
mod tests;
