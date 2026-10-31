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

    let control = control_of(&app, &ctx, PaneSide::Flow);
    click_chart(&mut app, &ctx, control.center());
    assert_eq!(
        app.active_tab().focus,
        PaneSide::Time(0),
        "opening the flow pane's menu keeps focus where it was"
    );
    let open = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        open.iter().any(|text| text == "volume"),
        "the menu offers the registered kinds: {open:?}"
    );

    // The list sits below the control, the kinds in registry order at one row
    // each; a real click on a real row is what this asserts.
    let rows = app.active_tab().flow_pane.spec.registry().definitions();
    let volume = rows.iter().position(|row| row.label() == "volume").unwrap();
    let row_height = ctx.style().spacing.interact_size.y;
    let row = egui::pos2(
        control.left() + 20.0,
        control.bottom() + 10.0 + row_height * (volume as f32 + 0.5),
    );
    click_chart(&mut app, &ctx, row);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    let tab = app.active_tab();
    assert_eq!(
        tab.flow_pane.spec.selected_id(),
        "volume",
        "the click landed on this pane's list"
    );
    assert_eq!(tab.time_panes[0].state.spec(), &time_before);
    assert_eq!(tab.focus, PaneSide::Time(0));
}

/// A second pane's control opens its own menu, not the first's.
#[test]
fn each_pane_owns_its_menu() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    run_frame(&mut app, &ctx);
    let time = control_of(&app, &ctx, PaneSide::Time(0));
    let flow = control_of(&app, &ctx, PaneSide::Flow);
    assert_ne!(time, flow);

    click_chart(&mut app, &ctx, time.center());
    let ids = [
        app.active_tab().time_panes[0].id,
        app.active_tab().flow_pane.id,
    ];
    assert_ne!(ids[0], ids[1], "panes key their menus by their own id");
    let time_open = ctx.memory(|memory| memory.is_popup_open(crate::pane_header::menu_id(ids[0])));
    let flow_open = ctx.memory(|memory| memory.is_popup_open(crate::pane_header::menu_id(ids[1])));
    assert!(time_open && !flow_open);
}

/// `layout.pane.set_bar_spec` writes the same selection the header reads.
#[test]
fn a_spec_set_off_the_header_shows_on_it() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    set_flow_bars(&mut app, BarSpec::Tick(777));
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text.starts_with("Tick · 777")),
        "{texts:?}"
    );
}
