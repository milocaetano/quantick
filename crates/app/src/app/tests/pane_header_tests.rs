//! Every pane's header line names its own bars, and changes them, wherever
//! focus sits.

use super::candle_aggression::context_fixture;
use super::*;
use crate::state::BarSpec;
use quantick_engine::bar_selection::{BarInputAvailability, SelectionCommand};

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
