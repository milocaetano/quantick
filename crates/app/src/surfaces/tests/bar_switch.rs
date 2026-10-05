use std::time::Instant;

use eframe::egui;

use super::*;

fn frame(
    surface: &mut BarSwitchSurface,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> SurfaceResponse {
    frame_in(surface, ctx, events, &SurfaceEnv::quiet(Instant::now()))
}

fn frame_in(
    surface: &mut BarSwitchSurface,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    env: &SurfaceEnv<'_>,
) -> SurfaceResponse {
    let mut response = SurfaceResponse::default();
    let _ = ctx.run(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ctx| response = surface.draw(ctx, env),
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
    surface.open(0, crate::pane::PaneSide::Flow, "15");
    // egui holds a focus filter only from the frame after focus arrived.
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, vec![key(egui::Key::ArrowDown)]);
    let response = frame(&mut surface, &ctx, vec![key(egui::Key::Enter)]);
    let chosen = quantick_engine::bar_registry::BUILTIN_BARS.quick_candidates(15)[1];
    assert_eq!(
        response.bar_switch,
        Some(BarSwitchRequest {
            tab: 0,
            side: crate::pane::PaneSide::Flow,
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

#[test]
fn focusing_another_pane_closes_it_without_a_change() {
    let ctx = egui::Context::default();
    let mut surface = BarSwitchSurface::default();
    surface.open(0, crate::pane::PaneSide::Flow, "15");
    frame(&mut surface, &ctx, Vec::new());
    let mut env = SurfaceEnv::quiet(Instant::now());
    env.focused_side = crate::pane::PaneSide::Time(0);
    let response = frame_in(&mut surface, &ctx, vec![key(egui::Key::Enter)], &env);
    assert_eq!(response.bar_switch, None);
    assert!(!surface.is_open());
}

#[test]
fn losing_the_keyboard_closes_it_and_leaves_enter_alone() {
    let ctx = egui::Context::default();
    let mut surface = BarSwitchSurface::default();
    surface.open(0, crate::pane::PaneSide::Flow, "15");
    frame(&mut surface, &ctx, Vec::new());
    ctx.memory_mut(|memory| memory.surrender_focus(egui::Id::new("bar_switch").with("query")));
    let response = frame(&mut surface, &ctx, vec![key(egui::Key::Enter)]);
    assert_eq!(response.bar_switch, None);
    assert!(!surface.is_open());
}

#[test]
fn a_new_number_moves_the_highlight_back_to_the_top() {
    let ctx = egui::Context::default();
    let mut surface = BarSwitchSurface::default();
    surface.open(0, crate::pane::PaneSide::Flow, "15");
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, Vec::new());
    for _ in 0..3 {
        frame(&mut surface, &ctx, vec![key(egui::Key::ArrowDown)]);
    }
    frame(&mut surface, &ctx, vec![egui::Event::Text("0".into())]);
    let response = frame(&mut surface, &ctx, vec![key(egui::Key::Enter)]);
    let first = quantick_engine::bar_registry::BUILTIN_BARS.quick_candidates(150)[0];
    assert_eq!(
        response.bar_switch.map(|request| request.config),
        Some(first)
    );
}

#[test]
fn a_letter_after_the_number_lists_the_kind_that_declares_it() {
    let ctx = egui::Context::default();
    let mut surface = BarSwitchSurface::default();
    surface.open(0, crate::pane::PaneSide::Flow, "5");
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, vec![egui::Event::Text("0".into())]);
    frame(&mut surface, &ctx, vec![egui::Event::Text("R".into())]);
    let labels: Vec<String> = surface
        .candidates()
        .into_iter()
        .map(BarConfiguration::quick_label)
        .collect();
    assert_eq!(labels, ["50 Ticks (Renko)"]);
    let response = frame(&mut surface, &ctx, vec![key(egui::Key::Enter)]);
    assert_eq!(
        response
            .bar_switch
            .map(|request| request.config.to_config_string()),
        Some("renko:50".to_owned())
    );
}

#[test]
fn the_hook_opens_on_a_typed_letter_too() {
    let mut surface = BarSwitchSurface::default();
    surface.open(0, crate::pane::PaneSide::Flow, "50R");
    assert_eq!(
        surface.candidates(),
        quantick_engine::bar_registry::BUILTIN_BARS.quick_matches("50R")
    );
    assert_eq!(surface.candidates().len(), 1);
}

#[test]
fn at_full_length_a_typed_letter_replaces_the_letter() {
    let ctx = egui::Context::default();
    let mut surface = BarSwitchSurface::default();
    surface.open(0, crate::pane::PaneSide::Flow, "123456789T");
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, vec![egui::Event::Text("R".into())]);
    let query = |surface: &BarSwitchSurface| surface.open.as_ref().map(|open| open.query.clone());
    assert_eq!(query(&surface).as_deref(), Some("123456789R"));
    // A tenth digit is past what any rule reads: it is dropped, and the
    // letter stays.
    frame(&mut surface, &ctx, vec![egui::Event::Text("5".into())]);
    assert_eq!(query(&surface).as_deref(), Some("123456789R"));
    frame(&mut surface, &ctx, vec![egui::Event::Text("r".into())]);
    assert_eq!(query(&surface).as_deref(), Some("123456789r"));
}
