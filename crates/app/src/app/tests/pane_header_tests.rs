//! Every pane's header line names its own bars, view, layers, sizes and
//! indicators, and changes them, wherever focus sits.

use super::candle_aggression::context_fixture;
use super::*;
use crate::state::BarSpec;
use crate::ui_state::PanelSection;
use quantick_chart::pane_header::{BubbleSample, HeaderControl, LayerToggle};
use quantick_control_schema::orderflow_settings::{
    CLOSE_CAPABILITY_ID, OPEN_CAPABILITY_ID, SECTION_CAPABILITY_ID,
};
use quantick_engine::bar_selection::{BarInputAvailability, SelectionCommand};
use quantick_orderflow::{LaneFold, LaneSide, LaneView};
use quantick_workspace::orderflow_panel::{PANEL_MAX_WIDTH_PX, PANEL_MIN_WIDTH_PX, PanelControl};
use serde_json::json;

/// Where the bars control of the pane at `side` landed last frame.
fn control_of(app: &QuantickApp, ctx: &egui::Context, side: PaneSide) -> egui::Rect {
    let id = crate::pane_header::menu_id(app.active_tab().pane(side).id);
    ctx.data(|data| data.get_temp(id)).expect("the header drew")
}

fn set_flow_bars(app: &mut QuantickApp, spec: BarSpec) {
    app.active_tab_mut()
        .flow_pane
        .spec
        .update(
            SelectionCommand::Replace(spec.into()),
            BarInputAvailability::PRINTS,
        )
        .unwrap();
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
}

#[test]
fn every_pane_reads_its_own_bars_without_a_click() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    set_flow_bars(&mut app, BarSpec::Tick(2000));
    let focus = app.active_tab().focus;
    let texts = painted_text(&run_frame(&mut app, &ctx));

    for wanted in ["ORDER FLOW", "Tick · 2000", "Tick · 4"] {
        assert!(
            texts.iter().any(|text| text.starts_with(wanted)),
            "{wanted} is missing from {texts:?}"
        );
    }
    assert_eq!(app.active_tab().focus, focus, "reading moved nothing");
}

/// The flow pane's strip comes off its chart, as a time pane's does.
#[test]
fn the_flow_chart_starts_below_its_header() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    run_frame(&mut app, &ctx);
    let tab = app.active_tab();
    let control = control_of(&app, &ctx, PaneSide::Flow);
    let chart = tab.flow_pane.frame.area.expect("flow chart");
    assert!(
        control.bottom() <= chart.top(),
        "the control at {control:?} sits on the chart at {chart:?}"
    );
    assert_eq!(
        chart.top() - control.center().y,
        crate::time_header::HEIGHT_PX / 2.0,
        "the control centres in a strip of the time header's height"
    );
}

#[test]
fn the_bars_control_changes_only_its_own_pane_and_leaves_focus_alone() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    set_flow_bars(&mut app, BarSpec::Tick(2000));
    run_frame(&mut app, &ctx);
    app.active_tab_mut().focus = PaneSide::Time(0);
    run_frame(&mut app, &ctx);
    let time_before = *app.active_tab().time_panes[0].state.spec();
    let time_selected = app.active_tab().time_panes[0].spec.selected_id();

    let control = control_of(&app, &ctx, PaneSide::Flow);
    click_chart(&mut app, &ctx, control.center());
    assert_eq!(
        app.active_tab().focus,
        PaneSide::Time(0),
        "opening the flow pane's menu keeps focus where it was"
    );
    let open = run_frame(&mut app, &ctx);
    let row = painted_text_center(&open, "volume").expect("the menu offers the registered kinds");
    click_chart(&mut app, &ctx, row);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let tab = app.active_tab();
    assert_eq!(tab.flow_pane.spec.selected_id(), "volume");
    assert!(
        tab.flow_pane.state.spec().id() == "volume",
        "the chart rebuilt on what the header says"
    );
    assert_eq!(tab.time_panes[0].state.spec(), &time_before);
    assert_eq!(tab.time_panes[0].spec.selected_id(), time_selected);
    assert_eq!(tab.focus, PaneSide::Time(0));
}

/// Stacked time panes answer to their own controls: the lower pane's menu drives
/// the lower pane, with focus on the flow pane throughout.
#[test]
fn stacked_panes_each_own_their_menu_and_focus_never_moves() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    app.active_tab_mut().focus = PaneSide::Flow;
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let upper = *app.active_tab().time_panes[0].state.spec();
    let flow = *app.active_tab().flow_pane.state.spec();

    let control = control_of(&app, &ctx, PaneSide::Time(1));
    assert_ne!(control, control_of(&app, &ctx, PaneSide::Time(0)));
    click_chart(&mut app, &ctx, control.center());
    let open = run_frame(&mut app, &ctx);
    let ids: Vec<u64> = (0..2)
        .map(|slot| app.active_tab().time_panes[slot].id)
        .chain([app.active_tab().flow_pane.id])
        .collect();
    let opened: Vec<bool> = ids
        .iter()
        .map(|id| ctx.memory(|memory| memory.is_popup_open(crate::pane_header::menu_id(*id))))
        .collect();
    assert_eq!(opened, [false, true, false]);

    let row = painted_text_center(&open, "volume").expect("the menu offers the registered kinds");
    click_chart(&mut app, &ctx, row);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let tab = app.active_tab();
    assert_eq!(tab.time_panes[1].state.spec().id(), "volume");
    assert_eq!(tab.time_panes[0].state.spec(), &upper);
    assert_eq!(tab.flow_pane.state.spec(), &flow);
    assert_eq!(tab.focus, PaneSide::Flow);
}

fn control_rect(app: &QuantickApp, side: PaneSide, control: HeaderControl) -> Option<egui::Rect> {
    app.active_tab()
        .pane(side)
        .frame
        .header_controls
        .iter()
        .find_map(|(painted, rect)| (*painted == control).then_some(*rect))
}

fn flow_control(app: &QuantickApp, control: HeaderControl) -> egui::Rect {
    control_rect(app, PaneSide::Flow, control).expect("the header painted it")
}

const TRAIL: HeaderControl = HeaderControl::Side(LaneSide::Trail);
const TAPE: HeaderControl = HeaderControl::Side(LaneSide::Tape);

fn click_side(app: &mut QuantickApp, ctx: &egui::Context, control: HeaderControl) {
    let at = flow_control(app, control).center();
    click_chart(app, ctx, at);
    run_frame(app, ctx);
}

fn view_of(app: &QuantickApp) -> LaneView {
    app.active_tab().tape().cached_config().live_lane.view()
}

fn flow_fixture(ctx: &egui::Context) -> (QuantickApp, impl Sized) {
    let (mut app, events, commands) = context_fixture(ctx);
    set_flow_bars(&mut app, BarSpec::Tick(2000));
    app.active_tab_mut().focus = PaneSide::Flow;
    for _ in 0..2 {
        run_frame(&mut app, ctx);
    }
    (app, (events, commands))
}

fn on(app: &QuantickApp, layer: ChartLayer) -> bool {
    let style = &app.runtime.style;
    app.active_tab().flow_pane.layer_switched_on(layer, style)
}

fn press_key(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    key: egui::Key,
    modifiers: egui::Modifiers,
) {
    let down = |pressed| egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    };
    run_frame_with_modifiers(app, ctx, vec![down(true), down(false)], modifiers);
    run_frame(app, ctx);
}

fn drag(app: &mut QuantickApp, ctx: &egui::Context, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(from)]);
    run_frame_with_events(app, ctx, vec![button(from, true)]);
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(to)]);
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(to)]);
    run_frame_with_events(app, ctx, vec![button(to, false)]);
    run_frame(app, ctx);
}

/// The toolbar has no LAYERS, and neither does a header line now: a flow pane's
/// keeps its side switches and strip, a time pane's nothing, and each chart side's capsule
/// floats over that side, below the header, holding that side's own controls.
#[test]
fn each_pane_paints_only_the_controls_it_accepts() {
    let ctx = egui::Context::default();
    let (app, _ends) = flow_fixture(&ctx);
    let flow = &app.active_tab().flow_pane;
    let area = flow.frame.area.expect("flow chart");
    let divider = flow.frame.lane_divider_x.expect("the tape is beside");
    let chart = flow.frame.chart_rect.expect("flow chart");
    for control in [TRAIL, TAPE, HeaderControl::Layer(LayerToggle::LiveStrip)] {
        let rect = flow_control(&app, control);
        assert!(
            rect.bottom() <= area.top(),
            "{control:?} sits off the chart"
        );
        assert!(rect.left() > area.center().x, "{control:?} stands right");
    }
    for (control, right_side) in [
        (HeaderControl::Layer(LayerToggle::Bubbles), false),
        (HeaderControl::Sample(BubbleSample::Candle), false),
        (HeaderControl::Layer(LayerToggle::Heatmap), false),
        (HeaderControl::Layer(LayerToggle::Footprint), false),
        (HeaderControl::Layer(LayerToggle::TapeBubbles), true),
        (HeaderControl::Sample(BubbleSample::Tape), true),
        (HeaderControl::Layer(LayerToggle::TapeHeatmap), true),
    ] {
        let rect = flow_control(&app, control);
        let capsule = flow.frame.capsules[usize::from(right_side)].expect("its capsule");
        assert_eq!(capsule.top(), area.top() + 14.0, "14 points below the rule");
        assert!(capsule.contains_rect(rect), "{control:?} in its capsule");
        assert_eq!(
            rect.left() >= divider,
            right_side,
            "{control:?} on its side"
        );
        let edge = if right_side { chart.right() } else { divider };
        assert!(rect.right() <= edge - 28.0, "{control:?} inside its corner");
    }
    let time = &app.active_tab().pane(PaneSide::Time(0)).frame;
    let only: Vec<_> = time
        .header_controls
        .iter()
        .map(|(control, _)| *control)
        .collect();
    assert_eq!(only, [HeaderControl::Layer(LayerToggle::Footprint)]);
    let (_, footprint) = time.header_controls[0];
    let chart = time.area.expect("time chart");
    assert!(
        chart.contains_rect(footprint),
        "the capsule floats on the chart"
    );
}

/// Both lit is beside, one lit is that side alone, and a click on the last lit
/// does nothing. The named read agrees at every step.
#[test]
fn the_side_switches_set_the_view_and_the_last_lit_stays() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    for (control, wanted) in [
        (TAPE, LaneView::CandlesOnly),
        (TRAIL, LaneView::CandlesOnly),
        (TAPE, LaneView::Beside),
        (TRAIL, LaneView::TapeOnly),
        (TAPE, LaneView::TapeOnly),
        (TRAIL, LaneView::Beside),
    ] {
        click_side(&mut app, &ctx, control);
        assert_eq!(view_of(&app), wanted, "{control:?}");
    }
}

