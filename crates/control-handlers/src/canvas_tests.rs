//! The canvas capabilities through a fake canvas: every action is registered,
//! reaches the host as one validated act, and answers a result its own output
//! schema accepts.

use crate::canvas::{CanvasAct, CanvasHost, register};
use quantick_control::error::ControlError;
use quantick_control::wire::CanonicalDecimal;
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
                width_share: Some(CanonicalDecimal::new("0.6").unwrap()),
                height_share: None,
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
        (
            RESIZE_CAPABILITY_ID,
            json!({"slot": 0, "dimension": "width", "share": "0.6"}),
            Some(CanvasAct::Resize {
                slot: 0,
                dimension: CanvasDimension::Width,
                share: 0.6,
            }),
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
        (
            RESIZE_CAPABILITY_ID,
            json!({"slot": 0, "dimension": "width", "share": "0"}),
        ),
        (
            RESIZE_CAPABILITY_ID,
            json!({"slot": 0, "dimension": "width", "share": "1"}),
        ),
        (
            RESIZE_CAPABILITY_ID,
            json!({"slot": 0, "dimension": "height", "share": "0.1234567"}),
        ),
        (
            RESIZE_CAPABILITY_ID,
            json!({"slot": 0, "dimension": "depth", "share": "0.5"}),
        ),
        (
            RESIZE_CAPABILITY_ID,
            json!({"slot": 0, "dimension": "width", "share": "-0.5"}),
        ),
        (RESIZE_CAPABILITY_ID, json!({"slot": 0, "share": "0.5"})),
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

/// What a held chart's drop, its window and the launch hooks ask for, as the
/// acts the capabilities take.
#[test]
fn drops_windows_and_hooks_become_canvas_acts() {
    use crate::canvas::{
        PRIMARY_MONITOR, drop_act, drop_targets, held_by_hook, hook_acts, moved, placed_pop_out,
        settled, surface_widget, window_act,
    };
    use quantick_control_schema::canvas::{CanvasCorner, CanvasEdge};
    use quantick_workspace::canvas::{
        CanvasRect, CanvasState, Corner, DropOutcome, Edge, PaneKind, preset,
    };
    let area = CanvasRect::new(0.0, 40.0, 1200.0, 800.0);
    let named = |_: CanvasRect| Some("DISPLAY2".to_owned());
    let dock = drop_act(3, DropOutcome::Dock(Edge::Left), (0.0, 0.0), named);
    let edge = CanvasEdge::Left;
    assert_eq!(dock, CanvasAct::Dock { slot: 3, edge });
    let park = drop_act(3, DropOutcome::Park(Corner::TopRight), (0.0, 0.0), named);
    let corner = CanvasCorner::TopRight;
    assert_eq!(park, CanvasAct::Park { slot: 3, corner });
    let past = CanvasRect::new(1300.0, 100.0, 400.0, 300.0);
    let CanvasAct::PopOut { monitor, rect, .. } =
        drop_act(3, DropOutcome::PopOut(past), (100.0, 50.0), named)
    else {
        panic!("past the window's edge it pops out");
    };
    assert_eq!(monitor.as_deref(), Some("DISPLAY2"));
    let rect = rect.expect("its place");
    assert_eq!(
        (rect.x, rect.y),
        (1400.0, 150.0),
        "moved by the window's origin"
    );

    let closed = window_act(3, (None, true));
    assert_eq!(closed, Some(CanvasAct::Return { slot: 3 }));
    assert_eq!(window_act(3, (None, false)), None);
    let landed = Some((past, Some("DISPLAY2".to_owned())));
    let CanvasAct::PopOut { monitor, .. } = window_act(3, (landed, false)).unwrap() else {
        panic!("a moved window keeps the chart out");
    };
    assert_eq!(
        monitor.as_deref(),
        Some("DISPLAY2"),
        "on the monitor now holding it"
    );
    assert!(moved(past, CanvasRect::new(1300.0, 101.5, 400.0, 300.0)));
    assert!(!moved(past, CanvasRect::new(1300.4, 100.0, 400.0, 300.0)));
    // A drag saves once, when the window stands still.
    let (saved, mut seen) = (CanvasRect::new(0.0, 0.0, 400.0, 300.0), past);
    let step = CanvasRect::new(1310.0, 100.0, 400.0, 300.0);
    assert_eq!(settled(step, &mut seen, saved), None, "still moving");
    assert_eq!(settled(step, &mut seen, saved), Some(step), "settled");
    assert_eq!(
        settled(saved, &mut seen, saved),
        None,
        "back where it was saved"
    );

    let mut state = CanvasState::from_preset(preset("time+flow").unwrap());
    let menu = CanvasAct::PopOut {
        slot: 0,
        monitor: None,
        rect: None,
    };
    let CanvasAct::PopOut {
        rect: Some(rect), ..
    } = placed_pop_out(menu, (&state, area), (10.0, 20.0), named)
    else {
        panic!("a pop-out from the menu opens where the chart stands");
    };
    assert_eq!((rect.x, rect.y), (10.0, 60.0));
    assert!(drop_targets(&state, area).is_empty(), "nothing floats");
    let added = state.add(PaneKind::Time, area).unwrap();
    let targets = drop_targets(&state, area);
    assert_eq!(targets.len(), 8, "four edges dock, four corners park");
    assert!(
        targets
            .iter()
            .any(|(id, ..)| id == "canvas.park.bottom_right")
    );
    let (held, share) = held_by_hook(&state, &format!("{added}:0.02,0.5")).unwrap();
    assert_eq!((held.slot(), share), (added, (0.02, 0.5)));
    assert!(
        held_by_hook(&state, "0:0.5,0.5").is_none(),
        "a docked chart is not held"
    );

    let acts = hook_acts("add:timeframe-chart; dock:2:left;park:2:bottom_right;pop_out:2;nonsense");
    assert_eq!(acts.len(), 4, "{acts:?}");
    assert_eq!(
        acts[1],
        CanvasAct::Dock {
            slot: 2,
            edge: CanvasEdge::Left
        }
    );
    let depth = CanvasAct::AddWidget {
        widget_id: "depth".to_owned(),
    };
    assert_eq!(surface_widget(&depth), Some("depth"));
    assert_eq!(
        surface_widget(&acts[0]),
        None,
        "a chart lands on the canvas"
    );
    assert_eq!(PRIMARY_MONITOR, "primary");
}

/// A resize reaches the tree's shares through the model the drag moves, reads
/// back as the share sent, and a refusal names its stable reason.
#[test]
fn a_resize_moves_the_tree_share_and_reads_it_back() {
    use crate::canvas::{Drawn, act, hook_acts, result};
    use quantick_control_schema::canvas::CanvasDimension;
    use quantick_workspace::canvas::{CanvasRect, CanvasState, preset};
    let area = Drawn {
        area: CanvasRect::new(0.0, 0.0, 1600.0, 900.0),
    };
    let mut state = CanvasState::from_preset(preset("charts-2").unwrap());
    let resize = |slot, dimension, share| CanvasAct::Resize {
        slot,
        dimension,
        share,
    };
    let wide = resize(0, CanvasDimension::Width, 0.7);
    assert_eq!(
        act(&mut state, None, area, wide.clone()),
        Ok((true, Some(0)))
    );
    let read = result(&state, true, Some(0), true, area);
    let share = |slot: usize| {
        read.panes[slot]
            .width_share
            .as_ref()
            .map(|s| s.as_str().to_owned())
    };
    assert_eq!(share(0).as_deref(), Some("0.7"));
    assert_eq!(share(1).as_deref(), Some("0.3"));
    assert_eq!(
        read.panes[0].height_share, None,
        "no split lays them top to bottom"
    );
    assert_eq!(
        read.preset_id.as_deref(),
        Some("charts-2"),
        "still the layout"
    );
    assert_eq!(
        act(&mut state, None, area, wide),
        Ok((false, Some(0))),
        "a repeat"
    );

    let refusal = |error: ControlError| error.context.details.unwrap()["refusal"].clone();
    let tall = resize(0, CanvasDimension::Height, 0.5);
    let error = act(&mut state, None, area, tall).unwrap_err();
    assert_eq!(refusal(error), json!("canvas.no_seam"));
    let error = act(
        &mut state,
        None,
        area,
        resize(9, CanvasDimension::Width, 0.5),
    )
    .unwrap_err();
    assert_eq!(refusal(error), json!("canvas.unknown_pane"));

    let acts = hook_acts("resize:0:width:0.25;resize:0:width:1.5;resize:0:depth:0.5");
    assert_eq!(acts, vec![resize(0, CanvasDimension::Width, 0.25)]);
}

/// C16: "1 TF + Flow" is drawn by the column painter, whose split is the
/// tree's row share: the same resize a tree layout takes moves it, and the
/// readback shows the share sent.
#[test]
fn resize_works_on_a_column_layout() {
    use crate::canvas::{Drawn, act, view};
    use quantick_control_schema::canvas::CanvasDimension;
    use quantick_workspace::canvas::{CanvasRect, CanvasState, preset};
    let drawn = Drawn {
        area: CanvasRect::new(0.0, 0.0, 1600.0, 900.0),
    };
    let mut state = CanvasState::from_preset(preset("time+flow").unwrap());
    let resize = CanvasAct::Resize {
        slot: 0,
        dimension: CanvasDimension::Width,
        share: 0.6,
    };
    assert_eq!(act(&mut state, None, drawn, resize), Ok((true, Some(0))));
    let read = view(&state, drawn);
    let width = |slot: usize| {
        read.panes[slot]
            .width_share
            .as_ref()
            .map(|share| share.as_str().to_owned())
    };
    assert_eq!(width(0).as_deref(), Some("0.6"));
    assert_eq!(width(1).as_deref(), Some("0.4"));
    assert_eq!(read.preset_id.as_deref(), Some("time+flow"));
}

/// C19: with nine panes on the canvas, floats counted, a pick that needs one
/// more is refused with `canvas.full` and leaves the canvas as it was.
#[test]
fn a_pick_over_the_pane_cap_is_refused() {
    use crate::canvas::{Drawn, act};
    use quantick_workspace::canvas::{CanvasRect, CanvasState, MAX_CANVAS_PANES, PaneKind, preset};
    let area = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
    let mut state = CanvasState::from_preset(preset("flow").unwrap());
    while state.add(PaneKind::Flow, area).is_ok() {}
    assert_eq!(state.panes().len(), MAX_CANVAS_PANES);
    let before = state.clone();
    let pick = CanvasAct::PickLayout {
        preset_id: "charts-8".to_owned(),
    };
    let error = act(&mut state, None, Drawn { area }, pick).unwrap_err();
    assert_eq!(
        error.context.details.unwrap()["refusal"],
        json!("canvas.full")
    );
    assert_eq!(state, before, "nothing changed");
    let same = CanvasAct::PickLayout {
        preset_id: "flow".to_owned(),
    };
    assert_eq!(
        act(&mut state, None, Drawn { area }, same),
        Ok((false, None)),
        "the layout it already is reads as unchanged, not refused"
    );
}

/// The readback is what the screen shows: a folded chart's strip and the
/// chart beside it, and nothing while a chart is maximized.
#[test]
fn a_tree_share_reads_back_what_the_screen_shows() {
    use crate::canvas::{Drawn, view};
    use quantick_workspace::canvas::{CanvasRect, CanvasState, preset};
    let drawn = Drawn {
        area: CanvasRect::new(0.0, 0.0, 1600.0, 900.0),
    };
    let mut state = CanvasState::from_preset(preset("charts-2").unwrap());
    state.minimize(1).unwrap();
    let read = view(&state, drawn);
    let width = |slot: usize| {
        read.panes[slot]
            .width_share
            .as_ref()
            .map(|share| share.as_str().to_owned())
    };
    assert_eq!(width(0).as_deref(), Some("0.985"), "1576 of 1600 points");
    assert_eq!(width(1).as_deref(), Some("0.015"), "the 24-point strip");
    let mut state = CanvasState::from_preset(preset("charts-2").unwrap());
    state.maximize(0).unwrap();
    let read = view(&state, drawn);
    assert!(
        read.panes.iter().all(|pane| pane.width_share.is_none()),
        "a maximized chart sits in no split"
    );
}
