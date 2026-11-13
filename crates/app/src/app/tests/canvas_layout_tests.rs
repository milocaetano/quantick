//! The canvas draws every registered layout exactly as its pane tree lays it
//! out.
use super::*;

use crate::canvas_layout::{CANVAS_DIVIDER_PX, LAYOUT_PRESETS};
use quantick_workspace::canvas::CanvasRect;

/// `rect` less half a divider on every edge inside `area`: the seam the
/// canvas paints between neighbours.
fn seamed(rect: CanvasRect, area: egui::Rect) -> egui::Rect {
    let rect = egui::Rect::from_min_size(egui::pos2(rect.x, rect.y), egui::vec2(rect.w, rect.h));
    let half = CANVAS_DIVIDER_PX / 2.0;
    let inner = |edge: f32, outer: f32| (edge - outer).abs() > 0.5;
    egui::Rect::from_min_max(
        egui::pos2(
            rect.left()
                + if inner(rect.left(), area.left()) {
                    half
                } else {
                    0.0
                },
            rect.top()
                + if inner(rect.top(), area.top()) {
                    half
                } else {
                    0.0
                },
        ),
        egui::pos2(
            rect.right()
                - if inner(rect.right(), area.right()) {
                    half
                } else {
                    0.0
                },
            rect.bottom()
                - if inner(rect.bottom(), area.bottom()) {
                    half
                } else {
                    0.0
                },
        ),
    )
}

fn sorted(mut rects: Vec<egui::Rect>) -> Vec<[i64; 4]> {
    let key = |value: f32| (value * 100.0).round() as i64;
    let mut keys: Vec<_> = (rects.drain(..))
        .map(|rect| {
            [
                key(rect.left()),
                key(rect.top()),
                key(rect.right()),
                key(rect.bottom()),
            ]
        })
        .collect();
    keys.sort_unstable();
    keys
}

/// C12: every preset, applied the trader's way, draws one band per pane at
/// exactly `PaneTree::rects` of the canvas area.
#[test]
fn canvas_draws_every_preset_exactly() {
    let (mut app, _evt, _cmd, _book) = test_app();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    for preset in LAYOUT_PRESETS {
        app.apply_layout_preset(preset);
        for _ in 0..3 {
            run_frame(&mut app, &ctx);
        }
        let canvas = &app.active_tab().canvas;
        let area = canvas.last_canvas;
        assert!(area.width() > 0.0, "{} drew a canvas", preset.id);
        let whole = CanvasRect::new(area.left(), area.top(), area.width(), area.height());
        let expected = preset
            .tree
            .rects(whole)
            .into_iter()
            .map(|(_, rect)| seamed(rect, area));
        let drawn = canvas.drawn_bands.iter().map(|(_, band)| *band);
        assert_eq!(
            sorted(drawn.collect()),
            sorted(expected.collect()),
            "{} draws its tree",
            preset.id
        );
    }
}

/// Two charts side by side, laid out and settled.
fn two_charts(ctx: &egui::Context) -> QuantickApp {
    let (mut app, _evt, _cmd, _book) = test_app();
    run_frame(&mut app, ctx);
    let preset = (LAYOUT_PRESETS.iter())
        .find(|preset| preset.id == "charts-2")
        .expect("the two-chart preset is registered");
    app.apply_layout_preset(preset);
    for _ in 0..3 {
        run_frame(&mut app, ctx);
    }
    app
}