/// The scene reads what the switches read: lit is selected, and the last lit
/// names why it takes no press, with the call that does what the hand does.
#[test]
fn the_scene_reads_the_side_switches() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let pane_id = app.active_tab().flow_pane.id;
    let side = |app: &QuantickApp, name: &str| {
        scene_control(
            &observer_scene(app),
            &format!("pane.{pane_id}.header.side.{name}"),
        )
        .clone()
    };
    for name in ["trail", "tape"] {
        let control = side(&app, name);
        assert_eq!(control["selected"], true, "{name}");
        assert_eq!(control["availability"]["available"], true, "{name}");
        assert_eq!(control["role"], "toggle", "{name}");
        assert_eq!(control["capability_id"], "orderflow.tape.view.set");
    }
    assert_eq!(side(&app, "trail")["label"], "Trail");
    assert_eq!(side(&app, "tape")["label"], "Tape");

    click_side(&mut app, &ctx, TAPE);
    assert_eq!(side(&app, "tape")["selected"], false);
    assert_eq!(side(&app, "tape")["availability"]["available"], true);
    let last = side(&app, "trail");
    assert_eq!(last["selected"], true);
    assert_eq!(last["availability"]["available"], false);
    assert_eq!(
        last["availability"]["reason"],
        "the_last_side_showing_stays"
    );
}

/// The words stand beside the glyphs while the line has room.
#[test]
fn the_side_switches_wear_their_words_and_lose_them_on_a_narrow_line() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let texts = painted_text(&run_frame(&mut app, &ctx));
    for word in ["Trail", "Tape"] {
        assert!(texts.iter().any(|text| text == word), "{word} in {texts:?}");
    }
    let wide = flow_control(&app, TRAIL).width();
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    for _ in 0..2 {
        run_frame_at(&mut app, &ctx, egui::vec2(330.0, 700.0));
    }
    let bare = flow_control(&app, TRAIL).width();
    assert!(bare < wide, "{bare} at 330, {wide} wide");
    assert_eq!(bare, quantick_chart::pane_header::segment_px(None));
}

/// Each side's label reads over the chart, in the single-side views too, and
/// leaves with its side.
#[test]
fn each_side_wears_its_label() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = charted_fixture(&ctx);
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    let has = |app: &mut QuantickApp, head: &str| {
        painted_text(&run_frame(app, &ctx))
            .iter()
            .any(|text| text.starts_with(head))
    };
    run_frame(&mut app, &ctx);
    assert!(has(&mut app, "TRAIL · bars to "));
    assert!(has(&mut app, "TAPE · prints to "));
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::CandlesOnly);
    assert!(has(&mut app, "TRAIL · bars to "));
    assert!(!has(&mut app, "TAPE · "));
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    assert!(has(&mut app, "TAPE · prints to "));
    assert!(!has(&mut app, "TRAIL · "));
}

/// A side's capsule does not report how awake it is: the scene is the same at
/// rest and with the pointer on it.
#[test]
fn a_capsule_at_rest_answers_and_the_scene_does_not_see_its_tier() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut().focus = PaneSide::Time(0);
    run_frame(&mut app, &ctx);
    let tape = HeaderControl::Layer(LayerToggle::TapeHeatmap);
    let pane_id = app.active_tab().flow_pane.id;
    let id = format!("pane.{pane_id}.header.layer.tape_heatmap");
    let at_rest = scene_control(&observer_scene(&app), &id).clone();
    let at = flow_control(&app, tape).center();
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
    run_frame(&mut app, &ctx);
    assert_eq!(scene_control(&observer_scene(&app), &id), &at_rest);
    let before = on(&app, ChartLayer::TapeHeatmap);
    click_chart(&mut app, &ctx, at);
    run_frame(&mut app, &ctx);
    assert_ne!(on(&app, ChartLayer::TapeHeatmap), before);
}

/// The key goes to tape only and back beside, from candles only too, and never
/// opens a tab, on a focused flow pane; on a time pane it is swallowed and moves
/// nothing.
#[test]
fn the_tape_key_toggles_tape_only_and_never_opens_a_tab() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let mods = egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT);
    let tabs = app.runtime.tabs.len();
    press_key(&mut app, &ctx, egui::Key::T, mods);
    assert_eq!(view_of(&app), LaneView::TapeOnly);
    press_key(&mut app, &ctx, egui::Key::T, mods);
    assert_eq!(view_of(&app), LaneView::Beside);
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::CandlesOnly);
    press_key(&mut app, &ctx, egui::Key::T, mods);
    assert_eq!(view_of(&app), LaneView::TapeOnly);
    press_key(&mut app, &ctx, egui::Key::T, mods);
    assert_eq!(view_of(&app), LaneView::Beside, "no remembered view");

    app.active_tab_mut().focus = PaneSide::Time(0);
    run_frame(&mut app, &ctx);
    press_key(&mut app, &ctx, egui::Key::T, mods);
    assert_eq!(view_of(&app), LaneView::Beside, "a time pane has no tape");
    assert_eq!(app.runtime.tabs.len(), tabs, "it never opened a tab");
    assert!(
        !app.surfaces.source_picker.is_open(),
        "and never reached the source picker"
    );
}

/// The trail key mirrors the tape key: the trail alone and back beside, from tape
/// only too, swallowed on a time pane, and it never reaches `Ctrl+R`'s replay.
#[test]
fn the_trail_key_mirrors_the_tape_key_and_never_opens_replay() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let mods = egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT);
    press_key(&mut app, &ctx, egui::Key::R, mods);
    assert_eq!(view_of(&app), LaneView::CandlesOnly);
    press_key(&mut app, &ctx, egui::Key::R, mods);
    assert_eq!(view_of(&app), LaneView::Beside);
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    press_key(&mut app, &ctx, egui::Key::R, mods);
    assert_eq!(view_of(&app), LaneView::CandlesOnly);
    press_key(&mut app, &ctx, egui::Key::R, mods);
    assert_eq!(view_of(&app), LaneView::Beside, "no remembered view");

    app.active_tab_mut().focus = PaneSide::Time(0);
    run_frame(&mut app, &ctx);
    press_key(&mut app, &ctx, egui::Key::R, mods);
    assert_eq!(view_of(&app), LaneView::Beside, "a time pane has no tape");
    assert!(
        !app.replay_view.browser_is_open(),
        "and it never reached the replay browser"
    );
}

/// A glyph switches its own pane's layer, wherever focus sits, and leaves focus
/// alone. The footprint on a time pane is that pane's and nobody else's.
#[test]
fn a_glyph_switches_its_own_pane_wherever_focus_sits() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let style = app.runtime.style;
    let footprint = ChartLayer::Footprint;
    let side = PaneSide::Time(0);
    let time_before = app
        .active_tab()
        .pane(side)
        .layer_switched_on(footprint, &style);
    let flow_before = on(&app, footprint);
    let glyph = control_rect(&app, side, HeaderControl::Layer(LayerToggle::Footprint))
        .expect("the time pane's own footprint");
    click_chart(&mut app, &ctx, glyph.center());
    run_frame(&mut app, &ctx);
    assert_ne!(
        app.active_tab()
            .pane(side)
            .layer_switched_on(footprint, &style),
        time_before
    );
    assert_eq!(on(&app, footprint), flow_before);
    assert_eq!(app.active_tab().focus, PaneSide::Flow, "focus stayed");

    let bubbles = on(&app, ChartLayer::Bubbles);
    let glyph = flow_control(&app, HeaderControl::Layer(LayerToggle::Bubbles));
    click_chart(&mut app, &ctx, glyph.center());
    run_frame(&mut app, &ctx);
    assert_ne!(on(&app, ChartLayer::Bubbles), bubbles);
}

/// A right click looks: it opens what configures the layer and switches nothing.
#[test]
fn a_right_click_opens_a_layers_settings_and_switches_nothing() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let secondary = |at, pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    for (toggle, section) in [
        (LayerToggle::Bubbles, PanelSection::Bubbles),
        (LayerToggle::Heatmap, PanelSection::L2),
        (LayerToggle::LiveStrip, PanelSection::L2),
    ] {
        let symbol = app.active_tab().symbol.clone();
        app.runtime
            .tabs
            .with_panel_folds(0)
            .expect("the tab")
            .1
            .set(&symbol, section, false);
        let before = on(&app, toggle.layer());
        let at = flow_control(&app, HeaderControl::Layer(toggle)).center();
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
        run_frame_with_events(&mut app, &ctx, vec![secondary(at, true)]);
        run_frame_with_events(&mut app, &ctx, vec![secondary(at, false)]);
        run_frame(&mut app, &ctx);
        assert!(
            app.active_tab().tape().settings_panel.is_open(),
            "{toggle:?}"
        );
        let folds = app.runtime.tabs.panel_folds();
        assert!(folds.section_open(&symbol, section), "{toggle:?} expands");
        assert_eq!(on(&app, toggle.layer()), before, "looking is not enabling");
    }
}

/// With the candles hidden their capsule goes with them; the keys stay refused,
/// the layers keep their state and the look returns. The strip stays live.
#[test]
fn candles_hidden_take_their_capsule_keep_their_state_and_the_strip_stays_live() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let toggles = [
        LayerToggle::Bubbles,
        LayerToggle::Heatmap,
        LayerToggle::Footprint,
    ];
    let at = |app: &QuantickApp| {
        toggles.map(|toggle| control_rect(app, PaneSide::Flow, HeaderControl::Layer(toggle)))
    };
    let states = toggles.map(|toggle| on(&app, toggle.layer()));
    let view_glyph = flow_control(&app, TRAIL);

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    run_frame(&mut app, &ctx);
    assert_eq!(at(&app), [None; 3], "no candles, no capsule for them");
    assert_eq!(flow_control(&app, TRAIL), view_glyph);
    flow_control(&app, HeaderControl::Layer(LayerToggle::TapeBubbles));

    press_key(&mut app, &ctx, egui::Key::B, egui::Modifiers::CTRL);
    press_key(&mut app, &ctx, egui::Key::F, egui::Modifiers::CTRL);
    assert_eq!(toggles.map(|toggle| on(&app, toggle.layer())), states);

    let caps = app.active_tab().capabilities(&app.config);
    let flow = &app.active_tab().flow_pane;
    for toggle in toggles {
        assert!(
            flow.layer_blocked(toggle.layer(), caps).is_some(),
            "{toggle:?}"
        );
    }
    assert_eq!(flow.layer_blocked(ChartLayer::LiveStrip, caps), None);
    let strip = flow_control(&app, HeaderControl::Layer(LayerToggle::LiveStrip));
    let strip_before = on(&app, ChartLayer::LiveStrip);
    click_chart(&mut app, &ctx, strip.center());
    run_frame(&mut app, &ctx);
    assert_ne!(
        on(&app, ChartLayer::LiveStrip),
        strip_before,
        "the strip moved"
    );
    let flow = &app.active_tab().flow_pane;
    if !strip_before {
        assert!(flow.live_strip_width(caps) > 0.0, "and reserves its band");
    }

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::Beside);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        toggles.map(|toggle| on(&app, toggle.layer())),
        states,
        "the look returns"
    );
    assert!(at(&app).iter().all(Option::is_some), "and its capsule");
    press_key(&mut app, &ctx, egui::Key::B, egui::Modifiers::CTRL);
    assert_ne!(
        on(&app, ChartLayer::Bubbles),
        states[0],
        "the key works again, so its silence above was the gate's"
    );
}

