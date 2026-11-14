//! Render the registered parameter schema; no bar-kind dispatch lives here.
use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::bar_registry::NumberKind;
use quantick_engine::bar_selection::{BarInputAvailability, BarSelection, SelectionCommand};
use rust_decimal::{
    Decimal,
    prelude::{FromPrimitive, ToPrimitive},
};

/// The registered bar kinds as a pick list: the body of every bars menu.
pub(crate) fn draw_kinds(
    ui: &mut egui::Ui,
    selection: &mut BarSelection,
    inputs: BarInputAvailability,
) {
    let mut selected = None;
    for definition in selection.registry().definitions() {
        let reason = selection
            .availability(definition.id, inputs)
            .expect("registered kind");
        let item = ui.add_enabled(
            reason.is_none(),
            egui::SelectableLabel::new(
                selection.selected_id() == definition.id,
                definition.label(),
            ),
        );
        if item.clicked() {
            selected = Some(definition.id);
        }
        if let Some(reason) = reason {
            item.on_disabled_hover_text(reason.reason());
        }
    }
    if let Some(id) = selected {
        let _ = selection.update(SelectionCommand::Select(id), inputs);
    }
}

pub(crate) fn draw(ui: &mut egui::Ui, selection: &mut BarSelection, inputs: BarInputAvailability) {
    let config = selection.spec();
    let definition = config.definition();
    for choice in definition.choices {
        let reason = inputs.refusal(definition.requirements_for(Some(choice.id)));
        let chip = ui
            .add_enabled(
                reason.is_none(),
                egui::SelectableLabel::new(config.choice() == Some(choice.id), choice.id),
            )
            .on_hover_text(choice.hover);
        if chip.clicked() {
            let _ = selection.update(
                SelectionCommand::Choice {
                    name: definition.choice_parameter.expect("choice parameter"),
                    value: choice.id,
                },
                inputs,
            );
        }
        if let Some(reason) = reason {
            chip.on_disabled_hover_text(reason.reason());
        }
    }
    let descriptor = &definition.parameter;
    let editor = &descriptor.editor;
    if !editor.label.is_empty() {
        ui.label(editor.label);
    }
    let mut value = selection.spec().parameter();
    let mut changed = false;
    let presets = ui.make_persistent_id(("parameter_presets", definition.id));
    let mut number = |ui: &mut egui::Ui| {
        if !editor.presets.is_empty()
            && !preset_list(
                ui,
                presets,
                editor.presets,
                descriptor.kind,
                &mut value,
                &mut changed,
            )
        {
            return;
        }
        let response = match descriptor.kind {
            NumberKind::Count => {
                let mut count = value.to_u64().expect("count representation");
                let response = ui.add(
                    egui::DragValue::new(&mut count)
                        .range(editor.min..=editor.max)
                        .speed(editor.step),
                );
                if response.changed() {
                    value = Decimal::from(count);
                }
                response
            }
            NumberKind::Duration
                if quantick_engine::time_bucket::calendar_months(
                    value.to_i64().expect("interval representation"),
                )
                .is_some() =>
            {
                // A calendar month has no fixed length to drag, and the button
                // already names it: nothing stands beside it.
                ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover())
            }
            NumberKind::Duration => {
                let mut interval = value.to_i64().expect("interval representation");
                let response = ui.add(crate::time_header::interval_drag(
                    &mut interval,
                    editor.min..=editor.max,
                    editor.step,
                ));
                if response.changed() {
                    value = Decimal::from(interval);
                }
                response
            }
            NumberKind::Decimal => ui.add(
                egui::DragValue::from_get_set(|new| {
                    if let Some(new) = new {
                        value = Decimal::from_f64(new).unwrap_or(value);
                    }
                    value.to_f64().unwrap_or_default()
                })
                .range(editor.min..=editor.max)
                .speed(editor.step),
            ),
        };
        changed |= response.changed();
        if !editor.hover.is_empty() {
            response.on_hover_text(editor.hover);
        }
    };
    let reason = inputs.refusal(selection.spec().requirements());
    let response = ui
        .add_enabled_ui(reason.is_none(), |ui| {
            if editor.presets.is_empty() {
                number(ui);
            } else {
                ui.vertical(number);
            }
        })
        .response;
    if let Some(reason) = reason {
        response.on_disabled_hover_text(reason.reason());
    }
    if changed {
        let _ = selection.update(
            SelectionCommand::Parameter {
                name: descriptor.name,
                value,
            },
            inputs,
        );
    }
}

/// The presets behind one button that names the value; a click unfolds them as
/// a list inside the open menu, so no second popup closes the first. Returns
/// whether the free editor shows: after "Custom…", or for a value no preset
/// names.
///
/// The list keeps its fold and its Custom… while it paints frame after frame;
/// a frame without it is a closed menu, and the next paint opens folded.
fn preset_list(
    ui: &mut egui::Ui,
    id: egui::Id,
    presets: &[(&str, i64)],
    kind: NumberKind,
    value: &mut Decimal,
    changed: &mut bool,
) -> bool {
    let pass = ui.ctx().cumulative_pass_nr();
    let (mut open, mut custom) = ui
        .data(|data| data.get_temp::<(bool, bool, u64)>(id))
        .filter(|(_, _, painted)| painted + 1 >= pass)
        .map_or((false, false), |(open, custom, _)| (open, custom));
    let named = |value: Decimal| {
        presets
            .iter()
            .find(|(_, preset)| value == Decimal::from(*preset))
            .map(|(label, _)| *label)
    };
    // Sticky: a drag from an unnamed value across a preset keeps its editor.
    custom |= named(*value).is_none();
    let words = named(*value).map_or_else(|| value_words(kind, *value), str::to_owned);
    let button = ui
        .add(egui::Button::new(format!("{words} {}", icons::CARET_DOWN)))
        .on_hover_text("Choose a preset, or Custom…");
    if button.clicked() {
        open = !open;
    }
    if open {
        for (label, preset) in presets {
            let preset = Decimal::from(*preset);
            if ui.selectable_label(*value == preset, *label).clicked() {
                *changed |= *value != preset;
                *value = preset;
                (open, custom) = (false, false);
            }
        }
        // A calendar month has no length to edit, so Custom… would open nothing.
        let month = kind == NumberKind::Duration
            && value
                .to_i64()
                .and_then(quantick_engine::time_bucket::calendar_months)
                .is_some();
        if !month && ui.selectable_label(custom, CUSTOM).clicked() {
            (open, custom) = (false, true);
        }
    }
    ui.data_mut(|data| data.insert_temp(id, (open, custom, pass)));
    custom || named(*value).is_none()
}

/// The last row of an unfolded preset list: it reveals the free editor.
const CUSTOM: &str = "Custom…";

/// A value no preset names, written the way its kind reads.
fn value_words(kind: NumberKind, value: Decimal) -> String {
    match (kind, value.to_i64()) {
        (NumberKind::Duration, Some(ms)) => quantick_engine::fmt_time_interval(ms),
        _ => value.normalize().to_string(),
    }
}

#[cfg(test)]
mod tests;
