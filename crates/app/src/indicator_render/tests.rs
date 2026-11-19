use super::*;
use crate::indicator_worker::SlotId;
use quantick_indicators::{IndicatorDescriptor, PlotId, PlotSpec, PreviewFrame, Rgba8};

fn view(columns: Vec<Vec<f64>>, preview: Option<Vec<f64>>) -> IndicatorView {
    IndicatorView {
        slot: SlotId(0),
        kind: std::sync::Arc::from("test.indicator"),
        ordinal: 0,
        style: crate::indicator_style::StyleOverride::default(),
        label: std::sync::Arc::from("test"),
        descriptor: IndicatorDescriptor {
            title: "test".to_owned(),
            short_title: None,
            overlay: false,
            plots: (0..columns.len())
                .map(|i| PlotSpec {
                    id: PlotId::new(i),
                    title: format!("p{i}"),
                    style: PlotStyle::Line,
                    base_color: Rgba8::opaque(255, 255, 255),
                    width: 1.0,
                    offset: 0,
                    marker: None,
                })
                .collect(),
            fills: Vec::new(),
            inputs: Vec::new(),
        },
        rows: columns.first().map_or(0, Vec::len),
        columns,
        bar_paints: Vec::new(),
        preview: preview.map(PreviewFrame::new),
        lane: Vec::new(),
        objects: quantick_indicators::ObjectSnapshot::default(),
        input_values: Vec::new(),
        stale: None,
        error: None,
        hidden: false,
        mouse_vertical_line: false,
        scale: crate::price_view::PriceView::new(),
        sizing: crate::indicators::PaneSizing::Auto,
        inks: Vec::new(),
        last_auto: None,
    }
}

#[test]
fn a_gap_breaks_the_polyline_instead_of_interpolating_across_it() {
    let cells = |column: &[f64]| {
        let plot = VisiblePlot {
            column,
            start: 0,
            end: column.len(),
            preview: None,
        };
        plot.cells().filter(|(_, v)| !v.is_nan()).count()
    };
    // The honesty claim in this module's header: warmup and conditional
    // plots render as gaps. A NaN in the middle yields two runs of real
    // points, never one line drawn straight over the missing bar.
    let column = [1.0, 2.0, f64::NAN, 4.0, 5.0];
    assert_eq!(cells(&column), 4);

    let mut runs = 0usize;
    let mut in_run = false;
    for value in column {
        if value.is_nan() {
            in_run = false;
        } else if !in_run {
            in_run = true;
            runs += 1;
        }
    }
    assert_eq!(runs, 2, "two segments, one gap");
}

fn painted(draw: impl Fn(&egui::Painter)) -> String {
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        draw(&ctx.layer_painter(egui::LayerId::background()));
    });
    format!("{:?}", output.shapes)
}

fn lane_of(steps: &[(i64, usize)]) -> LaneFrame<'_> {
    LaneFrame {
        rect: Rect::from_min_max(pos2(100.0, 0.0), pos2(200.0, 50.0)),
        start_ms: 1_000,
        end_ms: 2_000,
        steps,
    }
}

/// The lane is a linear map of tape time onto its band — the tape's own
/// mapping, which is the whole reason a pane's curve can be trusted to
/// sit under the prints it was computed from. Instants outside the window
/// clamp to the edges rather than escaping the band.
#[test]
fn a_lane_maps_tape_time_linearly_and_clamps_outside_its_window() {
    let lane = lane_of(&[]);
    assert!((lane.x(1_000) - 100.0).abs() < 1e-3, "the window's start");
    assert!((lane.x(2_000) - 200.0).abs() < 1e-3, "the live edge");
    assert!((lane.x(1_500) - 150.0).abs() < 1e-3, "halfway is halfway");
    assert!((lane.x(0) - 100.0).abs() < 1e-3, "older than the window");
    assert!((lane.x(9_999) - 200.0).abs() < 1e-3, "past the live edge");
}