fn set_capabilities(app: &mut QuantickApp, capabilities: quantick_feed::config::FeedCapabilities) {
    app.active_tab_mut().feed_capabilities = quantick_feed::fixed_capabilities(capabilities);
}

/// What the data blocks leaves the capsule and the scene with its sample; the
/// layer keeps its switch, the named read keeps the reason, and the glyph comes
/// back when the data does. A capsule with nothing left is not painted.
#[test]
fn a_glyph_the_data_blocks_is_not_painted_and_returns_with_the_data() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let pane_id = app.active_tab().flow_pane.id;
    let live = app.active_tab().capabilities(&app.config);
    let mut no_book = live;
    no_book.book_capture = false;
    let mut no_volume = live;
    no_volume.traded_volume = false;
    let painted = |app: &QuantickApp, control| control_rect(app, PaneSide::Flow, control).is_some();
    let heatmap = HeaderControl::Layer(LayerToggle::Heatmap);
    let tape_heatmap = HeaderControl::Layer(LayerToggle::TapeHeatmap);
    let bubbles = HeaderControl::Layer(LayerToggle::Bubbles);
    let candle = HeaderControl::Sample(BubbleSample::Candle);
    let footprint = HeaderControl::Layer(LayerToggle::Footprint);

    set_capabilities(&mut app, no_book);
    run_frame(&mut app, &ctx);
    assert!(!painted(&app, heatmap), "book offline blocks L2");
    assert!(!painted(&app, tape_heatmap), "on both sides");
    assert!(painted(&app, footprint), "the footprint stays");
    assert!(painted(&app, bubbles));
    let ids = scene_control_ids(&observer_scene(&app));
    assert!(!ids.contains(&format!("pane.{pane_id}.header.layer.heatmap")));

    set_capabilities(&mut app, no_volume);
    run_frame(&mut app, &ctx);
    assert!(!painted(&app, bubbles), "no traded volume blocks bubbles");
    assert!(!painted(&app, candle), "and their sample");
    assert!(painted(&app, heatmap), "L2 is back with the book");

    no_volume.book_capture = false;
    set_capabilities(&mut app, no_volume);
    run_frame(&mut app, &ctx);
    let frame = &app.active_tab().flow_pane.frame;
    assert_eq!(frame.capsules, [None; 2], "nothing left, no capsule");

    set_capabilities(&mut app, live);
    run_frame(&mut app, &ctx);
    for control in [heatmap, tape_heatmap, bubbles, candle, footprint] {
        assert!(painted(&app, control), "{control:?} returns with the data");
    }
}

/// A press on a capsule is the capsule's. An armed drawing tool places no anchor
/// under it, a right drag starts no ruler, and neither moves focus.
#[test]
fn a_press_on_a_capsule_never_reaches_the_chart() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let bubbles = flow_control(&app, HeaderControl::Layer(LayerToggle::Bubbles));
    let before = on(&app, ChartLayer::Bubbles);
    let focus = app.active_tab().focus;
    app.toolrail
        .arm(crate::toolrail::Tool::Drawing(super::drawing_tool(
            "horizontal-line",
        )));
    click_chart(&mut app, &ctx, bubbles.center());
    run_frame(&mut app, &ctx);
    assert_ne!(on(&app, ChartLayer::Bubbles), before, "the glyph answered");
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "no anchor landed under the capsule"
    );
    app.toolrail.arm(crate::toolrail::Tool::Pointer);
    run_frame(&mut app, &ctx);
    let secondary = |at, pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let (from, to) = (bubbles.center(), bubbles.center() + egui::vec2(-60.0, 60.0));
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(from), secondary(from, true)],
    );
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(to)]);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(to), secondary(to, false)],
    );
    assert!(
        app.chrome_reads().quick_range_actions().is_none(),
        "no ruler started under the capsule"
    );
    assert_eq!(app.active_tab().focus, focus, "focus stayed");
}

/// A side narrower than its capsule paints none; the named call still reaches
/// the layer.
#[test]
fn a_side_too_narrow_for_its_capsule_paints_none() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    set_fold(&mut app, LaneFold::Tape);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(
        control_rect(
            &app,
            PaneSide::Flow,
            HeaderControl::Layer(LayerToggle::TapeBubbles)
        )
        .is_none()
    );
    flow_control(&app, HeaderControl::Layer(LayerToggle::Bubbles));
}

/// A sample grows as it is dragged up, reads back through the named call's
/// value and returns to `1x` on a double click.
#[test]
fn a_dragged_sample_sizes_its_bubbles_and_a_double_click_returns_to_one() {
    use quantick_control_schema::bubble_scale::BubbleScaleTarget;
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let size = |app: &QuantickApp| {
        app.active_tab()
            .tape()
            .bubble_display_scale(BubbleScaleTarget::Tape.into())
    };
    assert_eq!(size(&app), 1.0);
    let sample = flow_control(&app, HeaderControl::Sample(BubbleSample::Tape));
    let to = sample.center() - egui::vec2(0.0, 40.0);
    drag(&mut app, &ctx, sample.center(), to);
    assert!((size(&app) - 2.0).abs() < 0.01, "{}", size(&app));

    let far = sample.center() - egui::vec2(0.0, 4_000.0);
    drag(&mut app, &ctx, sample.center(), far);
    assert_eq!(size(&app), 3.0, "the range holds");
    let center = flow_control(&app, HeaderControl::Sample(BubbleSample::Tape)).center();
    drag(&mut app, &ctx, center, center + egui::vec2(0.0, 4_000.0));
    assert_eq!(size(&app), 0.25, "both ways");

    let center = flow_control(&app, HeaderControl::Sample(BubbleSample::Tape)).center();
    click_chart(&mut app, &ctx, center);
    click_chart(&mut app, &ctx, center);
    run_frame(&mut app, &ctx);
    assert_eq!(size(&app), 1.0, "a double click is 1x");
}

/// A sample shows only while its target is there; the candle sample's slot stays
/// reserved, so the glyphs never shift, and the scene lists what was painted.
#[test]
fn a_sample_hides_with_its_target_and_its_slot_stays() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let candle = HeaderControl::Sample(BubbleSample::Candle);
    let tape = HeaderControl::Sample(BubbleSample::Tape);
    let heatmap = HeaderControl::Layer(LayerToggle::Heatmap);
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.show_aggressions = true);
    run_frame(&mut app, &ctx);
    let glyph = flow_control(&app, heatmap);
    flow_control(&app, candle);
    flow_control(&app, tape);

    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.show_aggressions = false);
    run_frame(&mut app, &ctx);
    assert!(control_rect(&app, PaneSide::Flow, candle).is_none());
    assert_eq!(
        flow_control(&app, heatmap),
        glyph,
        "the slot stays, outlined, so the capsule never shifts"
    );

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::CandlesOnly);
    run_frame(&mut app, &ctx);
    assert!(control_rect(&app, PaneSide::Flow, tape).is_none());
    let pane_id = app.active_tab().flow_pane.id;
    let ids = scene_control_ids(&observer_scene(&app));
    assert!(!ids.contains(&format!("pane.{pane_id}.header.sample.tape")));
    assert!(ids.contains(&format!("pane.{pane_id}.header.side.tape")));

    // Each sample goes with its own target only: candles hidden, the tape's stays.
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.show_aggressions = true);
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    run_frame(&mut app, &ctx);
    assert!(control_rect(&app, PaneSide::Flow, candle).is_none());
    flow_control(&app, tape);
}

/// The header is painted whichever side is on show, and the side switches are
/// what take the tape's band away and give it back.
#[test]
fn the_header_is_painted_in_all_three_views() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    for (control, view, lane) in [
        (TAPE, LaneView::CandlesOnly, false),
        (TAPE, LaneView::Beside, true),
        (TRAIL, LaneView::TapeOnly, true),
        (TRAIL, LaneView::Beside, true),
    ] {
        let button = flow_control(&app, control);
        click_chart(&mut app, &ctx, button.center());
        run_frame(&mut app, &ctx);
        assert_eq!(view_of(&app), view);
        assert_eq!(flow_control(&app, control), button, "{view:?}");
        flow_control(&app, HeaderControl::Layer(LayerToggle::LiveStrip));
        let (candles, tape) = (view != LaneView::TapeOnly, view != LaneView::CandlesOnly);
        for (toggle, side) in [
            (LayerToggle::Bubbles, candles),
            (LayerToggle::Footprint, candles),
            (LayerToggle::TapeBubbles, tape),
            (LayerToggle::TapeHeatmap, tape),
        ] {
            let painted = control_rect(&app, PaneSide::Flow, HeaderControl::Layer(toggle));
            assert_eq!(painted.is_some(), side, "{view:?}: {toggle:?}");
        }
        let flow = &app.active_tab().flow_pane;
        let chart = flow.frame.area.expect("flow chart");
        assert!(button.bottom() <= chart.top(), "{view:?}: off the chart");
        assert_eq!(flow.frame.lane_divider_x.is_some(), lane, "{view:?}");
    }
}

/// A flow pane with candles to pan and zoom, laid out.
fn charted_fixture(ctx: &egui::Context) -> (QuantickApp, impl Sized) {
    let (mut app, commands) = app_with_history(200);
    app.active_tab_mut().focus = PaneSide::Flow;
    for _ in 0..2 {
        run_frame(&mut app, ctx);
    }
    (app, commands)
}

