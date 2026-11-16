//! Scripted builds learn where a theme or menu row was painted.
use eframe::egui;

/// Publish the real hit rectangle only in scripted builds; no popup state changes.
pub(crate) fn record(_response: &egui::Response, _target: &'static str) {
    #[cfg(any(feature = "scenario-harness", test))]
    _response.ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new(("scripted_menu", _target)),
            Some(_response.rect),
        )
    });
}
