use super::*;
use crate::surfaces::Surface;
use quantick_chart_interaction::pane::PaneSide;
use quantick_drawings::DrawingId;
use std::time::Instant;
fn surface() -> StrategyPopupSurface {
    super::super::Adapter::new(View::default(), effects)
}
fn open(surface: &mut StrategyPopupSurface) {
    surface.update(Intent::Open {
        tab: 7,
        side: PaneSide::Flow,
        drawing: DrawingId(1),
        form: Box::new(presets::StoredPreset::starting_point(
            quantick_engine::Side::Buy,
        )),
    });
}
#[test]
fn a_closed_dialog_asks_for_nothing() {
    let ctx = egui::Context::default();
    let mut surface = surface();
    let _ = ctx.run(Default::default(), |ctx| {
        assert_eq!(
            surface.draw(ctx, &SurfaceEnv::quiet(Instant::now())),
            SurfaceResponse::default()
        );
    });
}
#[test]
fn an_untouched_dialog_survives_the_frame() {
    let ctx = egui::Context::default();
    let mut surface = surface();
    open(&mut surface);
    surface.update(Intent::Edit(Field::Text(TextField::Quantity, "3".into())));
    let _ = ctx.run(Default::default(), |ctx| {
        let response = surface.draw(
            ctx,
            &SurfaceEnv {
                active_tab: 7,
                ..SurfaceEnv::quiet(Instant::now())
            },
        );
        assert_eq!(response, SurfaceResponse::default());
    });
    assert_eq!(surface.model_state.popup().unwrap().form.quantity, "3");
}

#[test]
fn painted_alarm_section_consumes_only_the_deferred_picker_it_reaches() {
    use crate::surfaces::View as _;
    let ctx = egui::Context::default();
    let mut surface = surface();
    surface.update(Intent::StageSoundPicker);
    assert_eq!(
        frame(&mut surface, &ctx, vec![]).1,
        SurfaceResponse::default()
    );
    assert!(
        surface.model_state.sound_picker_pending(),
        "closed paint retains staging"
    );
    open(&mut surface);
    let _ = ctx.run(Default::default(), |ctx| {
        surface.with_updates(
            |response| assert_eq!(response, SurfaceResponse::default()),
            |view, session| view.paint_updated(session, &mut ViewContext { ctx, ui: None }, None),
        );
    });
    assert!(
        surface.model_state.sound_picker_pending(),
        "omitted environment retains staging"
    );
    let _ = ctx.run(Default::default(), |ctx| {
        assert_eq!(
            surface.draw(ctx, &SurfaceEnv::quiet(Instant::now())),
            SurfaceResponse::default()
        );
    });
    assert!(
        !surface.model_state.is_open(),
        "foreign owner closes before painting"
    );
    assert!(surface.model_state.sound_picker_pending());
    open(&mut surface);
    surface.update(Intent::Edit(Field::Alarm(false)));
    for _ in 0..2 {
        assert_eq!(
            frame(&mut surface, &ctx, vec![]).1,
            SurfaceResponse::default()
        );
    }
    assert!(
        !surface.model_state.sound_picker_pending(),
        "alarm-off section consumes before its early return"
    );
    assert!(!ctx.memory(|memory| memory.any_popup_open()));
    surface.update(Intent::Cancel);
    open(&mut surface);
    surface.update(Intent::Edit(Field::Alarm(true)));
    for _ in 0..2 {
        assert_eq!(
            frame(&mut surface, &ctx, vec![]).1,
            SurfaceResponse::default()
        );
    }
    assert!(
        !ctx.memory(|memory| memory.any_popup_open()),
        "later opening does not inherit the consumed picker"
    );
}

