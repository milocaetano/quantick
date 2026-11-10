//! One tab's canvas: docking, folds, removal, floating panes and the saved
//! comparison.

use super::*;
use crate::workspace_book::WorkspaceBook;
use crate::workspace_recipe::{DeclaredLayout, WorkspaceRecipe};

const CANVAS: CanvasRect = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);

fn canvas(id: &str) -> CanvasState {
    CanvasState::from_preset(preset(id).unwrap())
}

#[test]
fn left_edge_dock_turns_one_tf_flow_into_two_tf_flow() {
    let mut state = canvas("time+flow");
    let slot = state.add(PaneKind::Time, CANVAS).unwrap();
    assert_eq!(slot, 2, "a new pane takes a slot nobody held");
    assert_eq!(state.mode(slot), Some(PaneMode::Floating));
    assert_eq!(
        state.preset().unwrap().id,
        "time+flow",
        "floating panes leave the tree alone"
    );

    assert_eq!(state.dock(slot, Edge::Left), Ok(true));
    assert_eq!(state.mode(slot), Some(PaneMode::Docked));
    assert_eq!(matching_preset(state.tree()).unwrap().label, "2 TF + Flow");
    let rects = state.rects(CANVAS);
    let left = rects.iter().find(|(held, _)| *held == slot).unwrap().1;
    assert_eq!(left.x, 0.0, "the docked pane is the leftmost");

    // A dock the catalogue has no name for is a custom arrangement.
    let mut custom = canvas("time+flow");
    let slot = custom.add(PaneKind::Time, CANVAS).unwrap();
    custom.dock(slot, Edge::Bottom).unwrap();
    assert_eq!(matching_preset(custom.tree()), None);
    assert_eq!(custom.preset(), None);

    // Only a floating pane docks.
    assert_eq!(
        state.dock(0, Edge::Left),
        Err(CanvasRefusal::NotFloating(0))
    );
    assert_eq!(
        state.dock(42, Edge::Left),
        Err(CanvasRefusal::UnknownPane(42))
    );
}

#[test]
fn pane_transitions_and_remove_confirmation() {
    let mut state = canvas("time+time+flow");
    let flow = 2;

    // Minimize folds to a strip; the others share the rest.
    assert_eq!(state.minimize(0), Ok(true));
    assert_eq!(state.minimize(0), Ok(false), "a repeat changes nothing");
    assert_eq!(state.mode(0), Some(PaneMode::Minimized));
    let rects = state.rects(CANVAS);
    assert_eq!(rects[0].1.w, MINIMIZED_STRIP_PX);
    let spent: f32 = rects.iter().map(|(_, rect)| rect.w).sum();
    assert!(
        (spent - CANVAS.w).abs() < 1e-2,
        "the row still spends the canvas"
    );
    assert_eq!(state.minimize(1), Ok(true));
    assert_eq!(
        state.minimize(flow),
        Err(CanvasRefusal::LastOpenPane),
        "one chart stays unfolded"
    );
    assert_eq!(state.restore(1), Ok(true));
    assert_eq!(state.restore(1), Ok(false));

    // Maximize fills the canvas until restored; it unfolds a folded pane.
    assert_eq!(state.maximize(0), Ok(true));
    assert_eq!(state.mode(0), Some(PaneMode::Maximized));
    assert_eq!(state.rects(CANVAS), vec![(0, CANVAS)]);
    assert_eq!(state.maximize(0), Ok(false));
    assert_eq!(state.restore(0), Ok(true));
    assert_eq!(state.mode(0), Some(PaneMode::Docked));
    assert_eq!(state.rects(CANVAS).len(), 3);
    assert_eq!(state.restore(9), Err(CanvasRefusal::UnknownPane(9)));
    assert_eq!(state.minimize(9), Err(CanvasRefusal::UnknownPane(9)));

    // Removing the last flow lays the rest out as a TIMEFRAMES preset.
    let saved = state.clone();
    assert!(
        remove_needs_confirm(&saved, &state, flow),
        "the save holds the flow pane"
    );
    assert_eq!(state.remove(flow), Ok(true));
    assert!(!state.holds(flow));
    assert_eq!(state.preset().unwrap().group, PresetGroup::Timeframes);
    assert_eq!(state.preset().unwrap().label, "2 charts");
    for id in [
        "stack2+flow",
        "stack3+flow",
        "grid4+flow",
        "flow-over-time",
        "flow-over-2time",
    ] {
        let mut with_flow = canvas(id);
        let flow = with_flow
            .tree()
            .panes()
            .iter()
            .find(|(_, kind)| *kind == PaneKind::Flow)
            .unwrap()
            .0;
        with_flow.remove(flow).unwrap();
        let now = with_flow
            .preset()
            .unwrap_or_else(|| panic!("{id} left a custom canvas"));
        assert_eq!(now.group, PresetGroup::Timeframes, "{id}");
        assert_eq!(now.kinds.len(), preset(id).unwrap().kinds.len() - 1, "{id}");
    }

    // The last docked chart stays; a chart the save does not hold goes quietly.
    let mut single = canvas("flow");
    assert_eq!(single.remove(0), Err(CanvasRefusal::LastDockedPane));
    let added = single.add(PaneKind::Time, CANVAS).unwrap();
    assert!(!remove_needs_confirm(&canvas("flow"), &single, added));
    assert!(remove_needs_confirm(&canvas("flow"), &single, 0));
    assert_eq!(single.remove(added), Ok(true));
    assert!(!single.holds(added));
}

