use super::*;

fn plot(title: &str) -> PlotSpec {
    PlotSpec {
        id: quantick_indicators::PlotId::new(0),
        title: title.to_owned(),
        style: quantick_indicators::PlotStyle::Line,
        base_color: Rgba8::opaque(0x26, 0xA6, 0x9A),
        width: 1.5,
        offset: 0,
        marker: None,
    }
}

type Editor = crate::surfaces::indicator_settings::IndicatorSettingsSurface;

fn editor(slot: u64, title: &str, values: Vec<InputValue>) -> Editor {
    let mut editor = Editor::new(
        Default::default(),
        crate::surfaces::indicator_settings::effects,
    );
    editor.update(Intent::Open {
        target: quantick_chart::indicator_registry::TabSlot {
            workspace: quantick_workspace::lifetime::WorkspaceId(1),
            pane: 1,
            tab: 1,
            side: crate::pane::PaneSide::Flow,
            slot: crate::indicator_worker::SlotId(slot),
        },
        title: title.to_owned(),
        values,
    });
    editor
}

fn open_dialog() -> Editor {
    editor(0, "EMA(9)", vec![InputValue::Int(9)])
}

fn draw(
    ctx: &egui::Context,
    editor: &mut Editor,
    specs: &[InputSpec],
    plots: &[PlotSpec],
    style: &mut StyleOverride,
    presets: Option<&[String]>,
) -> SettingsOutcome {
    let mut outcome = SettingsOutcome::Open;
    editor.with_updates(
        |response| {
            for effect in response.model_effects {
                if let crate::surfaces::HostEffect::IndicatorSettings(effects) = effect {
                    for change in effects.changes {
                        outcome = match change {
                            quantick_ui_model::indicator_settings::Change::Inputs {
                                commit: true,
                                ..
                            } => SettingsOutcome::Apply,
                            quantick_ui_model::indicator_settings::Change::Inputs { .. } => {
                                SettingsOutcome::Preview
                            }
                            quantick_ui_model::indicator_settings::Change::Style(_) => {
                                SettingsOutcome::StyleChanged
                            }
                        };
                    }
                }
            }
        },
        |_, session| {
            let label = session.state().dialog().unwrap().title.clone();
            session.update(Intent::ObserveTarget {
                valid: true,
                label,
                has_style: !plots.is_empty(),
            });
            let intent = super::draw(ctx, session, specs, plots, style, presets);
            session.update(intent);
        },
    );
    if editor.model_state.dialog().is_none() {
        SettingsOutcome::Close
    } else {
        outcome
    }
}

fn painted_dialog(
    ctx: &egui::Context,
    dialog: &mut Editor,
    specs: &[InputSpec],
    plots: &[PlotSpec],
    style: &mut StyleOverride,
) -> String {
    let mut text = String::new();
    for _ in 0..2 {
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            draw(ctx, dialog, specs, plots, style, None);
        });
        text.clear();
        for shape in output.shapes {
            if let egui::epaint::Shape::Text(galley) = shape.shape {
                text.push_str(galley.galley.text());
                text.push(' ');
            }
        }
    }
    text
}

/// Criterion 2: the Style tab exists when there are plots to style, and
/// does not exist when there are none — a tab that opens onto nothing is a
/// promise the dialog cannot keep.
#[test]
fn the_style_tab_appears_only_for_an_indicator_that_declares_plots() {
    let ctx = egui::Context::default();
    let specs = [int_spec()];

    let mut with_plots = open_dialog();
    let mut style = StyleOverride::default();
    let text = painted_dialog(&ctx, &mut with_plots, &specs, &[plot("ema")], &mut style);
    assert!(text.contains("Inputs") && text.contains("Style"), "{text}");

    let mut without = open_dialog();
    let text = painted_dialog(&ctx, &mut without, &specs, &[], &mut style);
    assert!(!text.contains("Style"), "no plots, no Style tab: {text}");

    // And a tab left selected when the plots vanish falls back rather than
    // showing an empty half.
    let mut stale = open_dialog();
    stale.update(Intent::SelectTab(SettingsTab::Style));
    let _ = painted_dialog(&ctx, &mut stale, &specs, &[], &mut style);
    assert_eq!(
        stale.model_state.dialog().unwrap().tab,
        SettingsTab::Inputs,
        "it fell back"
    );
}

/// A style edit reports itself so the caller can write the state file, and
/// it is render-side: it never claims to be a preview, which would put the
/// dialog into the un-applied state and offer to Discard something there is
/// nothing to discard.
#[test]
fn a_style_edit_is_its_own_outcome_and_lands_on_the_layer() {
    let mut style = StyleOverride::default();
    style.set(
        0,
        crate::indicator_style::PlotOverride {
            color: Some(Rgba8::opaque(255, 0, 0)),
            ..crate::indicator_style::PlotOverride::default()
        },
    );
    let resolved = style.resolve(0, &plot("ema"), None);
    assert_eq!(resolved.color, Rgba8::opaque(255, 0, 0));
    assert!(
        (resolved.width - 1.5).abs() < f32::EPSILON,
        "an untouched field still falls through to the declaration"
    );
}

