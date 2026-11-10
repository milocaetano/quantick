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
        quantick_chart::flow_band::SIDE_HEADER_HEIGHT_PX / 2.0,
        "the control centres in its side's header"
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

/// The toolbar has no LAYERS: a flow pane's band keeps its side switches, each
/// side header docks that side's capsule, the strip ending the trail's against the
/// divider, and a time pane's capsule docks at its own header's right end.
#[test]
fn each_pane_paints_only_the_controls_it_accepts() {
    let ctx = egui::Context::default();
    let (app, _ends) = flow_fixture(&ctx);
    let flow = &app.active_tab().flow_pane;
    let area = flow.frame.area.expect("flow chart");
    let divider = flow.frame.lane_divider_x.expect("the tape is beside");
    for control in [TRAIL, TAPE] {
        let rect = flow_control(&app, control);
        assert!(
            rect.bottom() <= area.top(),
            "{control:?} sits off the chart"
        );
        assert!(rect.right() < area.center().x, "{control:?} stands left");
    }
    let strip = flow_control(&app, HeaderControl::Layer(LayerToggle::LiveStrip));
    let footprint = flow_control(&app, HeaderControl::Layer(LayerToggle::Footprint));
    assert!(
        strip.left() > footprint.right(),
        "the strip ends the trail's capsule"
    );
    for (control, right_side) in [
        (HeaderControl::Layer(LayerToggle::Bubbles), false),
        (HeaderControl::Sample(BubbleSample::Candle), false),
        (HeaderControl::Layer(LayerToggle::Heatmap), false),
        (HeaderControl::Layer(LayerToggle::Footprint), false),
        (HeaderControl::Layer(LayerToggle::LiveStrip), false),
        (HeaderControl::Layer(LayerToggle::TapeBubbles), true),
        (HeaderControl::Sample(BubbleSample::Tape), true),
        (HeaderControl::Layer(LayerToggle::TapeHeatmap), true),
    ] {
        let rect = flow_control(&app, control);
        assert!(rect.bottom() <= area.top(), "{control:?} in the header");
        assert_eq!(
            rect.left() >= divider,
            right_side,
            "{control:?} on its side"
        );
        let edge = if right_side { area.right() } else { divider };
        assert!(rect.right() <= edge - 8.0, "{control:?} inside its end");
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
    let header = chart.top() - crate::time_header::HEIGHT_PX;
    assert!(
        footprint.top() >= header && footprint.bottom() <= chart.top(),
        "the capsule docks in the header: {footprint:?} over {chart:?}"
    );
    assert!(chart.right() - footprint.right() < 20.0, "at its right end");
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
    // What each side shows, in the words of its tooltip.
    use quantick_chart::pane_header::{TAPE_DESCRIPTION, TRAIL_DESCRIPTION};
    assert_eq!(side(&app, "trail")["description"], TRAIL_DESCRIPTION);
    assert_eq!(side(&app, "tape")["description"], TAPE_DESCRIPTION);

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
        run_frame_at(&mut app, &ctx, egui::vec2(240.0, 700.0));
    }
    let bare = flow_control(&app, TRAIL).width();
    assert!(bare < wide, "{bare} at 240, {wide} wide");
    assert_eq!(bare, quantick_chart::pane_header::segment_px(None));
    assert!(
        bare >= 20.0,
        "a bare key is still a target a hand can hit: {bare}"
    );
}

