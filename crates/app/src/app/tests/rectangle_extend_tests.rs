//! A rectangle's side zones, driven through the real pointer path: the
//! border line resizes sideways, one click in the band just inside a side
//! runs that side to the chart edge, and a double click inside puts the
//! original size back. Each zone shows its own cursor.

use super::*;

const LEFT: f32 = 500.0;
const RIGHT: f32 = 800.0;
const TOP: f32 = 250.0;
const BOTTOM: f32 = 400.0;
const MID_Y: f32 = (TOP + BOTTOM) / 2.0;
/// Inside a band, past a border's click reach, so it holds for an outline
/// rectangle too.
const BAND_INSET: f32 = 12.0;

/// A filled rectangle drawn by hand, with egui's click sequence quiet so
/// the next two clicks read as a double, not a triple.
fn drawn_rectangle() -> (QuantickApp, egui::Context, mpsc::Receiver<FeedCommand>) {
    drawn_rectangle_with_fill(40)
}

fn drawn_rectangle_with_fill(
    fill_alpha: u8,
) -> (QuantickApp, egui::Context, mpsc::Receiver<FeedCommand>) {
    let (mut app, commands) = super::bare_canvas::app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(LEFT, TOP),
        egui::pos2(RIGHT, BOTTOM),
    );
    run_frame(&mut app, &ctx);
    {
        let drawings = &mut app.active_tab_mut().flow_pane.drawings;
        assert_eq!(drawings.items().len(), 1, "the drag placed one rectangle");
        // The interior takes part in the hit-test only while filled.
        drawings.items_mut()[0].style.fill_alpha = fill_alpha;
    }
    wait_out_the_click_sequence(&mut app, &ctx);
    (app, ctx, commands)
}

fn extent(app: &QuantickApp) -> (bool, bool) {
    let payload = app.active_tab().flow_pane.drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<drawings::RectanglePayload>()
        .expect("a rectangle carries a rectangle payload");
    (payload.extend_left, payload.extend_right)
}

/// The drawn rectangle's left and right side on screen, wherever the
/// placement snapped its anchors.
fn drawn_sides(app: &QuantickApp) -> (f32, f32) {
    let pane = &app.active_tab().flow_pane;
    let chart = pane
        .frame
        .chart_area
        .expect("the first frame laid out the chart");
    let history_right = pane.frame.lane_divider_x.unwrap_or(chart.right());
    let xs: Vec<f32> = pane.drawings.items()[0]
        .points
        .iter()
        .map(|point| {
            pane.model
                .viewport
                .x_at_bar_position(point.bar, history_right, pane.slots())
        })
        .collect();
    (xs[0].min(xs[1]), xs[0].max(xs[1]))
}

fn click(app: &mut QuantickApp, ctx: &egui::Context, position: egui::Pos2) {
    click_chart(app, ctx, position);
    run_frame(app, ctx);
}

fn double_click(app: &mut QuantickApp, ctx: &egui::Context, position: egui::Pos2) {
    click_chart(app, ctx, position);
    click_chart(app, ctx, position);
    run_frame(app, ctx);
}

fn wait_out_the_click_sequence(app: &mut QuantickApp, ctx: &egui::Context) {
    let quiet = ctx.options(|options| options.input_options.max_double_click_delay) * 2.0;
    let frames = (quiet / f64::from(ctx.input(|input| input.predicted_dt))).ceil() as usize + 2;
    for _ in 0..frames {
        run_frame(app, ctx);
    }
}

fn cursor_at(app: &mut QuantickApp, ctx: &egui::Context, position: egui::Pos2) -> egui::CursorIcon {
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(position)]);
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(position)])
        .platform_output
        .cursor_icon
}

/// One click in the band just inside a side runs that side to the chart
/// edge, as one undo step, and leaves the other side alone.
#[test]
fn one_click_in_a_band_extends_that_side() {
    for fill in [40, 0] {
        let (mut app, ctx, _commands) = drawn_rectangle_with_fill(fill);
        let (_, right) = drawn_sides(&app);
        assert_eq!(extent(&app), (false, false));
        let undo_before = app.active_tab().flow_pane.drawings.undo_depth();
        click(&mut app, &ctx, egui::pos2(right - BAND_INSET, MID_Y));
        assert_eq!(extent(&app), (false, true), "the right band, fill {fill}");
        assert_eq!(
            app.active_tab().flow_pane.drawings.undo_depth(),
            undo_before + 1,
            "the extension is one undo step"
        );
        wait_out_the_click_sequence(&mut app, &ctx);
        // A selection may narrow the canvas: read the sides again.
        let (left, _) = drawn_sides(&app);
        click(&mut app, &ctx, egui::pos2(left + BAND_INSET, MID_Y));
        assert_eq!(
            extent(&app),
            (true, true),
            "then the left band, fill {fill}"
        );
    }
}

