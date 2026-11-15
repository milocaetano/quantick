//! A folded pane's rail paints the workspace's marks (band, the edges the
//! window's chrome does not already draw, a chevron toward the open side and
//! a tick per pane behind it) in the theme's rail tokens, on both painters,
//! upright and flat, in Dark and in Light, lit only while the pointer is on
//! its hit area and no other drag is under way. The seam beside a rail is no
//! line; a divider between two open panes paints as before; the rail keeps
//! its 8 px and the pane beside it its place; its hover words reach the scene.
use super::*;

use crate::canvas_layout::{CANVAS_DIVIDER_PX, COLLAPSED_PANE_WIDTH_PX, LAYOUT_PRESETS};
use crate::floating_panes::{model_rect, screen_rect};
use quantick_appearance::{DARK, LIGHT, ResolvedTheme, Token};
use quantick_workspace::canvas::{
    CanvasRect, RailInk, RailLook, rail_marks, tree_rails, tree_seams,
};

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

/// `rail` painted as `look` says, in `source`'s rail inks at rest or `lit`:
/// every mark the workspace lays, each in its token. A canvas rail keeps
/// its 8 px; the column painter's stack folds a chart to its own collapsed
/// share, as it did before.
fn assert_rail(
    output: &egui::FullOutput,
    rail: egui::Rect,
    look: RailLook,
    (source, lit): (&str, bool),
) {
    let across = if look.flat {
        rail.height()
    } else {
        rail.width()
    };
    let stacked = look.flat && rail.width() < 600.0;
    assert!(
        stacked || (across - COLLAPSED_PANE_WIDTH_PX).abs() < 0.01,
        "8 px: {rail:?}"
    );
    let inks = quantick_appearance::rail::rail_inks(lit).map(|token| ink(source, token));
    let painted = painted_in(output, rail);
    for (mark, worn) in rail_marks(model_rect(rail), look) {
        let (at, fill) = (screen_rect(mark), inks[worn.index()]);
        assert!(
            painted
                .iter()
                .any(|(rect, colour)| near(*rect, at) && *colour == fill),
            "{worn:?} at {at:?} in {fill:?}; painted {painted:?}"
        );
    }
}

/// How many marks of `ink` the rail lays.
fn count(rail: egui::Rect, look: RailLook, ink: RailInk) -> usize {
    (rail_marks(model_rect(rail), look).iter())
        .filter(|(_, worn)| *worn == ink)
        .count()
}

/// The tab on the tree painter at `preset`, in `source`.
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
    model_rect(app.active_tab().canvas.last_canvas)
}

fn scene_words(app: &QuantickApp, id: &str) -> Option<String> {
    let scene = crate::control::scene_snapshot(app);
    let entry = scene
        .controls
        .into_iter()
        .find(|entry| entry.control_id == id)?;
    entry.description
}

/// The column painter's two rails: the charts folded left of the flow open
/// right, their left line the tool rail's; the flow folded right beside the
/// sidebar opens left, its right line the sidebar's. One tick each; the
/// scene says what each holds.
#[test]
fn the_column_painters_rails_share_the_chrome_line_and_point_the_way_they_open() {
    for source in [DARK, LIGHT] {
        for flow in [false, true] {
            let ctx = egui::Context::default();
            let (mut app, _commands) = split_app(&ctx, 200);
            dress(&mut app, &ctx, source);
            assert!(app.active_tab_mut().canvas.fold(flow, true));
            run_frame(&mut app, &ctx);
            let output = run_frame(&mut app, &ctx);
            let rail = app.active_tab().collapsed_rail_rect().expect("a rail");
            let look = RailLook {
                flat: false,
                opens_forward: !flow,
                ticks: 1,
                on_chrome: [!flow, flow],
            };
            assert_rail(&output, rail, look, (source, false));
            assert_eq!(count(rail, look, RailInk::Edge), 1, "one edge, inside");
            let id = app.active_tab().canvas.state.side_rail_id(flow).unwrap();
            let words = scene_words(&app, &id).expect("the rail says what it holds");
            let what = if flow { "Order flow · " } else { "Time · " };
            assert!(
                words.starts_with(what) && words.ends_with(" · drag to open"),
                "{words}"
            );
        }
    }
}

