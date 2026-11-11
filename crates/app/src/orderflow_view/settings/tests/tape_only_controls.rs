//! The settings surface must describe the projection currently being painted.

use super::*;
use quantick_orderflow::HeatmapConfig;
use quantick_orderflow::{BubbleSizeReference, LaneWindow};

type PaintedText = Vec<(String, egui::Rect)>;

#[test]
fn bubble_persistence_is_reachable_in_the_real_panel_after_the_l2_section() {
    use quantick_stores::bubble_asset_store::{AssetBinding, AssetBubblesStore};
    use tokio::sync::mpsc;
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for width in [1000.0, 1280.0] {
            let ctx = egui::Context::default();
            let mut appearance = quantick_appearance::Appearance::default();
            appearance.apply(source).unwrap();
            crate::theme::sync(&ctx, &appearance);
            let (_events_tx, events) = mpsc::channel(8);
            let (_book_tx, book_events) = mpsc::channel(8);
            let (commands, _commands_rx) = mpsc::channel(8);
            let mut tab = crate::tab::Tab::new(
                1,
                "metatrader-b3".into(),
                "WINV26".into(),
                crate::state::BarSpec::Tick(50),
                quantick_feed::FeedHandle {
                    events,
                    book_events,
                    commands,
                    notices: quantick_feed::silent_notices(),
                    capabilities: quantick_feed::fixed_capabilities(
                        crate::config::ProviderKind::Binance.capabilities(),
                    ),
                    latency: quantick_feed::unsplit_latency(),
                    replay: None,
                },
                crate::scratch::thread_dir("bubble-panel-readability"),
            );
            let config: crate::config::AppConfig =
                toml::from_str(include_str!("../../../../config/feeds.toml")).unwrap();
            let (binding, settings) = AssetBinding::bind(
                AssetBubblesStore::default().shared(),
                &config,
                ("metatrader-b3", "WINV26"),
                tab.tape().look.presets(),
                false,
            );
            tab.tape_mut().bind_asset(binding, &settings);
            tab.tape_mut().settings_panel.remember_width(320.0);
            let mut folds = crate::ui_state::AssetFolds::default();
            folds.set("WINV26", crate::ui_state::PanelSection::L2, true);
            crate::orderflow_panel::open_on(&mut tab, crate::ui_state::PanelSection::Bubbles);
            let mut reached = false;
            for frame in 0..30 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 780.0),
                        )),
                        time: Some(f64::from(frame) / 60.0),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let chart = ui.max_rect();
                            crate::orderflow_panel::draw(&mut tab, ui, chart, &mut folds);
                        });
                    },
                );
                for clipped in output.shapes {
                    if let egui::Shape::Text(text) = clipped.shape
                        && text.galley.job.text.starts_with("settings kept for asset")
                    {
                        let bounds = text.galley.rect.translate(text.pos.to_vec2());
                        reached |= clipped.clip_rect.contains_rect(bounds);
                    }
                }
            }
            assert!(
                reached,
                "opening Bubbles exposes its complete persistence note through the actual panel scroll area"
            );
        }
    }
}

#[test]
fn bubble_preset_editor_accepts_selection_and_rejects_disabled_typing() {
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        let ctx = egui::Context::default();
        let mut appearance = quantick_appearance::Appearance::default();
        appearance.apply(source).unwrap();
        crate::theme::sync(&ctx, &appearance);
        let mut view = view(false);
        view.set_preset_name_draft_for_test("");
        let mut draw = |events, enabled| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(520.0, 780.0),
                    )),
                    time: Some(ctx.input(|input| input.time) + 1.0 / 60.0),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add_enabled_ui(enabled, |ui| view.draw_bubble_presets(ui, false));
                    });
                },
            )
        };
        let output = draw(vec![], true);
        let pos = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == "preset name" => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()).center())
                }
                _ => None,
            })
            .unwrap();
        for pressed in [true, false] {
            draw(
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                true,
            );
        }
        let id = ctx
            .memory(|memory| memory.focused())
            .expect("bubble preset name takes focus");
        draw(vec![egui::Event::Text("Readable".into())], true);
        draw(
            vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            }],
            true,
        );
        let range = egui::TextEdit::load_state(&ctx, id)
            .unwrap()
            .cursor
            .char_range()
            .unwrap()
            .sorted();
        assert_eq!((range[0].index, range[1].index), (0, 8));
        ctx.memory_mut(|memory| memory.surrender_focus(id));
        for pressed in [true, false] {
            draw(
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                false,
            );
        }
        let output = draw(vec![egui::Event::Text("Forbidden".into())], false);
        assert!(ctx.memory(|memory| memory.focused()).is_none());
        assert!(output.shapes.iter().any(|clipped| matches!(&clipped.shape,
            egui::Shape::Text(text) if text.galley.job.text == "Readable")));
        assert!(!output.shapes.iter().any(|clipped| matches!(&clipped.shape,
            egui::Shape::Text(text) if text.galley.job.text.contains("Forbidden"))));
    }
}

