//! The column painter's frame with a side folded: the open pane comes up to
//! the rail, no gutter where the divider stood; open, the divider stands.

use super::*;

const START: f32 = 44.0;
const WIDTH: f32 = 1320.0;

fn frame(folds: (bool, bool)) -> ColumnFrame {
    column_frame(
        CanvasLayout::TimeAndFlow,
        1,
        (START, WIDTH),
        &mut 0.4,
        folds,
    )
}

#[test]
fn a_folded_side_leaves_no_gutter_between_its_rail_and_the_open_pane() {
    let context = frame((true, false));
    let rail = context.context_rail.expect("the charts' rail");
    assert_eq!(rail.0, START);
    assert_eq!(context.flow.0, rail.1, "the flow pane meets the rail");
    assert_eq!(context.flow.1, START + WIDTH);

    let flow = frame((false, true));
    let rail = flow.flow_rail.expect("the flow's rail");
    assert_eq!(rail.1, START + WIDTH);
    assert_eq!(
        flow.time.expect("the charts").1,
        rail.0,
        "the charts meet the rail"
    );

    let open = frame((false, false));
    let (time, divider) = (open.time.unwrap(), open.divider.unwrap());
    assert_eq!(
        (time.1, divider.1),
        (divider.0, open.flow.0),
        "open: the divider stands"
    );
}
