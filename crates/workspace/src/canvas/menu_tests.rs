//! A chart's ⋯ menu, its strip's name, the remove dialog's words and the
//! seats that tie a slot to a built chart.

use super::*;

fn canvas(id: &str) -> CanvasState {
    CanvasState::from_preset(preset(id).unwrap())
}

#[test]
fn the_menu_offers_restore_size_only_while_the_chart_fills_the_canvas() {
    let mut state = canvas("time+flow");
    let rows = pane_menu(&state, 0);
    let labels: Vec<_> = rows.iter().map(|item| item.label()).collect();
    assert_eq!(
        labels,
        [
            "Minimize",
            "Maximize",
            "Float",
            "Open in new window",
            "Remove chart"
        ]
    );
    state.maximize(0).unwrap();
    assert_eq!(pane_menu(&state, 0)[1], PaneMenuItem::Restore);
    assert_eq!(pane_menu(&state, 0)[1].label(), "Restore size");
    assert_eq!(pane_menu(&state, 1)[1], PaneMenuItem::Maximize);
    let lone = canvas("flow");
    assert_eq!(
        pane_menu(&lone, 0),
        [PaneMenuItem::Maximize],
        "the last chart stays"
    );
    for item in rows.into_iter().chain([PaneMenuItem::Restore]) {
        assert_eq!(PaneMenuItem::parse(item.id()), Some(item));
    }
    assert_eq!(PaneMenuItem::parse("close"), None);
    assert_eq!(menu_hook("1"), Some((1, None)));
    let pressed = Some((0, Some(PaneMenuItem::Remove)));
    assert_eq!(menu_hook("0:remove"), pressed);
    assert_eq!(menu_hook("0:close"), None);
    assert_eq!(menu_hook("first"), None);
}

/// A folded chart's menu brings it back where Minimize stood: folding it
/// again would do nothing, so it is not offered.
#[test]
fn a_minimized_pane_is_not_offered_minimize() {
    let mut state = canvas("time+time+flow");
    state.minimize(0).unwrap();
    let rows = pane_menu(&state, 0);
    assert!(rows.contains(&PaneMenuItem::Restore), "{rows:?}");
    assert!(!rows.contains(&PaneMenuItem::Minimize), "{rows:?}");
    let labels: Vec<_> = rows.iter().map(|item| item.label()).collect();
    assert_eq!(
        labels,
        [
            "Restore size",
            "Maximize",
            "Float",
            "Open in new window",
            "Remove chart"
        ]
    );
    assert_eq!(pane_menu(&state, 1)[0], PaneMenuItem::Minimize, "open");
}

#[test]
fn a_strip_is_named_by_kind_bars_and_market() {
    let time = strip_label(PaneKind::Time, "1m", "BTCUSDT");
    assert_eq!(time, "Time · 1m · BTCUSDT");
    let flow = strip_label(PaneKind::Flow, "Tick · 50", "BTCUSDT");
    assert_eq!(flow, "Order flow · Tick · 50 · BTCUSDT");
}

#[test]
fn the_remove_dialog_says_what_is_saved_and_what_the_layout_becomes() {
    let saved = canvas("time+flow");
    let live = saved.clone();
    let prompt = remove_prompt("Fluxo", &saved, &live, 0, false);
    assert_eq!(prompt.title, "Remove this chart from “Fluxo”?");
    assert_eq!(
        prompt.lines,
        [
            "“Fluxo” is saved with this chart (1 TF + Flow).".to_owned(),
            "Removing it changes the layout to Flow.".to_owned(),
            "The saved “Fluxo” keeps the chart until you save.".to_owned(),
        ]
    );
    let auto = remove_prompt("Fluxo", &saved, &live, 0, true);
    assert_eq!(
        auto.lines[2],
        "Auto save is on, so removing it saves “Fluxo” at once."
    );
    // Removing the last flow lays the charts out as TIMEFRAMES.
    let flow = remove_prompt("Fluxo", &saved, &live, 1, false);
    assert_eq!(flow.lines[1], "Removing it changes the layout to 1 chart.");
    let labels = RemoveAnswer::ALL.map(RemoveAnswer::label);
    assert_eq!(labels, ["Minimize instead", "Cancel", "Remove"]);
}

#[test]
fn seats_tie_each_slot_to_its_built_chart_both_ways() {
    let mut state = canvas("time+time+flow");
    let mut seats = Seats::default();
    assert_eq!(slot_seat(&state, &mut seats, 0), Some(Some(0)));
    assert_eq!(slot_seat(&state, &mut seats, 1), Some(Some(1)));
    assert_eq!(slot_seat(&state, &mut seats, 2), Some(None), "the flow");
    assert_eq!(slot_seat(&state, &mut seats, 9), None);
    state.remove(0).unwrap();
    assert_eq!(
        seat_slot(&state, &mut seats, Some(1)),
        Some(1),
        "removing a chart never moves another"
    );
    assert_eq!(seat_slot(&state, &mut seats, Some(0)), None);
    assert_eq!(seat_slot(&state, &mut seats, None), Some(2));
}

