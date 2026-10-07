//! The menus' one apply site: an intent in, the pane's state out. No egui
//! frame runs here — the menus' clicks are covered where they are drawn; this
//! pins what each answer does once the pane applies it.

use quantick_orderflow::LaneWindow;

use super::*;
use crate::drawings::DrawingId;
use crate::pane::menus::PaneMenuIntent;

fn tool(id: &str) -> drawings::DrawingTool {
    drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .unwrap_or_else(|| panic!("the {id} tool is registered"))
}

fn pane_with_rectangle() -> (ChartPane, DrawingId) {
    let rectangle = tool("rectangle");
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    pane.drawings.place(rectangle, ChartPoint::at(1.0, 100.0));
    pane.drawings.place(rectangle, ChartPoint::at(5.0, 110.0));
    pane.drawings.select(None);
    assert_eq!(pane.drawings.items().len(), 1);
    let id = pane.drawings.items()[0].id;
    (pane, id)
}

fn apply(pane: &mut ChartPane, intents: Vec<PaneMenuIntent>) {
    with_chrome(Tool::Pointer, |chrome| {
        pane.apply_menu_intents(intents, chrome)
    });
}

/// A strategy riding `drawing`, armed straight on the anchors.
fn arm_strategy(pane: &mut ChartPane, drawing: DrawingId) {
    let instance = crate::strategy_anchors::AnchoredInstance {
        drawing,
        preset: "BF".to_owned(),
        spec: crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Sell),
        armed: quantick_strategy::ArmedStrategy::new(
            quantick_strategy::StrategyParams {
                side: quantick_engine::Side::Sell,
                quantity: rust_decimal::Decimal::ONE,
                tp_mult: rust_decimal::Decimal::ONE,
                sl_mult: rust_decimal::Decimal::ONE,
                rearm: quantick_strategy::Rearm::OneShot,
                on_break: quantick_strategy::BreakPolicy::Ignore,
                execution: quantick_strategy::Execution::Paper,
            },
            Box::new(quantick_strategy::ForceTrigger::new(
                quantick_strategy::ForceParams::default_band(),
            )),
        ),
        alarm: None,
        cue: crate::audio::Cue::default(),
        mark: crate::strategy_anchors::AlarmMark::Quiet,
    };
    assert!(pane.strategies.anchors.arm(instance).is_empty());
}

fn strategy_state(pane: &ChartPane, drawing: DrawingId) -> Option<quantick_strategy::ArmedState> {
    pane.strategies
        .anchors
        .for_drawing(drawing)
        .map(|instance| instance.armed.state().clone())
}

#[test]
fn the_drawing_section_intents_write_the_drawing() {
    let (mut pane, id) = pane_with_rectangle();
    apply(
        &mut pane,
        vec![
            PaneMenuIntent::SelectDrawing(id),
            PaneMenuIntent::RenameDrawing {
                id,
                name: "range high".to_owned(),
            },
            PaneMenuIntent::SetDrawingHidden { id, hidden: true },
            PaneMenuIntent::SetDrawingLocked { id, locked: true },
        ],
    );
    let drawing = &pane.drawings.items()[0];
    assert_eq!(pane.drawings.selected(), Some(0));
    assert_eq!(drawing.name.as_deref(), Some("range high"));
    assert!(drawing.hidden);
    assert!(drawing.locked);
}

#[test]
fn delete_removes_an_unlocked_drawing_and_spares_a_locked_one() {
    let (mut pane, id) = pane_with_rectangle();
    arm_strategy(&mut pane, id);
    apply(
        &mut pane,
        vec![
            PaneMenuIntent::SetDrawingLocked { id, locked: true },
            PaneMenuIntent::DeleteDrawing(id),
        ],
    );
    assert_eq!(
        pane.drawings.items().len(),
        1,
        "a locked object never deletes by accident"
    );
    assert!(strategy_state(&pane, id).is_some());

    apply(
        &mut pane,
        vec![
            PaneMenuIntent::SetDrawingLocked { id, locked: false },
            PaneMenuIntent::DeleteDrawing(id),
        ],
    );
    assert!(pane.drawings.items().is_empty());
    assert!(
        strategy_state(&pane, id).is_none(),
        "the strategy dies with its drawing"
    );
}

#[test]
fn intents_for_a_drawing_that_is_gone_do_nothing() {
    let (mut pane, id) = pane_with_rectangle();
    apply(&mut pane, vec![PaneMenuIntent::DeleteDrawing(id)]);
    assert!(pane.drawings.items().is_empty());
    // Every drawing intent at the stale id: no panic, nothing written.
    apply(
        &mut pane,
        vec![
            PaneMenuIntent::SelectDrawing(id),
            PaneMenuIntent::RenameDrawing {
                id,
                name: "ghost".to_owned(),
            },
            PaneMenuIntent::SetDrawingLocked { id, locked: true },
            PaneMenuIntent::SetDrawingHidden { id, hidden: true },
            PaneMenuIntent::DeleteDrawing(id),
            PaneMenuIntent::StrategyAdd(id),
            PaneMenuIntent::StrategyDisarm(id),
            PaneMenuIntent::StrategyRearm(id),
            PaneMenuIntent::StrategyRemove(id),
        ],
    );
    assert!(pane.drawings.items().is_empty());
    assert_eq!(pane.drawings.selected(), None);
    assert_eq!(pane.strategies.popup_request, None);
}

