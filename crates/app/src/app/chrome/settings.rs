//! Feed the Settings model its owners' values and carry out what its rows ask.
use eframe::egui;
use quantick_control_handlers::appearance::AppearancePort as _;
use quantick_control_schema::settings_sections::SettingsSection;
use quantick_ui_model::settings_window::{Action, AgentNote, Effects, Facts, Intent};

use crate::app::{LayersPort as _, QuantickApp, deal_recording_wiring as deals};
use crate::control::ControlAccess;

/// Bring Settings up on `section`, on this window.
pub(in crate::app) fn open(
    app: &mut QuantickApp,
    viewport: egui::ViewportId,
    section: SettingsSection,
) {
    app.chrome.window_menu.windows_viewport = Some(viewport);
    app.surfaces.settings_window.update(Intent::Open(section));
}

/// Draw Settings while it is up, on the window that owns it.
pub(in crate::app) fn draw(app: &mut QuantickApp, ctx: &egui::Context) {
    if app.surfaces.settings_window.model_state.shown().is_none() {
        return;
    }
    let access = app.control.control_access.as_ref();
    let agent_note = access.and_then(|access| match access.start_error() {
        Some(error) => Some(AgentNote::Error(error.to_owned())),
        None => (access.status_note()).map(|note| AgentNote::Status(note.to_owned())),
    });
    let facts = Box::new(Facts {
        timezone: app.runtime.tz,
        theme: app.runtime.appearance.builtin(),
        show_perf: app.health.show_perf,
        progressive_history: app.runtime.history.progressive_history,
        venue_lead_in: app.runtime.history.venue_lead_in,
        record_deals: deals::default_for(app),
        agent: access.map(ControlAccess::panel_facts).unwrap_or_default(),
        agent_note,
    });
    app.surfaces.settings_window.update(Intent::Observe(facts));
    let response = app.surfaces.draw_slot_unscoped(
        quantick_ui_model::Slot::Settings,
        &mut crate::surfaces::ViewContext { ctx, ui: None },
    );
    for effect in response.model_effects {
        if let quantick_ui_model::HostEffect::SettingsWindow(Effects {
            action: Some(action),
        }) = effect
        {
            settle(app, ctx, action);
        }
    }
}

pub(in crate::app) fn settle(app: &mut QuantickApp, ctx: &egui::Context, action: Action) {
    match action {
        Action::OpenAppearance => app
            .layer_wiring()
            .open_settings(crate::surfaces::ChartSettingsTab::Candles),
        Action::OpenAgentAccess => {
            if app.control.control_access.is_some() {
                app.surfaces
                    .access_panel
                    .update(quantick_ui_model::access_panel::Intent::Open);
            }
        }
        Action::Timezone(tz) => app.runtime.tz = tz,
        Action::PerfReadings(on) => app.health.show_perf = on,
        Action::ProgressiveHistory(on) => app.runtime.history.progressive_history = on,
        Action::VenueLeadIn(on) => app.runtime.history.venue_lead_in = on,
        Action::RecordDeals(on) => {
            deals::set_default(&mut app.chrome.record_deals, &mut app.runtime.tabs, on);
        }
        Action::AgentSwitch(on) => {
            if let Some(access) = app.control.control_access.as_mut() {
                access.set_switch(ctx, on);
            }
        }
        Action::Theme(choice) => match app.appearance_store_mut().apply_builtin(choice, true) {
            Ok(_) => app.publish_appearance(),
            Err(error) => {
                let failed = Intent::ThemeFailed(error.to_string());
                app.surfaces.settings_window.update(failed);
            }
        },
    }
}
