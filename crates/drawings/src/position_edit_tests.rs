use super::*;
use crate::{
    ChartPoint, DrawingBand, DrawingIntent, DrawingStyle, Drawings, NewDrawing, Placement, update,
};

/// A long entered at 100 with its target measured at 120, at 1 : 2.
fn target_measured_long() -> Drawing<DrawingTool> {
    let mut drawings = Drawings::default();
    let payload = PositionPayload {
        ratio: 2.0,
        ..PositionPayload::default()
    };
    let placement = Placement {
        tool: DrawingTool::by_id("long-position").unwrap(),
        points: vec![ChartPoint::at(10.0, 100.0), ChartPoint::at(30.0, 120.0)],
        band: DrawingBand::Price,
        opening: NewDrawing::explicit(DrawingStyle::default(), Box::new(payload)),
        author: None,
        name: None,
        text: None,
    };
    update(&mut drawings, DrawingIntent::Place(placement));
    drawings.items()[0].clone()
}

fn payload(drawing: &Drawing<DrawingTool>) -> &PositionPayload {
    position_of(drawing).unwrap().1
}

fn legs(drawing: &Drawing<DrawingTool>) -> (f64, f64, f64) {
    let plan = position_plan(drawing.tool, &drawing.points, drawing.payload.as_ref()).unwrap();
    (plan.entry, plan.stop, plan.target)
}

#[test]
fn a_preset_on_a_target_measured_plan_moves_only_the_target() {
    let drawing = target_measured_long();
    assert_eq!(legs(&drawing), (100.0, 90.0, 120.0));
    let edited = edited_position(&drawing, PositionEdit::Ratio(3.0)).unwrap();
    assert_eq!(legs(&edited), (100.0, 90.0, 130.0));
    assert_eq!(
        edited.points[0], drawing.points[0],
        "the entry anchor holds"
    );
}

#[test]
fn colours_and_opacity_change_the_look_and_no_price() {
    let drawing = target_measured_long();
    let mut edited = drawing.clone();
    for edit in [
        PositionEdit::Colour(PositionLeg::Target, [1, 2, 3]),
        PositionEdit::Colour(PositionLeg::Stop, [4, 5, 6]),
        PositionEdit::ZoneOpacity(40),
    ] {
        edited = edited_position(&edited, edit).unwrap();
    }
    let look = payload(&edited);
    assert_eq!(
        (look.target_rgb, look.stop_rgb, look.zone_opacity_pct),
        ([1, 2, 3], [4, 5, 6], 40)
    );
    assert_eq!(edited.points, drawing.points);
}

#[test]
fn a_lock_keeps_prices_and_width_but_not_colours() {
    let mut drawing = target_measured_long();
    drawing.locked = true;
    for edit in [PositionEdit::Ratio(1.0), PositionEdit::WidthBars(30.0)] {
        assert_eq!(
            edited_position(&drawing, edit).unwrap_err(),
            PositionEditRefusal::Locked
        );
    }
    assert!(edited_position(&drawing, PositionEdit::ZoneOpacity(10)).is_ok());
}

#[test]
fn an_out_of_range_value_or_another_tool_is_refused() {
    let drawing = target_measured_long();
    for edit in [
        PositionEdit::Ratio(0.0),
        PositionEdit::ZoneOpacity(101),
        PositionEdit::WidthBars(1.0),
    ] {
        assert!(matches!(
            edited_position(&drawing, edit),
            Err(PositionEditRefusal::Invalid(_))
        ));
    }
    let mut line = drawing.clone();
    line.tool = DrawingTool::by_id("horizontal-line").unwrap();
    assert_eq!(
        edited_position(&line, PositionEdit::ZoneOpacity(10)).unwrap_err(),
        PositionEditRefusal::NotAPosition
    );
}

#[test]
fn the_menu_checks_the_preset_or_shows_the_custom_row_on_top() {
    let drawing = target_measured_long();
    let menu = ratio_menu(&drawing).unwrap();
    assert_eq!(menu.button, "R/R 1:2");
    let checked: Vec<_> = menu.rows.iter().map(|row| row.checked).collect();
    assert_eq!(checked, [false, true, false]);
    // A dragged stop at 92: risk 8, reward 20.
    let mut custom = drawing.clone();
    custom.payload = Box::new(PositionPayload {
        ratio: 2.5,
        projected_offset: Some(-8.0),
        ..payload(&drawing).clone()
    });
    let menu = ratio_menu(&custom).unwrap();
    assert_eq!(menu.button, "R/R 1:2.5");
    assert_eq!(
        menu.rows[0],
        RatioRow {
            label: "Custom 1:2.5".into(),
            pick: None,
            checked: true
        }
    );
    assert!(menu.rows[1..].iter().all(|row| !row.checked));
    assert_eq!(menu.rows.len(), 4);
}

/// A flat plan keeps its own bar: its colours, and no ratio to show.
#[test]
fn a_flat_plan_keeps_its_colour_cells_without_a_ratio() {
    let mut drawing = target_measured_long();
    drawing.points[1].price = drawing.points[0].price;
    let face = position_face(&drawing).expect("a position's face");
    assert_eq!(face.ratio, None);
    assert_eq!(face.target_rgb, payload(&drawing).target_rgb);
}