/// Each side's plate names it in its header and leaves with its side. How far
/// its data reaches, and on the tape whether more is coming, is the plate's
/// tooltip: nothing of it is printed inside the chart.
#[test]
fn each_side_wears_its_label() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = charted_fixture(&ctx);
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    let clock = |text: &String| text.starts_with("prints to ") || text.starts_with("no prints");
    let has = |app: &mut QuantickApp, head: &str| {
        painted_text(&run_frame(app, &ctx))
            .iter()
            .any(|text| text.starts_with(head))
    };
    run_frame(&mut app, &ctx);
    assert!(has(&mut app, "TRAIL"));
    assert!(has(&mut app, "TAPE"));
    assert!(!painted_text(&run_frame(&mut app, &ctx)).iter().any(clock));
    assert!(!has(&mut app, "bars to "), "the trail's clock is a tooltip");
    let plate = painted_text_rects(&run_frame(&mut app, &ctx), "TAPE")[0];
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(plate.center())],
    );
    let tip = (0..90)
        .map(|_| painted_text(&run_frame(&mut app, &ctx)))
        .find(|texts| texts.iter().any(clock));
    assert!(
        tip.is_some(),
        "the TAPE plate's tooltip says how far the prints reach"
    );
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::CandlesOnly);
    assert!(has(&mut app, "TRAIL"));
    assert!(!has(&mut app, "TAPE"));
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    assert!(has(&mut app, "TAPE"));
    assert!(!has(&mut app, "prints to "));
    assert!(!has(&mut app, "TRAIL"));
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
        !app.surfaces.market_palette.is_open(),
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
        let at = control_rect(&app, PaneSide::Flow, HeaderControl::Layer(toggle))
            .unwrap_or_else(|| panic!("{toggle:?} in {:?}", app.active_tab().flow_pane.frame.area))
            .center();
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
/// the layers keep their state and the look returns. The strip goes with the
/// trail; its switch stays filed.
#[test]
fn candles_hidden_take_their_capsule_keep_their_state_and_the_strip_waits() {
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
    // Its glyph lives in the trail's capsule, gone with the trail, and so is the
    // strip; its switch is still filed for the trail's return.
    let strip = HeaderControl::Layer(LayerToggle::LiveStrip);
    assert_eq!(control_rect(&app, PaneSide::Flow, strip), None);
    let strip_before = on(&app, ChartLayer::LiveStrip);
    let actions = &mut Default::default();
    let flow = &mut app.active_tab_mut().flow_pane;
    flow.set_layer_visible(ChartLayer::LiveStrip, !strip_before, actions);
    run_frame(&mut app, &ctx);
    assert_ne!(
        on(&app, ChartLayer::LiveStrip),
        strip_before,
        "the strip moved"
    );
    let flow = &app.active_tab().flow_pane;
    assert_eq!(
        flow.live_strip_width(caps),
        0.0,
        "the tape alone has no strip"
    );

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
    app.active_tab_mut().link.feed_capabilities = quantick_feed::fixed_capabilities(capabilities);
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

/// A narrow tape's header also spans the price axis, so its capsule stays;
/// `flow_band`'s tests walk the fit's order down to nothing. Narrow is
/// measured rather than assumed: the tape's own lane is thinner than the
/// capsule's slots, so only the axis beside it can carry them.
#[test]
fn a_narrow_tape_keeps_its_capsule() {
    use quantick_chart::flow_band::{SECOND_SIDE_PAD_LEFT_PX, SIDE_PAD_RIGHT_PX};
    use quantick_chart::pane_header::SLOT_GAP_PX;
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    // The tape's capsule at the default share, where all of it fits.
    let frame = &app.active_tab().flow_pane.frame;
    let divider = frame.lane_divider_x.expect("a tape beside the candles");
    let slots = frame
        .header_controls
        .iter()
        .filter(|(painted, rect)| {
            matches!(painted, HeaderControl::Layer(_) | HeaderControl::Sample(_))
                && rect.left() > divider
        })
        .fold(egui::Rect::NOTHING, |all, (_, rect)| all.union(*rect))
        .width();
    let room = SECOND_SIDE_PAD_LEFT_PX + 2.0 * SLOT_GAP_PX + slots + SIDE_PAD_RIGHT_PX;
    let gutter = frame.price_gutter.expect("a price axis").width();
    let chart = frame.chart_rect.expect("the pane drew").width();
    assert!(
        room - gutter < slots,
        "the axis must be wide enough to carry what the lane alone cannot"
    );
    // Thinner than the slots, and with the axis beside it past the room.
    let lane = (room - gutter + slots) / 2.0;
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.live_lane.width_share = lane / chart);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let frame = &app.active_tab().flow_pane.frame;
    let divider = frame.lane_divider_x.expect("still beside");
    let axis = frame.price_gutter.expect("a price axis").left();
    assert!(
        axis - divider < slots,
        "the tape alone cannot hold its capsule"
    );
    flow_control(&app, HeaderControl::Layer(LayerToggle::TapeBubbles));
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
    for (control, view) in [
        (TAPE, LaneView::CandlesOnly),
        (TAPE, LaneView::Beside),
        (TRAIL, LaneView::TapeOnly),
        (TRAIL, LaneView::Beside),
    ] {
        let button = flow_control(&app, control);
        click_chart(&mut app, &ctx, button.center());
        run_frame(&mut app, &ctx);
        assert_eq!(view_of(&app), view);
        assert_eq!(flow_control(&app, control), button, "{view:?}");
        let (candles, tape) = (view != LaneView::TapeOnly, view != LaneView::CandlesOnly);
        for (toggle, side) in [
            (LayerToggle::Bubbles, candles),
            (LayerToggle::Footprint, candles),
            (LayerToggle::LiveStrip, candles),
            (LayerToggle::TapeBubbles, tape),
            (LayerToggle::TapeHeatmap, tape),
        ] {
            let painted = control_rect(&app, PaneSide::Flow, HeaderControl::Layer(toggle));
            assert_eq!(painted.is_some(), side, "{view:?}: {toggle:?}");
        }
        let flow = &app.active_tab().flow_pane;
        let chart = flow.frame.area.expect("flow chart");
        assert!(button.bottom() <= chart.top(), "{view:?}: off the chart");
        let band = flow
            .frame
            .candles_right
            .map(|x| flow.frame.chart_rect.unwrap().right() - x);
        let rail = (view == LaneView::CandlesOnly).then_some(quantick_orderflow::LANE_FOLD_RAIL_PX);
        if rail.is_some() {
            assert_eq!(band, rail, "{view:?}: only the rail's band is left");
        } else {
            assert!(band.is_some_and(|px| px > 100.0), "{view:?}: {band:?}");
        }
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
    assert!(
        !scene_control_ids(&scene).contains(&id("layer.live_strip")),
        "the strip's glyph goes with the trail's capsule"
    );
    let sample = scene_control(&scene, &id("sample.tape"));
    assert_eq!(sample["capability_id"], "orderflow.bubbles.size.set");
    assert!(sample["bounds"]["x_pt"].is_string());
    assert!(
        !scene_control_ids(&scene)
            .iter()
            .any(|id| id.starts_with("toolbar.layers")),
        "the toolbar has no LAYERS"
    );
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::Beside);
    run_frame(&mut app, &ctx);
    let scene = observer_scene(&app);
    let strip = scene_control(&scene, &id("layer.live_strip"));
    assert_eq!(strip["availability"]["available"], true);
}

/// On a narrow pane the words and the mark go; the switches and the gear stay,
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

/// Close a side of the lane as the divider does.
fn set_fold(app: &mut QuantickApp, fold: LaneFold) {
    app.active_tab_mut().tape_mut().set_lane_fold(fold);
}

