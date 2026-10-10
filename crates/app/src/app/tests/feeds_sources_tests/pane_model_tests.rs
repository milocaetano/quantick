// Pane model integration preserves the existing chart behavior.
use super::*;

/// The dark chart: with the view panned into history, a coarser spec cuts
/// the same trades into far fewer bars, and the old right-edge index falls
/// off the end of the series — leaving the window over empty space, where
/// nothing is drawn at all.
#[test]
fn a_rebuild_keeps_the_view_on_the_market_time_it_was_showing() {
    let (mut app, _cmd_rx) = app_with_history(400);
    // Pan back to bar 200 of 400 and remember what the edge was showing.
    let slots = app.active_tab().flow_pane.slots();
    app.active_tab_mut()
        .flow_pane
        .model
        .viewport
        .pan_pixels(200.0 * 8.0, slots);
    assert!(!app.active_tab().flow_pane.model.viewport.follows_live());
    let was_showing = app
        .active_tab()
        .flow_pane
        .right_edge_time()
        .expect("a bar under the edge");

    // Coarsen: 400 trades become 10 bars, so index 200 no longer exists.
    app.active_tab_mut()
        .flow_pane
        .spec
        .retain(crate::state::BarSpec::Tick(40));
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    assert_eq!(app.active_tab().flow_pane.state.bars().len(), 10);

    let slots = app.active_tab().flow_pane.slots();
    let (start, end) = app
        .active_tab()
        .flow_pane
        .model
        .viewport
        .visible_range(800.0, slots);
    assert!(
        start < end,
        "the window must still hold bars, got {start}..{end} of {slots}"
    );
    let now_showing = app
        .active_tab()
        .flow_pane
        .right_edge_time()
        .expect("still on a bar");
    let bar = &app.active_tab().flow_pane.state.bars()[app
        .active_tab()
        .flow_pane
        .model
        .viewport
        .right_edge_bar(slots) as usize];
    assert!(
        bar.open_time <= was_showing && was_showing <= bar.close_time,
        "the edge bar ({}..{}) must span the time it was showing ({was_showing})",
        bar.open_time,
        bar.close_time
    );
    assert!(now_showing <= was_showing, "never jumps into the future");
}
