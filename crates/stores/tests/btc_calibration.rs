//! Public Binance aggregate executions, quiet/busy completed minute windows.
//! Capture requests and exchange-volume cross-checks live beside each fixture.
//! Different market windows calibrate each source; they do not compare liquidity.

use std::str::FromStr as _;

use quantick_engine::{Bar, Side, Trade};
use quantick_orderflow::projection::{
    TapeDotGeometry, TapeDotMemory, TapeDotView, project_live, project_settled,
};
use quantick_orderflow::{
    BarTimeline, BubbleSizeReference, DotSizing, HeatmapConfig, LiquidityHistory, LiveEdge,
    PriceWindow, VolumeDots, bubble_radius,
};
use quantick_stores::bubble_presets;
use rust_decimal::Decimal;
#[path = "btc_tick_calibration/mod.rs"]
mod tick;

fn dec(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

fn recordings() -> [(&'static str, &'static str, &'static str); 4] {
    [
        (
            "spot quiet",
            include_str!("fixtures/btc/spot-quiet.csv"),
            "2.80847000",
        ),
        (
            "spot busy",
            include_str!("fixtures/btc/spot-busy.csv"),
            "28.27907000",
        ),
        (
            "perpetual quiet",
            include_str!("fixtures/btc/perpetual-quiet.csv"),
            "30.446",
        ),
        (
            "perpetual busy",
            include_str!("fixtures/btc/perpetual-busy.csv"),
            "403.007",
        ),
    ]
}

fn trades(csv: &str) -> Vec<Trade> {
    csv.lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<_> = line.split(',').collect();
            Trade {
                agg_id: fields[0].parse().unwrap(),
                timestamp_ms: fields[1].parse().unwrap(),
                price: dec(fields[2]),
                quantity: dec(fields[3]),
                side: match fields[4] {
                    "buy" => Side::Buy,
                    "sell" => Side::Sell,
                    _ => panic!("recorded aggressor side"),
                },
            }
        })
        .collect()
}

fn config(native: bool, reference: f64) -> HeatmapConfig {
    let mut config = HeatmapConfig {
        enabled: false,
        show_aggressions: true,
        ..HeatmapConfig::default()
    };
    bubble_presets::embedded()
        .get("default")
        .unwrap()
        .apply_to(&mut config);
    config.live_lane.enabled = true;
    config.live_lane.show_aggressions = true;
    if native {
        config.volume_dots.enabled = true;
        config.volume_dots.auto_full = false;
        config.volume_dots.full_quantity = reference;
        config.live_lane.native_tape = true;
        config.bubbles.size_reference = BubbleSizeReference::Fixed;
        config.bubbles.size_reference_quantity = reference;
        config.bubbles.max_radius = 26.0;
        config.bubbles.label_min_radius = 12.0;
        config.bubbles.show_trade_count = false;
    }
    config
}

fn projected(
    prints: &[Trade],
    mut config: HeatmapConfig,
    perpetual: bool,
) -> (
    Vec<quantick_orderflow::AggressionPrimitive>,
    Decimal,
    PriceWindow,
) {
    config.price_grouping = if perpetual { dec("0.1") } else { dec("0.01") };
    let mut history = LiquidityHistory::new(config);
    let mut partial = Bar::opened_by(&prints[0]);
    for (index, trade) in prints.iter().enumerate() {
        history.record_aggression(trade);
        if index > 0 {
            partial.extend(trade);
        }
    }
    let now = prints[0].timestamp_ms / 60_000 * 60_000 + 60_000;
    let timeline = BarTimeline::from_bars(
        0,
        &[],
        Some(&partial),
        Some(LiveEdge {
            now_ms: now,
            window_ms: 60_000,
            reference_ms: 60_000,
            on_newest_bar: true,
        }),
    )
    .with_full_lane_coverage();
    let prices = PriceWindow::new(partial.low - Decimal::ONE, partial.high + Decimal::ONE).unwrap();
    let dots = history.config().volume_dots.enabled.then_some(VolumeDots {
        native_tape: true,
        tape_window_ms: 100,
        tape_level_ticks: 1,
        candle_level_ticks: 1,
        bars: vec![(partial.open_time, partial.close_time)],
        forming: Some(partial.open_time),
    });
    let settled = project_settled(&history, &timeline, prices, dots.as_ref());
    let live = project_live(&history, &timeline, prices, &settled, dots.as_ref());
    let projection = settled.with_live(live, history.config());
    (
        projection
            .aggressions
            .into_iter()
            .filter(|mark| mark.live)
            .collect(),
        projection.aggression_reference,
        prices,
    )
}