/// The candles closed by name read as tape only, whichever door closed them: Trail
/// unlit, Tape the last lit and refusing, the glyphs grey in place.
#[test]
fn candles_closed_are_tape_only_and_the_last_lit_rule_sees_them() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let toggles = [
        LayerToggle::Bubbles,
        LayerToggle::Heatmap,
        LayerToggle::Footprint,
    ];
    let pane_id = app.active_tab().flow_pane.id;
    let id = |control: &str| format!("pane.{pane_id}.header.{control}");
    set_fold(&mut app, LaneFold::Candles);
    run_frame(&mut app, &ctx);
    assert_eq!(view_of(&app), LaneView::TapeOnly);
    let caps = app.active_tab().capabilities(&app.config);
    let flow = &app.active_tab().flow_pane;
    assert_eq!(
        toggles.map(|toggle| flow.layer_blocked(toggle.layer(), caps)),
        [Some(quantick_layers::blocks::TAPE_ONLY_CANDLES); 3],
    );
    let scene = observer_scene(&app);
    assert_eq!(scene_control(&scene, &id("side.trail"))["selected"], false);
    let tape = scene_control(&scene, &id("side.tape"));
    assert_eq!(tape["selected"], true);
    assert_eq!(
        tape["availability"]["reason"],
        "the_last_side_showing_stays"
    );

    click_side(&mut app, &ctx, TAPE);
    assert_eq!(view_of(&app), LaneView::TapeOnly, "the last lit refuses");
    click_side(&mut app, &ctx, TRAIL);
    assert_eq!(view_of(&app), LaneView::Beside);
}

/// The tape closed by name is the tape off: its switch unlit and the way back at
/// the default width, Trail the last lit.
#[test]
fn a_tape_closed_unlights_its_switch_and_the_switch_brings_it_back() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.show_aggressions = true);
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.live_lane.width_share = 0.6);
    run_frame(&mut app, &ctx);

    set_fold(&mut app, LaneFold::Tape);
    run_frame(&mut app, &ctx);
    assert_eq!(view_of(&app), LaneView::CandlesOnly);
    flow_control(&app, HeaderControl::Sample(BubbleSample::Candle));
    let tape = HeaderControl::Sample(BubbleSample::Tape);
    assert!(
        control_rect(&app, PaneSide::Flow, tape).is_none(),
        "no tape side"
    );

    let pane_id = app.active_tab().flow_pane.id;
    let scene = observer_scene(&app);
    let view = scene_control(&scene, &format!("pane.{pane_id}.header.side.tape"));
    assert_eq!(view["selected"], false);
    let trail = scene_control(&scene, &format!("pane.{pane_id}.header.side.trail"));
    assert_eq!(
        trail["availability"]["reason"],
        "the_last_side_showing_stays"
    );

    click_side(&mut app, &ctx, TAPE);
    assert_eq!(view_of(&app), LaneView::Beside);
    let lane = &app.active_tab().tape().cached_config().live_lane;
    assert_eq!(
        lane.width_share,
        quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE
    );
}

/// Held by the market: the scene says why Tape refuses, under its own code and
/// its own word, Trail is the last side showing, the readback names the hold and
/// the stored view stays beside. A hand on the rail is what ends it.
#[test]
fn a_market_hold_reads_in_the_scene_and_never_in_the_stored_view() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    run_frame(&mut app, &ctx);
    let quiet = quantick_chart::connection_state::ConnectionState {
        link: quantick_chart::connection_state::Link::Quiet,
        ..app.active_tab().connection.state()
    };
    let tape = app.active_tab_mut().tape_mut();
    tape.observe_market(quiet);
    assert_eq!(tape.cached_config().live_lane.view(), LaneView::Beside);
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
    assert!(tape.set_lane_fold(LaneFold::Open));
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
    assert!(tape.set_lane_fold(LaneFold::Open));
    assert!(tape.cached_config().live_lane.market_hold.stood_down);
    tape.reset_for_symbol("AFTER");
    assert!(!tape.cached_config().live_lane.market_hold.stood_down);
    tape.observe_market(quiet);
    assert!(
        tape.cached_config().live_lane.market_held(),
        "the new market's own closed spell folds"
    );
}

