//! Candle widgets emit intents; cached rows and drafts belong to the model.
use crate::{surfaces::SurfaceEnv, theme};
use eframe::egui;
use quantick_ui_model::{
    UpdateSession,
    chart_settings::{Intent, Model},
};
pub(crate) fn draw(
    ui: &mut egui::Ui,
    session: &mut UpdateSession<'_, Model>,
    env: &SurfaceEnv<'_>,
    now: u64,
) {
    let palette = theme::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.heading("Preset");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(session.state().reset.is_some(), egui::Button::new("Reset"))
                .on_hover_text("put back the appearance this window opened on")
                .clicked()
            {
                session.update(Intent::ResetCandles(now));
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        for preset in crate::style::CandlePreset::ALL {
            if ui
                .selectable_label(
                    session.state().builtin_candle == Some(preset),
                    preset.label(),
                )
                .clicked()
            {
                session.update(Intent::SelectCandle(preset.label().into(), now));
            }
        }
        if !session.state().candle_rows.is_empty() {
            ui.separator();
            ui.label(theme::supporting_text(ui, "Saved").small());
        }
        for index in 0..session.state().candle_rows.len() {
            let name = session.state().candle_rows[index].clone();
            let label = ui
                .selectable_label(
                    session.state().matching_candle.as_deref() == Some(name.as_str()),
                    &name,
                )
                .on_hover_text("your saved look ? click to load ? right-click for more");
            if label.clicked() {
                session.update(Intent::SelectCandle(name.clone(), now));
            }
            label.context_menu(|ui| {
                if ui.button("Delete").clicked() {
                    session.update(Intent::DeleteCandle(name.clone()));
                    ui.close_menu();
                }
            });
        }
    });
    ui.small(format!(
        "current: {}",
        session
            .state()
            .matching_candle
            .as_deref()
            .unwrap_or("Custom")
    ));
    ui.horizontal(|ui| {
        let mut name = session.state().candle_name.clone();
        let hint = theme::supporting_text(ui, "name this look");
        if theme::preset_input(
            ui,
            egui::TextEdit::singleline(&mut name)
                .desired_width(150.0)
                .char_limit(crate::candle_presets::MAX_NAME_LEN)
                .hint_text(hint),
        )
        .changed()
        {
            session.update(Intent::CandleName(name));
        }
        if ui
            .add_enabled(
                session.state().can_save_candle(),
                egui::Button::new("save preset"),
            )
            .on_hover_text(
                "store candles and canvas under that name; a name you already used is overwritten",
            )
            .clicked()
        {
            session.update(Intent::SaveCandle);
        }
    });
    if let Some(refusal) = &session.state().candle_refusal {
        ui.label(
            egui::RichText::new(refusal)
                .font(palette.body_font(10.5))
                .color(palette.warn()),
        );
    }
    ui.separator();
    let mut edited = session.state().style;
    let body = crate::candle_view::draw_style_contents(ui, &mut edited, env.style_surface);
    if body.changed {
        session.update(Intent::EditStyle(edited, body.applied_preset, now));
    }
}
