//! A folded pane's rail paints the lit band (the designers' mock, option 1)
//! on both painters, upright and flat, in Dark and in Light: the band, a
//! 1 px edge along each long side and a 44 px grip, each in the theme's rail
//! tokens, lit only while the pointer is on the rail's hit area. A divider
//! between two open panes is not a rail and paints as before; the rail keeps
//! its 8 px and the pane beside it its place.
use super::*;

use crate::canvas_layout::{CANVAS_DIVIDER_PX, COLLAPSED_PANE_WIDTH_PX, LAYOUT_PRESETS};
use quantick_appearance::{DARK, LIGHT, ResolvedTheme, Token};
use quantick_workspace::canvas::{CanvasRect, SplitAxis, tree_rails, tree_seams};

const AT_REST: [Token; 3] = [Token::RailBand, Token::RailEdge, Token::RailGrip];
const LIT: [Token; 3] = [
    Token::RailBandHover,
    Token::RailEdgeHover,
    Token::RailGripHover,
];

/// `token` as the bundled theme `source` resolves it, in the renderer's bytes.
fn ink(source: &str, token: Token) -> egui::Color32 {
    let [r, g, b, a] = ResolvedTheme::parse(source)
        .unwrap()
        .color(token)
        .to_array();
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
}

fn dress(app: &mut QuantickApp, ctx: &egui::Context, source: &str) {
    app.runtime
        .appearance
        .apply_candidate(source, false)
        .unwrap();
    run_frame(app, ctx);
}

fn egui_rect(at: CanvasRect) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(at.x, at.y), egui::vec2(at.w, at.h))
}

fn near(a: egui::Rect, b: egui::Rect) -> bool {
    (a.min - b.min).length() < 0.01 && (a.max - b.max).length() < 0.01
}

/// Every filled rectangle the frame painted inside `rail`.
fn painted_in(output: &egui::FullOutput, rail: egui::Rect) -> Vec<(egui::Rect, egui::Color32)> {
    (painted_rects(output).into_iter())
        .filter(|shape| rail.expand(0.01).contains_rect(shape.rect))
        .map(|shape| (shape.rect, shape.fill))
        .collect()
}

/// `rail` painted as the lit band, `flat` along a column fold, in
/// `source`'s rail tokens at rest or `lit`.
fn assert_lit_band(
    output: &egui::FullOutput,
    rail: egui::Rect,
    flat: bool,
    source: &str,
    lit: bool,
) {
    let across = if flat { rail.height() } else { rail.width() };
    assert!(
        (across - COLLAPSED_PANE_WIDTH_PX).abs() < 0.01,
        "the rail keeps its 8 px: {rail:?}"
    );
    let [band, edge, grip] = (if lit { LIT } else { AT_REST }).map(|token| ink(source, token));
    let (first, last) = if flat {
        let strip = egui::vec2(rail.width(), 1.0);
        let last = egui::pos2(rail.left(), rail.bottom() - 1.0);
        (
            egui::Rect::from_min_size(rail.min, strip),
            egui::Rect::from_min_size(last, strip),
        )
    } else {
        let strip = egui::vec2(1.0, rail.height());
        let last = egui::pos2(rail.right() - 1.0, rail.top());
        (
            egui::Rect::from_min_size(rail.min, strip),
            egui::Rect::from_min_size(last, strip),
        )
    };
    let size = if flat {
        egui::vec2(44.0, 2.0)
    } else {
        egui::vec2(2.0, 44.0)
    };
    let painted = painted_in(output, rail);
    for (what, at, fill) in [
        ("band", rail, band),
        ("first edge", first, edge),
        ("last edge", last, edge),
        (
            "44 px grip",
            egui::Rect::from_center_size(rail.center(), size),
            grip,
        ),
    ] {
        assert!(
            painted
                .iter()
                .any(|(rect, colour)| near(*rect, at) && *colour == fill),
            "the {what} at {at:?} in {fill:?}; painted {painted:?}"
        );
    }
}

/// The tab on the tree painter at `preset`, three frames in, in `source`.
fn tree_app(ctx: &egui::Context, preset: &str, source: &str) -> QuantickApp {
    let (mut app, _evt, _cmd, _book) = test_app();
    run_frame(&mut app, ctx);
    let preset = (LAYOUT_PRESETS.iter())
        .find(|listed| listed.id == preset)
        .expect("a registered preset");
    app.apply_layout_preset(preset);
    dress(&mut app, ctx, source);
    run_frame(&mut app, ctx);
    app
}

fn whole(app: &QuantickApp) -> CanvasRect {
    let area = app.active_tab().canvas.last_canvas;
    CanvasRect::new(area.left(), area.top(), area.width(), area.height())
}

/// The column painter's two rails, the charts folded left of the flow and
/// the flow folded right beside the sidebar.
#[test]
fn the_column_painters_rails_paint_the_lit_band_in_dark_and_light() {
    for source in [DARK, LIGHT] {
        for flow in [false, true] {
            let ctx = egui::Context::default();
            let (mut app, _commands) = split_app(&ctx, 200);
            dress(&mut app, &ctx, source);
            assert!(app.active_tab_mut().canvas.fold(flow, true));
            run_frame(&mut app, &ctx);
            let output = run_frame(&mut app, &ctx);
            let rail = app.active_tab().collapsed_rail_rect().expect("a rail");
            assert_lit_band(&output, rail, false, source, false);
        }
    }
}