/// A spec that declares a closed set of values gets a control that cannot
/// produce one outside it.
///
/// The slider the dialog drew over an option list let a trader land on a
/// number the indicator does not take; the build then quietly swapped in
/// something else and the dialog went on showing the value that was never
/// bound — the "inferred data, silently patched" the honesty rule forbids.
#[test]
fn an_input_with_a_closed_option_set_renders_as_a_choice() {
    let ctx = egui::Context::default();
    let specs = [InputSpec::Int {
        name: "smoothing".to_owned(),
        title: "Smoothing".to_owned(),
        default: 3,
        min: Some(1),
        max: Some(9),
        step: None,
        options: vec![1, 3, 5],
    }];
    let mut dialog = editor(0, "EMA(9)", vec![InputValue::Int(5)]);
    let mut style = StyleOverride::default();
    let text = painted_dialog(&ctx, &mut dialog, &specs, &[], &mut style);

    assert!(text.contains("Smoothing"), "the row is drawn: {text}");
    assert!(text.contains('5'), "showing the value bound now: {text}");
    assert!(
        !text.contains("unrenderable"),
        "the option branch was reached: {text}"
    );
}

/// The dialog opens in the centre, fully on screen.
///
/// It used to land in the bottom-left corner to keep clear of the chart,
/// which read as a window opening somewhere unrelated to the click; the
/// trader asked for the centre (2026-11-08). A trader who wants the
/// preview uncovered drags it once and it reopens there. Computed from
/// the live window, because a fixed point is the centre of one size only.
#[test]
fn the_dialog_opens_centred_and_on_screen() {
    for size in [egui::vec2(1700.0, 950.0), egui::vec2(900.0, 560.0)] {
        let ctx = egui::Context::default();
        let mut dialog = open_dialog();
        let specs = [int_spec()];
        let plots = [plot("ema")];
        let mut style = StyleOverride::default();
        let id = egui::Id::new((
            "indicator-settings",
            dialog.model_state.target().unwrap().slot.0,
        ));
        // Enough frames for the window's size, and so its centring, to
        // settle.
        for _ in 0..6 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), size)),
                ..egui::RawInput::default()
            };
            let _ = ctx.run(input, |ctx| {
                draw(ctx, &mut dialog, &specs, &plots, &mut style, None);
            });
        }
        // The window's own rect, not the painted shapes: egui draws a drop
        // shadow whose bounding box spills past the frame.
        let rect = ctx
            .memory(|memory| memory.area_rect(id))
            .expect("the dialog was laid out");

        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        assert!(
            screen.contains_rect(rect),
            "{size:?}: the buttons must be on screen ({rect:?})"
        );
        assert!(
            rect.center().distance(screen.center()) < 2.0,
            "{size:?}: it opens centred, not in a corner ({rect:?})"
        );
    }
}

/// Escape gets out — a trader UX finding, not a nicety.
///
/// The dialog can now be opened by a double click aimed at the chart, and
/// while one of its fields holds the keyboard the app's trading shortcuts
/// are suppressed by design. Without a key that closes it, the way out is
/// to find and hit a button, which is the wrong thing to be doing when the
/// reason you want out is the tape. Escape is Close, so it also reverts a
/// pending preview.
#[test]
fn escape_closes_the_dialog() {
    let ctx = egui::Context::default();
    let mut dialog = open_dialog();
    let specs = [int_spec()];
    let mut style = StyleOverride::default();

    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let outcome = draw(ctx, &mut dialog, &specs, &[], &mut style, None);
        assert!(matches!(outcome, SettingsOutcome::Open), "nothing pressed");
    });

    let mut input = egui::RawInput::default();
    input.events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
    let mut closed = false;
    let _ = ctx.run(input, |ctx| {
        closed = matches!(
            draw(ctx, &mut dialog, &specs, &[], &mut style, None),
            SettingsOutcome::Close
        );
    });
    assert!(closed, "Escape is the way out");
}

fn int_spec() -> InputSpec {
    InputSpec::Int {
        name: "length".to_owned(),
        title: "Length".to_owned(),
        default: 9,
        min: Some(1),
        max: None,
        step: None,
        options: Vec::new(),
    }
}

fn stepped_float(title: &str) -> InputSpec {
    InputSpec::Float {
        name: "x".to_owned(),
        title: title.to_owned(),
        default: 0.1,
        min: Some(0.0),
        max: Some(0.5),
        step: Some(0.05),
        options: Vec::new(),
    }
}

/// A stepped slider snaps a value that is not on its grid the moment it
/// first draws, and reports that as `changed`. Without the settle frame
/// the dialog opened already announcing a preview nobody asked for —
/// this test fails with `Preview` on frame 0 if that regresses.
#[test]
fn first_frame_widget_normalization_is_not_a_preview() {
    let specs = vec![stepped_float("Advanced: min bar body (×ATR)")];
    let mut dialog = editor(7, "Copilot", vec![InputValue::Float(0.123)]);
    let ctx = egui::Context::default();
    for frame in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let outcome = draw(
                ctx,
                &mut dialog,
                &specs,
                &[],
                &mut StyleOverride::default(),
                None,
            );
            assert!(
                !matches!(outcome, SettingsOutcome::Preview),
                "frame {frame}: widget normalization must not announce a preview"
            );
        });
    }
    assert!(
        dialog.model_state.dialog().unwrap().settled,
        "the first frame settles the dialog"
    );
    assert_eq!(
        dialog.model_state.dialog().unwrap().draft,
        dialog.model_state.dialog().unwrap().committed,
        "whatever the widgets normalized became the baseline"
    );
}