/// A held rail is the hand's way through: a click stands the hold down and brings
/// the tape back at the default width, and a segment click on a side closed in a
/// closed spell is not taken straight back.
#[test]
fn a_rail_click_ends_a_hold_at_the_default_and_a_switch_is_not_held_again() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    run_frame(&mut app, &ctx);
    let quiet = quantick_chart::connection_state::ConnectionState {
        link: quantick_chart::connection_state::Link::Quiet,
        ..app.active_tab().connection.state()
    };
    let tape = app.active_tab_mut().tape_mut();
    tape.edit_config(|config| config.live_lane.width_share = 0.6);
    tape.observe_market(quiet);
    assert!(tape.cached_config().live_lane.market_held());
    assert_eq!(tape.cached_config().live_lane.width_share, 0.6, "untouched");
    run_frame(&mut app, &ctx);
    let chart = app.active_tab().flow_pane.frame.chart_rect.unwrap();
    click_chart(
        &mut app,
        &ctx,
        egui::pos2(chart.right() - 4.0, chart.center().y),
    );
    let lane = &app.active_tab().tape().cached_config().live_lane;
    assert!(!lane.market_held());
    assert_eq!(
        lane.width_share,
        quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE
    );

    let tape = app.active_tab_mut().tape_mut();
    tape.set_lane_view(LaneView::CandlesOnly);
    tape.observe_market(quiet);
    tape.set_lane_view(LaneView::Beside);
    tape.observe_market(quiet);
    assert!(!tape.cached_config().live_lane.market_held());
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
    assert!(!app.surfaces.market_palette.is_open());
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
/// answers a click: the trail's header, plate and all, gives the rest up first.
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
                    let side = crate::pane_header::SideHeader {
                        which: quantick_chart::pane_header::CapsuleSide::Candles,
                        salt: 7,
                        pad_left: quantick_chart::flow_band::SIDE_PAD_LEFT_PX,
                        plate: true,
                        clock: &String::new,
                        bars: Some(crate::pane_header::BarsPart {
                            header: &header,
                            selection: &mut selection,
                            inputs: BarInputAvailability::PRINTS,
                            rebuilding: false,
                        }),
                        window: None,
                        capsule: None,
                    };
                    let mut intent = None;
                    clicked = crate::pane_header::draw_side_header(
                        ui,
                        strip,
                        side,
                        &mut painted,
                        &mut intent,
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
    app.active_tab_mut().canvas.set_flow_collapsed(true);
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

/// Where the Bubbles header rests once scrolled to the top, under the layout
/// branch's title bar.
const LANDED_Y: f32 = 166.0;

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
    app.active_tab_mut().canvas.set_flow_collapsed(true);
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

/// Tape alone, nothing is printed over its top: no Buy/Sell key, no `prints to`
/// plate. The plate's words are the TAPE plate's tooltip.
#[test]
fn the_tape_alone_prints_nothing_over_its_top() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = charted_fixture(&ctx);
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    for _ in 0..40 {
        run_frame(&mut app, &ctx);
    }
    let output = run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.frame.flow_legend, None);
    let texts = painted_text(&output);
    assert!(
        !texts
            .iter()
            .any(|text| text.starts_with("prints to ") || text == "Buy"),
        "{texts:?}"
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

/// The numbers of the approved mock, as the trader cut them: the 26 px band holds
/// the view keys 20 tall, the 28 px row under it the capsules, docked 18 tall
/// clear of the band, and the selector, and the 3 px frame closes the chart on the
/// other three sides.
#[test]
fn the_flow_pane_wears_its_band_frame_and_side_headers() {
    use quantick_chart::flow_band::{
        BAND_HEIGHT_PX, DOCKED_SLOT_HEIGHT_PX, FRAME_PX, SECOND_SIDE_PAD_LEFT_PX,
        SIDE_HEADER_HEIGHT_PX,
    };
    let ctx = egui::Context::default();
    let (app, _ends) = flow_fixture(&ctx);
    let flow = &app.active_tab().flow_pane;
    let area = flow.frame.area.expect("flow chart");
    let row_top = area.top() - SIDE_HEADER_HEIGHT_PX;
    let band_top = row_top - BAND_HEIGHT_PX;
    for control in [TRAIL, TAPE] {
        let rect = flow_control(&app, control);
        assert!(
            rect.top() >= band_top && rect.bottom() <= row_top,
            "{control:?}"
        );
        assert_eq!(
            rect.height(),
            quantick_chart::pane_header::SEGMENT_HEIGHT_PX
        );
    }
    let bubbles = flow_control(&app, HeaderControl::Layer(LayerToggle::Bubbles));
    assert_eq!(bubbles.height(), DOCKED_SLOT_HEIGHT_PX);
    assert!(
        bubbles.top() - row_top >= 4.0,
        "clear of the band: {bubbles:?}"
    );
    let split = flow.frame.lane_divider_x.expect("the split");
    let selector = flow_control(&app, HeaderControl::TapeWindow);
    assert!(
        selector.left() - split > SECOND_SIDE_PAD_LEFT_PX,
        "{selector:?}"
    );
    for (control, tape) in [
        (HeaderControl::Layer(LayerToggle::Bubbles), false),
        (HeaderControl::Sample(BubbleSample::Candle), false),
        (HeaderControl::Layer(LayerToggle::Heatmap), false),
        (HeaderControl::Layer(LayerToggle::Footprint), false),
        (HeaderControl::Layer(LayerToggle::LiveStrip), false),
        (HeaderControl::TapeWindow, true),
        (HeaderControl::Layer(LayerToggle::TapeBubbles), true),
        (HeaderControl::Sample(BubbleSample::Tape), true),
        (HeaderControl::Layer(LayerToggle::TapeHeatmap), true),
    ] {
        let rect = flow_control(&app, control);
        assert!(
            rect.top() >= row_top && rect.bottom() <= area.top(),
            "{control:?}"
        );
        assert_eq!(rect.left() >= split, tape, "{control:?} on its side");
    }
    assert_eq!(
        flow.frame.capsules, [None; 2],
        "nothing floats on the chart"
    );
    let carved = crate::pane::split_flow_pane(
        egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(900.0, 650.0)),
        1.0,
    );
    assert_eq!(carved.chart.left(), 100.0 + FRAME_PX);
    assert_eq!(carved.chart.right(), 900.0 - FRAME_PX);
    assert_eq!(carved.chart.bottom(), 650.0 - FRAME_PX);
    assert_eq!(
        carved.chart.top(),
        50.0 + BAND_HEIGHT_PX + SIDE_HEADER_HEIGHT_PX
    );
}

/// The headers split where the chart split, in the frame the divider moved.
#[test]
fn the_side_headers_follow_a_dragged_divider() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let flow = &app.active_tab().flow_pane;
    let before = flow.frame.lane_divider_x.expect("the split");
    let y = flow.frame.area.expect("flow chart").center().y;
    drag(
        &mut app,
        &ctx,
        egui::pos2(before, y),
        egui::pos2(before - 40.0, y),
    );
    let moved = app.active_tab().flow_pane.frame.lane_divider_x.unwrap();
    assert!(moved < before, "{moved} from {before}");
    let selector = flow_control(&app, HeaderControl::TapeWindow);
    let footprint = flow_control(&app, HeaderControl::Layer(LayerToggle::Footprint));
    assert!(selector.left() > moved && footprint.right() < moved);
}

/// A view with one side leaves one header, whole, carrying that side's controls.
#[test]
fn a_closed_side_leaves_one_header_at_full_width() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    click_side(&mut app, &ctx, TAPE);
    run_frame(&mut app, &ctx);
    assert_eq!(view_of(&app), LaneView::CandlesOnly);
    assert!(control_rect(&app, PaneSide::Flow, HeaderControl::TapeWindow).is_none());
    let footprint = flow_control(&app, HeaderControl::Layer(LayerToggle::Footprint));
    let area = app.active_tab().flow_pane.frame.area.expect("flow chart");
    assert!(
        area.right() - footprint.right() < 50.0,
        "at the pane's far end"
    );

    click_side(&mut app, &ctx, TAPE);
    click_side(&mut app, &ctx, TRAIL);
    run_frame(&mut app, &ctx);
    assert_eq!(view_of(&app), LaneView::TapeOnly);
    assert!(
        control_rect(
            &app,
            PaneSide::Flow,
            HeaderControl::Layer(LayerToggle::Footprint)
        )
        .is_none()
    );
    // The tape alone carries its selector after its plate, and no bars.
    let selector = flow_control(&app, HeaderControl::TapeWindow);
    let area = app.active_tab().flow_pane.frame.area.expect("flow chart");
    assert!(
        selector.left() - area.left() < area.width() / 2.0,
        "in the header's near half: {selector:?} in {area:?}"
    );
}