/// The tree painter's rails: the flow at the window's right edge, the whole
/// context side of grid4+flow behind one rail, and the flow folded flat
/// above the chart it stood over.
#[test]
fn the_tree_painters_rails_paint_the_lit_band_upright_and_flat_in_dark_and_light() {
    for source in [DARK, LIGHT] {
        for (preset, flow) in [
            ("time+time+flow", true),
            ("grid4+flow", false),
            ("flow-over-time", true),
        ] {
            let ctx = egui::Context::default();
            let mut app = tree_app(&ctx, preset, source);
            assert!(app.active_tab_mut().canvas.state.fold_flow_side(flow, true));
            run_frame(&mut app, &ctx);
            let output = run_frame(&mut app, &ctx);
            let rails = tree_rails(
                &app.active_tab().canvas.state,
                whole(&app),
                CANVAS_DIVIDER_PX,
            );
            assert_eq!(rails.len(), 1, "{preset}: one rail");
            let flat = rails[0].axis == SplitAxis::Column;
            assert_eq!(flat, preset == "flow-over-time", "{preset}");
            assert_lit_band(&output, egui_rect(rails[0].rect), flat, source, false);
        }
    }
}

/// The rail lights only while the pointer is on its hit area, which reaches
/// into the chart beside it, and goes back to rest once it leaves; on both
/// painters.
#[test]
fn a_rail_lights_only_while_the_pointer_is_on_its_hit_area() {
    let hover = |app: &mut QuantickApp, ctx: &egui::Context, at: egui::Pos2| {
        run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(at)]);
        run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(at)])
    };
    for source in [DARK, LIGHT] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        dress(&mut app, &ctx, source);
        assert!(app.active_tab_mut().canvas.fold(true, true));
        run_frame(&mut app, &ctx);
        let rail = app.active_tab().flow_rail_rect().expect("the flow's rail");
        let reach = egui::pos2(rail.left() - 10.0, rail.center().y);
        assert_lit_band(&hover(&mut app, &ctx, reach), rail, false, source, true);
        let away = egui::pos2(rail.left() - 60.0, rail.center().y);
        assert_lit_band(&hover(&mut app, &ctx, away), rail, false, source, false);

        let mut app = tree_app(&ctx, "time+time+flow", source);
        assert!(app.active_tab_mut().canvas.state.fold_flow_side(true, true));
        run_frame(&mut app, &ctx);
        let rails = tree_rails(
            &app.active_tab().canvas.state,
            whole(&app),
            CANVAS_DIVIDER_PX,
        );
        let rail = egui_rect(rails[0].rect);
        assert_lit_band(
            &hover(&mut app, &ctx, rail.center()),
            rail,
            false,
            source,
            true,
        );
        let away = egui::pos2(rail.left() - 60.0, rail.center().y);
        assert_lit_band(&hover(&mut app, &ctx, away), rail, false, source, false);
    }
}

/// A divider between two open panes is no rail: one fill of the theme's
/// border, on both painters.
#[test]
fn a_divider_between_open_panes_paints_as_before() {
    for source in [DARK, LIGHT] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        dress(&mut app, &ctx, source);
        let output = run_frame(&mut app, &ctx);
        let divider = app.active_tab().canvas_divider_rect().expect("a divider");
        let border = ink(source, Token::Border);
        assert_eq!(
            painted_in(&output, divider),
            vec![(divider, border)],
            "column"
        );

        let mut app = tree_app(&ctx, "time+time+flow", source);
        let output = run_frame(&mut app, &ctx);
        for seam in tree_seams(
            &app.active_tab().canvas.state,
            whole(&app),
            CANVAS_DIVIDER_PX,
        ) {
            let at = egui_rect(seam.rect);
            assert_eq!(painted_in(&output, at), vec![(at, border)], "tree");
        }
    }
}

/// The pane beside a rail stands where it stood before the rail was
/// restyled, to the byte, on both painters: the paint changed, the geometry
/// did not. The numbers are the base's, on the test window.
#[test]
fn the_pane_beside_a_rail_keeps_the_place_it_had() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    assert!(app.active_tab_mut().canvas.fold(true, true));
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let tab = app.active_tab();
    let rail = egui::Rect::from_min_max(egui::pos2(1356.0, 80.0), egui::pos2(1364.0, 872.0));
    assert_eq!(tab.flow_rail_rect(), Some(rail), "column rail");
    let beside = egui::Rect::from_min_max(egui::pos2(44.0, 114.0), egui::pos2(1352.0, 844.0));
    assert_eq!(
        tab.time_panes[0].frame.area,
        Some(beside),
        "column neighbour"
    );

    let mut app = tree_app(&ctx, "time+time+flow", DARK);
    assert!(app.active_tab_mut().canvas.state.fold_flow_side(true, true));
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let tab = app.active_tab();
    let rails = tree_rails(&tab.canvas.state, whole(&app), CANVAS_DIVIDER_PX);
    assert_eq!(
        rails[0].rect,
        CanvasRect::new(1356.0, 80.0, 8.0, 792.0),
        "tree rail"
    );
    let beside = egui::Rect::from_min_max(egui::pos2(701.0, 114.0), egui::pos2(1352.0, 844.0));
    assert_eq!(tab.time_panes[1].frame.area, Some(beside), "tree neighbour");
}