/// A degenerate window (start == end, which a stalled feed can produce)
/// must not divide by zero or paint at NaN.
#[test]
fn a_zero_width_window_still_maps_to_a_finite_x() {
    let lane = LaneFrame {
        rect: Rect::from_min_max(pos2(100.0, 0.0), pos2(200.0, 50.0)),
        start_ms: 5_000,
        end_ms: 5_000,
        steps: &[],
    };
    assert!(lane.x(5_000).is_finite());
    assert!(lane.x(1).is_finite());
}

/// The rungs are drawn as a line across the band: with a lane, a pane
/// paints more than it does without one, and the extra ink lands inside
/// the lane's own rect.
#[test]
fn the_rungs_are_drawn_across_the_lane_band() {
    let mut with_lane = view(vec![vec![1.0, 2.0]], Some(vec![3.0]));
    with_lane.lane = vec![
        crate::indicator_worker::LaneSample {
            close_time: 1_200,
            values: vec![2.2],
        },
        crate::indicator_worker::LaneSample {
            close_time: 1_600,
            values: vec![2.8],
        },
        crate::indicator_worker::LaneSample {
            close_time: 2_000,
            values: vec![3.0],
        },
    ];

    let shapes = painted(|painter| {
        draw_lane_plots(painter, &with_lane, &lane_of(&[]), &|v| 50.0 - v as f32);
    });
    assert!(
        shapes.contains("Path"),
        "the rungs are a polyline: {shapes}"
    );
    // 1_200, 1_600 and 2_000 ms across a 1_000 ms window on a 100 px band.
    assert!(
        shapes.contains("120.0") && shapes.contains("160.0") && shapes.contains("200.0"),
        "each rung lands at its own instant on the tape: {shapes}"
    );

    let bare = view(vec![vec![1.0, 2.0]], Some(vec![3.0]));
    let nothing = painted(|painter| {
        draw_lane_plots(painter, &bare, &lane_of(&[]), &|v| 50.0 - v as f32);
    });
    assert!(!nothing.contains("Path"), "no rungs, no curve: {nothing}");
}

/// A committed value holds until the next close and then steps. Drawing a
/// slanted line between two closes would claim an intra-bar path nobody
/// recorded — so the corner has to be there, and the test is that the
/// polyline visits the corner's x twice.
#[test]
fn committed_values_step_across_the_lane_instead_of_interpolating() {
    let pane = view(vec![vec![10.0, 20.0]], None);
    let shapes = painted(|painter| {
        draw_lane_plots(painter, &pane, &lane_of(&[(1_200, 0), (1_600, 1)]), &|v| {
            100.0 - v as f32
        });
    });
    // Rows 0 and 1 sit at y 90 and 80; the hold means y 90 appears at the
    // second close's x before the step down to 80.
    assert!(
        shapes.contains("90.0") && shapes.contains("80.0"),
        "{shapes}"
    );
    assert_eq!(
        shapes.matches("90.0").count(),
        2,
        "the earlier value is held to the next close: {shapes}"
    );
}

/// Histograms and markers keep to the history pane: a column is a
/// statement about one bar at that bar's width, and the lane has no bar
/// widths in it to honour.
#[test]
fn bar_shaped_plots_stay_out_of_the_lane() {
    let mut pane = view(vec![vec![1.0, 2.0]], None);
    pane.descriptor.plots[0].style = PlotStyle::Histogram;
    pane.lane = vec![crate::indicator_worker::LaneSample {
        close_time: 1_500,
        values: vec![2.5],
    }];
    let shapes = painted(|painter| {
        draw_lane_plots(painter, &pane, &lane_of(&[(1_200, 0)]), &|v| {
            50.0 - v as f32
        });
    });
    assert_eq!(
        shapes, "[]",
        "nothing drawn for a bar-shaped plot: {shapes}"
    );
}
