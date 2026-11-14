//! Footprint widgets over cached preset rows and model-owned drafts.
use crate::{
    footprint_panel::{self, PanelInput, PanelOutcome},
    theme,
};
use eframe::egui;
use quantick_ui_model::{
    UpdateSession,
    chart_settings::{Intent, Model},
};
pub(crate) fn draw(ui: &mut egui::Ui, session: &mut UpdateSession<'_, Model>) {
    let palette = theme::get(ui.ctx());
    let target = &session.state().footprint_target;
    let customized = session.state().footprint_customized;
    ui.label(
        egui::RichText::new(format!(
            "{target}: {}",
            if customized {
                "these knobs are this chart's own"
            } else {
                "this chart follows your last setup"
            }
        ))
        .font(palette.body_font(10.5))
        .color(palette.text_muted()),
    );
    ui.separator();
    ui.heading("Presets");
    ui.horizontal_wrapped(|ui| {
        if session.state().footprint_rows.is_empty() {
            ui.label(
                egui::RichText::new("none saved yet")
                    .font(palette.body_font(11.0))
                    .color(palette.text_faint()),
            );
        }
        for index in 0..session.state().footprint_rows.len() {
            let name = session.state().footprint_rows[index].clone();
            let selected = session.state().matching_footprint[index];
            let response = ui
                .selectable_label(selected, &name)
                .on_hover_text("click to load ? right-click to delete");
            if response.clicked() {
                session.update(Intent::SelectFootprint(name.clone()));
            }
            if response.secondary_clicked() {
                session.update(Intent::DeleteFootprint(name));
                break;
            }
        }
    });
    ui.horizontal(|ui| {
        let mut name = session.state().footprint_name.clone();
        let hint = theme::supporting_text(ui, "name this setup");
        if theme::preset_input(
            ui,
            egui::TextEdit::singleline(&mut name)
                .desired_width(150.0)
                .char_limit(crate::footprint_presets::MAX_NAME_LEN)
                .hint_text(hint),
        )
        .changed()
        {
            session.update(Intent::FootprintName(name));
        }
        if ui
            .add_enabled(
                session.state().can_save_footprint(),
                egui::Button::new("save preset"),
            )
            .on_hover_text(
                "store these knobs under that name; a name you already used is overwritten",
            )
            .clicked()
        {
            session.update(Intent::SaveFootprint);
        }
        if session.state().footprint_full {
            ui.label(
                egui::RichText::new("preset list is full")
                    .font(palette.body_font(10.5))
                    .color(palette.warn()),
            );
        }
    });
    let mut edited = session.state().footprint.clone();
    let outcome = footprint_panel::draw(
        ui,
        PanelInput {
            config: &mut edited,
            customized: session.state().footprint_customized,
        },
    );
    match outcome {
        PanelOutcome::Untouched => (),
        PanelOutcome::Changed => session.update(Intent::EditFootprint(edited)),
        PanelOutcome::ResetToDefault => session.update(Intent::ResetFootprint),
    }
}