/// A double click anywhere inside, corners excluded, ends at the original
/// size - even one whose first click landed in a band and extended it.
#[test]
fn a_double_click_inside_puts_back_the_original_size() {
    let (mut app, ctx, _commands) = drawn_rectangle();
    let (_, right) = drawn_sides(&app);
    click(&mut app, &ctx, egui::pos2(right - BAND_INSET, MID_Y));
    wait_out_the_click_sequence(&mut app, &ctx);
    let (left, right) = drawn_sides(&app);
    click(&mut app, &ctx, egui::pos2(left + BAND_INSET, MID_Y));
    wait_out_the_click_sequence(&mut app, &ctx);
    assert_eq!(extent(&app), (true, true));
    double_click(&mut app, &ctx, egui::pos2((left + right) / 2.0, MID_Y));
    assert_eq!(extent(&app), (false, false), "the centre puts it back");
    wait_out_the_click_sequence(&mut app, &ctx);
    let (_, right) = drawn_sides(&app);

    double_click(&mut app, &ctx, egui::pos2(right - BAND_INSET, MID_Y));
    assert_eq!(
        extent(&app),
        (false, false),
        "a double click in the band ends at the original size"
    );
    assert!(
        !app.surfaces.chart_settings.is_open(),
        "a drawing's double click is the drawing's, not the chart settings'"
    );
}

/// The border line itself, not a corner, drags that side sideways.
#[test]
fn dragging_a_side_line_resizes_the_rectangle() {
    let (mut app, ctx, _commands) = drawn_rectangle();
    let (left, right) = drawn_sides(&app);
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(right, MID_Y),
        egui::pos2(right + 60.0, MID_Y + 30.0),
    );
    run_frame(&mut app, &ctx);
    let (left_after, right_after) = drawn_sides(&app);
    assert!(
        (left_after - left).abs() < 0.5,
        "the left side held: {left} -> {left_after}"
    );
    assert!(
        right_after > right + 40.0,
        "the right side grew: {right} -> {right_after}"
    );
    assert_eq!(extent(&app), (false, false));
}

/// Each zone shows its own OS cursor, selected or not, and no two zones
/// share one: the border line resizes sideways, the band says a click
/// acts, the inside moves, and a corner, which a press resizes, says so.
#[test]
fn each_zone_shows_its_own_cursor() {
    let (mut app, ctx, _commands) = drawn_rectangle();
    for selected in [true, false] {
        let index = selected.then_some(0);
        app.active_tab_mut().flow_pane.drawings.select(index);
        run_frame(&mut app, &ctx);
        let (left, right) = drawn_sides(&app);
        let bottom = app.active_tab().flow_pane.drawings.items()[0]
            .points
            .iter()
            .map(|point| price_y(&app, PaneSide::Flow, point.price))
            .fold(f32::NEG_INFINITY, f32::max);
        let corner = egui::pos2(right, bottom);
        let frame = &app.active_tab().flow_pane.frame;
        assert!(
            frame.chart_area.is_some_and(|rect| rect.contains(corner))
                && frame
                    .capsules
                    .iter()
                    .flatten()
                    .all(|rect| !rect.contains(corner)),
            "the projected resize corner {corner:?} must be on the chart outside capsules {:?}; placement top was {TOP}",
            frame.capsules
        );
        assert!(
            app.drawings
                .chrome
                .context_bar_rect()
                .is_none_or(|rect| !rect.contains(corner)),
            "the projected resize corner {corner:?} must be outside the selected drawing toolbar {:?}",
            app.drawings.chrome.context_bar_rect()
        );
        let mut at = |x: f32, y: f32| cursor_at(&mut app, &ctx, egui::pos2(x, y));
        let zones = [
            at(left, MID_Y),
            at(left + BAND_INSET, MID_Y),
            at((left + right) / 2.0, MID_Y),
            at(corner.x, corner.y),
        ];
        assert_eq!(
            zones[..3],
            [
                egui::CursorIcon::ResizeHorizontal,
                egui::CursorIcon::PointingHand,
                egui::CursorIcon::Move,
            ],
            "selected {selected}"
        );
        assert_eq!(
            at(right - BAND_INSET, MID_Y),
            zones[1],
            "selected {selected}"
        );
        assert_eq!(at(right, MID_Y), zones[0], "selected {selected}");
        assert_eq!(
            zones[3],
            egui::CursorIcon::ResizeNwSe,
            "a corner resizes, selected {selected}"
        );
        for (index, cursor) in zones.iter().enumerate() {
            assert!(
                !zones[index + 1..].contains(cursor),
                "{cursor:?} is shared, selected {selected}: {zones:?}"
            );
        }
    }
}

/// A band that visibly runs back to the chart's left edge is a region the
/// strategy seat reads as active there too, as `extend_right` is forward.
#[test]
fn extend_left_makes_the_region_active_before_its_left_anchor() {
    let rectangle = drawing_tool("rectangle");
    let mut drawings = drawings::Drawings::default();
    drawings.place(rectangle, drawings::ChartPoint::at(5.0, 100.0));
    drawings.place(rectangle, drawings::ChartPoint::at(8.0, 110.0));
    let id = drawings.items()[0].id;
    let active = |drawings: &drawings::Drawings| {
        crate::pane::strategies::drawing_region(drawings, id, 2)
            .expect("the region is testable")
            .1
    };
    assert!(
        !active(&drawings),
        "before the left anchor the region waits"
    );
    drawings.items_mut()[0]
        .payload
        .as_any_mut()
        .downcast_mut::<drawings::RectanglePayload>()
        .expect("a rectangle carries a rectangle payload")
        .extend_left = true;
    assert!(active(&drawings), "extended left, it is active there");
}

