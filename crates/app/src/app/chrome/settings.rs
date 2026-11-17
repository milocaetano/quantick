//! Feed the Settings model its owners' values and carry out what its rows ask.
use eframe::egui;
use quantick_control_handlers::settings::{SettingsAct, SettingsOwner, SettingsReadings};
use quantick_control_schema::settings_sections::SettingsSection;
use quantick_ui_model::settings_window::{AgentNote, Effects, Facts, Intent};

use crate::app::chart_layers_wiring::LayerWiring;
use crate::app::{deal_recording_wiring as deals, desk_wiring::DeskAdapter};
use crate::control::ControlAccess;

/// Bring Settings up on `section`, on this window.
pub(in crate::app) fn open(
    desk: &mut DeskAdapter,
    viewport: egui::ViewportId,
    section: SettingsSection,
) {
    desk.chrome.window_menu.windows_viewport = Some(viewport);
    desk.surfaces.settings_window.update(Intent::Open(section));
}

/// Draw Settings while it is up, on the window that owns it.
pub(in crate::app) fn draw(desk: &mut DeskAdapter, ctx: &egui::Context) {
    if desk.surfaces.settings_window.model_state.shown().is_none() {
        return;
    }
    let access = desk.control.control_access.as_ref();
    let agent_note = access.and_then(|gate| AgentNote::of(gate.start_error(), gate.status_note()));
    let facts = Box::new(Facts {
        readings: desk.readings(),
        agent: access.map(ControlAccess::panel_facts).unwrap_or_default(),
        agent_note,
    });
    desk.surfaces.settings_window.update(Intent::Observe(facts));
    let response = desk.surfaces.draw_slot_unscoped(
        quantick_ui_model::Slot::Settings,
        &mut crate::surfaces::ViewContext { ctx, ui: None },
    );
    for effect in response.model_effects {
        if let quantick_ui_model::HostEffect::SettingsWindow(Effects { action: Some(act) }) = effect
            && let Err(error) = desk.settle(act)
        {
            desk.surfaces
                .settings_window
                .update(Intent::ThemeFailed(error));
        }
    }
}

impl SettingsOwner for DeskAdapter<'_> {
    fn readings(&self) -> SettingsReadings {
        let feed = &self.runtime.active_tab().feed_id;
        SettingsReadings {
            timezone: self.runtime.tz,
            theme: self.runtime.appearance.builtin(),
            show_perf: self.health.show_perf,
            progressive_history: self.runtime.history.progressive_history,
            venue_lead_in: self.runtime.history.venue_lead_in,
            record_deals: (self.chrome.record_deals)
                .unwrap_or_else(|| self.config.records_deals(feed)),
        }
    }

    fn settle(&mut self, act: SettingsAct) -> Result<(), String> {
        match act {
            SettingsAct::OpenAppearance => {
                let chart_settings = &mut self.surfaces.chart_settings.model_state;
                (LayerWiring::of(self.runtime, chart_settings))
                    .open_settings(crate::surfaces::ChartSettingsTab::Candles);
            }
            SettingsAct::OpenAgentAccess => {
                if self.control.control_access.is_some() {
                    (self.surfaces.access_panel)
                        .update(quantick_ui_model::access_panel::Intent::Open);
                }
            }
            SettingsAct::Timezone(tz) => self.runtime.tz = tz,
            SettingsAct::PerfReadings(on) => self.health.show_perf = on,
            SettingsAct::ProgressiveHistory(on) => self.runtime.history.progressive_history = on,
            SettingsAct::VenueLeadIn(on) => self.runtime.history.venue_lead_in = on,
            SettingsAct::RecordDeals(on) => {
                deals::set_default(&mut self.chrome.record_deals, &mut self.runtime.tabs, on);
            }
            SettingsAct::AgentSwitch(on) => {
                let ctx = self.presentation.context.as_ref();
                if let (Some(access), Some(ctx)) = (self.control.control_access.as_mut(), ctx) {
                    access.set_switch(ctx, on);
                }
            }
            SettingsAct::Theme(choice) => {
                let applied = self.runtime.appearance.apply_builtin(choice, true);
                applied.map_err(|error| error.to_string())?;
                let ctx = self.presentation.context.as_ref();
                crate::app::frame::sync_appearance(ctx, self.runtime, &mut self.drawings.presets);
            }
        }
        Ok(())
    }
}