/// What the candles show: their zoom and where their right edge is.
fn candle_viewport(app: &QuantickApp) -> (f32, f32) {
    let pane = &app.active_tab().flow_pane;
    let total = pane.state.bars().len() + usize::from(pane.state.partial().is_some());
    (
        pane.model.viewport.px_per_bar(),
        pane.model.viewport.right_edge_bar(total),
    )
}

/// A dragged sample owns its pointer: its own target moves, the other sample's
/// does not and the candles neither pan nor zoom.
#[test]
fn dragging_a_sample_moves_its_own_target_and_never_the_chart() {
    use quantick_control_schema::bubble_scale::BubbleScaleTarget;
    let ctx = egui::Context::default();
    let (mut app, _ends) = charted_fixture(&ctx);
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.show_aggressions = true);
    run_frame(&mut app, &ctx);
    let size = |app: &QuantickApp, target: BubbleScaleTarget| {
        app.active_tab().tape().bubble_display_scale(target.into())
    };
    let before = candle_viewport(&app);
    let candle = flow_control(&app, HeaderControl::Sample(BubbleSample::Candle)).center();
    drag(&mut app, &ctx, candle, candle - egui::vec2(30.0, 40.0));
    assert!((size(&app, BubbleScaleTarget::Candle) - 2.0).abs() < 0.01);
    assert_eq!(size(&app, BubbleScaleTarget::Tape), 1.0);
    assert_eq!(candle_viewport(&app), before);

    let tape = flow_control(&app, HeaderControl::Sample(BubbleSample::Tape)).center();
    drag(&mut app, &ctx, tape, tape + egui::vec2(-30.0, 20.0));
    assert!((size(&app, BubbleScaleTarget::Tape) - 0.5).abs() < 0.01);
    assert!((size(&app, BubbleScaleTarget::Candle) - 2.0).abs() < 0.01);
    assert_eq!(candle_viewport(&app), before);

    let chart = app.active_tab().flow_pane.frame.area.expect("flow chart");
    let bare = egui::pos2(chart.left() + 100.0, chart.center().y);
    drag(&mut app, &ctx, bare, bare + egui::vec2(60.0, 0.0));
    assert_ne!(
        candle_viewport(&app),
        before,
        "the same drag pans the chart"
    );
}

/// The wheel over the header reaches neither the sample nor the chart beneath
/// it; on the bare chart it zooms the candles.
#[test]
fn a_wheel_over_the_header_is_private_and_the_chart_below_it_still_zooms() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = charted_fixture(&ctx);
    let wheel = |app: &mut QuantickApp, at| {
        run_frame_with_events(
            app,
            &ctx,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 80.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        run_frame(app, &ctx);
    };
    let before = candle_viewport(&app);
    let sample = flow_control(&app, HeaderControl::Sample(BubbleSample::Tape)).center();
    wheel(&mut app, sample);
    let view = flow_control(&app, TAPE).center();
    wheel(&mut app, view);
    assert_eq!(candle_viewport(&app), before);
    assert_eq!(
        app.active_tab().tape().bubble_display_scale(
            quantick_control_schema::bubble_scale::BubbleScaleTarget::Tape.into()
        ),
        1.0
    );

    let flow = &app.active_tab().flow_pane;
    let chart = flow.frame.area.expect("flow chart");
    let below = egui::pos2(chart.left() + 100.0, chart.center().y);
    wheel(&mut app, below);
    assert_ne!(candle_viewport(&app).0, before.0, "the bare chart zooms");
}

/// A hovered or resized header raises no widget id clash at any display density.
#[test]
fn the_header_hovers_and_resizes_without_id_diagnostics_at_each_density() {
    fn has_diagnostic(shape: &egui::Shape) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.job.text.contains("widget ID"),
            egui::Shape::Rect(rect) => rect.stroke.color == egui::Color32::RED,
            egui::Shape::Vec(shapes) => shapes.iter().any(has_diagnostic),
            _ => false,
        }
    }
    for density in [1.0, 1.25, 1.5, 2.0] {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.warn_on_id_clash = true);
        ctx.set_zoom_factor(density);
        let (mut app, _ends) = flow_fixture(&ctx);
        run_frame(&mut app, &ctx);
        let controls = app.active_tab().flow_pane.frame.header_controls.clone();
        assert!(!controls.is_empty(), "density {density}");
        for (control, rect) in controls {
            for _ in 0..2 {
                let output = run_frame_with_events(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerMoved(rect.center())],
                );
                assert!(
                    !output
                        .shapes
                        .iter()
                        .any(|shape| has_diagnostic(&shape.shape)),
                    "{control:?} at density {density}"
                );
            }
        }
    }
}

/// The scene names every painted control with the capability that does what the
/// hand does, and the same state the glyph reads.
#[test]
fn the_scene_names_each_header_control_and_its_call() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    run_frame(&mut app, &ctx);
    let pane_id = app.active_tab().flow_pane.id;
    let scene = observer_scene(&app);
    let id = |control: &str| format!("pane.{pane_id}.header.{control}");
    let view = scene_control(&scene, &id("side.tape"));
    assert_eq!(view["capability_id"], "orderflow.tape.view.set");
    assert_eq!(view["label"], "Tape");
    assert_eq!(view["owner"]["kind"], "tab");
    let l2 = scene_control(&scene, &id("layer.tape_heatmap"));
    assert_eq!(l2["capability_id"], "layers.visibility.set");
    assert_eq!(l2["label"], "Tape L2");
    assert!(
        !scene_control_ids(&scene).contains(&id("layer.heatmap")),
        "a blocked glyph is not there to name"
    );
    let strip = scene_control(&scene, &id("layer.live_strip"));
    assert_eq!(strip["availability"]["available"], true);
    let sample = scene_control(&scene, &id("sample.tape"));
    assert_eq!(sample["capability_id"], "orderflow.bubbles.size.set");
    assert!(sample["bounds"]["x_pt"].is_string());
    assert!(
        !scene_control_ids(&scene)
            .iter()
            .any(|id| id.starts_with("toolbar.layers")),
        "the toolbar has no LAYERS"
    );
}

/// On a narrow pane the words and the mark go; the switches and strip keep the right end,
/// nothing overlaps, and what clipped away the scene does not list.
#[test]
fn a_narrow_header_drops_the_mark_and_never_overlaps() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    for width in [1_400.0, 420.0, 340.0, 260.0] {
        for _ in 0..2 {
            run_frame_at(&mut app, &ctx, egui::vec2(width, 700.0));
        }
        let controls = app.active_tab().flow_pane.frame.header_controls.clone();
        let indicators = glyph_of(&app, &ctx, PaneSide::Flow);
        for (index, (control, rect)) in controls.iter().enumerate() {
            assert!(
                !rect.shrink(0.5).intersects(indicators),
                "{control:?} {rect:?} overlaps the indicators button at {width}"
            );
            for (other, other_rect) in controls.iter().skip(index + 1) {
                assert!(
                    !rect.shrink(0.5).intersects(*other_rect),
                    "{control:?} {rect:?} overlaps {other:?} {other_rect:?} at {width}"
                );
            }
        }
        assert!(
            controls.iter().any(|(control, _)| *control == TRAIL),
            "the switches stay at {width}"
        );
    }
}

/// The footprint's settings edit the focused chart, so a right click on the flow
/// pane's glyph makes that the focused chart, as a time pane's glyph does.
#[test]
fn a_right_click_on_the_flow_footprint_glyph_focuses_the_flow_pane() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut().focus = PaneSide::Time(0);
    run_frame(&mut app, &ctx);
    let at = flow_control(&app, HeaderControl::Layer(LayerToggle::Footprint)).center();
    let secondary = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
    run_frame_with_events(&mut app, &ctx, vec![secondary(true)]);
    run_frame_with_events(&mut app, &ctx, vec![secondary(false)]);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().focus, PaneSide::Flow);
}

/// Hiding the candles keeps their place, through every door out and back: the
/// frame reads the live edge from a copy and the stored view waits, unclamped.
#[test]
fn a_panned_view_survives_tape_only_and_the_way_back() {
    let mods = egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT);
    // Back into history, and forward into the projection margin a hidden
    // pane's zero-width window would clamp away.
    for pan in [400.0, -120.0] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = charted_fixture(&ctx);
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.model.viewport.pan_pixels(pan, pane.slots());
        run_frame(&mut app, &ctx);
        let placed = candle_viewport(&app);
        assert!(!app.active_tab().flow_pane.model.viewport.follows_live());

        press_key(&mut app, &ctx, egui::Key::T, mods);
        assert_eq!(view_of(&app), LaneView::TapeOnly);
        run_frame(&mut app, &ctx);
        let flow = &app.active_tab().flow_pane;
        assert!(flow.shown_viewport().follows_live(), "the frame reads live");
        assert!(!flow.model.viewport.follows_live(), "the view waits");
        assert_eq!(candle_viewport(&app), placed, "pan {pan}: hidden");
        press_key(&mut app, &ctx, egui::Key::T, mods);
        assert_eq!(view_of(&app), LaneView::Beside);
        assert_eq!(candle_viewport(&app), placed, "pan {pan}: the key");

        for _ in 0..2 {
            click_side(&mut app, &ctx, TRAIL);
        }
        assert_eq!(view_of(&app), LaneView::Beside);
        assert_eq!(candle_viewport(&app), placed, "pan {pan}: the switch");
    }
}

/// Fold a side of the lane as the divider does.
fn set_fold(app: &mut QuantickApp, fold: LaneFold) {
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| _ = config.live_lane.set_fold(fold));
}

fn flow_fold(app: &QuantickApp) -> LaneFold {
    app.active_tab().tape().cached_config().live_lane.fold
}