/// Where the header last drew this pane's bars control and indicators button,
/// cleared first so a rect left from an earlier frame cannot pass for this one.
fn bars_and_indicators(app: &mut QuantickApp, ctx: &egui::Context) -> [Option<egui::Rect>; 2] {
    let id = app.active_tab().flow_pane.id;
    let ids = [
        crate::pane_header::menu_id(id),
        crate::pane_header::indicators_id(id),
    ];
    ctx.data_mut(|data| ids.map(|id| data.remove::<egui::Rect>(id)));
    run_frame(app, ctx);
    ctx.data(|data| ids.map(|id| data.get_temp(id)))
}

/// The bars and the `ƒx` are the trail's: the tape alone shows neither, and keeps
/// its plate, its window selector and its capsule. Back beside, the trail has them.
#[test]
fn a_tape_alone_shows_no_bars_and_no_indicators_button() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    assert_eq!(bars_and_indicators(&mut app, &ctx), [None, None]);
    let selector = flow_control(&app, HeaderControl::TapeWindow);
    let bubbles = flow_control(&app, HeaderControl::Layer(LayerToggle::TapeBubbles));
    assert!(
        selector.right() <= bubbles.left(),
        "{selector:?} {bubbles:?}"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(texts.iter().any(|text| text == "TAPE"), "{texts:?}");

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::Beside);
    let [bars, indicators] = bars_and_indicators(&mut app, &ctx);
    let (bars, indicators) = (bars.expect("the bars"), indicators.expect("the ƒx"));
    let split = app.active_tab().flow_pane.frame.lane_divider_x.unwrap();
    assert!(bars.right() <= indicators.left() && indicators.right() < split);
}

/// The flow pane's time axis and its bottom padding stand on the frame's tone, the
/// chart's dark ending at the plot's bottom edge, square across the price gutter's
/// corner; a context pane keeps its axis on the chart's own background.
#[test]
fn the_flow_time_axis_stands_on_the_frame_tone() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let output = run_frame(&mut app, &ctx);
    fn rects(shape: &egui::Shape, found: &mut Vec<(egui::Rect, egui::Color32)>) {
        match shape {
            egui::Shape::Rect(rect) => found.push((rect.rect, rect.fill)),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| rects(shape, found)),
            _ => {}
        }
    }
    let mut painted = Vec::new();
    output
        .shapes
        .iter()
        .for_each(|clipped| rects(&clipped.shape, &mut painted));
    let axis_of = |side| {
        let frame = &app.active_tab().pane(side).frame;
        let (area, strip) = (
            frame.area.expect("drawn"),
            frame.time_strip.expect("an axis"),
        );
        (area, area.with_min_y(strip.top()))
    };
    let (area, axis) = axis_of(PaneSide::Flow);
    assert!(
        axis.height() > 0.0 && axis.bottom() == area.bottom(),
        "{axis:?} in {area:?}"
    );
    let at = |rect: egui::Rect, fill| painted.iter().position(|seen| *seen == (rect, fill));
    let canvas = at(area, crate::theme::CANVAS).expect("the chart's dark goes down first");
    let tone = at(axis, crate::theme::FLOW_SURFACE).expect("then the frame's tone below the plot");
    assert!(canvas < tone, "the tone lies over the dark, not under it");
    let (_, context_axis) = axis_of(PaneSide::Time(0));
    assert_eq!(
        at(context_axis, crate::theme::FLOW_SURFACE),
        None,
        "context panes keep theirs"
    );
}

/// The docked capsules, on the flow pane's sides and on a time pane alike, stand
/// bare: no case encloses their icons, and neighbouring icons keep clear air
/// between them. The band's keys wear no body either (below).
#[test]
fn the_headers_paint_no_wrapper_behind_their_icons() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let output = run_frame(&mut app, &ctx);
    fn rects(shape: &egui::Shape, found: &mut Vec<egui::epaint::RectShape>) {
        match shape {
            egui::Shape::Rect(rect) => found.push(*rect),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| rects(shape, found)),
            _ => {}
        }
    }
    let mut painted = Vec::new();
    output
        .shapes
        .iter()
        .for_each(|clipped| rects(&clipped.shape, &mut painted));
    let trail = [
        LayerToggle::Bubbles,
        LayerToggle::Heatmap,
        LayerToggle::Footprint,
        LayerToggle::LiveStrip,
    ];
    let tape = [LayerToggle::TapeBubbles, LayerToggle::TapeHeatmap];
    let capsules: [(PaneSide, &[LayerToggle]); 3] = [
        (PaneSide::Flow, &trail),
        (PaneSide::Flow, &tape),
        (PaneSide::Time(0), &[LayerToggle::Footprint]),
    ];
    for (side, toggles) in capsules {
        let mut icons: Vec<_> = toggles
            .iter()
            .filter_map(|toggle| control_rect(&app, side, HeaderControl::Layer(*toggle)))
            .collect();
        assert_eq!(icons.len(), toggles.len(), "{side:?} docks {toggles:?}");
        icons.sort_by(|a, b| a.left().total_cmp(&b.left()));
        let all = icons
            .iter()
            .fold(egui::Rect::NOTHING, |all, icon| all.union(*icon));
        // A row or band fill is taller than a docked capsule; a case is not.
        let case = painted.iter().find(|shape| {
            shape.rect.height() <= quantick_chart::flow_band::DOCKED_CAPSULE_HEIGHT_PX + 1.0
                && shape.rect.contains_rect(all)
        });
        assert!(
            case.is_none(),
            "{side:?}: a case around the icons: {case:?}"
        );
        for pair in icons.windows(2) {
            assert!(
                pair[1].left() - pair[0].right() >= 6.0,
                "{side:?}: {pair:?}"
            );
        }
    }
}

