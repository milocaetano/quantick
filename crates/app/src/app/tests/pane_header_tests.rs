//! Every pane's header line names its own bars, view, layers and sizes, and
//! changes them, wherever focus sits.

use super::candle_aggression::context_fixture;
use super::*;
use crate::dock::DockTab;
use crate::state::BarSpec;
use quantick_chart::pane_header::{BubbleSample, HeaderControl, LayerToggle};
use quantick_engine::bar_selection::{BarInputAvailability, SelectionCommand};
use quantick_orderflow::LaneView;

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

/// The toolbar has no LAYERS: each pane carries its own, a time pane only its
/// footprint, in the same strip as its bars.
#[test]
fn each_pane_paints_only_the_controls_it_accepts() {
    let ctx = egui::Context::default();
    let (app, _ends) = flow_fixture(&ctx);
    for toggle in LayerToggle::ALL {
        flow_control(&app, HeaderControl::Layer(toggle));
    }
    flow_control(&app, HeaderControl::View);
    let time = &app
        .active_tab()
        .pane(PaneSide::Time(0))
        .frame
        .header_controls;
    let only: Vec<_> = time.iter().map(|(control, _)| *control).collect();
    assert_eq!(only, [HeaderControl::Layer(LayerToggle::Footprint)]);
    let area = app.active_tab().flow_pane.frame.area.expect("flow chart");
    let strip_bottom = flow_control(&app, HeaderControl::View).bottom();
    assert!(strip_bottom <= area.top(), "the controls sit off the chart");
}

/// One button walks the three views and its state agrees with the named read.
#[test]
fn the_view_button_cycles_the_three_views() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    for wanted in [
        LaneView::CandlesOnly,
        LaneView::TapeOnly,
        LaneView::Beside,
        LaneView::CandlesOnly,
    ] {
        let button = flow_control(&app, HeaderControl::View);
        click_chart(&mut app, &ctx, button.center());
        run_frame(&mut app, &ctx);
        assert_eq!(view_of(&app), wanted);
    }
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

/// A glyph switches its own pane's layer, wherever focus sits, and leaves focus
/// alone. The footprint on a time pane is that pane's and nobody else's.
#[test]
fn a_glyph_switches_its_own_pane_wherever_focus_sits() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let style = app.runtime.style.clone();
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
    for (toggle, tab) in [
        (LayerToggle::Bubbles, DockTab::Bubbles),
        (LayerToggle::Heatmap, DockTab::L2),
        (LayerToggle::LiveStrip, DockTab::L2),
    ] {
        let before = on(&app, toggle.layer());
        let at = flow_control(&app, HeaderControl::Layer(toggle)).center();
        run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
        run_frame_with_events(&mut app, &ctx, vec![secondary(at, true)]);
        run_frame_with_events(&mut app, &ctx, vec![secondary(at, false)]);
        run_frame(&mut app, &ctx);
        assert!(app.dock.visible(), "{toggle:?}");
        assert_eq!(app.dock.tab(), Some(tab), "{toggle:?}");
        assert_eq!(on(&app, toggle.layer()), before, "looking is not enabling");
    }
}

/// With the candles hidden their glyphs stay where they were, refuse the hand
/// and the keys, and keep their state; the strip stays live and paints.
#[test]
fn candles_hidden_grey_their_glyphs_keep_their_state_and_the_strip_stays_live() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    let toggles = [
        LayerToggle::Bubbles,
        LayerToggle::Heatmap,
        LayerToggle::Footprint,
    ];
    let at =
        |app: &QuantickApp| toggles.map(|toggle| flow_control(app, HeaderControl::Layer(toggle)));
    let (before, states) = (at(&app), toggles.map(|toggle| on(&app, toggle.layer())));
    let view_glyph = flow_control(&app, HeaderControl::View);

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::TapeOnly);
    run_frame(&mut app, &ctx);
    assert_eq!(at(&app), before, "the glyphs never shift");
    assert_eq!(flow_control(&app, HeaderControl::View), view_glyph);

    for rect in before {
        click_chart(&mut app, &ctx, rect.center());
        run_frame(&mut app, &ctx);
    }
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
    assert_eq!(
        toggles.map(|toggle| on(&app, toggle.layer())),
        states,
        "the look returns"
    );
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
        "the slot stays reserved"
    );

    app.active_tab_mut()
        .tape_mut()
        .set_lane_view(LaneView::CandlesOnly);
    run_frame(&mut app, &ctx);
    assert!(control_rect(&app, PaneSide::Flow, tape).is_none());
    let pane_id = app.active_tab().flow_pane.id;
    let ids = scene_control_ids(&observer_scene(&app));
    assert!(!ids.contains(&format!("pane.{pane_id}.header.sample.tape")));
    assert!(ids.contains(&format!("pane.{pane_id}.header.view")));
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
    let view = scene_control(&scene, &id("view"));
    assert_eq!(view["capability_id"], "orderflow.tape.view.set");
    assert_eq!(view["label"], "Tape only");
    assert_eq!(view["owner"]["kind"], "pane_header");
    let l2 = scene_control(&scene, &id("layer.heatmap"));
    assert_eq!(l2["capability_id"], "layers.visibility.set");
    assert_eq!(
        l2["availability"]["reason"],
        "candle_layer_hidden_in_tape_only"
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

/// On a narrow pane the mark goes, then the samples; nothing overlaps, and what
/// clipped away the scene does not list.
#[test]
fn a_narrow_header_drops_the_mark_then_the_samples_and_never_overlaps() {
    let ctx = egui::Context::default();
    let (mut app, _ends) = flow_fixture(&ctx);
    app.active_tab_mut().set_layout(CanvasLayout::Single);
    let sample = HeaderControl::Sample(BubbleSample::Tape);
    let mut seen_with = false;
    let mut seen_without = false;
    for width in [1_400.0, 420.0, 340.0, 260.0] {
        for _ in 0..2 {
            run_frame_at(&mut app, &ctx, egui::vec2(width, 700.0));
        }
        let controls = app.active_tab().flow_pane.frame.header_controls.clone();
        if controls.iter().any(|(control, _)| *control == sample) {
            seen_with = true;
        } else {
            seen_without = true;
        }
        for (index, (control, rect)) in controls.iter().enumerate() {
            for (other, other_rect) in controls.iter().skip(index + 1) {
                assert!(
                    !rect.shrink(0.5).intersects(*other_rect),
                    "{control:?} {rect:?} overlaps {other:?} {other_rect:?} at {width}"
                );
            }
        }
    }
    assert!(
        seen_with && seen_without,
        "the rule fired across the widths"
    );
}
