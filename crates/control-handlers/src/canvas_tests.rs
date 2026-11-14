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
                railed: false,
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
                target: quantick_workspace::canvas::DockTarget::Canvas(
                    quantick_workspace::canvas::Edge::Left,
                ),
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
        (
            quantick_control_schema::canvas::SEAM_DRAG_CAPABILITY_ID,
            json!({"path": [0], "index": 0, "travel": "-1600"}),
            Some(CanvasAct::SeamDrag {
                path: vec![0],
                index: 0,
                travel: -1600.0,
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
        (DOCK_CAPABILITY_ID, json!({"slot": 1})),
        (DOCK_CAPABILITY_ID, json!({"slot": 1, "beside": 0})),
        (
            DOCK_CAPABILITY_ID,
            json!({"slot": 1, "edge": "left", "swap_with": 0}),
        ),
        (
            DOCK_CAPABILITY_ID,
            json!({"slot": 1, "beside": 0, "side": "top", "swap_with": 0}),
        ),
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
    use quantick_control_schema::canvas::CanvasCorner;
    use quantick_workspace::canvas::{
        CanvasRect, CanvasState, Corner, DockTarget, DropOutcome, Edge, PaneKind, preset,
    };
    let area = CanvasRect::new(0.0, 40.0, 1200.0, 800.0);
    let named = |_: CanvasRect| Some("DISPLAY2".to_owned());
    let target = DockTarget::Beside {
        slot: 0,
        side: Edge::Top,
    };
    let dock = drop_act(3, DropOutcome::Dock(target), (0.0, 0.0), named);
    assert_eq!(dock, CanvasAct::Dock { slot: 3, target });
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
    assert!(
        drop_targets(&state, area, None).is_empty(),
        "nothing floats"
    );
    let held = drop_targets(&state, area, Some(0));
    let ids: Vec<_> = held.iter().map(|(id, ..)| id.as_str()).collect();
    assert!(
        ids.contains(&"canvas.dock.swap.1"),
        "a held chart sees the targets: {ids:?}"
    );
    assert!(
        !ids.iter()
            .any(|id| id.ends_with(".0") || id.contains(".0.")),
        "never its own: {ids:?}"
    );
    let added = state.add(PaneKind::Time, area).unwrap();
    let targets = drop_targets(&state, area, None);
    assert_eq!(
        targets.len(),
        8 + 2 * 5,
        "four edges dock, four corners park, each docked chart offers four sides and a centre"
    );
    let ids: Vec<_> = targets.iter().map(|(id, ..)| id.as_str()).collect();
    for id in [
        "canvas.park.bottom_right",
        "canvas.dock.beside.1.bottom",
        "canvas.dock.swap.0",
    ] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let (held, share) = held_by_hook(&state, &format!("{added}:0.02,0.5")).unwrap();
    assert_eq!((held.slot(), share), (added, (0.02, 0.5)));
    assert!(
        held_by_hook(&state, "0:0.5,0.5").is_none(),
        "a docked chart is not held"
    );

    let acts = hook_acts(
        "add:timeframe-chart; dock:2:left;park:2:bottom_right;pop_out:2;nonsense;dock:2:beside:0:top;dock:2:swap:1;dock:2:beside:0",
    );
    assert_eq!(acts.len(), 6, "{acts:?}");
    let dock = |target| CanvasAct::Dock { slot: 2, target };
    assert_eq!(acts[1], dock(DockTarget::Canvas(Edge::Left)));
    let beside = DockTarget::Beside {
        slot: 0,
        side: Edge::Top,
    };
    assert_eq!(acts[4], dock(beside));
    assert_eq!(acts[5], dock(DockTarget::Swap(1)));
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
        columns: false,
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
/// readback shows the share sent. Its stacked heights stay its own.
#[test]
fn resize_works_on_a_column_layout() {
    use crate::canvas::{Drawn, act, view};
    use quantick_control_schema::canvas::CanvasDimension;
    use quantick_workspace::canvas::{CanvasRect, CanvasState, preset};
    let drawn = Drawn {
        area: CanvasRect::new(0.0, 0.0, 1600.0, 900.0),
        columns: true,
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

    // The stacked charts' heights are the column painter's own: a height
    // resize is refused, naming the call that moves them, and none reads back.
    let mut stacked = CanvasState::from_preset(preset("stack2+flow").unwrap());
    let before = stacked.clone();
    let tall = CanvasAct::Resize {
        slot: 0,
        dimension: CanvasDimension::Height,
        share: 0.7,
    };
    let error = act(&mut stacked, None, drawn, tall).unwrap_err();
    assert_eq!(
        error.context.details.unwrap()["refusal"],
        json!("canvas.column_layout")
    );
    assert!((error.context.next_steps.iter()).any(|step| step.contains("layout.pane.resize_pair")));
    assert_eq!(stacked, before, "nothing moved");
    let read = view(&stacked, drawn);
    assert!(read.panes.iter().all(|pane| pane.height_share.is_none()));
}

/// C19: with nine panes on the canvas, floats counted, a pick that needs one
/// more is refused with `canvas.full` and leaves the canvas as it was.
#[test]
fn a_pick_over_the_pane_cap_is_refused() {
    use crate::canvas::{Drawn, act};
    use quantick_workspace::canvas::{CanvasRect, CanvasState, MAX_CANVAS_PANES, PaneKind, preset};
    let area = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
    // One flow pane at most, so the floats are charts: a pick of a flow
    // layout needs a flow pane the canvas does not hold.
    let mut state = CanvasState::from_preset(preset("time").unwrap());
    while state.add(PaneKind::Time, area).is_ok() {}
    assert_eq!(state.panes().len(), MAX_CANVAS_PANES);
    let before = state.clone();
    let pick = CanvasAct::PickLayout {
        preset_id: "grid4+flow".to_owned(),
    };
    let error = act(
        &mut state,
        None,
        Drawn {
            area,
            columns: false,
        },
        pick,
    )
    .unwrap_err();
    assert_eq!(
        error.context.details.unwrap()["refusal"],
        json!("canvas.full")
    );
    assert_eq!(state, before, "nothing changed");
    let same = CanvasAct::PickLayout {
        preset_id: "time".to_owned(),
    };
    assert_eq!(
        act(
            &mut state,
            None,
            Drawn {
                area,
                columns: false
            },
            same
        ),
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
        columns: false,
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

/// A host that acts on a real canvas, as the window does.
struct LiveCanvas {
    state: quantick_workspace::canvas::CanvasState,
    drawn: crate::canvas::Drawn,
}

impl CanvasHost for LiveCanvas {
    fn act_on_canvas(&mut self, act: CanvasAct) -> Result<CanvasResult, ControlError> {
        let (changed, slot) = crate::canvas::act(&mut self.state, None, self.drawn, act)?;
        Ok(crate::canvas::result(
            &self.state,
            changed,
            slot,
            false,
            self.drawn,
        ))
    }
}

/// C13: a docked chart floated, then docked beside a named chart over the
/// control plane, reads back docked and stands beside it; a swap names its
/// partner, and a target that is not docked is refused by its stable code.
#[test]
fn dock_beside_a_named_pane_round_trips() {
    use quantick_control_schema::canvas::{DOCK_CAPABILITY_ID, FLOAT_CAPABILITY_ID};
    use quantick_workspace::canvas::{CanvasRect, CanvasState, preset};
    let area = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
    let mut host = LiveCanvas {
        state: CanvasState::from_preset(preset("time+time+flow").unwrap()),
        drawn: crate::canvas::Drawn {
            area,
            columns: false,
        },
    };
    let mut registry = ActionRegistry::<LiveCanvas, ()>::new();
    register(&mut registry).unwrap();
    let actor = crate::test_support::FakeWindow::assistant();
    let call = |host: &mut LiveCanvas, id: &str, input: Value| {
        let action = registry.lookup(id, CAPABILITY_VERSION).unwrap();
        action.input.validate(&input).unwrap();
        (action.handler)(host, &mut (), &actor, &input)
    };
    let mode = |read: &Value, slot: u32| {
        let panes = read["panes"].as_array().unwrap().iter();
        let pane = panes.clone().find(|pane| pane["slot"] == slot).unwrap();
        pane["mode"].as_str().unwrap().to_owned()
    };
    let read = call(&mut host, FLOAT_CAPABILITY_ID, json!({"slot": 0})).unwrap();
    assert_eq!(mode(&read, 0), "floating");
    let beside = json!({"slot": 0, "beside": 2, "side": "bottom"});
    let read = call(&mut host, DOCK_CAPABILITY_ID, beside).unwrap();
    assert_eq!(mode(&read, 0), "docked");
    assert_eq!(read["slot"], 0);
    let rects = host.state.rects(area);
    let at = |held| rects.iter().find(|(slot, _)| *slot == held).unwrap().1;
    let (flow, docked) = (at(2), at(0));
    assert_eq!((docked.x, docked.w), (flow.x, flow.w), "under the flow");
    assert_eq!(flow.bottom(), docked.y, "beside the named chart, below it");

    call(&mut host, FLOAT_CAPABILITY_ID, json!({"slot": 1})).unwrap();
    let swap = json!({"slot": 1, "swap_with": 2});
    let read = call(&mut host, DOCK_CAPABILITY_ID, swap).unwrap();
    assert_eq!(
        (mode(&read, 1), mode(&read, 2)),
        ("docked".to_owned(), "floating".to_owned())
    );
    let gone = json!({"slot": 2, "beside": 9, "side": "left"});
    let error = call(&mut host, DOCK_CAPABILITY_ID, gone).unwrap_err();
    assert_eq!(
        error.context.details.unwrap()["refusal"],
        json!("canvas.unknown_pane")
    );
}

/// The add a second order flow would make is refused by its stable code,
/// and no pane is added.
#[test]
fn a_second_flow_is_refused_over_the_control_plane() {
    use crate::canvas::{Drawn, act};
    use quantick_workspace::canvas::{CanvasRect, CanvasState, preset};
    let drawn = Drawn {
        area: CanvasRect::new(0.0, 0.0, 1600.0, 900.0),
        columns: false,
    };
    let mut state = CanvasState::from_preset(preset("time+flow").unwrap());
    let before = state.clone();
    let add = CanvasAct::AddWidget {
        widget_id: "order-flow".to_owned(),
    };
    let error = act(&mut state, None, drawn, add).unwrap_err();
    assert_eq!(
        error.context.details.unwrap()["refusal"],
        json!("canvas.second_flow")
    );
    assert_eq!(state, before);
}

/// A dragged ⋯ holds its chart once the pointer has travelled the lift
/// distance: a docked chart stays docked, held at no more than half the
/// canvas centred on the press; a floating one is held where it floats; the
/// last docked chart is not held.
#[test]
fn a_dragged_menu_button_lifts_its_chart() {
    use crate::canvas::lifted;
    use quantick_workspace::canvas::{
        CanvasPoint, CanvasRect, CanvasState, FloatHost, LIFT_PX, PaneKind, PaneMode, preset,
    };
    let area = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
    let mut state = CanvasState::from_preset(preset("time+flow").unwrap());
    let press = Some((300.0, 12.0));
    let still = lifted(
        &state,
        area,
        Some(1),
        press,
        Some((300.0 + LIFT_PX - 1.0, 12.0)),
    );
    assert!(still.is_none(), "a slow click never lifts");
    let held = lifted(&state, area, Some(1), press, Some((330.0, 40.0))).unwrap();
    assert_eq!(held.slot(), 1);
    assert_eq!(
        state.mode(1),
        Some(PaneMode::Docked),
        "it stays docked until the drop"
    );
    let at = held.rect(CanvasPoint::new(300.0, 12.0));
    assert_eq!((at.w, at.h), (800.0, 450.0), "half the canvas at most");
    assert_eq!((at.x + at.w / 2.0, at.y + at.h / 2.0), (300.0, 12.0));
    assert!(lifted(&state, area, None, press, press).is_none());
    let mut folded = CanvasState::from_preset(preset("time+time+flow").unwrap());
    folded.minimize(0).unwrap();
    let far = Some((300.0, 80.0));
    assert!(
        lifted(&folded, area, Some(0), press, far).is_none(),
        "a folded strip is not lifted"
    );

    let added = state.add(PaneKind::Time, area).unwrap();
    let far = Some((40.0, 40.0));
    let held = lifted(&state, area, Some(added), Some((1.0, 1.0)), far).unwrap();
    let Some(FloatHost::Canvas { rect }) = state.host(added) else {
        panic!("floating");
    };
    assert_eq!(held.rect(CanvasPoint::new(1.0, 1.0)), *rect);
    state.float(0, area).unwrap();
    assert!(
        lifted(&state, area, Some(1), Some((1.0, 1.0)), far).is_none(),
        "the last docked chart stays"
    );
}

/// `canvas.seam.drag` runs the hand's drag: travel past the threshold folds
/// the far chart and names it, exactly at the threshold the chart stays
/// open, travel back from the rail reopens it, and a maximized chart leaves
/// no seam to drag.
#[test]
fn a_seam_drag_call_folds_past_the_threshold_and_reopens_back() {
    use crate::canvas::{Drawn, act};
    use quantick_workspace::canvas::{self, COLLAPSE_AT_PX, CanvasRect, CanvasState};
    let area = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
    let drawn = Drawn {
        area,
        columns: false,
    };
    let mut state = CanvasState::from_preset(canvas::preset("charts-2").unwrap());
    let drag = |travel| CanvasAct::SeamDrag {
        path: Vec::new(),
        index: 0,
        travel,
    };
    let at = 800.0 - COLLAPSE_AT_PX;
    assert_eq!(
        act(&mut state, None, drawn, drag(at)).unwrap(),
        (true, None)
    );
    assert!(state.railed().is_empty(), "exactly at the threshold: open");
    let folded = act(&mut state, None, drawn, drag(1.0)).unwrap();
    assert_eq!(folded, (true, Some(1)), "past it the far chart folds");
    assert!(state.railed().contains(&1));
    let panes = crate::canvas::view(&state, drawn).panes;
    let railed: Vec<_> = (panes.iter())
        .map(|pane| (pane.slot, pane.railed))
        .collect();
    assert_eq!(
        railed,
        [(0, false), (1, true)],
        "the readback names the rail"
    );
    let back = act(&mut state, None, drawn, drag(-400.0)).unwrap();
    assert_eq!(back, (true, Some(1)), "dragged back it reopens");
    assert!(state.railed().is_empty());
    state.maximize(0).unwrap();
    let refused = act(&mut state, None, drawn, drag(10.0)).unwrap_err();
    let code = refused
        .context
        .details
        .as_ref()
        .map(|details| details["refusal"].clone());
    assert_eq!(code, Some(serde_json::json!("canvas.no_seam")));
}
