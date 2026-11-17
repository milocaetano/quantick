//! Paint chart settings from the registered headless model.
pub(crate) mod candles;
pub(crate) mod footprint;
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use eframe::egui;
pub(crate) use quantick_ui_model::chart_settings::ChartSettingsTab;
use quantick_ui_model::chart_settings::{Effects, Intent, Model};
use std::time::Instant;
#[derive(Default)]
pub(crate) struct View {
    epoch: Option<Instant>,
}
impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "chart-settings"
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, env: &SurfaceEnv<'_>) -> Option<Intent> {
        let on = |name: &str| crate::hooks::captured::var(name).is_some_and(|value| value == "1");
        let tab = if on("QUANTICK_FOOTPRINT_PANEL") {
            ChartSettingsTab::Footprint
        } else if on("QUANTICK_STYLE_PANEL") {
            ChartSettingsTab::Candles
        } else {
            return None;
        };
        Some(Intent::OpenCurrent {
            tab,
            target: quantick_chart::pane_style::StyleTarget {
                tab_id: env.active_tab,
                pane_id: env.picker_pane_id,
            },
        })
    }
    fn paint_updated(
        &mut self,
        session: &mut quantick_ui_model::UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) {
        let Some(env) = env else {
            return;
        };
        let epoch = *self.epoch.get_or_insert(env.now);
        let now =
            u64::try_from(env.now.saturating_duration_since(epoch).as_millis()).unwrap_or(u64::MAX);
        if session.state().is_open() {
            let mut open = true;
            crate::popup::centred(
                egui::Window::new("chart settings"),
                egui::Id::new(self.id()),
                target.context(),
            )
            .open(&mut open)
            .default_width(390.0)
            .min_width(350.0)
            .resizable(true)
            .show(target.context(), |ui| {
                if session.state().tab == ChartSettingsTab::Candles {
                    ui.label(&session.state().target_label);
                    ui.strong("Only this chart");
                }
                if session.state().editor.stale {
                    ui.label("This chart is no longer available. Open settings on another chart.");
                    ui.disable();
                }
                ui.horizontal(|ui| {
                    for tab in ChartSettingsTab::ALL {
                        if ui
                            .selectable_label(session.state().tab == tab, tab.title())
                            .clicked()
                        {
                            session.update(Intent::SelectTab(tab));
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(egui::Id::new(self.id()).with("body"))
                    .max_height(650.0)
                    .show(ui, |ui| match session.state().tab {
                        ChartSettingsTab::Candles => candles::draw(ui, session, env, now),
                        ChartSettingsTab::Footprint => footprint::draw(ui, session),
                    });
            });
            if !open {
                session.update(Intent::Close);
            }
        }
        session.update(Intent::Tick(now));
    }
}
pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    SurfaceResponse::effect_if(
        effects != Effects::default(),
        quantick_ui_model::HostEffect::ChartSettings(effects),
    )
}
#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_STYLE_PANEL", "QUANTICK_FOOTPRINT_PANEL"];