#[test]
fn a_closing_menu_commits_the_rename_in_flight_and_only_a_changed_one() {
    let (mut pane, id) = pane_with_rectangle();

    pane.context_menu.drawing = Some(id);
    pane.context_menu.rename = String::new();
    assert!(
        pane.context_menu.close(&pane.drawings).is_none(),
        "an untouched name asks for nothing"
    );
    assert!(
        pane.context_menu.drawing.is_none(),
        "closing lets go of the drawing"
    );

    pane.context_menu.drawing = Some(id);
    pane.context_menu.rename = "vwap anchor".to_owned();
    let commit = pane.context_menu.close(&pane.drawings);
    apply(&mut pane, commit.into_iter().collect());
    assert_eq!(
        pane.drawings.items()[0].name.as_deref(),
        Some("vwap anchor")
    );
}

#[test]
fn closing_on_a_deleted_drawing_still_empties_the_rename_buffer() {
    let (mut pane, id) = pane_with_rectangle();
    pane.context_menu.drawing = Some(id);
    pane.context_menu.rename = "half typed".to_owned();
    apply(&mut pane, vec![PaneMenuIntent::DeleteDrawing(id)]);
    assert!(pane.context_menu.close(&pane.drawings).is_none());
    assert!(pane.context_menu.rename.is_empty());
}

#[test]
fn a_layer_intent_is_the_layer_setter() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let style = crate::style::ChartStyle::default();
    let layer = ChartLayer::PointerPrice;
    let before = pane.layer_visible(layer, &style);
    apply(
        &mut pane,
        vec![PaneMenuIntent::SetLayerVisible {
            layer,
            visible: !before,
        }],
    );
    assert_eq!(pane.layer_visible(layer, &style), !before);
}

#[test]
fn the_tape_intents_write_the_lanes_own_fields() {
    let mut pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let window = LaneWindow::Fixed { ms: 60_000 };
    let ignore = !pane
        .orderflow
        .as_ref()
        .expect("a flow pane has a tape")
        .ignore_flow_opening();
    apply(
        &mut pane,
        vec![
            PaneMenuIntent::SetLaneWindow(window),
            PaneMenuIntent::SetIgnoreFlowOpening(ignore),
        ],
    );
    let orderflow = pane.orderflow.as_ref().expect("still a flow pane");
    assert_eq!(orderflow.live_lane_window(), window);
    assert_eq!(orderflow.ignore_flow_opening(), ignore);
}

#[test]
fn a_place_arms_the_pointer_and_opens_the_caret_as_before() {
    let mut pane = pane_of_seconds(20);
    with_chrome(Tool::Crosshair, |chrome| {
        pane.apply_menu_intent(
            PaneMenuIntent::Place {
                tool: tool("text"),
                point: ChartPoint::at(3.0, 100.0),
            },
            chrome,
        );
        assert_eq!(chrome.toolrail.tool(), Tool::Pointer, "one-shot placing");
        assert!(*chrome.begin_text_edit, "a text object wants the caret");
    });
    assert_eq!(pane.drawings.items().len(), 1);
}

#[test]
fn chrome_intents_raise_the_hosts_flags() {
    let mut pane = pane_with_indicator("rsi", vec![vec![1.0, 2.0]]);
    let slot = pane.indicators.all()[0].slot;
    with_chrome(Tool::Pointer, |chrome| {
        pane.apply_menu_intent(PaneMenuIntent::OpenFootprintSettings, chrome);
        assert!(chrome.layers.open_footprint_settings);

        pane.apply_menu_intent(PaneMenuIntent::ToggleIndicatorHidden(slot), chrome);
        assert!(chrome.layers.indicators_changed);

        pane.apply_menu_intent(PaneMenuIntent::ObjectsAsk(Box::default()), chrome);
        assert_eq!(
            chrome.drawing_chrome.menu_target(),
            Some(pane.id),
            "the ask is tagged with the pane the menu belongs to"
        );
    });
    assert!(pane.indicators.all()[0].hidden);
}

#[test]
fn strategy_intents_drive_the_instance_on_the_drawing() {
    use quantick_strategy::ArmedState;
    let (mut pane, id) = pane_with_rectangle();

    apply(&mut pane, vec![PaneMenuIntent::StrategyAdd(id)]);
    assert_eq!(pane.strategies.popup_request, Some(id));

    arm_strategy(&mut pane, id);
    apply(&mut pane, vec![PaneMenuIntent::StrategyDisarm(id)]);
    assert!(matches!(
        strategy_state(&pane, id),
        Some(ArmedState::Disarmed { .. })
    ));

    apply(&mut pane, vec![PaneMenuIntent::StrategyRearm(id)]);
    assert!(matches!(strategy_state(&pane, id), Some(ArmedState::Armed)));

    apply(&mut pane, vec![PaneMenuIntent::StrategyRemove(id)]);
    assert!(strategy_state(&pane, id).is_none());
}
