use super::*;

/// The candles' own area — the live lane included, since it shares the
/// price axis, and the indicator band below excluded, since it does not.
const CHART: egui::Rect = egui::Rect {
    min: egui::pos2(0.0, 0.0),
    max: egui::pos2(400.0, 200.0),
};

fn scale() -> PriceScale {
    PriceScale::from_range(100.0, 200.0, CHART.top(), CHART.bottom())
}

fn read(at: egui::Pos2, bar: Option<PointerBar>) -> Option<PointerReadout> {
    readout(Some(at), CHART, &scale(), bar)
}

/// Over the candles, the height is a price and the axis can say which.
#[test]
fn the_pointer_over_the_candles_reads_a_price() {
    let hit = read(egui::pos2(10.0, 100.0), None).expect("over the chart");
    assert!(
        (hit.price - 150.0).abs() < 1e-9,
        "half way down a 100..200 scale is 150, got {}",
        hit.price
    );
}

/// The marks follow the mouse and stop existing when it leaves — the whole
/// difference between a compass and a stale annotation.
#[test]
fn a_pointer_off_the_chart_reads_nothing() {
    assert!(
        readout(None, CHART, &scale(), None).is_none(),
        "no pointer over this pane at all"
    );
    assert!(
        read(egui::pos2(500.0, 100.0), None).is_none(),
        "out in the gutter, past the canvas"
    );
    assert!(
        read(egui::pos2(200.0, 400.0), None).is_none(),
        "below the canvas, on the time strip itself"
    );
}

/// The time half is absent where no bar is, and the readout says so by
/// carrying `None` rather than a number nobody can stand behind.
#[test]
fn empty_canvas_past_the_newest_bar_reads_a_price_but_no_time() {
    let hit = read(egui::pos2(390.0, 50.0), None).expect("over the chart");
    assert!(hit.price.is_finite(), "the height is still a price");
    assert_eq!(hit.bar, None, "no bar under the pointer, so no instant");
}

/// And where a bar *is* under the pointer, both halves are there.
#[test]
fn a_bar_under_the_pointer_reads_its_opening_instant() {
    let bar = PointerBar {
        slot: 7,
        open_time_unix_ms: 1_700_000_000_000,
        interval_ms: None,
    };
    let hit = read(egui::pos2(120.0, 50.0), Some(bar)).expect("over the chart");
    assert_eq!(hit.bar, Some(bar));
}

/// The scale is the one the axis labels are drawn from, upside down
/// included: a compass that reported the upright price on an inverted
/// chart would be worse than no compass.
#[test]
fn the_reading_follows_the_chart_upside_down() {
    let inverted = scale().with_inverted(true);
    let hit = readout(Some(egui::pos2(10.0, 0.0)), CHART, &inverted, None).expect("over the chart");
    assert!(
        (hit.price - 100.0).abs() < 1e-9,
        "the top of an inverted 100..200 scale is the low, got {}",
        hit.price
    );
}