/// A slot added after a removal sits on the chart the removal freed, and that
/// chart is handed back to be built again: never the removed chart's look.
#[test]
fn a_slot_added_after_a_removal_reseeds_the_freed_chart() {
    const AREA: CanvasRect = CanvasRect::new(0.0, 0.0, 1200.0, 800.0);
    let mut state = canvas("time+time+flow");
    let mut seats = Seats::default();
    assert_eq!(slot_seat(&state, &mut seats, 0), Some(Some(0)));
    assert_eq!(seats.take_reseed(), None, "first seats are fresh charts");
    state.remove(0).unwrap();
    let added = state.add(PaneKind::Time, AREA).unwrap();
    assert_eq!(slot_seat(&state, &mut seats, added), Some(Some(0)));
    assert_eq!(seats.take_reseed(), Some(0), "the freed chart is reseeded");
    assert_eq!(seats.take_reseed(), None, "once");
    assert_eq!(slot_seat(&state, &mut seats, 1), Some(Some(1)));

    // A picked layout seats its charts afresh, on purpose: nothing reseeds.
    state.remove(added).unwrap();
    let _ = slot_seat(&state, &mut seats, 1);
    seats.clear();
    state.apply_preset(preset("time+time+flow").unwrap());
    let _ = slot_seat(&state, &mut seats, 1);
    assert_eq!(seats.take_reseed(), None);
}

#[test]
fn a_chart_pops_out_from_its_menu_and_returns_from_its_window() {
    let mut state = canvas("time+flow");
    assert!(pane_menu(&state, 0).contains(&PaneMenuItem::OpenInNewWindow));
    assert!(!pane_menu(&state, 0).contains(&PaneMenuItem::ReturnToCanvas));
    state
        .pop_out(0, "DISPLAY2", CanvasRect::new(0.0, 0.0, 800.0, 500.0))
        .unwrap();
    let rows = pane_menu(&state, 0);
    let labels: Vec<_> = rows.iter().map(|item| item.label()).collect();
    assert_eq!(labels, ["Return to canvas", "Remove chart"]);
    assert!(
        !pane_menu(&state, 1).contains(&PaneMenuItem::OpenInNewWindow),
        "the last docked chart stays on the canvas"
    );
    for item in [PaneMenuItem::OpenInNewWindow, PaneMenuItem::ReturnToCanvas] {
        assert_eq!(PaneMenuItem::parse(item.id()), Some(item));
    }
}

#[test]
fn the_add_menu_lists_the_registry_and_says_what_can_be_added() {
    let state = canvas("time+flow");
    let rows = add_menu(&state, |widget| widget.id == "big_trades");
    let ids: Vec<_> = rows.iter().map(|(widget, _)| widget.id).collect();
    let registry: Vec<_> = WIDGETS.iter().map(|widget| widget.id).collect();
    assert_eq!(ids, registry, "one row per registry entry, in its order");
    let ready = |id: &str| (rows.iter().find(|(widget, _)| widget.id == id).unwrap().1).is_ok();
    assert!(ready("timeframe-chart"));
    assert!(
        !ready("order-flow"),
        "the canvas already holds the order flow"
    );
    assert!(ready("big_trades"));
    assert!(!ready("depth"), "a surface its feed cannot serve");
    let mut full = canvas("grid4+flow");
    while full
        .add(PaneKind::Time, CanvasRect::new(0.0, 0.0, 800.0, 600.0))
        .is_ok()
    {}
    assert!(
        add_menu(&full, |_| true)[0].1.is_err(),
        "a full canvas adds no chart"
    );
    assert!(widget("depth").is_some_and(|widget| widget.kind.is_none()));
}

/// C14: a docked chart's menu offers Float; a floating chart's offers a Dock
/// row per canvas edge and no Float; the last docked chart cannot float.
#[test]
fn menus_offer_float_and_dock() {
    let mut state = canvas("time+flow");
    assert!(pane_menu(&state, 0).contains(&PaneMenuItem::Float));
    state
        .float(0, CanvasRect::new(0.0, 0.0, 400.0, 300.0))
        .unwrap();
    let rows = pane_menu(&state, 0);
    let labels: Vec<_> = rows.iter().map(|item| item.label()).collect();
    assert_eq!(
        labels,
        [
            "Dock left",
            "Dock right",
            "Dock top",
            "Dock bottom",
            "Open in new window",
            "Remove chart"
        ]
    );
    assert!(
        !pane_menu(&state, 1).contains(&PaneMenuItem::Float),
        "the last docked chart stays docked"
    );
    for item in PaneMenuItem::ALL {
        assert_eq!(PaneMenuItem::parse(item.id()), Some(item));
    }
    assert_eq!(
        menu_hook("0:dock_left"),
        Some((0, Some(PaneMenuItem::Dock(Edge::Left))))
    );
}
