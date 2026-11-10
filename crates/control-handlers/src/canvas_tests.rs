//! The canvas capabilities through a fake canvas: every action is registered,
//! reaches the host as one validated act, and answers a result its own output
//! schema accepts.

use crate::canvas::{CanvasAct, CanvasHost, register};
use quantick_control::error::ControlError;
use quantick_control_host::actions::ActionRegistry;
use quantick_control_host::authority::CAPABILITY_VERSION;
use quantick_control_schema::canvas::{
    CANVAS_CAPABILITY_IDS, CanvasPaneDto, CanvasPaneKind, CanvasPaneMode, CanvasRectDto,
    CanvasResult,
};
use serde_json::{Value, json};

#[derive(Default)]
struct FakeCanvas {
    acts: Vec<CanvasAct>,
    menu_opened: usize,
}

impl CanvasHost for FakeCanvas {
    fn open_window_menu(&mut self) -> Result<bool, ControlError> {
        self.menu_opened += 1;
        Ok(true)
    }

    fn act_on_canvas(&mut self, act: CanvasAct) -> Result<CanvasResult, ControlError> {
        self.acts.push(act);
        Ok(CanvasResult {
            changed: true,
            slot: Some(2),
            preset_id: Some("time+time+flow".to_owned()),
            unsaved: true,
            panes: vec![CanvasPaneDto {
                slot: 2,
                kind: CanvasPaneKind::Time,
                mode: CanvasPaneMode::Docked,
            }],
        })
    }
}

/// A host that lends nothing: every call is refused by the port's defaults.
struct NoCanvas;
impl CanvasHost for NoCanvas {}

fn rect() -> CanvasRectDto {
    CanvasRectDto {
        x: 10.0,
        y: 20.0,
        w: 300.0,
        h: 200.0,
    }
}

/// One valid call per capability, with the act it must reach the host as.
fn calls() -> Vec<(&'static str, Value, Option<CanvasAct>)> {
    use quantick_control_schema::canvas::*;
    vec![
        (OPEN_WINDOW_MENU_CAPABILITY_ID, json!({}), None),
        (
            PICK_LAYOUT_CAPABILITY_ID,
            json!({"preset_id": "charts-4"}),
            Some(CanvasAct::PickLayout {
                preset_id: "charts-4".to_owned(),
            }),
        ),
        (
            MINIMIZE_CAPABILITY_ID,
            json!({"slot": 1}),
            Some(CanvasAct::Minimize { slot: 1 }),
        ),
        (
            MAXIMIZE_CAPABILITY_ID,
            json!({"slot": 1}),
            Some(CanvasAct::Maximize { slot: 1 }),
        ),
        (
            RESTORE_CAPABILITY_ID,
            json!({"slot": 1}),
            Some(CanvasAct::Restore { slot: 1 }),
        ),
        (
            REMOVE_CAPABILITY_ID,
            json!({"slot": 1, "confirm": true}),
            Some(CanvasAct::Remove {
                slot: 1,
                confirm: true,
            }),
        ),
        (
            ADD_WIDGET_CAPABILITY_ID,
            json!({"widget_id": "timeframe-chart"}),
            Some(CanvasAct::AddWidget {
                widget_id: "timeframe-chart".to_owned(),
            }),
        ),
        (
            FLOAT_CAPABILITY_ID,
            json!({"slot": 1, "rect": {"x": 10.0, "y": 20.0, "w": 300.0, "h": 200.0}}),
            Some(CanvasAct::Float {
                slot: 1,
                rect: Some(rect()),
            }),
        ),
        (
            DOCK_CAPABILITY_ID,
            json!({"slot": 2, "edge": "left"}),
            Some(CanvasAct::Dock {
                slot: 2,
                edge: CanvasEdge::Left,
            }),
        ),
        (
            PARK_CAPABILITY_ID,
            json!({"slot": 2, "corner": "bottom_right"}),
            Some(CanvasAct::Park {
                slot: 2,
                corner: CanvasCorner::BottomRight,
            }),
        ),
        (
            POP_OUT_CAPABILITY_ID,
            json!({"slot": 2, "monitor": "DISPLAY2"}),
            Some(CanvasAct::PopOut {
                slot: 2,
                monitor: Some("DISPLAY2".to_owned()),
                rect: None,
            }),
        ),
        (
            RETURN_CAPABILITY_ID,
            json!({"slot": 2}),
            Some(CanvasAct::Return { slot: 2 }),
        ),
    ]
}

#[test]
fn every_canvas_action_is_registered_and_readable() {
    let mut registry = ActionRegistry::<FakeCanvas, ()>::new();
    register(&mut registry).unwrap();
    let calls = calls();
    assert_eq!(
        calls.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
        CANVAS_CAPABILITY_IDS,
        "one call per canvas capability"
    );
    let actor = crate::test_support::FakeWindow::assistant();
    for (id, input, act) in calls {
        let action = registry
            .lookup(id, CAPABILITY_VERSION)
            .unwrap_or_else(|| panic!("{id} is not registered"));
        assert!(!action.descriptor.title.is_empty(), "{id}");
        assert!(!action.descriptor.read_only, "{id} acts");
        action
            .input
            .validate(&input)
            .unwrap_or_else(|error| panic!("{id}: {error:?}"));

        let mut host = FakeCanvas::default();
        let result = (action.handler)(&mut host, &mut (), &actor, &input).unwrap();
        action
            .output
            .validate(&result)
            .unwrap_or_else(|error| panic!("{id} answered outside its schema: {error:?}"));
        match act {
            None => {
                assert_eq!(host.menu_opened, 1, "{id}");
                assert_eq!(result, json!({"open": true}));
            }
            Some(act) => {
                assert_eq!(host.acts, vec![act], "{id}");
                let read: CanvasResult = serde_json::from_value(result).unwrap();
                assert!(read.changed && read.unsaved, "{id}");
                assert_eq!(read.preset_id.as_deref(), Some("time+time+flow"));
                assert_eq!(read.panes.len(), 1);
            }
        }

        // A host without a canvas refuses rather than pretending.
        let mut nothing = ActionRegistry::<NoCanvas, ()>::new();
        register(&mut nothing).unwrap();
        let refused = nothing.lookup(id, CAPABILITY_VERSION).unwrap();
        assert!(
            (refused.handler)(&mut NoCanvas, &mut (), &actor, &input).is_err(),
            "{id}"
        );
    }
    assert_eq!(registry.descriptors().count(), CANVAS_CAPABILITY_IDS.len());
}

