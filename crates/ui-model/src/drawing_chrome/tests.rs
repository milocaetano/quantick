use super::*;
use crate::Model as _;
use quantick_chart_interaction::pane::PaneSide;

#[test]
fn migration_preserves_state_and_effects() {
    let mut state = State::default();
    Model::update(&mut state, Intent::InspectorOpen(true));
    Model::update(&mut state, Intent::CarrySelection);
    Model::update(&mut state, Intent::SelectionChanged { changed: true });
    assert!(
        state.core.shared.open,
        "placement carries settings past selection reset"
    );
    Model::update(
        &mut state,
        Intent::InspectorPrepare {
            index: 7,
            width: Some(900.0),
        },
    );
    assert!(
        state.core.shared.pinned,
        "fresh narrow charts use the pinned pass"
    );
    let effects = Model::update(
        &mut state,
        Intent::Actions(ActionInput {
            actions: InspectorActions {
                toggle_pin: true,
                toggle_lock: true,
                toggle_all_windows: true,
                ..Default::default()
            },
            index: 7,
            tab: 13,
            side: PaneSide::Flow,
            before: None,
            gesture_settled: true,
        }),
    );
    assert!(effects.toggle_selected.locked);
    assert!(effects.toggle_selected.all_windows);
    assert!(!state.core.shared.pinned);
    assert!(state.inspector.pin_touched);
    assert!(state.inspector.settle_frame);
    Model::update(
        &mut state,
        Intent::InspectorPrepare {
            index: 7,
            width: Some(900.0),
        },
    );
    assert_eq!(
        state.inspector.prepared, None,
        "unpin waits for settled canvas geometry"
    );
    Model::update(
        &mut state,
        Intent::InspectorPrepare {
            index: 7,
            width: Some(900.0),
        },
    );
    assert_eq!(state.inspector.prepared, Some(true));
    assert!(
        !state.core.shared.pinned,
        "an explicit pin preference survives narrow geometry"
    );
    Model::update(
        &mut state,
        Intent::Menu {
            pane: 42,
            ask: DrawingChromeAsk {
                manager_delete: Some(3),
                ..Default::default()
            },
        },
    );
    Model::update(
        &mut state,
        Intent::Menu {
            pane: 42,
            ask: DrawingChromeAsk {
                manager_delete: Some(9),
                show_all: true,
                ..Default::default()
            },
        },
    );
    let effects = Model::update(&mut state, Intent::TakeMenu(42));
    assert_eq!(effects.manager_delete, Some(3));
    assert!(effects.show_all);
    assert_eq!(
        Model::update(&mut state, Intent::TakeMenu(42)),
        DrawingChromeAsk::default()
    );
}

#[test]
fn the_selection_toggles_ask_the_store_in_order() {
    use quantick_drawings::DrawingIntent;
    let names = |toggles: SelectionToggles| -> Vec<String> {
        toggles
            .intents()
            .map(|intent| format!("{intent:?}"))
            .collect()
    };
    assert!(names(SelectionToggles::default()).is_empty());
    let all = SelectionToggles {
        hidden: true,
        locked: true,
        all_windows: true,
    };
    let expected = [
        DrawingIntent::ToggleHideSelected,
        DrawingIntent::ToggleLockSelected,
        DrawingIntent::ToggleAllWindowsSelected,
    ];
    let expected: Vec<String> = expected.iter().map(|i| format!("{i:?}")).collect();
    assert_eq!(names(all), expected);
    let locked = SelectionToggles {
        locked: true,
        ..SelectionToggles::default()
    };
    assert_eq!(names(locked), ["ToggleLockSelected"]);
}
