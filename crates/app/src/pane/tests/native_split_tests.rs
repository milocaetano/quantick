//! Candles beside the native tape share one price axis, and the tape fits
//! it: the candles never widen the range the tape is read on.

use eframe::egui;
use quantick_engine::Bar;
use rust_decimal::Decimal;

use crate::orderflow_view::LiveLane;
use crate::pane::ChartPane;
use crate::pane::frame_layout::{FrameLayout, Series};
use crate::price_view::PriceView;
use crate::state::BarSpec;

const LANE_PX: f32 = 350.0;

fn bar(low: i64, high: i64, close: i64) -> Bar {
    Bar {
        open_time: 0,
        close_time: 0,
        open: Decimal::from(close),
        high: Decimal::from(high),
        low: Decimal::from(low),
        close: Decimal::from(close),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 1,
    }
}

/// The candles span 400..1600 left of the divider; the tape beside them
/// traded 995..1015 and last printed at 1005. With the native tape on, the
/// one axis is the tape's range plus the fit's margin and the candles are
/// drawn against it; with the tape off the candles fit their own range.
#[test]
fn a_native_split_pane_fits_the_shared_axis_to_the_tape_beside_its_candles() {
    let pane = ChartPane::flow(1, BarSpec::Tick(50), "WINV26".to_owned());
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1_000.0, 600.0));
    let chart = pane
        .plot_areas(rect, crate::config::FeedCapabilities::none())
        .chart;
    let bars = [bar(400, 1_600, 1_000), bar(990, 1_010, 1_005)];
    let layout = |native_tape: bool| FrameLayout {
        areas: pane.plot_areas(rect, crate::config::FeedCapabilities::none()),
        chart_rect: chart,
        history_rect: egui::Rect::from_min_max(
            chart.min,
            egui::pos2(chart.right() - LANE_PX, chart.bottom()),
        ),
        live_lane: Some(LiveLane {
            width_px: LANE_PX,
            end_ms: 0,
        }),
        total: bars.len(),
        closed_total: bars.len(),
        start: 0,
        end: bars.len(),
        cw: 8.0,
        indicator_guide_x: None,
        tape_only: false,
        native_tape,
        tape_padding_px: 0.0,
    };
    assert_eq!(
        layout(true).lane_width_px(),
        LANE_PX,
        "beside the candles the tape keeps its share"
    );
    let ctx = egui::Context::default();
    let painter = egui::Painter::new(ctx, egui::LayerId::background(), rect);
    let fitted = |native_tape: bool, price_view: &PriceView, tape_range| {
        layout(native_tape)
            .resolve(
                &painter,
                Series {
                    prefix: &[],
                    closed: &bars,
                    partial: None,
                },
                tape_range,
                None,
                price_view,
                egui::Color32::BLACK,
            )
            .expect("a scale")
            .0
            .scale
    };
    let tape_range = Some((995.0, 1_015.0));
    let (lo, hi) = fitted(true, &PriceView::new(), tape_range).range();
    assert!((lo - 994.0).abs() < 1e-9, "{lo}");
    assert!((hi - 1_016.0).abs() < 1e-9, "{hi}");
    let (lo, hi) = fitted(false, &PriceView::new(), tape_range).range();
    assert!(
        lo < 400.0 && hi > 1_600.0,
        "with the tape off the candles decide again: {lo} {hi}"
    );

    // Manual Y stays the trader's: the shared axis honours it, and new prints
    // cannot replace it.
    let mut manual = PriceView::new();
    manual.pan(10_000.0, (994.0, 1_016.0));
    assert_eq!(
        fitted(true, &manual, tape_range).range(),
        (10_994.0, 11_016.0)
    );
    assert_eq!(
        fitted(true, &manual, Some((900.0, 1_200.0))).range(),
        (10_994.0, 11_016.0)
    );
}
