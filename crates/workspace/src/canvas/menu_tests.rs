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
    assert_eq!(labels, ["Minimize", "Maximize", "Remove chart"]);
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
    let mut seats = Vec::new();
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