#[test]
fn supporting_guidance_fits_narrow_surfaces() {
    use quantick_stores::bubble_asset_store::{AssetBinding, AssetBubblesStore};
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for width in [320.0, 520.0] {
            let ctx = egui::Context::default();
            let mut appearance = quantick_appearance::Appearance::default();
            appearance.apply(source).unwrap();
            crate::theme::sync(&ctx, &appearance);
            let mut view = view(false);
            let config: crate::config::AppConfig =
                toml::from_str(include_str!("../../../../config/feeds.toml")).unwrap();
            let (binding, settings) = AssetBinding::bind(
                AssetBubblesStore::default().shared(),
                &config,
                ("metatrader-b3", "WINV26"),
                view.look.presets(),
                false,
            );
            view.bind_asset(binding, &settings);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1000.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| view.draw_bubble_presets(ui, false));
                },
            );
            let text = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.job.text.starts_with("settings kept for asset") =>
                    {
                        Some(text)
                    }
                    _ => None,
                })
                .expect("asset persistence is explained");
            assert!(text.pos.x + text.galley.rect.right() <= width + 0.5);
            assert!(text.galley.job.sections[0].format.font_id.size >= 12.0);
        }
    }
}

fn collect_text(shape: egui::Shape, text: &mut PaintedText) {
    match shape {
        egui::Shape::Text(shape) => text.push((
            shape.galley.job.text.clone(),
            egui::Rect::from_min_size(shape.pos, shape.galley.size()),
        )),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text(shape, text);
            }
        }
        _ => {}
    }
}

fn paint(view: &mut OrderflowView, ctx: &egui::Context, events: Vec<egui::Event>) -> PaintedText {
    let output = ctx.run(
        egui::RawInput {
            time: Some(ctx.input(|input| input.time) + 1.0 / 60.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1_200.0, 3_000.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| view.draw_bubble_controls(ui));
        },
    );
    let mut text = Vec::new();
    for shape in output.shapes {
        collect_text(shape.shape, &mut text);
    }
    text
}

fn open_controls(view: &mut OrderflowView) -> (egui::Context, PaintedText) {
    let ctx = egui::Context::default();
    ctx.style_mut(|style| {
        style.animation_time = 0.0;
        style.interaction.tooltip_delay = 0.0;
        style.interaction.show_tooltips_only_when_still = false;
    });
    let mut text = paint(view, &ctx, Vec::new());
    for title in ["live lane", "labels", "colours"] {
        let pos = text
            .iter()
            .find(|(label, _)| label == title)
            .unwrap_or_else(|| panic!("missing section {title}"))
            .1
            .center();
        for pressed in [true, false] {
            let _ = paint(
                view,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        text = paint(view, &ctx, vec![egui::Event::PointerGone]);
    }
    (ctx, text)
}

fn view(tape_only: bool) -> OrderflowView {
    let mut view = OrderflowView::new("TEST");
    view.config = HeatmapConfig::default();
    view.config.live_lane.tape_only = tape_only;
    view.config.live_lane.window = LaneWindow::Fixed { ms: 30_000 };
    view.config.volume_dots.enabled = tape_only;
    view.config.volume_dots.auto_full = true;
    view.config.bubbles.size_reference = BubbleSizeReference::VisibleMax;
    view.config.bubble_region_rows = 4;
    view
}

fn has(text: &PaintedText, label: &str) -> bool {
    text.iter().any(|(painted, _)| painted == label)
}

#[test]
fn native_tape_settings_hide_inactive_controls_and_explain_the_active_scale() {
    let mut view = view(true);
    let before = view.config.clone();
    let (_, text) = open_controls(&mut view);
    for inactive in [
        "cluster",
        "fold dust",
        "region height",
        "summarize closed bars",
        "smallest print px",
        "full size at",
        "Auto · largest in session",
        "side separation px",
        "width",
        "bubble size",
    ] {
        assert!(!has(&text, inactive), "inactive tape control: {inactive}");
    }
    for meaningful in [
        "Tape only (hide trail)",
        "auto: on",
        "Ignore opening burst in scale",
        "window",
        "duration",
        "biggest print px",
        "render style",
        "fill opacity",
        "readable from px",
        "boundary and live-edge line",
        "quantity inside the bubble",
        "buy",
        "sell",
    ] {
        assert!(has(&text, meaningful), "missing tape control: {meaningful}");
    }
    let text = text
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("dot full size qty"));
    assert!(text.contains("hide below qty"));
    assert!(text.contains("100 ms"));
    assert!(text.contains("one price tick"));
    assert!(text.contains("largest visible dot"));
    assert_eq!(
        view.config, before,
        "opening settings must not rewrite a preset"
    );
}

#[test]
fn legacy_bubble_settings_keep_their_controls_and_leave_the_crypto_look_unchanged() {
    let mut view = view(false);
    let before = view.config.clone();
    let (_, text) = open_controls(&mut view);
    for active in [
        "cluster",
        "fold dust",
        "region height",
        "summarize closed bars",
        "smallest print px",
        "full size at",
        "Auto · largest in session",
        "side separation px",
        "width",
        "bubble size",
    ] {
        assert!(has(&text, active), "missing legacy control: {active}");
    }
    assert!(!has(&text, "Ignore opening burst in scale"));
    assert_eq!(view.config, before);
}

#[test]
fn the_tape_quantity_tooltip_does_not_claim_to_resize_the_independent_candle_overlay() {
    let mut view = view(true);
    let before = view.config.clone();
    let (ctx, text) = open_controls(&mut view);
    let pos = text
        .iter()
        .find(|(label, _)| label.starts_with("dot full size qty"))
        .expect("the typed tape reference is visible")
        .1
        .center();
    // egui suppresses tooltips after a click until it observes movement.
    // Its velocity history needs three samples and its post-click grace is
    // 100 ms. Approach for eight 60 Hz frames, rather than teleporting back
    // after PointerGone, then let the tooltip's sizing frame settle.
    for offset in [-7.0, -6.0, -5.0, -4.0, -3.0, -2.0, -1.0, 0.0] {
        let _ = paint(
            &mut view,
            &ctx,
            vec![egui::Event::PointerMoved(pos + egui::vec2(offset, 0.0))],
        );
    }
    // Hit testing uses the previous frame; a tooltip Area then needs its
    // invisible sizing frame before its text can be painted. Keep hovering
    // through those frames with an explicit, deterministic UI clock.
    for _ in 0..3 {
        let _ = paint(&mut view, &ctx, Vec::new());
    }
    let text = paint(&mut view, &ctx, Vec::new());
    let hovered =
        ctx.interaction_snapshot(|state| state.hovered.iter().copied().collect::<Vec<_>>());
    let responses: Vec<_> = hovered.iter().map(|id| ctx.read_response(*id)).collect();
    let tooltip = text
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        tooltip.contains("Typing a value turns auto off"),
        "hover must expose the reference tooltip at {pos:?}; hovered: {responses:?}; painted text: {tooltip}"
    );
    assert!(!tooltip.contains("both panes"));
    assert!(tooltip.contains("tape"));
    assert_eq!(view.config, before);
}