#[test]
fn floating_panes_park_pop_out_and_return() {
    let mut state = canvas("time+flow");
    assert_eq!(
        state.float(0, CanvasRect::new(10.0, 10.0, 300.0, 200.0)),
        Ok(true)
    );
    assert_eq!(state.mode(0), Some(PaneMode::Floating));
    assert_eq!(state.preset().unwrap().id, "flow");
    assert_eq!(
        state.float(1, CANVAS),
        Err(CanvasRefusal::LastDockedPane),
        "the canvas keeps a docked chart"
    );

    assert_eq!(state.park(0, Corner::BottomRight, CANVAS), Ok(true));
    let FloatHost::Canvas { rect } = &state.floating()[0].host else {
        panic!("a parked pane floats over the canvas");
    };
    assert_eq!(
        *rect,
        park_rect(CANVAS, Corner::BottomRight, (300.0, 200.0))
    );
    assert_eq!(state.park(0, Corner::BottomRight, CANVAS), Ok(false));
    assert_eq!(
        state.park(1, Corner::TopLeft, CANVAS),
        Err(CanvasRefusal::NotFloating(1))
    );

    let window = CanvasRect::new(2000.0, 100.0, 800.0, 600.0);
    assert_eq!(state.pop_out(0, "DISPLAY2", window), Ok(true));
    assert_eq!(state.mode(0), Some(PaneMode::OsWindow));
    assert_eq!(
        state.park(0, Corner::TopLeft, CANVAS),
        Err(CanvasRefusal::NotFloating(0))
    );
    assert_eq!(state.return_to_canvas(0, CANVAS), Ok(true));
    assert_eq!(state.mode(0), Some(PaneMode::Floating));
    assert_eq!(
        state.return_to_canvas(0, CANVAS),
        Err(CanvasRefusal::NotInOsWindow(0))
    );

    // Picking a layout re-docks by kind; floating panes keep floating.
    assert!(state.apply_preset(preset("time+flow").unwrap()));
    assert_eq!(state.mode(0), Some(PaneMode::Floating));
    let docked: Vec<_> = state.tree().panes();
    assert_eq!(docked.len(), 2);
    assert!(
        docked.iter().any(|(slot, _)| *slot == 1),
        "the flow pane kept its slot"
    );
    assert!(
        !docked.iter().any(|(slot, _)| *slot == 0),
        "a new time pane took a new slot"
    );

    // The canvas holds at most nine charts.
    let mut full = canvas("charts-8");
    full.add(PaneKind::Flow, CANVAS).unwrap();
    assert_eq!(full.add(PaneKind::Time, CANVAS), Err(CanvasRefusal::Full));
}

