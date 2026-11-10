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

/// Removing a chart never moves the charts kept onto another pane: each time
/// slot keeps the pane it was seated on.
#[test]
fn removing_a_chart_keeps_the_others_on_their_panes() {
    use crate::pane::PaneSide;
    use crate::tab::canvas::tree::tree_bands;
    let preset = crate::canvas_layout::preset("stack2+flow").unwrap();
    let mut state = quantick_workspace::canvas::CanvasState::from_preset(preset);
    let mut seats = smallvec::SmallVec::new();
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
    let before = tree_bands(&state, &mut seats, area);
    assert_eq!(
        before
            .iter()
            .filter(|band| band.side == PaneSide::Time(1))
            .count(),
        1
    );
    state.remove(0).unwrap();
    let after = tree_bands(&state, &mut seats, area);
    let sides: Vec<_> = after.iter().map(|band| band.side).collect();
    assert_eq!(
        sides,
        vec![PaneSide::Time(1), PaneSide::Flow],
        "the kept chart stays on pane 1"
    );
}
