use super::*;
use crate::timezone::TzOffset;
use quantick_chart::time_marks::fmt_calendar_label;

/// The pointer's tag on a day, week or month chart names the bucket's
/// date, not the display zone's clock at its UTC midnight.
#[test]
fn the_pointer_tag_names_the_date_on_a_calendar_chart() {
    use quantick_engine::time_bucket::{CALENDAR_MONTH_MS, DAY_MS, WEEK_MS};
    let oct_7 = 1_791_331_200_000;
    let brt = TzOffset::new(-180);
    assert_eq!(
        fmt_pointer_time(oct_7, Some(DAY_MS), brt),
        "Wed 07 Oct 2026"
    );
    assert_eq!(
        fmt_pointer_time(oct_7, Some(WEEK_MS), brt),
        "Mon 05 Oct 2026"
    );
    assert_eq!(
        fmt_pointer_time(oct_7, Some(CALENDAR_MONTH_MS), brt),
        "Oct 2026"
    );
    assert_eq!(
        fmt_pointer_time(oct_7, Some(3 * CALENDAR_MONTH_MS), brt),
        "Oct 2026"
    );
    // Below a day the tag is the instant with its date in the zone.
    let tag = "Tue 06 Oct '26 21:00:00";
    assert_eq!(fmt_pointer_time(oct_7, Some(60_000), brt), tag);
    assert_eq!(fmt_pointer_time(oct_7, None, brt), tag);
}

#[test]
fn a_calendar_chart_labels_its_bars_by_date() {
    use quantick_engine::time_bucket::{CALENDAR_MONTH_MS, DAY_MS, WEEK_MS};
    // 2025-12-29 (a Monday) and 2026-10-07 (a Wednesday), 00:00 UTC.
    let dec_29 = 20_451 * DAY_MS;
    let oct_7 = 20_733 * DAY_MS;
    assert_eq!(
        fmt_calendar_label(oct_7, 3_600_000, None),
        None,
        "1h keeps clock times"
    );
    assert_eq!(
        fmt_calendar_label(oct_7, DAY_MS, None).as_deref(),
        Some("07 Oct")
    );
    assert_eq!(
        fmt_calendar_label(oct_7 + 5 * 3_600_000, DAY_MS, Some(oct_7 - DAY_MS)).as_deref(),
        Some("07 Oct"),
        "the bucket's date, whatever time the bar's first trade had"
    );
    assert_eq!(
        fmt_calendar_label(dec_29 + 3 * DAY_MS, DAY_MS, Some(dec_29)).as_deref(),
        Some("2026"),
        "the year where it changes"
    );
    assert_eq!(
        fmt_calendar_label(oct_7, WEEK_MS, None).as_deref(),
        Some("05 Oct"),
        "a week by its Monday"
    );
    assert_eq!(
        fmt_calendar_label(oct_7, CALENDAR_MONTH_MS, None).as_deref(),
        Some("Oct 2026")
    );
    assert_eq!(
        fmt_calendar_label(oct_7, 3 * CALENDAR_MONTH_MS, None).as_deref(),
        Some("2026")
    );
    assert_eq!(calendar_label_sample(DAY_MS), Some("00 Mmm"));
}

#[test]
fn the_live_strip_carves_between_chart_and_gutter_only_when_shown() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

    let off = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
    assert!(off.live_strip.is_none());
    assert_eq!(off.chart.right(), off.price_gutter.left());

    let on = plot_split(
        area,
        crate::live_strip::LIVE_STRIP_WIDTH_PX,
        &[crate::indicators::PaneSizing::Auto; 0],
    );
    let strip = on.live_strip.expect("strip rect");
    assert_eq!(on.chart.right(), strip.left());
    assert_eq!(strip.right(), on.price_gutter.left());
    assert_eq!(strip.width(), crate::live_strip::LIVE_STRIP_WIDTH_PX);
    // The strip pays with the chart's pixels: the gutter stays put, and
    // the time axis keeps spanning exactly the chart body.
    assert_eq!(on.price_gutter, off.price_gutter);
    assert_eq!(
        on.chart.width(),
        off.chart.width() - crate::live_strip::LIVE_STRIP_WIDTH_PX
    );
    assert_eq!(on.time_strip.right(), on.chart.right());
}

