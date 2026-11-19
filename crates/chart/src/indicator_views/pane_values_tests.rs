use super::*;
use quantick_indicator_session::SlotId;
use quantick_indicators::{IndicatorDescriptor, PlotId, PlotSpec, PlotStyle, PreviewFrame, Rgba8};

fn marker_view(columns: Vec<Vec<f64>>, marker_at: usize) -> IndicatorView {
    let mut v = view(columns, None);
    v.descriptor.plots[marker_at].marker = Some(quantick_indicators::MarkerSpec {
        shape: quantick_indicators::MarkerShape::Circle,
        location: quantick_indicators::MarkerLocation::AboveBar,
        text: None,
    });
    v
}

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
        sizing: crate::indicator_views::PaneSizing::Auto,
        inks: Vec::new(),
        last_auto: None,
    }
}

#[test]
fn value_range_ignores_warmup_and_reports_none_when_all_of_it_is() {
    let all_nan = view(vec![vec![f64::NAN, f64::NAN]], None);
    assert!(
        value_range(&all_nan, 0, 2).is_none(),
        "a pane with nothing computed yet has no range to fit"
    );

    let mixed = view(vec![vec![f64::NAN, 3.0, -1.0, f64::NAN]], None);
    assert_eq!(value_range(&mixed, 0, 4), Some((-1.0, 3.0)));

    // The forming bar's value belongs to the fit; the window does not
    // clip it away.
    let with_preview = view(vec![vec![1.0, 2.0]], Some(vec![9.0]));
    assert_eq!(value_range(&with_preview, 0, 2), Some((1.0, 9.0)));
}

#[test]
fn a_marker_column_is_a_flag_and_never_scales_the_pane() {
    // The staged 1.0 marks "the condition fired here"; it is never used
    // as a y, because a non-absolute marker anchors to the pane edges.
    let mixed = marker_view(vec![vec![1.0, 1.0], vec![1.0e6, 1.1e6]], 0);
    assert_eq!(
        value_range(&mixed, 0, 2),
        Some((1.0e6, 1.1e6)),
        "the real plot keeps the pane"
    );
}

#[test]
fn last_value_prefers_the_preview_and_skips_trailing_warmup() {
    let previewed = view(vec![vec![1.0, 2.0]], Some(vec![7.5]));
    assert_eq!(last_value(&previewed), Some(7.5));

    // A NaN preview cell is "nothing to draw", not a value.
    let nan_preview = view(vec![vec![1.0, 2.0]], Some(vec![f64::NAN]));
    assert_eq!(last_value(&nan_preview), Some(2.0));

    let trailing_nan = view(vec![vec![1.0, 2.0, f64::NAN]], None);
    assert_eq!(last_value(&trailing_nan), Some(2.0));

    assert_eq!(last_value(&view(vec![vec![f64::NAN]], None)), None);
}

#[test]
fn the_auto_range_pads_the_values_and_has_nothing_to_pad_during_warmup() {
    let pane = view(vec![vec![0.0, 10.0]], None);
    let (lo, hi) = pane_auto_range(&pane, 0, 2).expect("two values are a range");
    assert!(lo < 0.0 && hi > 10.0, "the trace never touches the edges");
    assert!(
        (lo + 0.8).abs() < 1e-9 && (hi - 10.8).abs() < 1e-9,
        "8% of the span on each side: {lo}..{hi}"
    );

    assert!(
        pane_auto_range(&view(vec![vec![f64::NAN]], None), 0, 1).is_none(),
        "nothing computed yet is nothing to scale — and no axis to draw"
    );
}