#[test]
fn canvas_state_round_trips_and_marks_dirty() {
    // A workspace whose canvas changes reads unsaved; saving clears it.
    let mut book = WorkspaceBook::default();
    book.set_autosave(false);
    book.open_tab(1);
    let mut recipe = WorkspaceRecipe {
        layout: DeclaredLayout::TimeAndFlow,
        canvas: Some(canvas("time+flow")),
        ..WorkspaceRecipe::default()
    };
    book.observe(1, &recipe).unwrap();
    book.save_as(1, "Scalp").unwrap();
    assert!(!book.differs(1));
    assert_eq!(book.saved_fields(1).1, None);

    let mut edited = recipe.canvas.clone().unwrap();
    edited.minimize(0).unwrap();
    recipe.canvas = Some(edited);
    assert_eq!(book.observe(1, &recipe), Ok(true));
    assert!(book.differs(1), "a canvas edit marks the workspace unsaved");
    assert!(book.tab_state(1).unwrap().modified);
    assert_eq!(
        book.saved_fields(1).1.as_ref(),
        Some(&recipe),
        "the edit is kept with the tab"
    );
    assert!(
        book.unsaved_edits(1)
            .unwrap()
            .changes
            .contains(&"panes".to_owned()),
        "the close prompt names the canvas"
    );
    book.save_to(1).unwrap();
    assert!(!book.differs(1), "saving clears it");
    assert_eq!(book.recipe("Scalp").unwrap().canvas, recipe.canvas);

    // Floating and OS-window panes round-trip through the saved file.
    let mut state = canvas("time+flow");
    let floating = state.add(PaneKind::Time, CANVAS).unwrap();
    let popped = state.add(PaneKind::Flow, CANVAS).unwrap();
    state
        .pop_out(
            popped,
            "DISPLAY2",
            CanvasRect::new(1920.0, 0.0, 800.0, 600.0),
        )
        .unwrap();
    state.minimize(0).unwrap();
    let saved = WorkspaceRecipe {
        canvas: Some(state.clone()),
        ..WorkspaceRecipe::default()
    };
    let text = toml::to_string_pretty(&saved).unwrap();
    let read: WorkspaceRecipe = toml::from_str(&text).unwrap();
    assert_eq!(read, saved, "{text}");
    let read = read.canvas.unwrap();
    assert_eq!(read.mode(floating), Some(PaneMode::Floating));
    assert_eq!(read.mode(popped), Some(PaneMode::OsWindow));
    assert_eq!(read.mode(0), Some(PaneMode::Minimized));

    // A row layout written before the canvas model loads unchanged.
    let old = concat!(
        "layout = \"time+flow\"\n",
        "split_fraction = 0.35\n",
        "context_collapsed = false\n",
        "flow_collapsed = false\n",
        "context_heights = []\n",
        "context_panes_collapsed = []\n",
        "flow_indicators = []\n",
        "context_panes = []\n",
        "\n[layers]\n",
    );
    let read: WorkspaceRecipe = toml::from_str(old).unwrap();
    assert_eq!(read.canvas, None);
    assert_eq!(read.layout, DeclaredLayout::TimeAndFlow);
    assert_eq!(read.split_fraction, Some(0.35));
    assert_eq!(read.canvas_state(), canvas("time+flow"));
    let written = toml::to_string_pretty(&read).unwrap();
    assert!(
        !written.contains("canvas"),
        "an old recipe is written back as it was"
    );
    assert_eq!(toml::from_str::<WorkspaceRecipe>(&written).unwrap(), read);
    for layout in [
        DeclaredLayout::Flow,
        DeclaredLayout::Time,
        DeclaredLayout::TimeAndFlow,
        DeclaredLayout::TimeTimeAndFlow,
    ] {
        let recipe = WorkspaceRecipe {
            layout,
            ..WorkspaceRecipe::default()
        };
        assert_eq!(
            recipe.canvas_state().preset().unwrap().id,
            layout.preset_id()
        );
    }
}

/// A plain save numbers its slots from its preset; a live tab that reached
/// the same layout another way numbers them as it went. The confirmation
/// follows the pane, not the number.
#[test]
fn remove_confirmation_follows_the_pane_not_its_slot_number() {
    let saved = canvas("time+flow");
    let mut live = canvas("time");
    live.apply_preset(preset("flow").unwrap());
    live.apply_preset(preset("time+flow").unwrap());
    let slot_of = |kind| {
        (live.panes().into_iter())
            .find(|(_, held, _)| *held == kind)
            .unwrap()
            .0
    };
    let (time, flow) = (slot_of(PaneKind::Time), slot_of(PaneKind::Flow));
    assert!(!saved.holds(time), "the numberings differ");
    assert!(remove_needs_confirm(&saved, &live, time), "the saved chart");
    assert!(remove_needs_confirm(&saved, &live, flow), "the saved flow");
    let added = live.add(PaneKind::Time, CANVAS).unwrap();
    assert!(!remove_needs_confirm(&saved, &live, added), "a new chart");
    assert!(!remove_needs_confirm(&saved, &live, 77), "no such pane");
    assert_eq!(pane_identity(&live, added), Some((PaneKind::Time, 1)));
}

/// A chart saved in its own window on a monitor that is gone comes back
/// over the canvas; one on a monitor still there stays out.
#[test]
fn a_window_on_a_gone_monitor_falls_back_to_the_canvas() {
    let mut state = canvas("time+time+flow");
    let rect = CanvasRect::new(2000.0, 100.0, 900.0, 600.0);
    state.pop_out(0, "DISPLAY2", rect).unwrap();
    state.pop_out(1, "DISPLAY1", rect).unwrap();
    let back = state.settle_monitors(&["DISPLAY1".to_owned()], CANVAS);
    assert_eq!(back, vec![0]);
    assert_eq!(state.mode(0), Some(PaneMode::Floating));
    assert_eq!(state.mode(1), Some(PaneMode::OsWindow));
    let Some(FloatHost::Canvas { rect: landed }) = state.host(0) else {
        panic!("back over the canvas");
    };
    assert_eq!((landed.w, landed.h), (900.0, 600.0), "its size kept");
    assert_eq!(landed.centred_in(CANVAS), *landed, "centred");
    assert!(
        state
            .settle_monitors(&["DISPLAY1".to_owned()], CANVAS)
            .is_empty()
    );
}