/// The tree painter's rails: the flow at the window's right edge, the
/// whole context side of grid4+flow behind one rail with four ticks, and
/// the flow folded flat under the toolbar. The seam beside each is no
/// line: nothing of the border's colour stands between rail and pane.
#[test]
fn the_tree_painters_rails_paint_one_band_and_leave_their_seam_unlined() {
    for source in [DARK, LIGHT] {
        for (preset, flow, look) in [
            (
                "time+time+flow",
                true,
                RailLook {
                    flat: false,
                    opens_forward: false,
                    ticks: 1,
                    on_chrome: [false, true],
                },
            ),
            (
                "grid4+flow",
                false,
                RailLook {
                    flat: false,
                    opens_forward: true,
                    ticks: 4,
                    on_chrome: [true, false],
                },
            ),
            (
                "flow-over-time",
                true,
                RailLook {
                    flat: true,
                    opens_forward: true,
                    ticks: 1,
                    on_chrome: [true, false],
                },
            ),
        ] {
            let ctx = egui::Context::default();
            let mut app = tree_app(&ctx, preset, source);
            assert!(app.active_tab_mut().canvas.state.fold_flow_side(flow, true));
            run_frame(&mut app, &ctx);
            let output = run_frame(&mut app, &ctx);
            let state = &app.active_tab().canvas.state;
            let rails = tree_rails(state, whole(&app), CANVAS_DIVIDER_PX);
            assert_eq!(rails.len(), 1, "{preset}: one rail");
            assert_eq!(rails[0].look(whole(&app)), look, "{preset}");
            assert_rail(&output, screen_rect(rails[0].rect), look, (source, false));
            let seams = tree_seams(state, whole(&app), CANVAS_DIVIDER_PX);
            let beside = seams
                .iter()
                .find(|seam| seam.rail.is_some())
                .expect("its seam");
            let border = ink(source, Token::Border);
            let gutter = painted_in(&output, screen_rect(beside.rect));
            assert!(
                gutter.iter().all(|(_, fill)| *fill != border),
                "{preset}: {gutter:?}"
            );
            let words = scene_words(&app, &rails[0].control_id()).expect("words");
            if preset == "grid4+flow" {
                assert_eq!(words, "4 charts · drag to open");
            }
        }
    }
}

/// The column painter's flat rail, a context chart folded in its stack (to
/// the stack's collapsed share, 17.5 px here, as before): lit while the pointer is on the dividers beside it, at rest away.
#[test]
fn the_column_painters_flat_context_rail_paints_and_lights_on_its_hit_area() {
    let hover = |app: &mut QuantickApp, ctx: &egui::Context, at: egui::Pos2| {
        run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(at)]);
        run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(at)])
    };
    for source in [DARK, LIGHT] {
        let launch = AppLaunch {
            scenario: crate::hooks::ScenarioInputs::from_pairs(&[
                ("QUANTICK_LAYOUT", "time+time+flow"),
                ("QUANTICK_CONTEXT_UPPER_COLLAPSED", "1"),
            ]),
            ..AppLaunch::default()
        };
        let (mut app, _evt, _cmd, _book) = test_app_with_launch(launch);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        dress(&mut app, &ctx, source);
        let output = run_frame(&mut app, &ctx);
        let band = ink(source, Token::RailBand);
        let rail = (painted_rects(&output).into_iter())
            .find(|shape| shape.fill == band && shape.rect.width() > shape.rect.height())
            .expect("the folded chart's flat rail")
            .rect;
        let look = RailLook {
            flat: true,
            opens_forward: true,
            ticks: 1,
            on_chrome: [true, false],
        };
        assert_rail(&output, rail, look, (source, false));
        assert_rail(
            &hover(&mut app, &ctx, rail.center()),
            rail,
            look,
            (source, true),
        );
        let away = rail.center() + egui::vec2(0.0, 120.0);
        assert_rail(&hover(&mut app, &ctx, away), rail, look, (source, false));
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
        let look = RailLook {
            flat: false,
            opens_forward: false,
            ticks: 1,
            on_chrome: [false, true],
        };
        let reach = egui::pos2(rail.left() - 10.0, rail.center().y);
        assert_rail(&hover(&mut app, &ctx, reach), rail, look, (source, true));
        let away = egui::pos2(rail.left() - 60.0, rail.center().y);
        assert_rail(&hover(&mut app, &ctx, away), rail, look, (source, false));

        let mut app = tree_app(&ctx, "time+time+flow", source);
        assert!(app.active_tab_mut().canvas.state.fold_flow_side(true, true));
        run_frame(&mut app, &ctx);
        let rails = tree_rails(
            &app.active_tab().canvas.state,
            whole(&app),
            CANVAS_DIVIDER_PX,
        );
        let (rail, look) = (screen_rect(rails[0].rect), rails[0].look(whole(&app)));
        assert_rail(
            &hover(&mut app, &ctx, rail.center()),
            rail,
            look,
            (source, true),
        );
        let away = egui::pos2(rail.left() - 60.0, rail.center().y);
        assert_rail(&hover(&mut app, &ctx, away), rail, look, (source, false));
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
            let at = screen_rect(seam.rect);
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