/// Candles folded to their rail: a fold is not a view, so both switches stay lit.
/// It moves no layer gate, so the glyphs and their keys stay live and both samples
/// stay. The stored view is what the switches set, and stored tape only is what
/// greys the glyphs.
#[test]
fn candles_folded_leave_the_switches_lit_and_the_gates_alone() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let heatmap = HeaderControl::Layer(LayerToggle::Heatmap);
    let toggles = [
        LayerToggle::Bubbles,
        LayerToggle::Heatmap,
        LayerToggle::Footprint,
    ];
    let gates = |app: &QuantickApp| {
        let caps = app.active_tab().capabilities(&app.config);
        let flow = &app.active_tab().flow_pane;
        toggles.map(|toggle| flow.layer_blocked(toggle.layer(), caps))
    };
    let pane_id = app.active_tab().flow_pane.id;
    let id = |control: &str| format!("pane.{pane_id}.header.{control}");
    let l2_gate = |app: &QuantickApp| {
        scene_control(&observer_scene(app), &id("layer.heatmap"))["availability"].clone()
    };
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.show_aggressions = true);
    run_frame(&mut app, &ctx);
    let open_gates = gates(&app);
    let open_l2 = l2_gate(&app);
    flow_control(&app, heatmap);

    set_fold(&mut app, LaneFold::Candles);
    run_frame(&mut app, &ctx);
    assert_eq!(view_of(&app), LaneView::Beside, "the stored view stays");
    assert!(
        control_rect(&app, PaneSide::Flow, heatmap).is_none(),
        "a folded side has no capsule"
    );
    flow_control(&app, HeaderControl::Sample(BubbleSample::Tape));
    assert_eq!(gates(&app), open_gates, "a fold moves no gate");
    let scene = observer_scene(&app);
    for side in ["side.trail", "side.tape"] {
        assert_eq!(scene_control(&scene, &id(side))["selected"], true, "{side}");
    }

    let before = on(&app, ChartLayer::Bubbles);
    press_key(&mut app, &ctx, egui::Key::B, egui::Modifiers::CTRL);
    assert_ne!(
        on(&app, ChartLayer::Bubbles),
        before,
        "its key still answers"
    );
    assert_eq!(open_l2["available"], true);

    click_side(&mut app, &ctx, TAPE);
    assert_eq!(
        view_of(&app),
        LaneView::CandlesOnly,
        "the stored view moves"
    );

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    run_frame(&mut app, &ctx);
    assert_eq!(
        gates(&app),
        [Some(quantick_layers::blocks::TAPE_ONLY_CANDLES); 3],
        "stored tape only greys them"
    );
}

/// The tape folded to its rail: its switch stays lit, its capsule folds away with
/// its side.
#[test]
fn a_tape_folded_keeps_its_switch_lit_and_folds_its_capsule() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.show_aggressions = true);
    run_frame(&mut app, &ctx);

    set_fold(&mut app, LaneFold::Tape);
    run_frame(&mut app, &ctx);
    assert_eq!(view_of(&app), LaneView::Beside, "the stored view stays");
    flow_control(&app, HeaderControl::Sample(BubbleSample::Candle));
    let tape = HeaderControl::Sample(BubbleSample::Tape);
    assert!(
        control_rect(&app, PaneSide::Flow, tape).is_none(),
        "no tape side"
    );

    let pane_id = app.active_tab().flow_pane.id;
    let scene = observer_scene(&app);
    let view = scene_control(&scene, &format!("pane.{pane_id}.header.side.tape"));
    assert_eq!(view["selected"], true);
}

/// Held by the market: the scene says why Tape refuses, under its own code and
/// its own word, Trail is the last side showing, the readback names the hold and
/// the saved fold stays open. A click on the rail is the hand that ends it.
#[test]
fn a_market_hold_reads_in_the_scene_and_never_in_the_saved_fold() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    run_frame(&mut app, &ctx);
    let quiet = quantick_chart::connection_state::ConnectionState {
        link: quantick_chart::connection_state::Link::Quiet,
        ..app.active_tab().connection.state()
    };
    let tape = app.active_tab_mut().tape_mut();
    tape.observe_market(quiet);
    assert_eq!(tape.cached_config().live_lane.fold, LaneFold::Open);
    let read = tape.bubbles_snapshot();
    assert!(read.tape_fold_held_by_market && read.fold_market_closed);
    assert_eq!(
        read.tape_fold,
        quantick_control_schema::tape_fold::TapeFold::Open
    );

    let pane_id = app.active_tab().flow_pane.id;
    let scene = observer_scene(&app);
    let tape = scene_control(&scene, &format!("pane.{pane_id}.header.side.tape"));
    assert_eq!(tape["label"], "Market closed");
    assert_eq!(tape["selected"], false);
    assert_eq!(tape["availability"]["available"], false);
    assert_eq!(tape["availability"]["reason"], "market_closed");
    let trail = scene_control(&scene, &format!("pane.{pane_id}.header.side.trail"));
    assert_eq!(trail["label"], "Trail");
    assert_eq!(
        trail["availability"]["reason"],
        "the_last_side_showing_stays"
    );

    let tape = app.active_tab_mut().tape_mut();
    assert!(tape.edit_config(|config| _ = config.live_lane.set_fold(LaneFold::Open)));
    assert!(!tape.cached_config().live_lane.market_held());
    tape.observe_market(quiet);
    assert!(
        !tape.cached_config().live_lane.market_held(),
        "a hand holds it off for the rest of the spell"
    );
}

/// Another market on a reused view starts fresh both ways: no hold from the last
/// one, and no hand's stand-down either, so its own closed spell folds.
#[test]
fn a_symbol_change_forgets_the_markets_hold_and_the_hands_stand_down() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    run_frame(&mut app, &ctx);
    let quiet = quantick_chart::connection_state::ConnectionState {
        link: quantick_chart::connection_state::Link::Quiet,
        ..app.active_tab().connection.state()
    };
    let tape = app.active_tab_mut().tape_mut();
    tape.observe_market(quiet);
    assert!(tape.cached_config().live_lane.market_held());
    tape.reset_for_symbol("NEXT");
    let hold = tape.cached_config().live_lane.market_hold;
    assert!(!hold.held && !hold.closed_spell, "no hold carried over");

    tape.observe_market(quiet);
    assert!(tape.edit_config(|config| _ = config.live_lane.set_fold(LaneFold::Open)));
    assert!(tape.cached_config().live_lane.market_hold.stood_down);
    tape.reset_for_symbol("AFTER");
    assert!(!tape.cached_config().live_lane.market_hold.stood_down);
    tape.observe_market(quiet);
    assert!(
        tape.cached_config().live_lane.market_held(),
        "the new market's own closed spell folds"
    );
}

/// A fold outlives every view, on the header too, and on the way back beside the
/// candles its rail is still the way out of it.
#[test]
fn a_fold_survives_every_view_and_its_rail_still_opens_it() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    set_fold(&mut app, LaneFold::Candles);
    run_frame(&mut app, &ctx);

    for (control, view) in [
        (TAPE, LaneView::CandlesOnly),
        (TAPE, LaneView::Beside),
        (TRAIL, LaneView::TapeOnly),
        (TRAIL, LaneView::Beside),
    ] {
        click_side(&mut app, &ctx, control);
        assert_eq!(view_of(&app), view);
        assert_eq!(flow_fold(&app), LaneFold::Candles, "{view:?}");
    }

    let flow = &app.active_tab().flow_pane;
    let chart = flow.frame.chart_rect.expect("the canvas laid out");
    let lane = &app.active_tab().tape().cached_config().live_lane;
    assert_eq!(
        lane.shown_view(),
        LaneView::TapeOnly,
        "folded again on the way back"
    );

    click_chart(
        &mut app,
        &ctx,
        egui::pos2(chart.left() + 4.0, chart.center().y),
    );
    run_frame(&mut app, &ctx);
    assert_eq!(flow_fold(&app), LaneFold::Open, "the rail opened it");
    assert_eq!(view_of(&app), LaneView::Beside);
}

/// An indicator pane shares the candles' time axis, but with the candles hidden
/// a drag or a scroll there has no candles to move: they come back following
/// live, at their zoom, rather than parked in the past.
#[test]
fn an_indicator_pane_gesture_in_tape_only_leaves_the_candles_following_live() {
    let add_a_pane = |app: &mut QuantickApp, ctx: &egui::Context| {
        add_pane_indicator(app, "CVD", vec![1.0; 200]);
        run_frame(app, ctx);
        run_frame(app, ctx);
    };
    // The same gesture beside the candles does move them, or this proves nothing.
    let beside = egui::Context::default();
    let (mut control, _control_ends) = charted_fixture(&beside);
    add_a_pane(&mut control, &beside);
    let body = pane_body(&control, 0);
    let start = egui::pos2(body.center().x, body.bottom() - 8.0);
    drag(
        &mut control,
        &beside,
        start,
        start + egui::vec2(-120.0, 0.0),
    );
    assert!(
        !control.active_tab().flow_pane.model.viewport.follows_live(),
        "the fixture's drag lands on the indicator pane"
    );

    let ctx = egui::Context::default();
    let (mut app, _ends) = charted_fixture(&ctx);
    add_a_pane(&mut app, &ctx);
    assert!(app.active_tab().flow_pane.model.viewport.follows_live());
    let placed = candle_viewport(&app);
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    run_frame(&mut app, &ctx);
    let body = pane_body(&app, 0);
    let start = egui::pos2(body.center().x, body.bottom() - 8.0);
    drag(&mut app, &ctx, start, start + egui::vec2(-120.0, 0.0));
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 240.0),
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
    assert_eq!(candle_viewport(&app), placed, "tape only moved nothing");

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::Beside);
    run_frame(&mut app, &ctx);
    assert!(app.active_tab().flow_pane.model.viewport.follows_live());
    assert_eq!(candle_viewport(&app), placed, "and back beside");
}

/// A snapshot describes the view on screen: with panned candles hidden it says
/// following live, and the slots it names are the live edge's, not the place
/// the stored view is waiting at.
#[test]
fn a_snapshot_in_tape_only_names_the_slots_on_screen() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = charted_fixture(&ctx);
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.model.viewport.pan_pixels(400.0, pane.slots());
    run_frame(&mut app, &ctx);
    let total = app.active_tab().flow_pane.slots();
    let panned = crate::control::chart::viewport_snapshot(&app.active_tab().flow_pane);
    assert!(panned.visible_end_slot_exclusive.0 < total as u64, "panned");

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    run_frame(&mut app, &ctx);
    let flow = &app.active_tab().flow_pane;
    assert!(!flow.model.viewport.follows_live(), "the stored view waits");
    let snapshot = crate::control::chart::viewport_snapshot(flow);
    assert!(snapshot.follows_live);
    assert_eq!(snapshot.visible_end_slot_exclusive.0, total as u64);
}

/// A control the strip clips, even by a pixel, is not listed: what the scene
/// names is whole, at every width a narrowing window walks through.
#[test]
fn the_header_lists_only_controls_wholly_inside_the_strip() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    for width in (160..=460).step_by(3) {
        let size = egui::vec2(width as f32, 700.0);
        for _ in 0..2 {
            run_frame_at(&mut app, &ctx, size);
        }
        let flow = &app.active_tab().flow_pane;
        let Some(chart) = flow.frame.area else {
            continue;
        };
        for (control, rect) in &flow.frame.header_controls {
            assert!(
                rect.right() <= chart.right(),
                "{control:?} {rect:?} runs past the strip's end {} at {width}",
                chart.right()
            );
        }
    }
}