/// The tree painter fills only its seams with the divider colour, never a
/// chart's header strip, and frames the focused chart in the active-widget
/// border rather than the accent.
#[test]
fn tree_seams_paint_as_dividers_and_headers_stay_chart_chrome() {
    let ctx = egui::Context::default();
    let mut app = two_charts(&ctx);
    let output = run_frame(&mut app, &ctx);
    let palette = crate::theme::get(&ctx);
    let area = app.active_tab().canvas.last_canvas;
    let bands: Vec<egui::Rect> = (app.active_tab().canvas.drawn_bands.iter())
        .map(|(_, band)| *band)
        .collect();
    assert_eq!(bands.len(), 2, "two docked charts");
    let rects = painted_rects(&output);
    let seam_fills: Vec<egui::Rect> = (rects.iter())
        .filter(|shape| shape.fill == palette.border())
        .map(|shape| shape.rect)
        .collect();
    for band in &bands {
        let header = band.left_top() + egui::vec2(band.width() / 2.0, 4.0);
        assert!(
            !seam_fills.iter().any(|fill| fill.contains(header)),
            "no divider fill lies under a chart's header strip at {header:?}"
        );
    }
    assert!(
        seam_fills.iter().any(|fill| {
            (fill.width() - CANVAS_DIVIDER_PX).abs() < 0.01
                && (fill.height() - area.height()).abs() < 0.5
                && fill.left() >= bands[0].left().min(bands[1].left())
                && fill.right() <= bands[0].right().max(bands[1].right())
        }),
        "the seam between the charts is one divider wide and the canvas tall"
    );
    let frame = egui::Stroke::new(crate::tab::FOCUS_FRAME_PX, palette.widget_active_border());
    assert!(
        rects.iter().any(|shape| shape.stroke == frame),
        "the focused chart is framed in the active-widget border"
    );
    let accent = egui::Stroke::new(crate::tab::FOCUS_FRAME_PX, palette.accent());
    assert!(
        !rects.iter().any(|shape| shape.stroke == accent),
        "never in the accent"
    );
}

/// Dragging the seam between two charts moves their shares, as the column
/// painter's divider moves its split.
#[test]
fn dragging_a_tree_seam_resizes_both_charts() {
    use quantick_workspace::canvas::{SplitAxis, tree_seams};
    let ctx = egui::Context::default();
    let mut app = two_charts(&ctx);
    let area = app.active_tab().canvas.last_canvas;
    let whole = CanvasRect::new(area.left(), area.top(), area.width(), area.height());
    let state = &app.active_tab().canvas.state;
    let first = state.tree().panes()[0].0;
    let before = state
        .pane_share(first, SplitAxis::Row, whole)
        .expect("the first chart holds a share of the row");
    let seam = tree_seams(state, whole, CANVAS_DIVIDER_PX)
        .into_iter()
        .next()
        .expect("one seam between the charts");
    assert!(seam.resizable);
    let grab = egui::pos2(
        seam.rect.x + seam.rect.w / 2.0,
        seam.rect.y + seam.rect.h / 2.0,
    );
    drag_chart(&mut app, &ctx, grab, grab + egui::vec2(80.0, 0.0));
    run_frame(&mut app, &ctx);
    let state = &app.active_tab().canvas.state;
    let after = state
        .pane_share(first, SplitAxis::Row, whole)
        .expect("still in the row");
    let moved = (after - before) * area.width();
    assert!(
        (moved - 80.0).abs() < 2.0,
        "the left chart widened by the drag: {before} -> {after} ({moved} px)"
    );
    let bands: Vec<egui::Rect> = (app.active_tab().canvas.drawn_bands.iter())
        .map(|(_, band)| *band)
        .collect();
    let (left, right) = if bands[0].left() < bands[1].left() {
        (bands[0], bands[1])
    } else {
        (bands[1], bands[0])
    };
    assert!(
        left.width() > right.width() + 100.0,
        "both charts redrew at the new boundary: {left:?} {right:?}"
    );
}

