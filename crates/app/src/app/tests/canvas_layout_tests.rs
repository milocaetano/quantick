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
