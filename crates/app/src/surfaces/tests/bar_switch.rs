use std::time::Instant;

use eframe::egui;

use super::*;

fn frame(
    surface: &mut BarSwitchSurface,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> SurfaceResponse {
    let env = SurfaceEnv::quiet(Instant::now());
    let mut response = SurfaceResponse::default();
    let _ = ctx.run(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ctx| response = surface.draw(ctx, &env),
    );
    response
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn a_typed_digit_opens_the_switch_on_the_registry_candidates() {
    let ctx = egui::Context::default();
    let mut surface = BarSwitchSurface::default();
    frame(&mut surface, &ctx, vec![egui::Event::Text("1".into())]);
    assert!(surface.is_open());
    frame(&mut surface, &ctx, vec![egui::Event::Text("5".into())]);
    assert_eq!(
        surface.candidates(),
        quantick_engine::bar_registry::BUILTIN_BARS.quick_candidates(15)
    );
}

#[test]
fn letters_and_modified_digits_leave_it_closed() {
    let ctx = egui::Context::default();
    let mut surface = BarSwitchSurface::default();
    frame(&mut surface, &ctx, vec![egui::Event::Text("h".into())]);
    assert!(!surface.is_open());
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Text("1".into())],
            modifiers: egui::Modifiers::CTRL,
            ..Default::default()
        },
        |ctx| {
            surface.draw(ctx, &SurfaceEnv::quiet(Instant::now()));
        },
    );
    assert!(!surface.is_open());
}

#[test]
fn enter_applies_the_highlighted_row_to_the_pane_it_opened_over() {
    let ctx = egui::Context::default();
    let mut surface = BarSwitchSurface::default();
    surface.open(7, crate::pane::PaneSide::Time(0), "15");
    // egui holds a focus filter only from the frame after focus arrived.
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, vec![key(egui::Key::ArrowDown)]);
    let response = frame(&mut surface, &ctx, vec![key(egui::Key::Enter)]);
    let chosen = quantick_engine::bar_registry::BUILTIN_BARS.quick_candidates(15)[1];
    assert_eq!(
        response.bar_switch,
        Some(BarSwitchRequest {
            tab: 7,
            side: crate::pane::PaneSide::Time(0),
            config: chosen,
        })
    );
    assert_eq!(chosen.to_config_string(), "time:15s");
    assert!(!surface.is_open());
}

#[test]
fn escape_closes_without_a_change() {
    let ctx = egui::Context::default();
    let mut surface = BarSwitchSurface::default();
    surface.open(0, crate::pane::PaneSide::Flow, "5");
    let response = frame(&mut surface, &ctx, vec![key(egui::Key::Escape)]);
    assert_eq!(response.bar_switch, None);
    assert!(!surface.is_open());
}
