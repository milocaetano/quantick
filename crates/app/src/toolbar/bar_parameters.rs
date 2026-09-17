//! Render the registered parameter schema; no bar-kind dispatch lives here.
use crate::theme;
use eframe::egui;
use quantick_engine::bar_registry::NumberKind;
use quantick_engine::bar_selection::{BarInputAvailability, BarSelection, SelectionCommand};
use rust_decimal::{
    Decimal,
    prelude::{FromPrimitive, ToPrimitive},
};

pub(super) fn draw(ui: &mut egui::Ui, selection: &mut BarSelection, inputs: BarInputAvailability) {
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
    let mut number = |ui: &mut egui::Ui| {
        for (label, preset) in editor.presets {
            let selected = value == Decimal::from(*preset);
            let (fill, ink) = if selected {
                (theme::ACCENT, theme::CHIP_INK)
            } else {
                (theme::CONTROL, theme::TEXT_MUTED)
            };
            let chip = ui.add(
                egui::Button::new(egui::RichText::new(*label).color(ink).small())
                    .fill(fill)
                    .stroke(egui::Stroke::NONE)
                    .rounding(egui::Rounding::same(9.0))
                    .min_size(egui::vec2(28.0, 18.0)),
            );
            if chip.clicked() && !selected {
                value = Decimal::from(*preset);
                changed = true;
            }
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
            NumberKind::Duration => {
                let mut interval = value.to_i64().expect("interval representation");
                let response = ui.add(
                    egui::DragValue::new(&mut interval)
                        .range(editor.min..=editor.max)
                        .speed(editor.step)
                        .suffix(" ms"),
                );
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
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    number(ui);
                });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bar_extension_fixture as seventh_bar;
    use quantick_engine::bar_registry::{BUILTIN_BARS, BarRegistry};
    use quantick_engine::{fixture, golden};

    #[test]
    fn passive_decimal_editor_preserves_legacy_range_clamping() {
        for (text, expected) in [
            ("volume:0.000000001", Decimal::new(1, 1)),
            ("volume:2000", Decimal::from(1000)),
            ("dollar:0.000000001", Decimal::from(1000)),
            ("dollar:2000000000", Decimal::from(1_000_000_000)),
        ] {
            let mut selection = BarSelection::new(BUILTIN_BARS.parse(text).unwrap());
            let ctx = egui::Context::default();
            // The focused toolbar draws this control even without input.
            // Its historical widget range is narrower than parser acceptance.
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(ui, &mut selection, BarInputAvailability::PRINTS);
                });
            });
            assert_eq!(selection.spec().parameter(), expected, "{text}");
        }
    }

    #[test]
    fn seventh_kind_reaches_real_editor_and_chart() {
        let registry = BarRegistry::new(
            BUILTIN_BARS
                .definitions()
                .iter()
                .copied()
                .chain([&seventh_bar::SEVENTH]),
        )
        .unwrap();
        let config = registry.parse("probe:3").unwrap();
        let mut selection = BarSelection::with_registry(registry, config).unwrap();
        let ctx = egui::Context::default();
        let mut text = String::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(ui, &mut selection, BarInputAvailability::PRINTS);
                });
            });
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(shape) = shape.shape {
                    text.push_str(shape.galley.text());
                }
            }
        }
        assert!(
            text.contains("probe count"),
            "the definition's own editor descriptor reached actual paint"
        );
        assert_eq!(selection.spec(), config, "drawing alone is not a command");
        let mut chart = crate::state::RetainedSeries::new(selection.spec());
        chart.ingest_backfill(
            &fixture::parse_trades(include_str!(
                "../../../engine/tests/fixtures/tick_trades.csv"
            ))
            .unwrap(),
        );
        let expected = fixture::parse_bars(include_str!(
            "../../../engine/tests/fixtures/tick_n3_expected.csv"
        ))
        .unwrap();
        assert_eq!(golden::diff_bars(&expected, chart.bars()), None);
        selection
            .update(
                SelectionCommand::Parameter {
                    name: "count",
                    value: 1.into(),
                },
                BarInputAvailability::PRINTS,
            )
            .unwrap();
        chart.set_spec(selection.spec());
        assert_eq!(
            chart.bars().len(),
            7,
            "the same registered factory also rebuilds the real chart"
        );
    }
}