/// Every shape a frame painted, flattened.
fn flat_shapes(output: &egui::FullOutput) -> Vec<egui::Shape> {
    fn flat(shape: &egui::Shape, found: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| flat(shape, found)),
            shape => found.push(shape.clone()),
        }
    }
    let mut shapes = Vec::new();
    output
        .shapes
        .iter()
        .for_each(|clipped| flat(&clipped.shape, &mut shapes));
    shapes
}

/// What a key on the band wears: its dot's fill and whether it glows, and the
/// fill of any body painted under it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct KeyLook {
    dot: Option<egui::Color32>,
    glow: bool,
    body: Option<egui::Color32>,
}

fn key_look(shapes: &[egui::Shape], key: egui::Rect) -> KeyLook {
    use quantick_chart::pane_header::{KEY_DOT_PX, key_dot_inset};
    let center = key.left_center() + egui::vec2(key_dot_inset(true), 0.0);
    let circles = |bigger: bool| {
        shapes.iter().filter_map(move |shape| match shape {
            egui::Shape::Circle(circle)
                if circle.center == center
                    && (circle.radius > KEY_DOT_PX / 2.0) == bigger
                    && (bigger || circle.radius == KEY_DOT_PX / 2.0) =>
            {
                Some(circle.fill)
            }
            _ => None,
        })
    };
    let body = shapes.iter().find_map(|shape| match shape {
        egui::Shape::Rect(seen) if key.expand(3.0).contains_rect(seen.rect) => Some(seen.fill),
        egui::Shape::Mesh(mesh) if key.expand(3.0).contains_rect(mesh.calc_bounds()) => {
            Some(egui::Color32::TRANSPARENT)
        }
        _ => None,
    });
    KeyLook {
        dot: circles(false).next_back(),
        glow: circles(true).next().is_some(),
        body,
    }
}

/// The band's keys wear no body: a dot lit and glowing for a shown side, grey for
/// the other, red while the market holds the tape. The gear is bare until its
/// panel opens, then sinks into a well.
#[test]
fn the_band_keys_wear_a_dot_and_no_body() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    // No pointer on the band, for a whole frame: a hovered key is brighter.
    let looks = |app: &mut QuantickApp| {
        run_frame_with_events(app, &ctx, vec![egui::Event::PointerGone]);
        let shapes = flat_shapes(&run_frame(app, &ctx));
        [TRAIL, TAPE, GEAR].map(|control| key_look(&shapes, flow_control(app, control)))
    };
    let palette = crate::theme::get(&ctx);
    let lit = KeyLook {
        dot: Some(palette.flow_key_lit_dot()),
        glow: true,
        body: None,
    };
    let unlit = KeyLook {
        dot: Some(palette.flow_key_unlit_dot()),
        glow: false,
        body: None,
    };
    let bare = KeyLook {
        dot: None,
        glow: false,
        body: None,
    };
    assert_eq!(
        looks(&mut app),
        [lit, lit, bare],
        "both sides, the panel shut"
    );
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::CandlesOnly);
    assert_eq!(looks(&mut app), [lit, unlit, bare], "the tape put away");
    let gear = flow_control(&app, GEAR).center();
    click_chart(&mut app, &ctx, gear);
    run_frame(&mut app, &ctx);
    assert!(panel_open(&app));
    let well = KeyLook {
        body: Some(palette.flow_key_down()),
        ..bare
    };
    assert_eq!(
        looks(&mut app),
        [lit, unlit, well],
        "the open panel sinks the gear"
    );

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::Beside);
    let quiet = quantick_chart::connection_state::ConnectionState {
        link: quantick_chart::connection_state::Link::Quiet,
        ..app.active_tab().connection.state()
    };
    app.active_tab_mut().tape_mut().observe_market(quiet);
    let [_, tape, _] = looks(&mut app);
    assert_eq!(
        tape,
        KeyLook {
            dot: Some(crate::theme::WARN),
            ..unlit
        },
        "held by the market"
    );
}

/// With no body to light or sink, a key still answers the hand: a lit key's dot
/// glows brighter under the pointer, and a held key dims its dot and word.
#[test]
fn a_bodiless_key_answers_hover_and_press() {
    use crate::pane_header::{DOT_GLOW_HOVERED, KEY_PRESSED_OPACITY};
    use quantick_chart::pane_header::{KEY_DOT_PX, key_dot_inset};
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerGone]);
    run_frame(&mut app, &ctx);
    let key = flow_control(&app, TRAIL);
    let center = key.left_center() + egui::vec2(key_dot_inset(true), 0.0);
    // The fills of the circles round the dot, widest first, and the dot's own.
    let circles = |output: &egui::FullOutput| {
        let mut found: Vec<_> = flat_shapes(output)
            .into_iter()
            .filter_map(|shape| match shape {
                egui::Shape::Circle(circle) if circle.center == center => {
                    Some((circle.radius, circle.fill))
                }
                _ => None,
            })
            .collect();
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        found
    };
    let palette = crate::theme::get(&ctx);
    let lit = palette.flow_key_lit_dot();
    let rest = circles(&run_frame(&mut app, &ctx));
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(center)]);
    let hovered = circles(&run_frame(&mut app, &ctx));
    let glow = DOT_GLOW_HOVERED
        .map(|(past, strength)| (KEY_DOT_PX / 2.0 + past, lit.gamma_multiply(strength)));
    assert_eq!(hovered[..2], glow, "a brighter glow under the pointer");
    assert_ne!(hovered[..2], rest[..2]);
    assert_eq!(hovered[2], (KEY_DOT_PX / 2.0, lit));
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(center),
            egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let pressed = circles(&run_frame(&mut app, &ctx));
    let dimmed = lit.gamma_multiply(KEY_PRESSED_OPACITY);
    assert_eq!(
        pressed.last(),
        Some(&(KEY_DOT_PX / 2.0, dimmed)),
        "held down"
    );
}