/// A seam grabbed off its centre follows the pointer from the grab point, as
/// the column painter's divider does: out and back to where it was pressed
/// leaves the charts exactly as they were, with no first-frame jump.
#[test]
fn a_tree_seam_grabbed_off_centre_moves_only_as_far_as_the_pointer() {
    use quantick_workspace::canvas::tree_seams;
    let ctx = egui::Context::default();
    let mut app = two_charts(&ctx);
    let area = app.active_tab().canvas.last_canvas;
    let whole = CanvasRect::new(area.left(), area.top(), area.width(), area.height());
    let shares = |app: &QuantickApp| match app.active_tab().canvas.state.tree() {
        quantick_workspace::canvas::PaneTree::Split { shares, .. } => shares.to_vec(),
        quantick_workspace::canvas::PaneTree::Pane { .. } => Vec::new(),
    };
    let before = shares(&app);
    let seam = tree_seams(&app.active_tab().canvas.state, whole, CANVAS_DIVIDER_PX)
        .into_iter()
        .next()
        .expect("one seam between the charts");
    let grab = egui::pos2(
        seam.rect.x + seam.rect.w / 2.0 + 3.0,
        seam.rect.y + seam.rect.h / 2.0,
    );
    for events in [
        vec![egui::Event::PointerMoved(grab), pointer_button(grab, true)],
        vec![egui::Event::PointerMoved(grab + egui::vec2(40.0, 0.0))],
        vec![egui::Event::PointerMoved(grab)],
        vec![egui::Event::PointerMoved(grab), pointer_button(grab, false)],
    ] {
        run_frame_with_events(&mut app, &ctx, events);
    }
    let after = shares(&app);
    let moved = (after[0] - before[0]) / (before[0] + before[1]) * area.width();
    assert!(
        moved.abs() < 0.05,
        "back where it was pressed, unmoved: {before:?} -> {after:?} ({moved} px)"
    );
}

/// "2 TF + Flow", which the tree paints: its seam dragged slowly to the
/// right edge folds the flow pane to the column painter's 8 px rail, the
/// scene names the rail with its restore, the collapse call reads it back,
/// and a drag back from the rail reopens the flow.
#[test]
fn dragging_a_time_time_flow_seam_to_the_edge_folds_the_flow_to_a_rail() {
    use quantick_control_handlers::layout::LayoutTab as _;
    use quantick_workspace::canvas::{COLLAPSED_PANE_WIDTH_PX, PaneKind, tree_rails, tree_seams};
    let ctx = egui::Context::default();
    let (mut app, _evt, _cmd, _book) = test_app();
    run_frame(&mut app, &ctx);
    let preset = (LAYOUT_PRESETS.iter())
        .find(|preset| preset.id == "time+time+flow")
        .expect("the 2 TF + Flow preset is registered");
    app.apply_layout_preset(preset);
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let area = app.active_tab().canvas.last_canvas;
    let whole = CanvasRect::new(area.left(), area.top(), area.width(), area.height());
    let state = &app.active_tab().canvas.state;
    let flow = (state.tree().panes().into_iter())
        .find(|(_, kind)| *kind == PaneKind::Flow)
        .expect("a flow pane")
        .0;
    let seam = (tree_seams(state, whole, CANVAS_DIVIDER_PX).into_iter())
        .find(|seam| seam.address.path.is_empty() && seam.address.index == 1)
        .expect("the seam before the flow pane");
    let grab = egui::pos2(seam.rect.x + seam.rect.w / 2.0, area.center().y);
    let slow = |app: &mut QuantickApp, from: egui::Pos2, to: egui::Pos2| {
        run_frame_with_events(app, &ctx, vec![egui::Event::PointerMoved(from)]);
        run_frame_with_events(
            app,
            &ctx,
            vec![egui::Event::PointerMoved(from), pointer_button(from, true)],
        );
        for step in 1..=24 {
            let at = from.lerp(to, step as f32 / 24.0);
            run_frame_with_events(app, &ctx, vec![egui::Event::PointerMoved(at)]);
        }
        run_frame_with_events(
            app,
            &ctx,
            vec![egui::Event::PointerMoved(to), pointer_button(to, false)],
        );
        run_frame(app, &ctx);
    };
    slow(&mut app, grab, egui::pos2(area.right() - 1.0, grab.y));
    let state = &app.active_tab().canvas.state;
    assert!(
        state.railed().contains(&flow),
        "the flow folded to its rail"
    );
    let rails = tree_rails(state, whole, CANVAS_DIVIDER_PX);
    assert_eq!(rails.len(), 1);
    assert!(
        (rails[0].rect.w - COLLAPSED_PANE_WIDTH_PX).abs() < 0.01,
        "the column painter's rail width: {:?}",
        rails[0].rect
    );
    assert!(
        app.active_tab().reading().flow_collapsed,
        "the layout call reads the fold"
    );
    let scene = crate::control::scene_snapshot(&app);
    let named = format!("pane_menu.{flow}.rail");
    let control = (scene.controls.iter())
        .find(|control| control.control_id == named)
        .expect("the scene names the rail");
    assert_eq!(
        control.capability_id.as_deref(),
        Some(quantick_control_handlers::canvas::item_capability(
            quantick_workspace::canvas::PaneMenuItem::Restore
        ))
    );
    let rail = rails[0].rect;
    let from = egui::pos2(rail.x + rail.w / 2.0, grab.y);
    slow(&mut app, from, egui::pos2(area.center().x, grab.y));
    let state = &app.active_tab().canvas.state;
    assert!(
        state.railed().is_empty(),
        "dragged back past the threshold, open"
    );
    let flow_rect = (state.rects(whole).into_iter())
        .find(|(slot, _)| *slot == flow)
        .expect("the flow pane")
        .1;
    // Pressed `grabbed` points past the seam's centre, it keeps them.
    let grabbed = from.x - (rail.x - CANVAS_DIVIDER_PX / 2.0);
    assert!(
        (flow_rect.x - (area.center().x - grabbed)).abs() < 1.0,
        "it opened under the pointer: {flow_rect:?}"
    );
}