/// The pane band is carved once, inside `plot_split`, so the rect the
/// renderer scales prices to is the rect the input handler hit-tests
/// against. When the two disagreed, a drawing was placed where you
/// clicked and then selected somewhere else — by 20% of the chart height
/// per visible pane.
#[test]
fn the_pane_band_comes_out_of_every_callers_chart_rect() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

    let none = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
    assert!(none.indicator_panes.is_empty());

    let one = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 1]);
    let pane = *one
        .indicator_panes
        .first()
        .expect("one visible pane claims one rect");
    assert!(
        one.chart.height() < none.chart.height(),
        "the band is paid for out of the candles' pixels"
    );
    assert_eq!(one.chart.bottom(), pane.rect.top(), "no gap, no overlap");
    assert_eq!(pane.rect.bottom(), none.chart.bottom());
    assert_eq!(one.chart.width(), none.chart.width());
    // The axes keep their column; the time strip is untouched.
    assert_eq!(one.price_gutter.x_range(), none.price_gutter.x_range());
    assert_eq!(one.time_strip, none.time_strip);

    let three = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 3]);
    assert_eq!(three.indicator_panes.len(), 3);
    assert!(three.chart.height() < one.chart.height());
}

/// The gutter is banded like the body it labels. Before it was, the whole
/// column belonged to the candles: dragging the numbers beside a CVD pane
/// stretched the *price* scale, and the pane — which had no axis at all —
/// did not move.
#[test]
fn every_pane_owns_the_gutter_band_beside_it() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

    let none = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 0]);
    assert!(none.pane_gutters.is_empty());
    assert_eq!(
        none.price_gutter.bottom(),
        none.chart.bottom(),
        "with no pane the gutter is the candles', top to bottom"
    );

    let two = plot_split(area, 0.0, &[crate::indicators::PaneSizing::Auto; 2]);
    assert_eq!(two.pane_gutters.len(), two.indicator_panes.len());
    assert_eq!(
        two.price_gutter.bottom(),
        two.chart.bottom(),
        "the candles' scale stops where the candles do"
    );
    for (pane, gutter) in two.indicator_panes.iter().zip(&two.pane_gutters) {
        assert_eq!(
            gutter.y_range(),
            pane.rect.y_range(),
            "band beside its pane"
        );
        assert_eq!(gutter.x_range(), two.price_gutter.x_range(), "one column");
    }
    // No pixel answers to two scales: the bands tile the gutter exactly.
    assert_eq!(two.price_gutter.bottom(), two.pane_gutters[0].top());
    assert_eq!(two.pane_gutters[0].bottom(), two.pane_gutters[1].top());

    // The strip pays out of the candles, not the gutter: the pane bands
    // keep the same column when the tape is shown.
    let with_strip = plot_split(
        area,
        crate::live_strip::LIVE_STRIP_WIDTH_PX,
        &[crate::indicators::PaneSizing::Auto; 2],
    );
    assert_eq!(with_strip.pane_gutters, two.pane_gutters);
}

/// Each pane zooms from the strip under it, and the split is exactly the
/// divider — so a drag can never mean both time axes at once.
#[test]
fn the_time_strip_splits_at_the_lane_divider() {
    let strip = egui::Rect::from_min_max(egui::pos2(0.0, 580.0), egui::pos2(1000.0, 600.0));

    let (history, lane) = split_time_strip(strip, Some(700.0));
    let lane = lane.expect("the lane owns the strip under it");
    assert_eq!(history.left(), strip.left());
    assert_eq!(history.right(), 700.0);
    assert_eq!(lane.left(), 700.0);
    assert_eq!(lane.right(), strip.right());

    // Without a lane the candles keep the whole strip, exactly as before.
    assert_eq!(split_time_strip(strip, None), (strip, None));
    // A divider off the strip is not a split either.
    assert_eq!(split_time_strip(strip, Some(-5.0)), (strip, None));
}

#[test]
fn fmt_time_in_utc() {
    // Epoch: 1970-01-01 00:00:00 UTC, then +1h 2m 3s.
    assert_eq!(fmt_time(0, TzOffset::new(0)), "00:00:00");
    assert_eq!(fmt_time(3_723_000, TzOffset::new(0)), "01:02:03");
}

#[test]
fn fmt_time_applies_the_offset() {
    // UTC midnight shown in UTC−03:00 is 21:00 of the previous day.
    assert_eq!(fmt_time(0, TzOffset::new(-180)), "21:00:00");
    // UTC midnight in UTC+05:30 is 05:30.
    assert_eq!(fmt_time(0, TzOffset::new(330)), "05:30:00");
}