/// The band reads left to right: the mark, its rule, Trail, Tape; the gear ends
/// it on the right. Dark keeps its slate band, Light turns graphite, each with its
/// own ink and closing rule.
#[test]
fn the_band_reads_mark_rule_keys_and_gear_in_both_themes() {
    let rgb = egui::Color32::from_rgb;
    for (source, band, ink, rule) in [
        (
            quantick_appearance::DARK,
            rgb(0x1E, 0x24, 0x33),
            rgb(0xE8, 0xEC, 0xF2),
            rgb(0x2E, 0x36, 0x48),
        ),
        (
            quantick_appearance::LIGHT,
            rgb(0x4A, 0x50, 0x59),
            rgb(0xF2, 0xF4, 0xF7),
            rgb(0x3A, 0x3F, 0x45),
        ),
    ] {
        let ctx = egui::Context::default();
        let (mut app, _ends) = flow_fixture(&ctx);
        app.runtime
            .appearance
            .apply_candidate(source, false)
            .unwrap();
        run_frame(&mut app, &ctx);
        let output = run_frame(&mut app, &ctx);
        let shapes = flat_shapes(&output);
        let mark = painted_text_rects(&output, "ORDER FLOW")[0];
        let [trail, tape, gear] = [TRAIL, TAPE, GEAR].map(|control| flow_control(&app, control));
        assert!(
            mark.right() < trail.left() && trail.right() < tape.left(),
            "{mark:?} {trail:?} {tape:?}"
        );
        assert!(tape.right() < gear.left(), "{tape:?} {gear:?}");
        let fill = shapes.iter().any(|shape| {
            matches!(shape, egui::Shape::Rect(seen)
                if seen.fill == band && seen.rect.contains_rect(trail.union(gear)))
        });
        assert!(fill, "the band's fill");
        let lines = |colour: egui::Color32| {
            shapes
                .iter()
                .filter_map(move |shape| match shape {
                    egui::Shape::LineSegment { points, stroke }
                        if stroke.color == egui::epaint::ColorMode::Solid(colour) =>
                    {
                        Some(*points)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let separator = lines(ink.gamma_multiply(0.35));
        assert!(
            separator
                .iter()
                .any(|[from, to]| from.x == to.x && from.x > mark.right() && from.x < trail.left()),
            "the rule between the mark and the keys: {separator:?}"
        );
        assert!(
            lines(rule)
                .iter()
                .any(|[from, to]| from.y == to.y && from.y > gear.bottom()),
            "the band's closing rule"
        );
        let mark_ink = shapes.iter().find_map(|shape| match shape {
            egui::Shape::Text(text) if text.galley.text() == "ORDER FLOW" => {
                Some(text.galley.job.sections[0].format.color)
            }
            _ => None,
        });
        assert_eq!(mark_ink, Some(ink), "the mark's ink");
    }
}

/// A key's tooltip: its name and shortcut on the first line, then what its side
/// shows, then why it refuses; the same words the scene carries.
#[test]
fn a_key_tooltip_names_the_key_and_says_what_its_side_shows() {
    use quantick_chart::pane_header::{TAPE_DESCRIPTION, TRAIL_DESCRIPTION};
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let tip = |app: &mut QuantickApp, control: HeaderControl, description: &str| {
        let at = flow_control(app, control).center();
        run_frame_with_events(app, &ctx, vec![egui::Event::PointerMoved(at)]);
        (0..90)
            .map(|_| painted_text(&run_frame(app, &ctx)))
            .find(|texts| texts.iter().any(|text| text == description))
            .unwrap_or_else(|| panic!("{control:?} shows its tooltip"))
    };
    let shortcut = |side| ctx.format_shortcut(&crate::chart_layers::shortcuts::side_shortcut(side));
    let texts = tip(&mut app, TRAIL, TRAIL_DESCRIPTION);
    for wanted in ["Trail".to_owned(), shortcut(LaneSide::Trail)] {
        assert!(texts.contains(&wanted), "{wanted} in {texts:?}");
    }
    let texts = tip(&mut app, TAPE, TAPE_DESCRIPTION);
    for wanted in ["Tape".to_owned(), shortcut(LaneSide::Tape)] {
        assert!(texts.contains(&wanted), "{wanted} in {texts:?}");
    }
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::CandlesOnly);
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerGone]);
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let texts = tip(&mut app, TRAIL, TRAIL_DESCRIPTION);
    assert!(
        texts
            .iter()
            .any(|text| text == "the last side showing stays"),
        "the refusal follows: {texts:?}"
    );
}

/// The scene lists what the row paints: beside the tape the flow pane's bars and
/// indicators calls are there, the trail's; the tape alone lists neither, while a
/// context pane keeps both.
#[test]
fn the_scene_lists_the_bars_and_indicators_only_with_the_trail() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let listed = |app: &QuantickApp, pane: u64| {
        let ids = scene_control_ids(&observer_scene(app));
        ["bars", "indicators"].map(|what| ids.contains(&format!("pane_header.{pane}.{what}")))
    };
    let flow = app.active_tab().flow_pane.id;
    let context = app.active_tab().pane(PaneSide::Time(0)).id;
    assert_eq!(view_of(&app), LaneView::Beside);
    assert_eq!(
        listed(&app, flow),
        [true, true],
        "the trail's, beside the tape"
    );
    assert!(
        bars_and_indicators(&mut app, &ctx)
            .iter()
            .all(Option::is_some)
    );

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    assert_eq!(
        bars_and_indicators(&mut app, &ctx),
        [None, None],
        "not painted"
    );
    assert_eq!(listed(&app, flow), [false, false], "and not listed");
    assert_eq!(
        listed(&app, context),
        [true, true],
        "a context pane keeps its own"
    );
}

