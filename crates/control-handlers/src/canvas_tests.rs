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