/// A flow pane folded by the column painter stays folded, the same rail,
/// when a floating chart drops the tab onto the tree painter and when that
/// chart leaves; the collapse readback agrees on both painters.
#[test]
fn a_column_fold_survives_the_switch_to_the_tree_painter_and_back() {
    use quantick_control_handlers::layout::LayoutTab as _;
    use quantick_workspace::canvas::{COLLAPSED_PANE_WIDTH_PX, PaneKind, tree_rails};
    let ctx = egui::Context::default();
    let (mut app, _evt, _cmd, _book) = test_app();
    run_frame(&mut app, &ctx);
    let preset = (LAYOUT_PRESETS.iter())
        .find(|preset| preset.id == "time+flow")
        .expect("the 1 TF + Flow preset is registered");
    app.apply_layout_preset(preset);
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    assert!(app.active_tab_mut().canvas.fold(true, true));
    run_frame(&mut app, &ctx);
    assert!(app.active_tab().canvas.column_painted());
    assert!(
        app.active_tab().flow_rail_rect().is_some(),
        "the column rail"
    );
    let area = app.active_tab().canvas.last_canvas;
    let whole = CanvasRect::new(area.left(), area.top(), area.width(), area.height());
    let float = (app.active_tab_mut().canvas.state)
        .add(PaneKind::Time, whole)
        .expect("room for a floating chart");
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let tab = app.active_tab();
    assert!(!tab.canvas.column_painted(), "the tree paints with a float");
    let rails = tree_rails(&tab.canvas.state, whole, CANVAS_DIVIDER_PX);
    assert_eq!(rails.len(), 1, "still folded on the tree");
    assert!((rails[0].rect.w - COLLAPSED_PANE_WIDTH_PX).abs() < 0.01);
    assert!(tab.reading().flow_collapsed && !tab.reading().collapsed);
    (app.active_tab_mut().canvas.state).remove(float).unwrap();
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    assert!(app.active_tab().canvas.column_painted());
    assert!(
        app.active_tab().flow_rail_rect().is_some(),
        "the column rail again"
    );
    assert!(app.active_tab().reading().flow_collapsed);
}