/// The live strip is the trail's. Beside the tape it stands on the candles' side
/// of the divider and the candles pay for it: the tape keeps its width, the
/// headers still split at the divider, and the candles end where the strip
/// starts. The trail alone keeps it against the price gutter; the
/// tape alone has none, its switch filed for the trail's return.
#[test]
fn the_live_strip_stands_on_the_trail() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let width = crate::live_strip::LIVE_STRIP_WIDTH_PX;
    let shown = |app: &mut QuantickApp, on: bool| {
        let flow = &mut app.active_tab_mut().flow_pane;
        flow.set_layer_visible(ChartLayer::LiveStrip, on, &mut Default::default());
        run_frame(app, &ctx);
        run_frame(app, &ctx);
        let frame = &app.active_tab().flow_pane.frame;
        let chart = frame.chart_rect.expect("flow chart");
        (chart, frame.lane_divider_x, frame.candles_right)
    };
    assert_eq!(view_of(&app), LaneView::Beside);
    let (chart, divider, candles) = shown(&mut app, false);
    let divider = divider.expect("a divider beside the tape");
    assert_eq!(
        candles,
        Some(divider),
        "no strip, the candles run to the tape"
    );
    let (on_chart, on_divider, on_candles) = shown(&mut app, true);
    assert_eq!(on_chart, chart, "the gutter keeps no strip beside the tape");
    assert_eq!(on_divider, Some(divider), "the tape keeps its width");
    assert_eq!(on_candles, Some(divider - width), "the candles pay for it");
    let glyph = flow_control(&app, HeaderControl::Layer(LayerToggle::LiveStrip));
    assert!(
        glyph.right() > divider - width && glyph.right() < divider,
        "the trail's header runs over the strip to the divider: {glyph:?}"
    );
    assert!(flow_control(&app, HeaderControl::TapeWindow).left() > divider);

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::CandlesOnly);
    let (chart, _, _) = shown(&mut app, true);
    let gutter = app
        .active_tab()
        .flow_pane
        .frame
        .price_gutter
        .expect("gutter");
    assert_eq!(chart.right(), gutter.left() - width, "against the gutter");

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    let (chart, _, _) = shown(&mut app, true);
    let caps = app.active_tab().capabilities(&app.config);
    assert_eq!(app.active_tab().flow_pane.live_strip_width(caps), 0.0);
    assert_eq!(chart.right(), gutter.left(), "no column on the tape alone");
    assert!(on(&app, ChartLayer::LiveStrip), "the switch is kept");
}

/// The window list open, the view drops the tape: the list closes with its
/// selector, so no popup stays open behind a control no one can see.
#[test]
fn the_window_list_closes_when_its_selector_goes() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let selector = flow_control(&app, HeaderControl::TapeWindow);
    click_chart(&mut app, &ctx, selector.center());
    run_frame(&mut app, &ctx);
    assert!(
        ctx.memory(|memory| memory.any_popup_open()),
        "the list opened"
    );
    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::CandlesOnly);
    run_frame(&mut app, &ctx);
    assert!(!ctx.memory(|memory| memory.any_popup_open()));
}

/// The list open, the flow pane collapses, then another tab takes the canvas: the
/// selector is not drawn either way, so its list closes wherever that was decided.
#[test]
fn the_window_list_closes_when_its_pane_or_its_tab_goes() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let open = |app: &mut QuantickApp| {
        run_frame(app, &ctx);
        let selector = flow_control(app, HeaderControl::TapeWindow);
        click_chart(app, &ctx, selector.center());
        run_frame(app, &ctx);
        assert!(
            ctx.memory(|memory| memory.any_popup_open()),
            "the list opened"
        );
    };
    open(&mut app);
    assert!(app.active_tab_mut().canvas.set_flow_collapsed(true));
    run_frame(&mut app, &ctx);
    assert!(!ctx.memory(|memory| memory.any_popup_open()), "collapsed");
    app.active_tab_mut().canvas.set_flow_collapsed(false);
    open(&mut app);
    let first = app.runtime.tabs.active_index();
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "OTHERUSDT".to_owned(), None);
    app.runtime.tabs.select(app.runtime.tabs.len() - 1);
    assert_ne!(app.runtime.tabs.active_index(), first);
    run_frame(&mut app, &ctx);
    assert!(!ctx.memory(|memory| memory.any_popup_open()), "another tab");
}

/// Once two panes share the canvas the focused one, flow or time, wears an
/// accent frame and no other pane does; a lone pane is focused by definition
/// and wears none.
#[test]
fn only_the_focused_pane_of_a_split_wears_the_accent_frame() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let accent = crate::theme::ACCENT;
    let framed = |output: &egui::FullOutput, pane: egui::Rect| {
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) => {
                    shape.stroke.color == accent
                        && shape.stroke.width >= 2.0
                        && shape.rect.contains(pane.center())
                }
                _ => false,
            })
            .count()
    };
    let area = |app: &QuantickApp, side: PaneSide| {
        app.active_tab()
            .pane(side)
            .frame
            .chart_area
            .expect("a drawn pane")
    };
    for focused in [PaneSide::Flow, PaneSide::Time(0)] {
        app.active_tab_mut().focus = focused;
        let output = run_frame(&mut app, &ctx);
        for side in [PaneSide::Flow, PaneSide::Time(0)] {
            assert_eq!(
                framed(&output, area(&app, side)),
                usize::from(side == focused),
                "focus on {focused:?}: the frame around {side:?}"
            );
        }
    }
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    assert_eq!(
        framed(&output, area(&app, PaneSide::Flow)),
        0,
        "a lone pane wears no frame"
    );
}
