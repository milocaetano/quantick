//! The column painter's frame with a side folded, and a stack with a chart
//! folded: the open pane comes up to the rail, no gutter where the divider
//! stood; open, the divider stands.

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

/// A stack with a chart folded: the open charts beside it take the divider
/// between them and it, either side; dividers between open charts stand.
#[test]
fn a_folded_chart_in_a_stack_leaves_no_gutter_beside_it() {
    let rail = PaneWidth::Collapsed { restore: 0.3 };
    let heights = [PaneWidth::Auto, rail, PaneWidth::Auto, PaneWidth::Auto];
    let plain = split(0.0, 900.0, &heights);
    let met = plain.meeting_rails(&heights);
    let (panes, dividers) = (met.panes(), met.dividers());
    assert_eq!(panes[0].1, dividers[0].1, "the chart above meets it");
    assert_eq!(panes[2].0, dividers[1].0, "the chart below meets it");
    assert_eq!(panes[1], plain.panes()[1], "the rail stays put");
    assert_eq!(panes[2].1, plain.panes()[2].1);
    assert_eq!(panes[3], plain.panes()[3], "open charts keep their divider");
    assert_eq!(
        dividers,
        plain.dividers(),
        "every divider still takes a hand"
    );
}
