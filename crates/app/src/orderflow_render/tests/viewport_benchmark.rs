//! Manual same-recording comparison of the production sphere mesh path.
use super::*;
use rust_decimal::{Decimal, prelude::ToPrimitive as _};
use std::time::Instant;

#[test]
#[ignore = "manual same-recording renderer timings; run without concurrent builds"]
fn recorded_viewport_sphere_renderer_benchmark() {
    let csv = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../stores/tests/fixtures/btc/perpetual-busy.csv"
    ));
    let trades: Vec<Trade> = csv
        .lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<_> = line.split(',').collect();
            Trade {
                agg_id: fields[0].parse().unwrap(),
                timestamp_ms: fields[1].parse().unwrap(),
                price: fields[2].parse().unwrap(),
                quantity: fields[3].parse().unwrap(),
                side: if fields[4] == "buy" {
                    Side::Buy
                } else {
                    Side::Sell
                },
            }
        })
        .collect();
    let bank = quantick_stores::bubble_presets::embedded();
    let tick = bank
        .get("Binance BTC perpetual spheres")
        .unwrap()
        .tick_bubbles
        .unwrap();
    let mut baseline = tick;
    baseline.viewport_scale = None;
    let range = (
        trades
            .iter()
            .map(|t| t.price)
            .min()
            .unwrap()
            .to_f64()
            .unwrap()
            - 1.0,
        trades
            .iter()
            .map(|t| t.price)
            .max()
            .unwrap()
            .to_f64()
            .unwrap()
            + 1.0,
    );
    let slots = trades.len().div_ceil(50);
    for px_per_bar in [1.662477, 22.0] {
        let frame = Arc::new(project_flow_tape(
            trades
                .iter()
                .enumerate()
                .map(|(ordinal, trade)| FlowExecution {
                    ordinal,
                    slot: ordinal / 50,
                    accepted_ordinal: ordinal % 50,
                    ticks_per_bar: 50.into(),
                    trade,
                    opening: false,
                }),
            1,
            trades.len(),
            trades.len(),
            FlowTapeView {
                first_slot: 0,
                end_slot: slots,
                clip_left: Decimal::ZERO,
                clip_right: slots.into(),
                width_px: slots as f32 * px_per_bar,
                height_px: 600.0,
                prices: PriceWindow::from_f64_range(range).unwrap(),
                reference: FlowReference::from(tick),
                radius_limit: tick.max_radius,
                merge_support_radius: 6.0,
                exclude_opening: false,
            },
        ));
        let history = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
        for moving in [false, true] {
            let contexts = [egui::Context::default(), egui::Context::default()];
            let mut samples: [Vec<f64>; 2] = Default::default();
            for iteration in 0..35 {
                let factor = if moving {
                    1.0 + iteration as f64 * 0.002
                } else {
                    1.0
                };
                let mut viewport = Viewport::new();
                viewport.set_px_per_bar(px_per_bar);
                let geometry = FlowExecutionGeometry::new(
                    viewport,
                    slots,
                    0,
                    (range.0, range.0 + (range.1 - range.0) * factor),
                    [0.0, 0.0, 1000.0, 600.0],
                    false,
                )
                .unwrap();
                for arm in if iteration % 2 == 0 { [0, 1] } else { [1, 0] } {
                    let style = if arm == 0 { baseline } else { tick };
                    let started = Instant::now();
                    let output = contexts[arm].run(
                        egui::RawInput {
                            screen_rect: Some(history),
                            ..Default::default()
                        },
                        |context| {
                            let painter = context.layer_painter(egui::LayerId::background());
                            let drawing = cached_flow(
                                &painter,
                                history,
                                &frame,
                                geometry,
                                crate::theme::CANVAS,
                                None,
                                Some((style, quantick_orderflow::HeatmapTheme::default())),
                            );
                            drawing.paint(&painter, history, false);
                            drawing.paint(&painter, history, true);
                        },
                    );
                    std::hint::black_box(
                        contexts[arm].tessellate(output.shapes, output.pixels_per_point),
                    );
                    if iteration >= 5 {
                        samples[arm].push(started.elapsed().as_secs_f64() * 1000.0);
                    }
                }
            }
            for (arm, mut timings) in samples.into_iter().enumerate() {
                timings.sort_by(f64::total_cmp);
                eprintln!(
                    "BTC_VIEWPORT_RENDER arm={} prints={} groups={} px_per_bar={px_per_bar} moving={moving} mean_ms={:.6} p95_ms={:.6}",
                    if arm == 0 {
                        "c33_policy"
                    } else {
                        "visible_camera_policy"
                    },
                    trades.len(),
                    frame.dots.len(),
                    timings.iter().sum::<f64>() / timings.len() as f64,
                    timings[timings.len() * 95 / 100]
                );
            }
        }
    }
}
