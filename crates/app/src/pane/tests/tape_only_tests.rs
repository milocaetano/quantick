//! A pane in tape-only mode fits its price axis to the tape, never to the
//! candles it no longer draws.

use eframe::egui;
use quantick_engine::Bar;
use rust_decimal::Decimal;

use crate::orderflow_view::LiveLane;
use crate::pane::ChartPane;
use crate::pane::frame_layout::{FrameLayout, Series};
use crate::price_view::PriceView;
use crate::state::BarSpec;

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

/// The candles span 400..1600; the tape traded 995..1015 and last printed
/// at 1005. Tape only, the axis is the tape's range plus the fit's margin;
/// otherwise the candles still decide it.
#[test]
fn a_tape_only_pane_fits_its_price_axis_to_the_tape_alone() {
    let pane = ChartPane::flow(1, BarSpec::Tick(50), "TESTUSDT".to_owned());
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1_000.0, 600.0));
    let areas = pane.plot_areas(rect, crate::config::FeedCapabilities::none());
    let chart = areas.chart;
    let bars = [bar(400, 1_600, 1_000), bar(990, 1_010, 1_005)];
    let layout = |tape_only: bool| FrameLayout {
        areas: pane.plot_areas(rect, crate::config::FeedCapabilities::none()),
        chart_rect: chart,
        history_rect: if tape_only {
            egui::Rect::from_min_max(chart.min, egui::pos2(chart.left(), chart.bottom()))
        } else {
            egui::Rect::from_min_max(chart.min, egui::pos2(chart.right() - 350.0, chart.bottom()))
        },
        live_lane: Some(LiveLane {
            width_px: if tape_only { chart.width() } else { 350.0 },
            end_ms: 0,
        }),
        total: bars.len(),
        closed_total: bars.len(),
        start: 0,
        end: bars.len(),
        cw: 8.0,
        indicator_guide_x: None,
        tape_only,
    };
    let ctx = egui::Context::default();
    let painter = egui::Painter::new(ctx, egui::LayerId::background(), rect);
    let fitted = |tape_only: bool| {
        let layout = layout(tape_only);
        layout
            .resolve(
                &painter,
                Series {
                    prefix: &[],
                    closed: &bars,
                    partial: None,
                },
                Some((995.0, 1_015.0)),
                None,
                &PriceView::new(),
                egui::Color32::BLACK,
            )
            .expect("a scale")
            .1
    };
    let (lo, hi) = fitted(true);
    assert!((lo - 994.0).abs() < 1e-9, "{lo}");
    assert!((hi - 1_016.0).abs() < 1e-9, "{hi}");
    let (lo, hi) = fitted(false);
    assert!(lo < 400.0 && hi > 1_600.0, "the candles still decide: {lo} {hi}");
}
