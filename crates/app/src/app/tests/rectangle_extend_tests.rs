//! A rectangle's double click, driven through the real pointer path: the
//! side it lands on is the side the band extends to, and the next double
//! click puts back exactly the extent that was there.

use super::*;

const LEFT: f32 = 500.0;
const RIGHT: f32 = 800.0;
const TOP: f32 = 250.0;
const BOTTOM: f32 = 400.0;

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

#[test]
fn a_double_click_by_a_side_extends_that_way_and_the_next_one_restores() {
    let mid_y = (TOP + BOTTOM) / 2.0;
    for (position, expected) in [
        (egui::pos2(LEFT + 6.0, mid_y), (true, false)),
        (egui::pos2(RIGHT - 6.0, mid_y), (false, true)),
        (egui::pos2((LEFT + RIGHT) / 2.0, mid_y), (true, true)),
    ] {
        let (mut app, ctx, _commands) = drawn_rectangle();
        assert_eq!(extent(&app), (false, false));
        let undo_before = app.active_tab().flow_pane.drawings.undo_depth();
        double_click(&mut app, &ctx, position);
        assert_eq!(extent(&app), expected, "double click at {position:?}");
        assert_eq!(
            app.active_tab().flow_pane.drawings.undo_depth(),
            undo_before + 1,
            "the extension is one undo step"
        );
        wait_out_the_click_sequence(&mut app, &ctx);
        // Back on the band's original body, which every extension keeps.
        double_click(&mut app, &ctx, egui::pos2((LEFT + RIGHT) / 2.0, mid_y));
        assert_eq!(
            extent(&app),
            (false, false),
            "the second double click restores"
        );
    }
}

/// A press in a side zone still moves the band, so the cursor says move
/// there too; the glyph alone announces the double click.
#[test]
fn the_side_zones_keep_the_move_cursor() {
    let (mut app, ctx, _commands) = drawn_rectangle();
    let mid_y = (TOP + BOTTOM) / 2.0;
    for x in [LEFT + 6.0, (LEFT + RIGHT) / 2.0, RIGHT - 6.0] {
        assert_eq!(
            cursor_at(&mut app, &ctx, egui::pos2(x, mid_y)),
            egui::CursorIcon::Move,
            "at x={x}"
        );
    }
}

/// The rectangle a trader draws by default has no fill, and only its
/// border takes a click; its centre still takes the double click.
#[test]
fn an_unfilled_rectangle_takes_the_centre_double_click() {
    let (mut app, ctx, _commands) = drawn_rectangle_with_fill(0);
    let centre = egui::pos2((LEFT + RIGHT) / 2.0, (TOP + BOTTOM) / 2.0);
    double_click(&mut app, &ctx, centre);
    assert_eq!(extent(&app), (true, true));
    wait_out_the_click_sequence(&mut app, &ctx);
    double_click(&mut app, &ctx, centre);
    assert_eq!(extent(&app), (false, false), "and the next one restores");
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