/// A held tape key acts once: its repeats are swallowed, never strobe the views
/// and never reach `Ctrl+T`.
#[test]
fn a_held_tape_key_acts_once_and_never_opens_a_tab() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let mods = egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT);
    let tabs = app.runtime.tabs.len();
    let press = |repeat| egui::Event::Key {
        key: egui::Key::T,
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: mods,
    };
    run_frame_with_modifiers(&mut app, &ctx, vec![press(false)], mods);
    assert_eq!(view_of(&app), LaneView::TapeOnly);
    for _ in 0..3 {
        run_frame_with_modifiers(&mut app, &ctx, vec![press(true)], mods);
        assert_eq!(view_of(&app), LaneView::TapeOnly, "a repeat moved the view");
    }
    assert_eq!(app.runtime.tabs.len(), tabs, "a repeat opened a tab");
    assert!(!app.surfaces.source_picker.is_open());
}

fn glyph_of(app: &QuantickApp, ctx: &egui::Context, side: PaneSide) -> egui::Rect {
    let id = crate::pane_header::indicators_id(app.active_tab().pane(side).id);
    ctx.data(|data| data.get_temp(id)).expect("the header drew")
}

/// `ƒx` opens the library on its own pane, and what the library then adds lands
/// on that pane, whichever pane had focus. Panes sharing a layout mirror the
/// addition, so the count is read on the target alone.
#[test]
fn the_indicators_glyph_opens_the_library_on_its_own_pane() {
    for (side, focus) in [
        (PaneSide::Flow, PaneSide::Time(0)),
        (PaneSide::Time(0), PaneSide::Flow),
    ] {
        let ctx = egui::Context::default();
        let (mut app, _events, _commands) = context_fixture(&ctx);
        app.active_tab_mut().focus = focus;
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        let before = app.active_tab().pane(side).indicators.all().len();

        let glyph = glyph_of(&app, &ctx, side);
        click_chart(&mut app, &ctx, glyph.center());
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        let pane = app.active_tab().pane(side).id;
        assert_eq!(app.active_tab().focused_side(), side);
        assert_eq!(
            app.surfaces.indicator_picker.state.target,
            Some((app.runtime.tabs.active_id(), pane)),
            "the library stays open on the pane that asked"
        );
        app.apply_toolbar_action(crate::toolbar::ToolbarAction::AddNative("native.ema"));
        settle_indicators(&mut app);
        assert_eq!(
            app.active_tab().pane(side).indicators.all().len(),
            before + 1
        );
    }
}

/// Bars and indicators are per pane now: the row above the charts names neither,
/// and each pane carries one glyph.
#[test]
fn the_toolbar_leaves_bars_and_indicators_to_the_panes() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    run_frame(&mut app, &ctx);
    let texts = painted_text(&run_frame(&mut app, &ctx));
    let buttons = texts.iter().filter(|text| *text == "ƒxIndicators").count();
    assert_eq!(buttons, 3, "one per pane in {texts:?}");
    assert!(
        !texts.iter().any(|text| text == "bars"),
        "bars is still painted in {texts:?}"
    );
}

/// However narrow the pane, the glyph is on it, off the bars control, and it
/// answers a click.
#[test]
fn a_narrow_pane_keeps_its_indicators_glyph_in_reach() {
    let ctx = egui::Context::default();
    let strip = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(90.0, 24.0));
    let header = quantick_chart::pane_header::PaneHeader::of(
        quantick_chart::pane_header::PaneIdentity::OrderFlow,
        BarSpec::Tick(2000).into(),
    );
    let mut selection = crate::state::SpecSelector::default();
    let mut painted = crate::pane_header::PaintedControls::new();
    let mut frame = |events: Vec<egui::Event>| {
        let mut clicked = false;
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let row = crate::pane_header::FlowRow {
                        strip: None,
                        state: None,
                        chip: None,
                        slot: None,
                        rebuilding: false,
                        settings_open: false,
                    };
                    (_, clicked) = crate::pane_header::draw_marked(
                        ui,
                        strip,
                        &header,
                        7,
                        &mut selection,
                        BarInputAvailability::PRINTS,
                        row,
                        &mut painted,
                    );
                });
            },
        );
        clicked
    };
    frame(Vec::new());
    let glyph: egui::Rect = ctx
        .data(|data| data.get_temp(crate::pane_header::indicators_id(7)))
        .expect("the header drew");
    assert!(strip.contains_rect(glyph), "{glyph:?} left {strip:?}");
    let press = |pressed| egui::Event::PointerButton {
        pos: glyph.center(),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(vec![egui::Event::PointerMoved(glyph.center())]);
    frame(vec![press(true)]);
    assert!(frame(vec![press(false)]));
}

/// A script reaches what the click reaches, and the scene names the calls.
#[test]
fn the_scene_names_each_panes_bars_and_indicators_calls() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    run_frame(&mut app, &ctx);
    let scene = observer_scene(&app);
    let controls = scene["controls"].as_array().unwrap();
    for (pane, _) in app.active_tab().panes() {
        for (control_id, capability) in [
            (
                format!("pane_header.{}.bars", pane.id),
                "layout.pane.set_bar_spec",
            ),
            (
                format!("pane_header.{}.indicators", pane.id),
                "indicator.picker.set",
            ),
        ] {
            let control = controls
                .iter()
                .find(|control| control["control_id"] == control_id.as_str())
                .unwrap_or_else(|| panic!("{control_id} is missing"));
            assert_eq!(control["capability_id"], capability);
        }
    }
}

/// The library is modal, and a click off it must hand the app back. It used to
/// raise the scrim over the window, leaving nothing the pointer could reach.
#[test]
fn a_click_outside_the_library_hands_the_app_back() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let glyph = glyph_of(&app, &ctx, PaneSide::Flow);
    click_chart(&mut app, &ctx, glyph.center());
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(app.surfaces.indicator_picker.state.target.is_some());

    click_chart(&mut app, &ctx, egui::pos2(30.0, 400.0));
    run_frame(&mut app, &ctx);
    assert!(app.surfaces.indicator_picker.state.target.is_none());

    click_chart(&mut app, &ctx, glyph.center());
    run_frame(&mut app, &ctx);
    assert!(
        app.surfaces.indicator_picker.state.target.is_some(),
        "the header answers again once the library is gone"
    );
}

const GEAR: HeaderControl = HeaderControl::Settings(PanelControl::Gear);
const PANEL_CLOSE: HeaderControl = HeaderControl::Settings(PanelControl::Close);

fn section_control(section: PanelSection) -> HeaderControl {
    HeaderControl::Settings(PanelControl::Section(section))
}

fn panel_open(app: &QuantickApp) -> bool {
    app.active_tab().tape().settings_panel.is_open()
}

fn scene_selected(app: &QuantickApp, control: HeaderControl) -> Option<bool> {
    let id = format!(
        "pane.{}.header.{}",
        app.active_tab().flow_pane.id,
        control.id_in_header()
    );
    crate::control::scene_snapshot(app)
        .controls
        .iter()
        .find(|entry| entry.control_id == id)
        .map(|entry| entry.selected)
}

fn settings_call(
    app: &mut QuantickApp,
    capability: &str,
    fields: serde_json::Value,
) -> Result<serde_json::Value, quantick_control::error::ControlError> {
    let mut input = json!({ "tab_id": app.runtime.tabs.active_id().to_string() });
    input
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    app.control_action(capability, 1, crate::control::ActionOrigin::Human, input)
}

/// The gear opens the panel beside the price axis and the chart pays for it, with
/// the header line untouched; a second click and the × give the width back.
#[test]
fn the_gear_opens_a_panel_the_chart_shrinks_for_and_closes_it() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let wide = app.active_tab().flow_pane.frame.area.expect("flow chart");
    let gear = flow_control(&app, GEAR);
    assert_eq!(scene_selected(&app, GEAR), Some(false));

    click_chart(&mut app, &ctx, gear.center());
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(panel_open(&app));
    let narrow = app.active_tab().flow_pane.frame.area.expect("flow chart");
    let width = wide.width() - narrow.width();
    assert!(
        (PANEL_MIN_WIDTH_PX..=PANEL_MAX_WIDTH_PX).contains(&width),
        "the chart gave up the panel's width, not {width}"
    );
    assert_eq!(narrow.left(), wide.left());
    assert_eq!(
        narrow.y_range(),
        wide.y_range(),
        "the panel sits below the header"
    );
    assert_eq!(flow_control(&app, GEAR), gear, "the gear stays on its line");
    assert_eq!(scene_selected(&app, GEAR), Some(true));
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text.starts_with("ORDER FLOW · ")),
        "{texts:?}"
    );

    click_chart(&mut app, &ctx, gear.center());
    run_frame(&mut app, &ctx);
    assert!(!panel_open(&app));
    assert_eq!(app.active_tab().flow_pane.frame.area, Some(wide));
    assert_eq!(
        scene_selected(&app, section_control(PanelSection::L2)),
        None
    );

    click_chart(&mut app, &ctx, gear.center());
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let close = flow_control(&app, PANEL_CLOSE);
    click_chart(&mut app, &ctx, close.center());
    run_frame(&mut app, &ctx);
    assert!(!panel_open(&app), "the × closes it");
}

/// Ctrl+Shift+O is the gear's left click.
#[test]
fn the_panel_key_opens_and_closes_it() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let keys = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
    press_key(&mut app, &ctx, egui::Key::O, keys);
    assert!(panel_open(&app));
    press_key(&mut app, &ctx, egui::Key::O, keys);
    assert!(!panel_open(&app));
}

/// A right click on the gear offers the two sections; its entries ask what a
/// capsule's glyph asks.
#[test]
fn the_gears_right_click_offers_the_two_sections() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let at = flow_control(&app, GEAR).center();
    let secondary = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
    run_frame_with_events(&mut app, &ctx, vec![secondary(true)]);
    run_frame_with_events(&mut app, &ctx, vec![secondary(false)]);
    let texts = painted_text(&run_frame(&mut app, &ctx));
    for section in PanelSection::ALL {
        assert!(
            texts.iter().any(|text| text == section.title()),
            "{texts:?}"
        );
    }
    assert!(!panel_open(&app), "the menu opens nothing by itself");
}

