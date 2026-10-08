//! The menus' one apply site: an intent in, the pane's state out — and, for
//! the drawing section, the intent a real click on the menu produces.

use quantick_orderflow::LaneWindow;

use super::*;
use crate::drawings::DrawingId;
use crate::pane::menus::{PaneMenuHosts, PaneMenuIntent};

const SCREEN: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 700.0));

/// One frame of the layer menu, drawn the way the pane draws it; returns the
/// intents it answered with, unapplied.
fn menu_frame(
    pane: &mut ChartPane,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> Vec<PaneMenuIntent> {
    with_chrome(Tool::Pointer, |chrome| {
        let mut intents = Vec::new();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(SCREEN),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (menu, model, view) = pane.menu_parts(chrome.capabilities, chrome.style);
                    let hosts = PaneMenuHosts {
                        paper: &mut *chrome.paper,
                    };
                    intents.extend(menu.draw_layer_menu(ui, &view, model, hosts));
                });
            },
        );
        intents
    })
}

/// Lay the menu out, then click the drawing-section entry named `label`.
fn click_entry(pane: &mut ChartPane, ctx: &egui::Context, label: &str) -> Vec<PaneMenuIntent> {
    let _ = menu_frame(pane, ctx, Vec::new());
    let pos = pane
        .context_menu
        .menu_rects
        .iter()
        .find(|(entry, _)| *entry == label)
        .unwrap_or_else(|| panic!("{label} is offered"))
        .1
        .center();
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    menu_frame(
        pane,
        ctx,
        vec![egui::Event::PointerMoved(pos), button(true), button(false)],
    )
}

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

    pane.model.menu.drawing = Some(id);
    pane.model.menu.rename = String::new();
    assert!(
        pane.context_menu
            .close(&mut pane.model, &pane.drawings)
            .is_empty(),
        "an untouched name asks for nothing"
    );
    assert!(
        pane.model.menu.drawing.is_none(),
        "closing lets go of the drawing"
    );

    pane.model.menu.drawing = Some(id);
    pane.model.menu.rename = "vwap anchor".to_owned();
    let commit = pane.context_menu.close(&mut pane.model, &pane.drawings);
    apply(&mut pane, commit.into_iter().collect());
    assert_eq!(
        pane.drawings.items()[0].name.as_deref(),
        Some("vwap anchor")
    );
}

#[test]
fn the_menus_delete_lets_go_of_the_drawing_and_its_half_typed_name() {
    let ctx = egui::Context::default();
    let (mut pane, id) = pane_with_rectangle();
    pane.model.menu.drawing = Some(id);
    pane.model.menu.rename = "half typed".to_owned();

    let intents = click_entry(&mut pane, &ctx, "Delete");
    assert!(matches!(
        intents.as_slice(),
        [PaneMenuIntent::DeleteDrawing(deleted)] if *deleted == id
    ));
    apply(&mut pane, intents);
    assert!(pane.drawings.items().is_empty());
    assert_eq!(pane.model.menu.drawing, None);
    assert!(pane.model.menu.rename.is_empty());
    assert!(
        pane.context_menu
            .close(&mut pane.model, &pane.drawings)
            .is_empty()
    );
}

#[test]
fn a_drawing_deleted_under_the_open_menu_drops_its_section_and_name() {
    let ctx = egui::Context::default();
    let (mut pane, id) = pane_with_rectangle();
    pane.model.menu.drawing = Some(id);
    pane.model.menu.rename = "half typed".to_owned();
    assert!(pane.drawings.remove_by_id(id));

    assert!(menu_frame(&mut pane, &ctx, Vec::new()).is_empty());
    assert_eq!(pane.model.menu.drawing, None);
    assert!(pane.model.menu.rename.is_empty());
    assert!(
        pane.context_menu.menu_rects.is_empty(),
        "no section for a ghost"
    );
}

#[test]
fn the_strategy_seat_answers_each_click_with_its_intent() {
    let ctx = egui::Context::default();
    let (mut pane, id) = pane_with_rectangle();
    pane.model.menu.drawing = Some(id);
    arm_strategy(&mut pane, id);

    let disarm = click_entry(&mut pane, &ctx, "Disarm");
    assert!(matches!(
        disarm.as_slice(),
        [PaneMenuIntent::StrategyDisarm(drawing)] if *drawing == id
    ));
    apply(&mut pane, disarm);

    let rearm = click_entry(&mut pane, &ctx, "Re-arm");
    assert!(matches!(
        rearm.as_slice(),
        [PaneMenuIntent::StrategyRearm(drawing)] if *drawing == id
    ));

    let remove = click_entry(&mut pane, &ctx, "Remove strategy");
    assert!(matches!(
        remove.as_slice(),
        [PaneMenuIntent::StrategyRemove(drawing)] if *drawing == id
    ));
}

#[test]
fn strategy_intents_leave_an_unswept_strategy_alone_once_its_drawing_is_gone() {
    use quantick_strategy::ArmedState;
    let (mut pane, id) = pane_with_rectangle();
    arm_strategy(&mut pane, id);
    // Removed without the orphan sweep: the instance is still on the anchors.
    assert!(pane.drawings.remove_by_id(id));

    apply(
        &mut pane,
        vec![
            PaneMenuIntent::StrategyDisarm(id),
            PaneMenuIntent::StrategyRearm(id),
            PaneMenuIntent::StrategyRemove(id),
        ],
    );
    assert!(matches!(strategy_state(&pane, id), Some(ArmedState::Armed)));
}

#[test]
fn an_unchanged_rename_blur_records_nothing() {
    let ctx = egui::Context::default();
    let (mut pane, id) = pane_with_rectangle();
    pane.model.menu.drawing = Some(id);
    pane.model.menu.rename = String::new();
    // Focus the field, then click away from it: the blur of an untouched
    // name asks for no rename, so no undo step is recorded.
    let _ = click_entry(&mut pane, &ctx, "Rename");
    let away = egui::pos2(390.0, 690.0);
    let button = |pressed| egui::Event::PointerButton {
        pos: away,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let blur = menu_frame(
        &mut pane,
        &ctx,
        vec![egui::Event::PointerMoved(away), button(true), button(false)],
    );
    assert!(
        !blur
            .iter()
            .any(|intent| matches!(intent, PaneMenuIntent::RenameDrawing { .. })),
        "an unchanged blur asks for no rename"
    );

    // The control: the same focus-and-blur with a changed name does rename,
    // so the silence above is the guard's, not a blur that never happened.
    pane.model.menu.rename = "new name".to_owned();
    let _ = click_entry(&mut pane, &ctx, "Rename");
    let blur = menu_frame(
        &mut pane,
        &ctx,
        vec![egui::Event::PointerMoved(away), button(true), button(false)],
    );
    assert!(
        blur.iter()
            .any(|intent| matches!(intent, PaneMenuIntent::RenameDrawing { name, .. } if name == "new name")),
        "a changed blur renames"
    );
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
                tool: tool("text").id(),
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

        pane.apply_menu_intent(PaneMenuIntent::ToggleIndicatorHidden(slot.0), chrome);
        assert!(chrome.layers.indicators_changed);

        pane.apply_menu_intent(
            PaneMenuIntent::ObjectsAsk(quantick_chart_interaction::pane::ObjectAction::DeleteAll),
            chrome,
        );
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
