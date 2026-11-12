use std::time::Instant;

use eframe::egui;

use super::*;
use crate::pane::PaneSide;
use crate::surfaces::Surface;
use quantick_engine::bar_registry::BarConfiguration;
type BarSwitchSurface = crate::surfaces::Adapter<Model, View>;
fn new_surface() -> BarSwitchSurface {
    BarSwitchSurface::new(View, effects)
}
trait TestSurface {
    fn open(&mut self, tab: u64, side: PaneSide, query: &str);
    fn open_following_focus(&mut self, tab: u64, side: PaneSide, query: &str);
    fn is_open(&self) -> bool;
    fn candidates(&self) -> Vec<BarConfiguration>;
}
impl TestSurface for BarSwitchSurface {
    fn open(&mut self, tab: u64, side: PaneSide, query: &str) {
        self.update(Intent::Open {
            tab,
            side,
            query: query.into(),
            follows_focus: false,
        });
    }
    fn open_following_focus(&mut self, tab: u64, side: PaneSide, query: &str) {
        self.update(Intent::Open {
            tab,
            side,
            query: query.into(),
            follows_focus: true,
        });
    }
    fn is_open(&self) -> bool {
        self.model_state.open().is_some()
    }
    fn candidates(&self) -> Vec<BarConfiguration> {
        self.model_state
            .open()
            .map(|open| open.candidates.clone())
            .unwrap_or_default()
    }
}

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
    let mut surface = new_surface();
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
    let mut surface = new_surface();
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
    let mut surface = new_surface();
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
fn focusing_another_pane_closes_it_without_a_change() {
    let ctx = egui::Context::default();
    let mut surface = new_surface();
    surface.open(0, crate::pane::PaneSide::Flow, "15");
    frame(&mut surface, &ctx, Vec::new());
    let mut env = SurfaceEnv::quiet(Instant::now());
    env.focused_side = crate::pane::PaneSide::Time(0);
    let response = frame_in(&mut surface, &ctx, vec![key(egui::Key::Enter)], &env);
    assert_eq!(response.bar_switch, None);
    assert!(!surface.is_open());
}

/// The hook opens on the first frame, before a restored workspace has put
/// focus on the time pane. It follows the focus there instead of closing,
/// and applies to the pane that holds it.
#[test]
fn the_hook_opened_switch_follows_focus_to_the_time_pane() {
    let ctx = egui::Context::default();
    let mut surface = new_surface();
    surface.open_following_focus(0, crate::pane::PaneSide::Flow, "15");
    frame(&mut surface, &ctx, Vec::new());
    let mut env = SurfaceEnv::quiet(Instant::now());
    env.focused_side = crate::pane::PaneSide::Time(0);
    frame_in(&mut surface, &ctx, Vec::new(), &env);
    assert!(surface.is_open(), "still open over the time pane");
    let response = frame_in(&mut surface, &ctx, vec![key(egui::Key::Enter)], &env);
    assert_eq!(
        response.bar_switch.map(|request| request.side),
        Some(crate::pane::PaneSide::Time(0))
    );
}

#[test]
fn losing_the_keyboard_closes_it_and_leaves_enter_alone() {
    let ctx = egui::Context::default();
    let mut surface = new_surface();
    surface.open(0, crate::pane::PaneSide::Flow, "15");
    frame(&mut surface, &ctx, Vec::new());
    ctx.memory_mut(|memory| memory.surrender_focus(egui::Id::new("bar_switch").with("query")));
    let response = frame(&mut surface, &ctx, vec![key(egui::Key::Enter)]);
    assert_eq!(response.bar_switch, None);
    assert!(!surface.is_open());
}

#[test]
fn a_letter_after_the_number_lists_the_kind_that_declares_it() {
    let ctx = egui::Context::default();
    let mut surface = new_surface();
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
fn at_full_length_a_typed_letter_replaces_the_letter() {
    let ctx = egui::Context::default();
    let mut surface = new_surface();
    surface.open(0, crate::pane::PaneSide::Flow, "123456789T");
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, vec![egui::Event::Text("R".into())]);
    let query =
        |surface: &BarSwitchSurface| surface.model_state.open().map(|open| open.query.clone());
    assert_eq!(query(&surface).as_deref(), Some("123456789R"));
    // A tenth digit is past what any rule reads: it is dropped, and the
    // letter stays.
    frame(&mut surface, &ctx, vec![egui::Event::Text("5".into())]);
    assert_eq!(query(&surface).as_deref(), Some("123456789R"));
    frame(&mut surface, &ctx, vec![egui::Event::Text("r".into())]);
    assert_eq!(query(&surface).as_deref(), Some("123456789r"));
}

