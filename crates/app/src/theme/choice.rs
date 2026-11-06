//! Native builtins use the same headless owner and publication port as control.
use eframe::egui;
use quantick_control_handlers::appearance::AppearancePort;

pub(crate) fn show(ui: &mut egui::Ui, host: &mut impl AppearancePort) {
    let menu = ui.menu_button("Theme", |ui| {
        let error_id = ui.id().with("appearance_error");
        if host.appearance_store().builtin().is_none() {
            ui.label("Custom theme");
        }
        for choice in quantick_appearance::Builtin::ALL {
            let selected = host.appearance_store().builtin() == Some(choice);
            let response = ui.selectable_label(selected, choice.label());
            record(&response, choice.label());
            if response.clicked() {
                match host.appearance_store_mut().apply_builtin(choice, true) {
                    Ok(_) => {
                        host.publish_appearance();
                        ui.data_mut(|data| data.remove::<String>(error_id));
                        ui.close_menu();
                    }
                    Err(error) => ui.data_mut(|data| data.insert_temp(error_id, error.to_string())),
                }
            }
        }
        if let Some(error) = ui.data(|data| data.get_temp::<String>(error_id)) {
            ui.colored_label(super::get(ui.ctx()).warn(), error);
        }
    });
    record(&menu.response, "Theme");
}

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