/// The panel's whole state by name, read back in the scene; a click on a header
/// folds the same state.
#[test]
fn the_panel_is_operated_by_name_and_read_back_in_the_scene() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let l2 = section_control(PanelSection::L2);
    let bubbles = section_control(PanelSection::Bubbles);
    let on_l2 = json!({ "section": "l2" });

    let open = settings_call(&mut app, OPEN_CAPABILITY_ID, on_l2.clone()).unwrap();
    assert_eq!(open["open"], true);
    assert_eq!(open["changed"], true);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(scene_selected(&app, GEAR), Some(true));
    assert_eq!(scene_selected(&app, l2), Some(true));
    let repeat = settings_call(&mut app, OPEN_CAPABILITY_ID, on_l2).unwrap();
    assert_eq!(repeat["changed"], false);

    let fold = json!({ "section": "l2", "open": false });
    let folded = settings_call(&mut app, SECTION_CAPABILITY_ID, fold).unwrap();
    assert_eq!(folded["l2_open"], false);
    run_frame(&mut app, &ctx);
    assert_eq!(scene_selected(&app, l2), Some(false));

    run_frame(&mut app, &ctx);
    assert_eq!(
        scene_selected(&app, bubbles),
        Some(true),
        "with L2 folded the next header rises into view"
    );
    let header = flow_control(&app, l2);
    click_chart(&mut app, &ctx, header.left_center() + egui::vec2(20.0, 0.0));
    run_frame(&mut app, &ctx);
    assert_eq!(
        scene_selected(&app, l2),
        Some(true),
        "the click folds the same state"
    );

    let bad = settings_call(
        &mut app,
        OPEN_CAPABILITY_ID,
        json!({ "section": "session" }),
    );
    assert!(bad.is_err(), "only l2 or bubbles");

    let shut = settings_call(&mut app, CLOSE_CAPABILITY_ID, json!({})).unwrap();
    assert_eq!(shut["open"], false);
    run_frame(&mut app, &ctx);
    assert_eq!(scene_selected(&app, GEAR), Some(false));
}

/// A fold is the asset's, on every tab showing it, and nothing the next asset
/// reads; whether the panel is open is the tab's.
#[test]
fn a_fold_follows_the_asset_and_open_follows_the_tab() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let symbol = app.active_tab().symbol.clone();
    settings_call(&mut app, OPEN_CAPABILITY_ID, json!({})).unwrap();
    let fold = json!({ "section": "bubbles", "open": false });
    settings_call(&mut app, SECTION_CAPABILITY_ID, fold).unwrap();
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), symbol.clone(), None);
    assert!(!panel_open(&app), "a new tab has its own panel");
    let same = settings_call(&mut app, OPEN_CAPABILITY_ID, json!({})).unwrap();
    assert_eq!(same["bubbles_open"], false, "the same asset reads the same");
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "OTHERUSDT".to_owned(), None);
    let other = settings_call(&mut app, OPEN_CAPABILITY_ID, json!({})).unwrap();
    assert_eq!(other["bubbles_open"], true, "another asset is untouched");
}

/// A tab not showing its flow pane has no gear, and a call says so.
#[test]
fn a_tab_without_its_flow_pane_has_no_gear_and_refuses_the_call() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut().set_flow_collapsed(true);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(scene_selected(&app, GEAR), None);
    assert!(settings_call(&mut app, OPEN_CAPABILITY_ID, json!({})).is_err());
    let keys = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
    press_key(&mut app, &ctx, egui::Key::O, keys);
    assert!(!panel_open(&app), "nor does the key open what cannot show");
}

/// An old workspace that left the L2 dock tab open opens the panel on L2, on the
/// tab it was active on, and leaves the dock collapsed; saving says so the new way.
#[test]
fn an_old_l2_or_bubbles_dock_tab_restores_to_the_panel() {
    for (old, section) in [
        (ui_state::SavedDockTab::L2, PanelSection::L2),
        (ui_state::SavedDockTab::Bubbles, PanelSection::Bubbles),
    ] {
        let ctx = egui::Context::default();
        let (mut app, _ends) = flow_fixture(&ctx);
        let (mut tabs, mut chrome) = app.arrangement_state().capture_arrangement();
        chrome.dock_tab = Some(old);
        let symbol = tabs[0].symbol.clone();
        chrome.orderflow_folds.set(&symbol, section, false);
        tabs[0].orderflow_panel = Default::default();
        app.arrangement_adapter()
            .restore_workspace(ui_state::Workspace::new(true, None, 0, tabs, Some(chrome)));
        assert!(panel_open(&app), "{old:?}");
        assert_eq!(app.dock.tab(), None, "{old:?} is no dock tab any more");
        // The scroll to the section is animated; a header out of view is not listed.
        for _ in 0..60 {
            run_frame(&mut app, &ctx);
        }
        assert_eq!(
            scene_selected(&app, section_control(section)),
            Some(true),
            "{old:?} opens expanded on its section"
        );
        let (saved, chrome) = app.arrangement_state().capture_arrangement();
        assert_eq!(chrome.dock_tab, None);
        assert!(saved[0].orderflow_panel.is_open(), "written the new way");
    }
}

/// A panel restored from an old workspace, on Bubbles.
fn restored_on_bubbles(
    ctx: &egui::Context,
) -> (QuantickApp, impl Sized, mpsc::Receiver<FeedCommand>) {
    let (mut app, events, commands) = context_fixture(ctx);
    set_flow_bars(&mut app, BarSpec::Tick(2000));
    app.active_tab_mut().focus = PaneSide::Flow;
    for _ in 0..2 {
        run_frame(&mut app, ctx);
    }
    let (mut tabs, mut chrome) = app.arrangement_state().capture_arrangement();
    chrome.dock_tab = Some(ui_state::SavedDockTab::Bubbles);
    tabs[0].orderflow_panel = Default::default();
    app.arrangement_adapter()
        .restore_workspace(ui_state::Workspace::new(true, None, 0, tabs, Some(chrome)));
    (app, events, commands)
}

fn run_frame_every(app: &mut QuantickApp, ctx: &egui::Context, dt: f32) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
        predicted_dt: dt,
        ..Default::default()
    };
    let now = frame_instant(ctx);
    let _ = ctx.run(input, |ctx| app.draw_frame(ctx, now));
}

const LANDED_Y: f32 = 138.0;

fn bubbles_header_y(app: &QuantickApp) -> f32 {
    flow_control(app, section_control(PanelSection::Bubbles)).top()
}

/// Frames are not a duration: at any rate a restored panel ends on top.
#[test]
fn a_restored_section_lands_on_top_at_any_frame_rate() {
    for dt in [1.0 / 60.0, 1.0 / 240.0, 1.0 / 1000.0] {
        let ctx = egui::Context::default();
        let (mut app, _events, _commands) = restored_on_bubbles(&ctx);
        for _ in 0..600 {
            run_frame_every(&mut app, &ctx, dt);
        }
        let y = bubbles_header_y(&app);
        assert!((y - LANDED_Y).abs() < 1.0, "{dt}: {y}");
    }
}

/// On a launch the book's first ladder comes after the landing and adds rows to the
/// L2 body above: the section stays on top.
#[test]
fn a_restored_section_stays_on_top_when_the_book_arrives_above_it() {
    use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};
    let ctx = egui::Context::default();
    let (mut app, _events, mut commands) = restored_on_bubbles(&ctx);
    for _ in 0..30 {
        run_frame(&mut app, &ctx);
    }
    assert!((bubbles_header_y(&app) - LANDED_Y).abs() < 1.0);
    let mut generation = None;
    while let Ok(command) = commands.try_recv() {
        if let FeedCommand::SetBookCapture {
            enabled: true,
            initial_generation,
            ..
        } = command
        {
            generation = Some(initial_generation);
        }
    }
    let symbol = app.active_tab().symbol.clone();
    let view = app.active_tab_mut().tape_mut();
    view.handle_depth_event(DepthEvent::Snapshot {
        symbol,
        generation: generation.expect("capture follows the feed"),
        observed_at_ms: 1_100,
        effective_at_ms: 999,
        price_step: None,
        snapshot: BookSnapshot::new(
            10,
            vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
            vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
            BookCoverage::Limited {
                levels_per_side: 1_000,
            },
        ),
    });
    view.flush_for_test();
    let mut lowest = LANDED_Y;
    for _ in 0..60 {
        run_frame(&mut app, &ctx);
        lowest = lowest.max(bubbles_header_y(&app));
    }
    assert!(lowest > LANDED_Y + 10.0, "the rows pushed it: {lowest}");
    let y = bubbles_header_y(&app);
    assert!((y - LANDED_Y).abs() < 1.0, "back on top: {y}");
}

/// A hand's wheel ends the landing: it does not fight the trader for the panel.
#[test]
fn a_wheel_over_the_panel_ends_a_landing() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = restored_on_bubbles(&ctx);
    for _ in 0..30 {
        run_frame(&mut app, &ctx);
    }
    let over = flow_control(&app, section_control(PanelSection::Bubbles)).center();
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(over)]);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
        }],
    );
    for _ in 0..30 {
        run_frame(&mut app, &ctx);
    }
    let y = bubbles_header_y(&app);
    assert!(y > LANDED_Y + 50.0, "the hand's scroll stands: {y}");
}

/// The panel reads back with nothing painted: the call that closes it, and a fold
/// made while it is closed, on a tab whose flow pane is folded away.
#[test]
fn the_settings_scope_reads_the_panel_without_a_paint() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let read = |app: &QuantickApp| {
        let scope = observer_scope("orderflow.settings");
        let capture = crate::control::standard_registry()
            .unwrap()
            .capture(app, &observer_instance(), std::slice::from_ref(&scope))
            .unwrap()
            .into_serialized()
            .unwrap();
        let row = &capture.scopes[&scope].value["tabs"][0];
        (
            row["open"] == true,
            row["l2_open"] == true,
            row["bubbles_open"] == true,
        )
    };
    assert_eq!(read(&app), (false, true, true));
    settings_call(&mut app, OPEN_CAPABILITY_ID, json!({})).unwrap();
    assert_eq!(read(&app), (true, true, true));
    settings_call(&mut app, CLOSE_CAPABILITY_ID, json!({})).unwrap();
    let fold = json!({ "section": "bubbles", "open": false });
    settings_call(&mut app, SECTION_CAPABILITY_ID, fold).unwrap();
    assert_eq!(read(&app), (false, true, false));
    app.active_tab_mut().set_flow_collapsed(true);
    run_frame(&mut app, &ctx);
    assert_eq!(read(&app), (false, true, false), "nothing of it painted");
}

