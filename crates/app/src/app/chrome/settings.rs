//! Feed the Settings model its owners' values and carry out what its rows ask.
use eframe::egui;
use quantick_control_handlers::appearance::AppearancePort as _;
use quantick_control_schema::settings_sections::SettingsSection;
use quantick_ui_model::settings_window::{Action, AgentFacts, AgentNote, Effects, Facts, Intent};

use crate::app::{QuantickApp, deal_recording_wiring as deals, menu_bar::MenuCommand};
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
pub(in crate::app) fn draw(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    commands: &mut Vec<MenuCommand>,
) {
    if app.surfaces.settings_window.model_state.shown().is_none() {
        return;
    }
    let facts = Box::new(facts(app));
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
            settle(app, ctx, action, commands);
        }
    }
}

fn facts(app: &QuantickApp) -> Facts {
    let access = app.control.control_access.as_ref();
    let (on, can_flip) = access.map_or((false, false), ControlAccess::switch_state);
    let note = access.and_then(|access| {
        (access
            .start_error()
            .map(|error| AgentNote::Error(error.to_owned())))
        .or_else(|| (access.status_note()).map(|note| AgentNote::Status(note.to_owned())))
    });
    Facts {
        timezone: app.runtime.tz,
        theme: app.runtime.appearance.builtin(),
        show_perf: app.health.show_perf,
        progressive_history: app.runtime.history.progressive_history,
        venue_lead_in: app.runtime.history.venue_lead_in,
        record_deals: deals::default_for(app),
        agent: AgentFacts {
            on,
            can_flip,
            hover: access.map_or("Local agent access…", ControlAccess::menu_label),
            note,
        },
    }
}

fn settle(app: &mut QuantickApp, ctx: &egui::Context, action: Action, out: &mut Vec<MenuCommand>) {
    match action {
        Action::OpenAppearance => out.push(MenuCommand::OpenAppearance),
        Action::OpenAgentAccess => out.push(MenuCommand::OpenAgentAccess),
        Action::Timezone(tz) => out.push(MenuCommand::SetTimezone(tz)),
        Action::PerfReadings(on) => out.push(MenuCommand::SetShowPerf(on)),
        Action::ProgressiveHistory(on) => out.push(MenuCommand::SetProgressiveHistory(on)),
        Action::VenueLeadIn(on) => out.push(MenuCommand::SetVenueLeadIn(on)),
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