fn frame(
    surface: &mut StrategyPopupSurface,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, SurfaceResponse) {
    let mut response = SurfaceResponse::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1366.0, 768.0),
            )),
            time: Some(ctx.input(|i| i.time) + 1.0 / 60.0),
            events,
            ..Default::default()
        },
        |ctx| {
            response = surface.draw(
                ctx,
                &SurfaceEnv {
                    active_tab: 7,
                    counted_bar_sides: &[PaneSide::Flow],
                    ..SurfaceEnv::quiet(Instant::now())
                },
            );
        },
    );
    (output, response)
}
fn text_center(output: &egui::FullOutput, expected: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == expected => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing painted {expected}"))
}
fn click(
    surface: &mut StrategyPopupSurface,
    ctx: &egui::Context,
    at: egui::Pos2,
) -> SurfaceResponse {
    let mut last = SurfaceResponse::default();
    for pressed in [true, false] {
        last = frame(
            surface,
            ctx,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        )
        .1;
    }
    last
}
#[test]
fn painted_side_submit_and_cancel_emit_model_intents() {
    let ctx = egui::Context::default();
    let mut surface = surface();
    open(&mut surface);
    frame(&mut surface, &ctx, vec![]);
    let (painted, _) = frame(&mut surface, &ctx, vec![]);
    click(&mut surface, &ctx, text_center(&painted, "SELL"));
    assert_eq!(surface.model_state.popup().unwrap().form.side, "sell");
    let (painted, _) = frame(&mut surface, &ctx, vec![]);
    let arm_center = text_center(&painted, "Arm");
    assert!(arm_center.y < 768.0, "footer remains reachable on a laptop");
    let response = click(&mut surface, &ctx, arm_center);
    assert_eq!(response.arm_strategy.unwrap().form.side, "sell");
    assert!(surface.model_state.is_open(), "host has not settled yet");
    let (painted, _) = frame(&mut surface, &ctx, vec![]);
    let response = click(&mut surface, &ctx, text_center(&painted, "Cancel"));
    assert_eq!(response, SurfaceResponse::default());
    assert!(!surface.model_state.is_open());
}
#[test]
fn painted_sound_test_routes_the_current_cue_without_closing() {
    let ctx = egui::Context::default();
    let mut surface = surface();
    open(&mut surface);
    surface.update(Intent::Edit(Field::Alarm(true)));
    surface.update(Intent::Edit(Field::AlarmCut(true)));
    // A tall viewport exposes every alarm row without a scroll gesture.
    let mut painted = None;
    for _ in 0..2 {
        painted = Some(ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1366.0, 1500.0),
                )),
                ..Default::default()
            },
            |ctx| {
                surface.draw(
                    ctx,
                    &SurfaceEnv {
                        active_tab: 7,
                        counted_bar_sides: &[PaneSide::Flow],
                        ..SurfaceEnv::quiet(Instant::now())
                    },
                );
            },
        ));
    }
    let at = text_center(&painted.unwrap(), "Test");
    let mut response = SurfaceResponse::default();
    for pressed in [true, false] {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1366.0, 1500.0),
                )),
                events: vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                response = surface.draw(
                    ctx,
                    &SurfaceEnv {
                        active_tab: 7,
                        counted_bar_sides: &[PaneSide::Flow],
                        ..SurfaceEnv::quiet(Instant::now())
                    },
                );
            },
        );
    }
    assert_eq!(
        response.test_alert,
        Some(crate::audio::Cue::new(
            AlertSound::default(),
            Some(presets::DEFAULT_ALARM_PLAY_SECS)
        ))
    );
    assert!(surface.model_state.is_open());
}

#[test]
fn painted_preset_save_and_delete_refresh_the_real_bank() {
    for refused in [false, true] {
        let dir = crate::scratch::ScratchDir::new("strategy-popup-bank");
        let path = dir.join("strategies.toml");
        if refused {
            std::fs::create_dir(&path).unwrap();
        }
        let ctx = egui::Context::default();
        let mut surface = surface();
        surface.view.bank = presets::StrategyBank::load_from(path.clone());
        open(&mut surface);
        frame(&mut surface, &ctx, vec![]);
        let (painted, _) = frame(&mut surface, &ctx, vec![]);
        click(&mut surface, &ctx, text_center(&painted, "preset name"));
        frame(
            &mut surface,
            &ctx,
            vec![egui::Event::Text("  remembered  ".into())],
        );
        let (painted, _) = frame(&mut surface, &ctx, vec![]);
        let response = click(&mut surface, &ctx, text_center(&painted, "Save preset"));
        assert_eq!(
            response,
            SurfaceResponse::default(),
            "bank effects settle locally once"
        );
        assert_eq!(surface.model_state.rows()[0].0, "remembered");
        assert!(
            surface.view.bank.get("remembered").is_some(),
            "memory still updates on write refusal"
        );
        if !refused {
            assert!(
                presets::StrategyBank::load_from(path.clone())
                    .get("remembered")
                    .is_some()
            );
        }
        surface.update(Intent::Edit(Field::Text(TextField::Quantity, "25".into())));
        let (painted, _) = frame(&mut surface, &ctx, vec![]);
        click(&mut surface, &ctx, text_center(&painted, "Save preset"));
        assert_eq!(
            surface.model_state.rows().len(),
            1,
            "overwrite retains a single row"
        );
        assert_eq!(surface.model_state.rows()[0].1.quantity, "25");
        let (painted, _) = frame(&mut surface, &ctx, vec![]);
        click(&mut surface, &ctx, text_center(&painted, "Delete preset"));
        assert!(surface.model_state.rows().is_empty());
        assert!(
            presets::StrategyBank::load_from(path)
                .get("remembered")
                .is_none()
        );
        assert!(surface.model_state.is_open());
    }
}
