//! Candles beside the native tape share a price axis fitted to both sources.

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
/// one axis includes both sources, and manual Y still belongs to the trader.
#[test]
fn a_native_split_pane_fits_the_shared_axis_to_visible_candles_and_tape() {
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
    let mut offline_layout = layout(true);
    offline_layout.live_lane = None;
    assert_eq!(offline_layout.lane_width_px(), LANE_PX);
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
    assert_eq!((lo, hi), (340.0, 1_660.0));
    assert_eq!(
        fitted(true, &PriceView::new(), Some((300.0, 1_800.0))).range(),
        (225.0, 1_875.0),
        "tape prices outside the candles also remain on the shared axis"
    );
    let mut inverted = PriceView::new();
    inverted.set_inverted(true);
    let inverted_scale = fitted(true, &inverted, tape_range);
    assert_eq!(inverted_scale.range(), (lo, hi));
    assert!(inverted_scale.y(400.0) < inverted_scale.y(1_600.0));
    let (lo, hi) = fitted(false, &PriceView::new(), tape_range).range();
    assert!(
        lo < 400.0 && hi > 1_600.0,
        "with the tape off the candles decide again: {lo} {hi}"
    );

    // Manual Y stays the trader's: the shared axis honours it, and new prints
    // cannot replace it.
    let mut manual = PriceView::new();
    manual.pan(10_000.0, (994.0, 1_016.0));
    manual.set_inverted(true);
    assert_eq!(
        fitted(true, &manual, tape_range).range(),
        (10_994.0, 11_016.0)
    );
    assert_eq!(
        fitted(true, &manual, Some((900.0, 1_200.0))).range(),
        (10_994.0, 11_016.0)
    );
    assert!(fitted(true, &manual, tape_range).is_inverted());

    // The viewport straddles venue history and engine bars. Neither an old
    // off-screen extreme nor a forming candle outside it can stretch Auto Y.
    let prefix = [bar(-10_000, 20_000, 1_000), bar(700, 900, 800)];
    let closed = [bar(990, 1_010, 1_005)];
    let partial = bar(850, 1_150, 1_005);
    for (start, end, expected) in [
        (1, 3, (684.25, 1_030.75)),
        (1, 4, (677.5, 1_172.5)),
        (2, 3, (988.75, 1_016.25)),
    ] {
        let mut layout = layout(true);
        layout.total = 4;
        layout.closed_total = 3;
        layout.start = start;
        layout.end = end;
        let (frame, _, _) = layout
            .resolve(
                &painter,
                Series {
                    prefix: &prefix,
                    closed: &closed,
                    partial: Some(&partial),
                },
                tape_range,
                Some((-10_000.0, 20_000.0)),
                &PriceView::new(),
                egui::Color32::BLACK,
            )
            .expect("visible candles and tape have a scale");
        assert_eq!(frame.scale.range(), expected, "slots {start}..{end}");
    }
}
