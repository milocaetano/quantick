//! A chart-only preset picker, beside the whole-look preset controls.
use super::OrderflowView;
use eframe::egui;
use quantick_control_handlers::bubble_preset::TickSpherePane;

impl OrderflowView {
    pub(crate) fn apply_tick_preset(&mut self, name: Option<&str>) -> Result<bool, String> {
        let style = self.look.tick_style(name)?;
        Ok(self.edit_config(|config| config.volume_dots.tick_bubbles = style))
    }

    pub(super) fn draw_tick_bubbles(&mut self, ui: &mut egui::Ui, tick_chart: bool) {
        let current = self.config.volume_dots.tick_bubbles;
        let rule = TickSpherePane::new(tick_chart, self.look.asset_mode().is_some());
        let Some(choices) = rule.offered(self.look.tick_presets(), current.is_some()) else {
            return;
        };
        let selected = self.look.tick_preset_worn(current);
        let label = selected.as_deref().unwrap_or(if current.is_some() {
            "Custom spheres"
        } else {
            "Regional FLOW"
        });
        egui::ComboBox::from_id_salt("tick_sphere_preset")
            .selected_text(label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current.is_none(), "Regional FLOW")
                    .clicked()
                {
                    let _ = self.apply_tick_preset(None);
                }
                for name in &choices {
                    if ui
                        .selectable_label(selected.as_ref() == Some(name), name)
                        .clicked()
                    {
                        let _ = self.apply_tick_preset(Some(name));
                    }
                }
            });
        ui.small(self.look.tick_hint());
    }
}