/// Typed one key at a time into the digit-opened popup: `1`, then `m` lists
/// one-minute bars, and `o` after it turns the same query into a month.
/// `d` and `w` list the daily and weekly bars.
#[test]
fn a_typed_unit_lists_the_day_week_and_month_bars() {
    let ctx = egui::Context::default();
    let listed = |surface: &BarSwitchSurface| -> Vec<String> {
        surface
            .candidates()
            .into_iter()
            .map(BarConfiguration::to_config_string)
            .collect()
    };
    let mut surface = new_surface();
    frame(&mut surface, &ctx, vec![egui::Event::Text("1".into())]);
    assert!(surface.is_open());
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, vec![egui::Event::Text("m".into())]);
    assert_eq!(listed(&surface), ["time:1m"]);
    frame(&mut surface, &ctx, vec![egui::Event::Text("o".into())]);
    assert_eq!(listed(&surface), ["time:1mo"]);
    let response = frame(&mut surface, &ctx, vec![key(egui::Key::Enter)]);
    assert_eq!(
        response
            .bar_switch
            .map(|request| request.config.to_config_string()),
        Some("time:1mo".to_owned())
    );
    for (typed, specs) in [
        ("2d", &["time:2d", "dollar:2"][..]),
        ("1w", &["time:1w"][..]),
    ] {
        let mut surface = new_surface();
        surface.open(0, crate::pane::PaneSide::Flow, typed);
        assert_eq!(listed(&surface), specs, "{typed}");
    }
}

#[test]
fn a_letter_and_enter_in_one_frame_answer_the_lettered_list() {
    let ctx = egui::Context::default();
    let mut surface = new_surface();
    surface.open(0, crate::pane::PaneSide::Flow, "2000");
    frame(&mut surface, &ctx, Vec::new());
    frame(&mut surface, &ctx, Vec::new());
    let response = frame(
        &mut surface,
        &ctx,
        vec![egui::Event::Text("t".into()), key(egui::Key::Enter)],
    );
    assert_eq!(
        response
            .bar_switch
            .map(|request| request.config.to_config_string())
            .as_deref(),
        Some("tick:2000")
    );
}

#[test]
fn a_focused_widget_keeps_typed_digits_and_escape_surrenders_query_focus() {
    let ctx = egui::Context::default();
    let mut surface = new_surface();
    let other = egui::Id::new("other editor");
    ctx.memory_mut(|memory| memory.request_focus(other));
    frame(&mut surface, &ctx, vec![egui::Event::Text("1".into())]);
    assert!(!surface.is_open());
    ctx.memory_mut(|memory| memory.surrender_focus(other));
    frame(&mut surface, &ctx, vec![egui::Event::Text("1".into())]);
    let query = egui::Id::new("bar_switch").with("query");
    assert!(surface.is_open());
    assert!(
        ctx.memory(|memory| memory.has_focus(query)),
        "opening paints and focuses in the same frame"
    );
    assert_eq!(
        frame(&mut surface, &ctx, vec![key(egui::Key::Escape)]).bar_switch,
        None
    );
    assert!(!surface.is_open());
    assert!(!ctx.memory(|memory| memory.has_focus(query)));
}

#[test]
fn clicking_a_painted_row_routes_its_current_candidate() {
    fn center(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.visual_bounding_rect().center())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| center(shape, label)),
            _ => None,
        }
    }
    let ctx = egui::Context::default();
    let mut surface = new_surface();
    surface.open(7, PaneSide::Time(1), "2000t");
    let env = SurfaceEnv {
        active_tab: 7,
        focused_side: PaneSide::Time(1),
        ..SurfaceEnv::quiet(Instant::now())
    };
    for _ in 0..3 {
        frame_in(&mut surface, &ctx, vec![], &env);
    }
    let output = ctx.run(Default::default(), |ctx| {
        surface.draw(ctx, &env);
    });
    let label = surface.candidates()[1].quick_label();
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| center(&shape.shape, &label))
        .expect("candidate row painted");
    let mut request = None;
    for pressed in [true, false] {
        request = request.or(frame_in(
            &mut surface,
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
            &env,
        )
        .bar_switch);
    }
    let request = request.expect("click selects current row");
    assert_eq!((request.tab, request.side), (7, PaneSide::Time(1)));
    assert_eq!(request.config.to_config_string(), "trades:2000");
    assert!(!surface.is_open());
}

#[test]
fn captured_hook_updates_the_registered_model_before_first_paint_once() {
    let ctx = egui::Context::default();
    let mut registry = crate::surfaces::Surfaces::default();
    crate::hooks::captured::set_on_this_thread(Some(&[("QUANTICK_BAR_SWITCH", "50R")]));
    let _ = ctx.run(Default::default(), |ctx| {
        registry.draw_all(ctx, &SurfaceEnv::quiet(Instant::now()));
    });
    crate::hooks::captured::set_on_this_thread(None);
    let open = registry
        .bar_switch
        .model_state
        .open()
        .expect("hook opened before paint");
    assert_eq!(open.query, "50R");
    assert!(open.follows_focus);
    assert!(!open.focus_pending, "first paint requested focus");
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("bar_switch").with("query"))));
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Escape)],
            ..Default::default()
        },
        |ctx| {
            registry.draw_all(ctx, &SurfaceEnv::quiet(Instant::now()));
        },
    );
    let _ = ctx.run(Default::default(), |ctx| {
        registry.draw_all(ctx, &SurfaceEnv::quiet(Instant::now()));
    });
    assert!(registry.bar_switch.model_state.open().is_none());
}