#[test]
fn recordings_match_exchange_minute_volumes_and_preserve_native_buy_sell_totals() {
    for (name, csv, volume) in recordings() {
        let prints = trades(csv);
        assert_eq!(
            prints.iter().map(|trade| trade.quantity).sum::<Decimal>(),
            dec(volume),
            "{name}"
        );
        let (marks, _, _) = projected(&prints, config(true, 30.0), name.starts_with("perpetual"));
        assert_eq!(
            marks.iter().map(|mark| mark.quantity).sum::<Decimal>(),
            dec(volume),
            "{name}"
        );
        assert_eq!(
            marks.iter().map(|mark| mark.buy_quantity).sum::<Decimal>(),
            prints
                .iter()
                .filter(|trade| trade.side == Side::Buy)
                .map(|trade| trade.quantity)
                .sum::<Decimal>(),
            "{name}"
        );
        assert_eq!(
            marks.iter().map(|mark| mark.trade_count).sum::<usize>(),
            prints.len(),
            "{name}"
        );
    }
}

#[test]
#[ignore = "manual actual-recording grouping, dust and saturation calibration"]
fn report_btc_projection_calibration() {
    for (name, csv, _) in recordings() {
        let prints = trades(csv);
        let perpetual = name.starts_with("perpetual");
        let candidates = if perpetual {
            vec![
                (0.0, 15.0),
                (30.0, 40.0),
                (60.0, 40.0),
                (80.0, 48.0),
                (120.0, 48.0),
                (160.0, 48.0),
            ]
        } else {
            vec![
                (0.0, 15.0),
                (2.0, 30.0),
                (5.0, 36.0),
                (8.0, 40.0),
                (12.0, 48.0),
            ]
        };
        for (reference, cap) in candidates {
            let native = reference > 0.0;
            let mut config = config(native, reference);
            config.bubbles.max_radius = cap;
            config.bubbles.label_min_radius = 10.0;
            let (marks, full, prices) = projected(&prints, config.clone(), perpetual);
            let (drawn, radii, effective_cap, effective_full) = if native {
                let mut memory = TapeDotMemory::default();
                let sizing = DotSizing {
                    native_tape: true,
                    tape_column_px: 1.0,
                    candle_column_px: 1.0,
                    px_per_price: 1.0,
                    typed_full: Some(Decimal::from_f64_retain(reference).unwrap()),
                };
                let frame = memory.project(
                    &marks,
                    TapeDotView {
                        now_ms: prints[0].timestamp_ms / 60_000 * 60_000 + 60_000,
                        window_ms: 60_000,
                        dot_window_ms: 100,
                        evicted_through_ms: None,
                        prices,
                        geometry: TapeDotGeometry {
                            left_x: 0.0,
                            right_x: 1.0,
                            width_px: 640.0,
                            height_px: 400.0,
                        },
                    },
                    sizing,
                    &config.bubbles,
                    &config.live_lane,
                    &[],
                );
                let mut bubbles = config.bubbles.clone();
                bubbles.max_radius = frame.max_radius;
                let radii: Vec<_> = frame
                    .marks
                    .iter()
                    .map(|mark| {
                        sizing.radius(&bubbles, &config.live_lane, mark, frame.full_quantity)
                    })
                    .collect();
                (frame.marks, radii, frame.max_radius, frame.full_quantity)
            } else {
                let radii = marks
                    .iter()
                    .map(|mark| {
                        bubble_radius(
                            mark.size,
                            config.bubbles.min_radius,
                            config.bubbles.max_radius,
                        )
                    })
                    .collect();
                (marks.clone(), radii, config.bubbles.max_radius, full)
            };
            let mut sorted = radii.clone();
            sorted.sort_by(f32::total_cmp);
            let labels = radii
                .iter()
                .filter(|radius| **radius >= config.bubbles.label_min_radius)
                .count();
            let saturated = radii
                .iter()
                .filter(|radius| **radius >= effective_cap * 0.999)
                .count();
            let quantities = drawn.iter().map(|mark| mark.quantity).sum::<Decimal>();
            let capped_quantity = drawn
                .iter()
                .zip(&radii)
                .filter(|(_, radius)| **radius >= effective_cap * 0.999)
                .map(|(mark, _)| mark.quantity)
                .sum::<Decimal>();
            eprintln!(
                "BTC_CALIBRATION {name} native={native} typed={reference} raw={} cells={} drawn={} exact_quantity={quantities} capped_quantity={capped_quantity} full={effective_full} cap={effective_cap:.2} radii_p10={:.2} p50={:.2} p90={:.2} max={:.2} labels_eligible={labels} saturated={saturated}",
                prints.len(),
                marks.len(),
                drawn.len(),
                sorted[sorted.len() / 10],
                sorted[sorted.len() / 2],
                sorted[sorted.len() * 9 / 10],
                sorted.last().unwrap()
            );
        }
    }
}