/// An old file's L2 tab behind a hidden dock was not on screen, and a tab whose flow
/// pane is folded has nothing to attach a panel to: neither opens it.
#[test]
fn an_old_dock_tab_that_was_not_showing_opens_no_panel() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let (mut tabs, mut chrome) = app.arrangement_state().capture_arrangement();
    chrome.dock_tab = Some(ui_state::SavedDockTab::L2);
    chrome.dock_visible = false;
    tabs[0].orderflow_panel = Default::default();
    let restore =
        |app: &mut QuantickApp, tabs: &[ui_state::SavedTab], chrome: &ui_state::SavedChrome| {
            let file = ui_state::Workspace::new(true, None, 0, tabs.to_vec(), Some(chrome.clone()));
            app.arrangement_adapter().restore_workspace(file);
        };
    restore(&mut app, &tabs, &chrome);
    assert!(!panel_open(&app), "a hidden dock showed no tab");

    chrome.dock_visible = true;
    tabs[0].flow_collapsed = true;
    restore(&mut app, &tabs, &chrome);
    assert!(!panel_open(&app), "a folded flow pane has no panel");

    tabs[0].flow_collapsed = false;
    restore(&mut app, &tabs, &chrome);
    assert!(
        panel_open(&app),
        "on screen, on a tab showing flow, it opens"
    );
}

/// Open and folded survive a save and a restore.
#[test]
fn the_panel_survives_a_save_and_a_restore() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    settings_call(&mut app, OPEN_CAPABILITY_ID, json!({})).unwrap();
    let fold = |open| json!({ "section": "l2", "open": open });
    settings_call(&mut app, SECTION_CAPABILITY_ID, fold(false)).unwrap();
    let (tabs, chrome) = app.arrangement_state().capture_arrangement();
    let symbol = tabs[0].symbol.clone();
    assert!(
        !chrome
            .orderflow_folds
            .section_open(&symbol, PanelSection::L2)
    );

    settings_call(&mut app, CLOSE_CAPABILITY_ID, json!({})).unwrap();
    settings_call(&mut app, SECTION_CAPABILITY_ID, fold(true)).unwrap();
    app.arrangement_adapter()
        .restore_workspace(ui_state::Workspace::new(true, None, 0, tabs, Some(chrome)));
    assert!(panel_open(&app));
    let folds = app.runtime.tabs.panel_folds();
    assert!(!folds.section_open(&symbol, PanelSection::L2));
}

/// The rectangles of every painted text saying exactly this.
fn painted_text_rects(output: &egui::FullOutput, wanted: &str) -> Vec<egui::Rect> {
    fn walk(shape: &egui::Shape, wanted: &str, found: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => {
                found.push(text.visual_bounding_rect());
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, wanted, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, wanted, &mut found);
    }
    found
}

/// A section's state stands at the panel's right, level with the title line's ×,
/// and clear of its own title, for both headers.
#[test]
fn a_sections_state_stands_right_and_never_over_its_title() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    settings_call(&mut app, OPEN_CAPABILITY_ID, json!({})).unwrap();
    let fold = json!({ "section": "l2", "open": false });
    settings_call(&mut app, SECTION_CAPABILITY_ID, fold).unwrap();
    for _ in 0..30 {
        run_frame(&mut app, &ctx);
    }
    let close = flow_control(&app, PANEL_CLOSE);
    let output = run_frame(&mut app, &ctx);
    for section in PanelSection::ALL {
        let header = flow_control(&app, section_control(section));
        let words = app.active_tab().tape().panel_section_state(section);
        let state = painted_text_rects(&output, &words)
            .into_iter()
            .find(|rect| rect.y_range().contains(header.center().y))
            .expect("its state is painted on its header's line");
        assert!(state.left() >= header.right(), "{section:?}: {state:?}");
        assert!(
            (close.right() - state.right()).abs() < 20.0,
            "{section:?} stands at the content's right: {state:?}, × at {close:?}"
        );
        assert!(
            (state.center().y - header.center().y).abs() < 2.0,
            "{section:?}"
        );
    }
}

/// Tape alone, the Buy/Sell key stands below the side's plate, never under it.
#[test]
fn the_tape_key_stands_below_the_sides_plate() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = charted_fixture(&ctx);
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    for _ in 0..40 {
        run_frame(&mut app, &ctx);
    }
    let flow = &app.active_tab().flow_pane;
    let area = flow.frame.area.expect("flow chart");
    let key = flow.frame.flow_legend.expect("the tape's key");
    let plate = area.top() + quantick_chart::side_label::SIDE_LABEL_BOTTOM_PX;
    assert!(
        key.top() >= plate,
        "{key:?} under the plate ending at {plate}"
    );
}

/// The panel open, the scene names both sections whether their headers are in view
/// or scrolled out; one out of view has no rectangle and says why, and its call works.
#[test]
fn the_scene_lists_both_sections_while_the_panel_is_open() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let (l2, bubbles) = (
        section_control(PanelSection::L2),
        section_control(PanelSection::Bubbles),
    );
    assert_eq!(
        scene_selected(&app, l2),
        None,
        "a closed panel lists neither"
    );
    settings_call(
        &mut app,
        OPEN_CAPABILITY_ID,
        json!({ "section": "bubbles" }),
    )
    .unwrap();
    for _ in 0..30 {
        run_frame(&mut app, &ctx);
    }
    let pane_id = app.active_tab().flow_pane.id;
    let scene = observer_scene(&app);
    let read = |control: HeaderControl| {
        scene_control(
            &scene,
            &format!("pane.{pane_id}.header.{}", control.id_in_header()),
        )
        .clone()
    };
    let (above, landed) = (read(l2), read(bubbles));
    assert_eq!(landed["bounds_availability"]["available"], true);
    assert_eq!(above["availability"]["available"], true, "its call works");
    assert_eq!(above["bounds_availability"]["available"], false);
    assert_eq!(
        above["bounds_availability"]["reason"],
        "the_control_is_off_screen_or_its_rectangle_unreportable"
    );
    assert_eq!(above["selected"], true, "open, wherever it has scrolled to");

    let size = egui::vec2(300.0, 700.0);
    for _ in 0..3 {
        run_frame_at(&mut app, &ctx, size);
    }
    for section in [l2, bubbles] {
        assert!(
            scene_selected(&app, section).is_some(),
            "{section:?}: open with no room beside the chart is still named"
        );
    }
}

/// egui's scroll target keeps one item spacing above what it brings to the top.
const LANDED_WITHIN_PX: f32 = 6.0;

/// A section asked for lands its header at the top of the scroll, the last one too,
/// on a panel opening for the first time and on one already open.
#[test]
fn a_section_asked_for_lands_at_the_top_of_the_scroll() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    settings_call(&mut app, OPEN_CAPABILITY_ID, json!({})).unwrap();
    for _ in 0..30 {
        run_frame(&mut app, &ctx);
    }
    let top = flow_control(&app, section_control(PanelSection::L2)).top();
    for section in ["bubbles", "l2", "bubbles"] {
        settings_call(&mut app, OPEN_CAPABILITY_ID, json!({ "section": section })).unwrap();
        for _ in 0..60 {
            run_frame(&mut app, &ctx);
        }
        let section = if section == "l2" {
            PanelSection::L2
        } else {
            PanelSection::Bubbles
        };
        let landed = flow_control(&app, section_control(section)).top();
        assert!(
            (landed - top).abs() < LANDED_WITHIN_PX,
            "{section:?}: {landed}, top {top}"
        );
    }

    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    settings_call(
        &mut app,
        OPEN_CAPABILITY_ID,
        json!({ "section": "bubbles" }),
    )
    .unwrap();
    for _ in 0..30 {
        run_frame(&mut app, &ctx);
    }
    let landed = flow_control(&app, section_control(PanelSection::Bubbles)).top();
    assert!(
        (landed - top).abs() < LANDED_WITHIN_PX,
        "from closed: {landed}, top {top}"
    );
}

/// Each entry of the gear's right-click menu opens the panel on its own section,
/// unfolded, and switches no layer.
#[test]
fn each_gear_menu_entry_opens_its_section() {
    for section in PanelSection::ALL {
        let ctx = egui::Context::default();
        let (mut app, _ends) = flow_fixture(&ctx);
        let fold = json!({ "section": section.id(), "open": false });
        settings_call(&mut app, SECTION_CAPABILITY_ID, fold).unwrap();
        run_frame(&mut app, &ctx);
        let layers = (on(&app, ChartLayer::Bubbles), on(&app, ChartLayer::Heatmap));
        let at = flow_control(&app, GEAR).center();
        let secondary = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
        run_frame_with_events(&mut app, &ctx, vec![secondary(true)]);
        run_frame_with_events(&mut app, &ctx, vec![secondary(false)]);
        let output = run_frame(&mut app, &ctx);
        let entry = painted_text_center(&output, section.title()).expect("the menu's entry");
        click_chart(&mut app, &ctx, entry);
        for _ in 0..30 {
            run_frame(&mut app, &ctx);
        }
        assert!(panel_open(&app), "{section:?}");
        let symbol = app.active_tab().symbol.clone();
        assert!(
            app.runtime
                .tabs
                .panel_folds()
                .section_open(&symbol, section),
            "{section:?} comes unfolded"
        );
        assert_eq!(
            (on(&app, ChartLayer::Bubbles), on(&app, ChartLayer::Heatmap)),
            layers,
            "looking is not enabling"
        );
    }
}

/// Held by the market, both segments and both keys do nothing: the view stands and
/// the hold stands, and a hand on the fold is the way out.
#[test]
fn a_market_hold_refuses_both_segments_and_both_keys() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let quiet = quantick_chart::connection_state::ConnectionState {
        link: quantick_chart::connection_state::Link::Quiet,
        ..app.active_tab().connection.state()
    };
    app.active_tab_mut().tape_mut().observe_market(quiet);
    let held = |app: &QuantickApp| {
        app.active_tab()
            .tape()
            .cached_config()
            .live_lane
            .market_held()
    };
    assert!(held(&app));
    run_frame(&mut app, &ctx);
    for control in [TAPE, TRAIL] {
        click_side(&mut app, &ctx, control);
        assert_eq!(view_of(&app), LaneView::Beside, "{control:?}");
        assert!(held(&app), "{control:?}");
    }
    let mods = egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT);
    for key in [egui::Key::T, egui::Key::R] {
        press_key(&mut app, &ctx, key, mods);
        assert_eq!(view_of(&app), LaneView::Beside, "{key:?}");
        assert!(held(&app), "{key:?}");
    }
}
