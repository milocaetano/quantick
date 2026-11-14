//! Stable dock vocabulary and pointer hit testing for app callers.
pub use quantick_workspace::dock::DockTab;
#[cfg(test)]
pub(crate) use quantick_workspace::dock::{DOCK_MAX_WIDTH, DOCK_MIN_WIDTH};

use eframe::egui;
use quantick_ui_model::dock::State;
/// Whether the pointer is on the dock — its strip or its open body, where
/// they were last laid out. Read before the dock draws: Del there is the
/// dock's (the Trades ledger deletes a session with it), not the chart's.
#[must_use]
pub fn holds_pointer(dock: &State, ctx: &egui::Context, alerts_open: bool) -> bool {
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return false;
    };
    let viewport = ctx.viewport_id();
    let body = dock
        .tab()
        .filter(|_| dock.visible())
        .map(|tab| egui::Id::new((viewport, "dock_body", tab.index())));
    [
        dock.visible()
            .then(|| egui::Id::new((viewport, "dock_strip"))),
        body,
        alerts_open.then(|| egui::Id::new((viewport, "alerts_manager_panel"))),
    ]
    .into_iter()
    .flatten()
    .filter_map(|id| egui::containers::panel::PanelState::load(ctx, id))
    .any(|panel| panel.rect.contains(pointer))
}
