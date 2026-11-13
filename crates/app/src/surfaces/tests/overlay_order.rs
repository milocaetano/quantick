use super::*;
use quantick_ui_model::Slot;
fn registry() -> Surfaces {
    let mut registry = Surfaces::default();
    registry.phase_trace = Some(vec![]);
    registry.first = Adapter::new(
        first_view::View {
            enabled: true,
            hook_intent: Some((3, 10)),
            ..Default::default()
        },
        first_view::effects,
    )
    .in_slots(&[Slot::LeadingOverlay, Slot::Overlay]);
    registry.second = Adapter::new(
        second_view::View {
            enabled: true,
            intent: Some((4, 17)),
            ..Default::default()
        },
        second_view::effects,
    )
    .in_slots(&[Slot::Overlay]);
    registry
}
fn order(registry: &Surfaces, phase: &str) -> Vec<&'static str> {
    registry
        .phase_trace
        .as_ref()
        .unwrap()
        .iter()
        .filter_map(|(p, entry)| {
            (*p == phase && matches!(*entry, "first" | "toast" | "script_editor" | "second"))
                .then_some(*entry)
        })
        .collect()
}
#[test]
fn leading_overlay_preserves_hooks_capture_and_actual_legacy_interleaving() {
    let mut registry = registry();
    let ctx = egui::Context::default();
    let env = SurfaceEnv::quiet(Instant::now());
    registry.capture_keyboard(&ctx);
    assert_eq!(
        order(&registry, "capture"),
        vec!["first", "toast", "script_editor", "second"]
    );
    let mut response = SurfaceResponse::default();
    let _ = ctx.run(Default::default(), |ctx| {
        response = registry.draw_slot(Slot::Toolbar, &mut ViewContext { ctx, ui: None }, &env);
    });
    assert_eq!(response, SurfaceResponse::default());
    assert!(order(&registry, "hook").is_empty());
    assert!(registry.hooks_pending());
    assert_eq!(registry.first.model_state, 0);
    assert_eq!(registry.first.view.hook_intent, Some((3, 10)));
    let _ = ctx.run(Default::default(), |ctx| {
        response = registry.draw_all(ctx, &env)
    });
    assert!(!registry.hooks_pending());
    assert_eq!(
        order(&registry, "hook"),
        vec!["first", "toast", "script_editor", "second"]
    );
    assert_eq!(
        order(&registry, "paint"),
        vec!["first", "toast", "script_editor", "second"]
    );
    let trace = registry.phase_trace.as_ref().unwrap();
    let first_paint = trace
        .iter()
        .position(|(phase, _)| *phase == "paint")
        .unwrap();
    assert!(
        trace[first_paint..]
            .iter()
            .all(|(phase, _)| *phase != "hook")
    );
    assert_eq!(
        registry.first.view.painted,
        Some(3),
        "leading hook precedes first paint"
    );
    assert_eq!(
        response.model_effects,
        vec![HostEffect::First((3, 10)), HostEffect::Second((8, 17))]
    );
    registry.phase_trace.as_mut().unwrap().clear();
    let _ = ctx.run(Default::default(), |ctx| {
        registry.draw_all(ctx, &env);
    });
    assert!(order(&registry, "hook").is_empty());
    assert_eq!(
        order(&registry, "paint"),
        vec!["first", "toast", "script_editor", "second"],
        "dual-slot entry paints once"
    );
}
#[test]
fn direct_ordinary_overlay_initializes_all_hooks_but_never_paints_leading_entries() {
    let mut registry = registry();
    let ctx = egui::Context::default();
    let env = SurfaceEnv::quiet(Instant::now());
    let _ = ctx.run(Default::default(), |ctx| {
        registry.draw_slot(Slot::Overlay, &mut ViewContext { ctx, ui: None }, &env);
    });
    assert_eq!(
        order(&registry, "hook"),
        vec!["first", "toast", "script_editor", "second"]
    );
    assert_eq!(
        order(&registry, "paint"),
        vec!["toast", "script_editor", "second"]
    );
    assert_eq!(registry.first.model_state, 3);
    assert_eq!(registry.first.view.painted, None);
    registry.phase_trace.as_mut().unwrap().clear();
    let _ = ctx.run(Default::default(), |ctx| {
        registry.draw_all(ctx, &env);
    });
    assert!(order(&registry, "hook").is_empty());
    assert_eq!(registry.first.view.painted, Some(3));
}
#[test]
fn ordinary_nonoverlay_keyboard_capture_is_retained_once() {
    let mut registry = registry();
    registry.second =
        Adapter::new(Default::default(), second_view::effects).in_slots(&[Slot::Toolbar]);
    registry.capture_keyboard(&egui::Context::default());
    assert_eq!(
        order(&registry, "capture"),
        vec!["first", "toast", "script_editor", "second"]
    );
}

#[test]
fn leading_effect_wins_and_real_toast_undo_survives_the_same_overlay_fold() {
    let mut registry = registry();
    registry.first = Adapter::new(
        first_view::View {
            enabled: true,
            ..Default::default()
        },
        |effect| SurfaceResponse {
            model_effects: vec![HostEffect::First(effect)],
            test_alert: Some(crate::audio::Cue::new(
                crate::audio::AlertSound::default(),
                Some(1),
            )),
            ..Default::default()
        },
    )
    .in_slots(&[Slot::LeadingOverlay, Slot::Overlay]);
    registry.second = Adapter::new(
        second_view::View {
            enabled: true,
            ..Default::default()
        },
        |effect| SurfaceResponse {
            model_effects: vec![HostEffect::Second(effect)],
            test_alert: Some(crate::audio::Cue::new(
                crate::audio::AlertSound::default(),
                Some(2),
            )),
            ..Default::default()
        },
    )
    .in_slots(&[Slot::Overlay]);
    registry
        .strategy_popup
        .update(quantick_ui_model::strategy_popup::Intent::Open {
            tab: 0,
            side: crate::pane::PaneSide::Flow,
            drawing: crate::drawings::DrawingId(1),
            form: Box::new(crate::strategy_presets::StoredPreset::starting_point(
                quantick_engine::Side::Buy,
            )),
        });
    let now = Instant::now();
    registry.toast.note_with_undo("removed fixture", now);
    let ctx = egui::Context::default();
    let env = SurfaceEnv::quiet(now);
    let input = |events| egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1366.0, 768.0),
        )),
        events,
        ..Default::default()
    };
    let mut painted = None;
    for _ in 0..2 {
        painted = Some(ctx.run(input(vec![]), |ctx| {
            registry.draw_all(ctx, &env);
        }));
    }
    let painted = painted.unwrap();
    for expected in ["Arm strategy", "removed fixture"] {
        assert!(painted.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == expected)), "real popup and toast coexist: {expected}");
    }
    let at = registry
        .toast
        .undo_rect()
        .expect("real legacy undo button")
        .center();
    let mut response = SurfaceResponse::default();
    for pressed in [true, false] {
        if !pressed {
            registry.first.view.intent = Some((5, 10));
            registry.second.view.intent = Some((4, 17));
        }
        let _ = ctx.run(
            input(vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]),
            |ctx| {
                response = registry.draw_all(ctx, &env);
            },
        );
    }
    assert!(
        response.undo_drawing,
        "actual legacy effect survives between model passes"
    );
    assert_eq!(
        response.test_alert,
        Some(crate::audio::Cue::new(
            crate::audio::AlertSound::default(),
            Some(1)
        ))
    );
    assert_eq!(
        response.model_effects,
        vec![HostEffect::First((5, 10)), HostEffect::Second((8, 17))]
    );
}
