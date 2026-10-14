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
        let label = self.look.tick_preset_label(current);
        egui::ComboBox::from_id_salt("tick_sphere_preset")
            .selected_text(&label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current.is_none(), "Regional FLOW")
                    .clicked()
                {
                    let _ = self.apply_tick_preset(None);
                }
                for name in &choices {
                    if ui.selectable_label(label == *name, name).clicked() {
                        let _ = self.apply_tick_preset(Some(name));
                    }
                }
            });
        ui.small("Tick chart spheres · quantity in source units · tape settings stay as selected");
    }
}