/// A band run to the chart edges paints across the whole width, but a
/// double click that misses its border and fill still belongs to the chart
/// outside the drawn rectangle: the view snaps back to live, and the band
/// keeps its extent. Well away from the band the chart's double click is
/// untouched too.
#[test]
fn an_extended_outline_band_leaves_the_charts_double_click_to_the_chart() {
    for position in [
        egui::pos2(LEFT - 150.0, MID_Y),
        egui::pos2(LEFT - 150.0, TOP - 100.0),
    ] {
        let (mut app, ctx, _commands) = drawn_rectangle_with_fill(0);
        let (left, _) = drawn_sides(&app);
        click(&mut app, &ctx, egui::pos2(left + BAND_INSET, MID_Y));
        wait_out_the_click_sequence(&mut app, &ctx);
        let (_, right) = drawn_sides(&app);
        click(&mut app, &ctx, egui::pos2(right - BAND_INSET, MID_Y));
        assert_eq!(extent(&app), (true, true));
        wait_out_the_click_sequence(&mut app, &ctx);
        {
            let pane = &mut app.active_tab_mut().flow_pane;
            let slots = pane.slots();
            pane.model.viewport.pan_pixels(120.0, slots);
        }
        run_frame(&mut app, &ctx);
        assert!(!app.active_tab().flow_pane.model.viewport.follows_live());
        double_click(&mut app, &ctx, position);
        assert!(
            app.active_tab().flow_pane.model.viewport.follows_live(),
            "the chart's double click snapped to live at {position:?}"
        );
        assert_eq!(extent(&app), (true, true), "the band held at {position:?}");
    }
}

/// egui's double click is time-only: a quick click in the left band and
/// then the right one is two clicks, and each extends its side.
#[test]
fn a_quick_click_in_each_band_extends_both_sides() {
    let (mut app, ctx, _commands) = drawn_rectangle();
    let (left, _) = drawn_sides(&app);
    click(&mut app, &ctx, egui::pos2(left + BAND_INSET, MID_Y));
    // A selection may narrow the canvas: read the sides again.
    let (_, right) = drawn_sides(&app);
    click(&mut app, &ctx, egui::pos2(right - BAND_INSET, MID_Y));
    assert_eq!(extent(&app), (true, true));
}

/// A held modifier yields to a drawing's point handles only: a corner keeps
/// its press, while a side line, like the body, leaves it to the order aim.
#[test]
fn a_modifier_press_on_a_side_line_still_rests_the_order() {
    let (mut app, ctx, _commands) = drawn_rectangle();
    let (left, _) = drawn_sides(&app);
    click_chart_with(
        &mut app,
        &ctx,
        egui::pos2(left, TOP),
        egui::Modifiers::SHIFT,
    );
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().paper.working_orders().is_empty(),
        "a corner keeps the press"
    );
    // The corner's press selected it, which may narrow the canvas.
    let (left, _) = drawn_sides(&app);
    click_chart_with(
        &mut app,
        &ctx,
        egui::pos2(left, MID_Y),
        egui::Modifiers::SHIFT,
    );
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().paper.working_orders().len(),
        1,
        "the side line leaves the press to the aim"
    );
}

/// A locked, filled zone is not a grip: a drag inside it pans the chart and
/// leaves the zone where it is. The trader's frozen chart was this zone,
/// extended across the pane, taking every press.
#[test]
fn a_drag_inside_a_locked_filled_rectangle_pans_the_chart() {
    let (mut app, ctx, _commands) = drawn_rectangle();
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_selected_locked(true);
    let edge = |app: &QuantickApp| {
        let pane = &app.active_tab().flow_pane;
        pane.model.viewport.right_edge_bar(pane.slots())
    };
    let before = edge(&app);
    let points = app.active_tab().flow_pane.drawings.items()[0]
        .points
        .clone();
    let inside = egui::pos2((LEFT + RIGHT) / 2.0, MID_Y);
    drag_chart(&mut app, &ctx, inside, inside + egui::vec2(80.0, 0.0));
    assert_ne!(edge(&app), before, "the drag pans the chart");
    assert_eq!(
        app.active_tab().flow_pane.drawings.items()[0].points,
        points,
        "the locked zone stays put"
    );
}

/// The press is the chart's, but a plain click inside a locked zone still
/// selects it: the context bar that unlocks it stays one click away.
#[test]
fn a_click_inside_a_locked_filled_rectangle_still_selects_it() {
    let (mut app, ctx, _commands) = drawn_rectangle();
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_selected_locked(true);
    app.active_tab_mut().flow_pane.drawings.select(None);
    click_chart(&mut app, &ctx, egui::pos2((LEFT + RIGHT) / 2.0, MID_Y));
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
}
