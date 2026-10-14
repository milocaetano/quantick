//! A chart-only preset picker, beside the whole-look preset controls.
use super::OrderflowView;
use eframe::egui;

impl OrderflowView {
    pub(crate) fn apply_tick_preset(&mut self, name: Option<&str>) -> Result<bool, String> {
        let style = self.look.tick_style(name)?;
        Ok(self.edit_config(|config| config.volume_dots.tick_bubbles = style))
    }

    pub(super) fn draw_tick_bubbles(&mut self, ui: &mut egui::Ui) {
        let choices = self.look.tick_presets();
        if choices.is_empty() {
            return;
        }
        let current = self.config.volume_dots.tick_bubbles;
        let selected = choices
            .iter()
            .find(|name| self.look.tick_style(Some(name)).ok() == Some(current));
        let label = selected.map_or(
            if current.is_some() {
                "Custom spheres"
            } else {
                "Regional FLOW"
            },
            String::as_str,
        );
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
                    if ui.selectable_label(selected == Some(name), name).clicked() {
                        let _ = self.apply_tick_preset(Some(name));
                    }
                }
            });
        ui.small(self.look.tick_hint());
    }
}