#[path = "native_split_controls_tests.rs"]
mod native_split_controls_tests;

#[test]
fn fixed_native_size_control_edits_the_drawn_reference_and_preserves_grouping() {
    for automatic in [false, true] {
        let mut view = view(true);
        view.config.live_lane.stable_size = true;
        view.config.volume_dots.auto_full = automatic;
        view.config.volume_dots.full_quantity = 1.0;
        view.config.bubbles.size_reference = BubbleSizeReference::Fixed;
        view.config.bubbles.size_reference_quantity = 8.0;
        let tick = quantick_orderflow::TickBubbleStyle {
            zoom_regions: false,
            peak_spheres: None,
            viewport_scale: None,
            reference_quantity: 1.0,
            reference_mode: quantick_orderflow::TickReferenceMode::VisibleMax,
            max_radius: 32.0,
            label_min_radius: 8.0,
            opacity: 0.9,
        };
        view.config.volume_dots.tick_bubbles = Some(tick);
        let before = view.config.clone();
        let (ctx, text) = open_controls(&mut view);
        assert_eq!(
            view.config, before,
            "opening the controls changes no settings"
        );
        assert_eq!(has(&text, "smallest print px"), !automatic);
        assert!(
            !text
                .iter()
                .any(|(label, _)| label.starts_with("dot full size qty"))
        );
        let control = text
            .iter()
            .find(|(label, _)| label.starts_with("Tape full size qty"))
            .expect("the effective Tape reference is editable");
        assert!(control.0.contains('8'));
        let pos = control.1.center();
        for pressed in [true, false] {
            let _ = paint(
                &mut view,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let _ = paint(
            &mut view,
            &ctx,
            vec![
                key(egui::Key::A, egui::Modifiers::COMMAND),
                egui::Event::Text("16".to_owned()),
            ],
        );
        let _ = paint(
            &mut view,
            &ctx,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(view.config.bubbles.size_reference_quantity, 16.0);
        assert_eq!(
            view.config.volume_dots.tick_bubbles,
            Some(tick),
            "editing the Tape reference preserves the candle reference and policy"
        );
        assert_eq!(
            view.config.volume_dots.full_quantity, 1.0,
            "grouping scale stays intact"
        );
        assert!(
            !view.config.volume_dots.auto_full,
            "typing selects the fixed scale"
        );
    }
}