#[test]
fn malformed_canvas_calls_never_reach_the_canvas() {
    use quantick_control_schema::canvas::*;
    let mut registry = ActionRegistry::<FakeCanvas, ()>::new();
    register(&mut registry).unwrap();
    let actor = crate::test_support::FakeWindow::assistant();
    let mut host = FakeCanvas::default();
    for (id, input) in [
        (MINIMIZE_CAPABILITY_ID, json!({"slot": 1, "tab_id": "1"})),
        (MINIMIZE_CAPABILITY_ID, json!({"slot": -1})),
        (DOCK_CAPABILITY_ID, json!({"slot": 1, "edge": "middle"})),
        (PARK_CAPABILITY_ID, json!({"slot": 1})),
        (
            FLOAT_CAPABILITY_ID,
            json!({"slot": 1, "rect": {"x": 0.0, "y": 0.0, "w": 0.0, "h": 10.0}}),
        ),
        (POP_OUT_CAPABILITY_ID, json!({"slot": 1, "monitor": "  "})),
        (PICK_LAYOUT_CAPABILITY_ID, json!({})),
        (OPEN_WINDOW_MENU_CAPABILITY_ID, json!({"at": "kebab"})),
    ] {
        let action = registry.lookup(id, CAPABILITY_VERSION).unwrap();
        assert!(
            (action.handler)(&mut host, &mut (), &actor, &input).is_err(),
            "{id} took {input}"
        );
    }
    assert!(host.acts.is_empty());
    assert_eq!(host.menu_opened, 0);
}

/// A menu row takes the capability's own act; Remove asks first only for a
/// chart the save holds; Esc restores a maximized chart while no dialog is
/// up; the dialog's answers act or do nothing.
#[test]
fn menu_rows_and_dialog_answers_take_the_capabilities_acts() {
    use crate::canvas::{confirmed, menu_step};
    use quantick_workspace::canvas::{CanvasState, PaneMenuItem, RemoveAnswer, preset};
    let saved = CanvasState::from_preset(preset("time+flow").unwrap());
    let mut live = saved.clone();
    let mut removing = None;
    let step = |picked, escape, live: &CanvasState, removing: &mut Option<u32>| {
        menu_step(picked, escape, removing, live, Some(&saved))
    };
    let minimize = Some((0, PaneMenuItem::Minimize));
    let act = step(minimize, false, &live, &mut removing);
    assert_eq!(act, Some(CanvasAct::Minimize { slot: 0 }));
    let restore = Some((0, PaneMenuItem::Restore));
    let act = step(restore, false, &live, &mut removing);
    assert_eq!(act, Some(CanvasAct::Restore { slot: 0 }));
    assert_eq!(
        step(None, true, &live, &mut removing),
        None,
        "nothing to restore"
    );
    live.maximize(1).unwrap();
    let act = step(None, true, &live, &mut removing);
    assert_eq!(act, Some(CanvasAct::Restore { slot: 1 }), "Esc restores");

    let remove = Some((0, PaneMenuItem::Remove));
    assert_eq!(step(remove, false, &live, &mut removing), None);
    assert_eq!(removing, Some(0), "a saved chart asks first");
    assert_eq!(
        step(None, true, &live, &mut removing),
        None,
        "Esc is the dialog's"
    );
    let area = quantick_workspace::canvas::CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
    let added = live
        .add(quantick_workspace::canvas::PaneKind::Time, area)
        .unwrap();
    let quiet = Some(CanvasAct::Remove {
        slot: added,
        confirm: true,
    });
    let remove_added = Some((added, PaneMenuItem::Remove));
    assert_eq!(step(remove_added, false, &live, &mut removing), quiet);
    assert_eq!(
        menu_step(remove_added, false, &mut None, &live, None),
        quiet
    );
    live.remove(0).unwrap();
    assert_eq!(step(None, false, &live, &mut removing), None);
    assert_eq!(removing, None, "a dialog over a gone chart drops");
    let mut asked = Some(1);
    assert_eq!(menu_step(None, false, &mut asked, &live, None), None);
    assert_eq!(asked, None, "a dialog with no save behind it drops");

    let minimized = Some(CanvasAct::Minimize { slot: 0 });
    assert_eq!(confirmed(RemoveAnswer::MinimizeInstead, 0), minimized);
    assert_eq!(confirmed(RemoveAnswer::Cancel, 0), None);
    let removed = Some(CanvasAct::Remove {
        slot: 0,
        confirm: true,
    });
    assert_eq!(confirmed(RemoveAnswer::Remove, 0), removed);
}
