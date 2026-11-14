//! A click activates whichever pane it lands in, in every layout and every
//! collapse state the canvas can draw, and only a split marks focus.

use super::*;

/// One layout and collapse state, and the panes it leaves on screen.
struct Arrangement {
    name: &'static str,
    layout: CanvasLayout,
    flow_collapsed: bool,
    context_collapsed: bool,
    slots_collapsed: &'static [bool],
    drawn: &'static [PaneSide],
}

const FLOW: PaneSide = PaneSide::Flow;
const TOP: PaneSide = PaneSide::Time(0);
const BOTTOM: PaneSide = PaneSide::Time(1);

const ARRANGEMENTS: &[Arrangement] = &[
    Arrangement {
        name: "time+flow",
        layout: CanvasLayout::TimeAndFlow,
        flow_collapsed: false,
        context_collapsed: false,
        slots_collapsed: &[false],
        drawn: &[TOP, FLOW],
    },
    Arrangement {
        name: "time+flow, flow put away",
        layout: CanvasLayout::TimeAndFlow,
        flow_collapsed: true,
        context_collapsed: false,
        slots_collapsed: &[false],
        drawn: &[TOP],
    },
    Arrangement {
        name: "time+flow, context put away",
        layout: CanvasLayout::TimeAndFlow,
        flow_collapsed: false,
        context_collapsed: true,
        slots_collapsed: &[false],
        drawn: &[FLOW],
    },
    Arrangement {
        name: "time+time+flow",
        layout: CanvasLayout::TimeTimeAndFlow,
        flow_collapsed: false,
        context_collapsed: false,
        slots_collapsed: &[false, false],
        drawn: &[TOP, BOTTOM, FLOW],
    },
    Arrangement {
        name: "time+time+flow, flow put away",
        layout: CanvasLayout::TimeTimeAndFlow,
        flow_collapsed: true,
        context_collapsed: false,
        slots_collapsed: &[false, false],
        drawn: &[TOP, BOTTOM],
    },
    Arrangement {
        name: "time+time+flow, top chart collapsed",
        layout: CanvasLayout::TimeTimeAndFlow,
        flow_collapsed: false,
        context_collapsed: false,
        slots_collapsed: &[true, false],
        drawn: &[BOTTOM, FLOW],
    },
    Arrangement {
        name: "time+time+flow, bottom chart collapsed",
        layout: CanvasLayout::TimeTimeAndFlow,
        flow_collapsed: false,
        context_collapsed: false,
        slots_collapsed: &[false, true],
        drawn: &[TOP, FLOW],
    },
    Arrangement {
        name: "time+time+flow, flow and top chart put away",
        layout: CanvasLayout::TimeTimeAndFlow,
        flow_collapsed: true,
        context_collapsed: false,
        slots_collapsed: &[true, false],
        drawn: &[BOTTOM],
    },
    Arrangement {
        name: "time+time+flow, context put away",
        layout: CanvasLayout::TimeTimeAndFlow,
        flow_collapsed: false,
        context_collapsed: true,
        slots_collapsed: &[false, false],
        drawn: &[FLOW],
    },
    Arrangement {
        name: "time",
        layout: CanvasLayout::Time,
        flow_collapsed: false,
        context_collapsed: false,
        slots_collapsed: &[false],
        drawn: &[TOP],
    },
    Arrangement {
        name: "flow",
        layout: CanvasLayout::Single,
        flow_collapsed: false,
        context_collapsed: false,
        slots_collapsed: &[],
        drawn: &[FLOW],
    },
];

fn arrange(app: &mut QuantickApp, ctx: &egui::Context, arrangement: &Arrangement) {
    let tab = app.active_tab_mut();
    tab.canvas.fold(true, false);
    tab.canvas.fold(false, false);
    tab.set_layout(arrangement.layout);
    run_frame(app, ctx);
    run_frame(app, ctx);
    let tab = app.active_tab_mut();
    let shares = vec![
        1.0 / arrangement.slots_collapsed.len().max(1) as f32;
        arrangement.slots_collapsed.len()
    ];
    tab.canvas
        .restore_context_heights(&shares, arrangement.slots_collapsed);
    tab.canvas.fold(false, arrangement.context_collapsed);
    tab.canvas.fold(true, arrangement.flow_collapsed);
    run_frame(app, ctx);
    run_frame(app, ctx);
}

fn chart_centre(app: &QuantickApp, side: PaneSide, name: &str) -> egui::Pos2 {
    app.active_tab()
        .pane(side)
        .frame
        .chart_area
        .unwrap_or_else(|| panic!("{name}: {side:?} is drawn and has a chart"))
        .center()
}

/// Focus frames (the active-widget border) around the centre of `side`'s
/// chart in `output`.
fn frames_around(
    ctx: &egui::Context,
    output: &egui::FullOutput,
    app: &QuantickApp,
    side: PaneSide,
) -> usize {
    let border = crate::theme::get(ctx).widget_active_border();
    let Some(chart) = app.active_tab().pane(side).frame.chart_area else {
        return 0;
    };
    output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) => {
                shape.stroke.color == border
                    && shape.stroke.width >= 2.0
                    && shape.rect.contains(chart.center())
            }
            _ => false,
        })
        .count()
}

#[test]
fn a_click_activates_every_drawn_pane_in_every_arrangement() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    run_frame(&mut app, &ctx);
    for arrangement in ARRANGEMENTS {
        let name = arrangement.name;
        arrange(&mut app, &ctx, arrangement);
        let drawn = arrangement.drawn;
        assert_eq!(
            app.active_tab().drawn_panes().count(),
            drawn.len(),
            "{name}: panes on screen"
        );
        // Every ordered pair: the click must *move* focus, never land on the
        // pane that already held it, and the frame must follow.
        for &to in drawn {
            for &from in drawn.iter().filter(|other| **other != to).chain(
                // A lone pane: one click, focused by definition.
                (drawn.len() == 1).then_some(&to),
            ) {
                for side in [from, to] {
                    let at = chart_centre(&app, side, name);
                    click_chart(&mut app, &ctx, at);
                    assert_eq!(
                        app.active_tab().focused_side(),
                        side,
                        "{name}: the click at {at:?} activates {side:?}"
                    );
                }
                let output = run_frame(&mut app, &ctx);
                for &side in drawn {
                    let framed = drawn.len() > 1 && side == to;
                    assert_eq!(
                        frames_around(&ctx, &output, &app, side),
                        usize::from(framed),
                        "{name}: focus on {to:?}, the frame around {side:?}"
                    );
                }
            }
        }
    }
}

/// The trader's report: two context charts, flow put away, and a click on
/// the bottom chart did nothing.
#[test]
fn the_bottom_chart_takes_focus_with_the_flow_pane_put_away() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    run_frame(&mut app, &ctx);
    let arrangement = ARRANGEMENTS
        .iter()
        .find(|arrangement| arrangement.name == "time+time+flow, flow put away")
        .expect("the reported arrangement");
    arrange(&mut app, &ctx, arrangement);
    assert_eq!(app.active_tab().focused_side(), TOP);
    let at = chart_centre(&app, BOTTOM, arrangement.name);
    click_chart(&mut app, &ctx, at);
    assert_eq!(app.active_tab().focused_side(), BOTTOM);
    // And it stays there across frames, framed: nothing resets it to the top.
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().focused_side(), BOTTOM);
    assert_eq!(frames_around(&ctx, &output, &app, BOTTOM), 1);
    assert_eq!(frames_around(&ctx, &output, &app, TOP), 0);
}
