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
            // An open bars menu draws this control even without input.
            // Its historical widget range is narrower than parser acceptance.
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(ui, &mut selection, BarInputAvailability::PRINTS);
                });
            });
            assert_eq!(selection.spec().parameter(), expected, "{text}");
        }
    }

    /// Paints `draw` once, then once per click, each click landing on the
    /// first text the frame before painted that starts with it; returns the
    /// texts the last frame painted.
    fn paint(selection: &mut BarSelection, ctx: &egui::Context, clicks: &[&str]) -> Vec<String> {
        let mut texts: Vec<(String, egui::Rect)> = Vec::new();
        for click in std::iter::once(None).chain(clicks.iter().map(Some)) {
            let mut input = egui::RawInput::default();
            if let Some(click) = click {
                let at = texts
                    .iter()
                    .find(|(text, _)| text.starts_with(*click))
                    .unwrap_or_else(|| panic!("{click} missing from {texts:?}"))
                    .1
                    .center();
                input.events.push(egui::Event::PointerMoved(at));
                for pressed in [true, false] {
                    input.events.push(egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
            }
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| draw(ui, selection, BarInputAvailability::PRINTS));
                });
            });
            texts = output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::epaint::Shape::Text(text) => Some((
                        text.galley.text().to_owned(),
                        text.galley.rect.translate(text.pos.to_vec2()),
                    )),
                    _ => None,
                })
                .collect();
        }
        texts.into_iter().map(|(text, _)| text).collect()
    }

    /// The time editor is one button naming the interval: no strip of preset
    /// chips, and a frame without a click leaves the selection as it was.
    #[test]
    fn the_time_kind_paints_one_interval_button_and_an_idle_frame_changes_nothing() {
        let config = BUILTIN_BARS.parse("time:300000").unwrap();
        let mut selection = BarSelection::new(config);
        let ctx = egui::Context::default();
        let painted = paint(&mut selection, &ctx, &[]);
        assert!(
            painted.iter().any(|text| text.starts_with("5m ")),
            "{painted:?}"
        );
        for chip in ["1m", "15m", "1h", CUSTOM] {
            assert!(
                !painted.iter().any(|text| text == chip),
                "{chip} in {painted:?}"
            );
        }
        assert_eq!(selection.spec(), config);
        assert_eq!(selection.pending(), None);
    }

    /// A click unfolds every preset and Custom…; picking one sets the interval
    /// and folds the list back.
    #[test]
    fn the_interval_button_unfolds_the_presets_and_a_pick_sets_the_interval() {
        let mut selection = BarSelection::new(BUILTIN_BARS.parse("time:300000").unwrap());
        let ctx = egui::Context::default();
        let painted = paint(&mut selection, &ctx, &["5m "]);
        for (label, _) in quantick_engine::bar_registry::TIME_PRESETS {
            assert!(
                painted.iter().any(|text| text == label),
                "{label} in {painted:?}"
            );
        }
        assert!(painted.iter().any(|text| text == CUSTOM), "{painted:?}");
        paint(&mut selection, &ctx, &["15m"]);
        assert_eq!(
            selection.spec(),
            BUILTIN_BARS.parse("time:900000").unwrap(),
            "the pick is the command"
        );
        let painted = paint(&mut selection, &ctx, &[]);
        assert!(
            painted.iter().any(|text| text.starts_with("15m ")),
            "{painted:?}"
        );
        assert!(!painted.iter().any(|text| text == "1h"), "the list folds");
    }

    /// Custom… reveals the free editor; an interval no preset names keeps it
    /// shown under a button that writes the interval out.
    #[test]
    fn custom_and_an_unnamed_interval_show_the_free_editor() {
        let mut selection = BarSelection::new(BUILTIN_BARS.parse("time:300000").unwrap());
        let ctx = egui::Context::default();
        let before = paint(&mut selection, &ctx, &[]);
        let after = paint(&mut selection, &ctx, &["5m ", CUSTOM]);
        assert!(after.len() > before.len(), "{before:?} then {after:?}");
        let mut odd = BarSelection::new(BUILTIN_BARS.parse("time:90000").unwrap());
        let painted = paint(&mut odd, &egui::Context::default(), &[]);
        assert!(
            painted.iter().any(|text| text.starts_with("90s ")),
            "{painted:?}"
        );
        assert!(
            painted.iter().filter(|text| text.contains("90s")).count() >= 2,
            "{painted:?}"
        );
    }

    /// A named interval shows no drag until Custom…, which brings one that
    /// writes the interval out.
    #[test]
    fn a_named_interval_shows_its_drag_only_after_custom() {
        let mut selection = BarSelection::new(BUILTIN_BARS.parse("time:300000").unwrap());
        let ctx = egui::Context::default();
        let idle = paint(&mut selection, &ctx, &[]);
        assert!(!idle.iter().any(|text| text == "5m"), "{idle:?}");
        let custom = paint(&mut selection, &ctx, &["5m ", CUSTOM]);
        assert!(custom.iter().any(|text| text == "5m"), "{custom:?}");
    }

    /// A drag from an unnamed interval that crosses a preset keeps its editor:
    /// the value is named now, but the hand is still on the drag.
    #[test]
    fn the_free_editor_survives_crossing_a_preset() {
        let mut selection = BarSelection::new(BUILTIN_BARS.parse("time:90000").unwrap());
        let ctx = egui::Context::default();
        let unnamed = paint(&mut selection, &ctx, &[]);
        assert!(unnamed.iter().any(|text| text == "90s"), "{unnamed:?}");
        selection
            .update(
                SelectionCommand::Replace(BUILTIN_BARS.parse("time:60000").unwrap()),
                BarInputAvailability::PRINTS,
            )
            .unwrap();
        let crossed = paint(&mut selection, &ctx, &[]);
        assert!(
            crossed.iter().any(|text| text.starts_with("1m ")),
            "{crossed:?}"
        );
        assert!(
            crossed.iter().any(|text| text == "1m"),
            "the drag stays: {crossed:?}"
        );
    }

    /// A calendar month has no length to edit, so its list offers no Custom….
    #[test]
    fn a_calendar_month_hides_custom() {
        let month = quantick_engine::time_bucket::CALENDAR_MONTH_MS;
        let mut selection =
            BarSelection::new(BUILTIN_BARS.parse(&format!("time:{month}")).unwrap());
        let ctx = egui::Context::default();
        let painted = paint(&mut selection, &ctx, &["1mo "]);
        assert!(painted.iter().any(|text| text == "1w"), "the list unfolded");
        assert!(!painted.iter().any(|text| text == CUSTOM), "{painted:?}");
    }

    /// A frame without the list is a closed menu: the next paint opens folded,
    /// with no Custom… left over.
    #[test]
    fn a_skipped_frame_resets_the_list() {
        let mut selection = BarSelection::new(BUILTIN_BARS.parse("time:300000").unwrap());
        let ctx = egui::Context::default();
        let custom = paint(&mut selection, &ctx, &["5m ", CUSTOM]);
        assert!(custom.iter().any(|text| text == "5m"), "{custom:?}");
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let reopened = paint(&mut selection, &ctx, &[]);
        assert!(!reopened.iter().any(|text| text == "5m"), "{reopened:?}");
        paint(&mut selection, &ctx, &["5m "]);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let reopened = paint(&mut selection, &ctx, &[]);
        assert!(!reopened.iter().any(|text| text == "1h"), "{reopened:?}");
    }

    /// A venue that prints no volume still draws every kind: one it cannot back
    /// can arrive from config or a previous session.
    #[test]
    fn a_quote_driven_feed_draws_the_menu_of_every_kind() {
        let inputs = BarInputAvailability {
            traded_volume: false,
            deal_counter: false,
            deal_count: false,
        };
        for definition in BUILTIN_BARS.definitions() {
            let mut selection = BarSelection::new(definition.default_config());
            let ctx = egui::Context::default();
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw_kinds(ui, &mut selection, inputs);
                    draw(ui, &mut selection, inputs);
                });
            });
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
        let mut chart = crate::state::ChartState::new(selection.spec());
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
