//! Cached tape placement and real worker-to-painter latency.
use super::*;
use quantick_engine::Side;
use quantick_orderflow::projection::PaneGeometry;

#[test]
fn zooming_the_tape_repositions_the_cached_frame_before_the_worker_answers() {
    let mut view = OrderflowView::new("WINV26");
    let before = view.config.clone();
    assert!(view.apply_preset("mini index regions"));
    view.commit_config_changes(before);
    view.set_live_lane_window(LaneWindow::Fixed { ms: 10_000 });
    let mut trade = Trade {
        agg_id: 1,
        timestamp_ms: 0,
        price: Decimal::from(100),
        quantity: Decimal::from(10),
        side: Side::Buy,
    };
    view.record_trade(&trade);
    let mut partial = Bar::opened_by(&trade);
    trade.agg_id = 2;
    trade.timestamp_ms = 1_000;
    view.record_trade(&trade);
    partial.extend(&trade);
    view.set_replay_clock_at(9_000, Some(1_000), None);
    let _ = view.project_visible(
        VisibleBarTimeline::new(1, 0, &[], Some(&partial)),
        true,
        true,
        Some(10_000),
        (90.0, 110.0),
        Some(PaneGeometry {
            px_per_bar: 10.0,
            lane_width_px: 600.0,
            lane_window_ms: 10_000,
            height_px: 400.0,
            lane_bars: vec![(0, 1_000)],
        }),
    );
    view.flush_for_test();
    let frame = view.published.frame.clone().expect("the tape projected");
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 400.0));
    let ink = |view: &OrderflowView| {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            view.draw_aggressions(
                &ctx.layer_painter(egui::LayerId::background()),
                rect,
                &viewport,
                1,
                &frame,
                egui::Color32::BLACK,
                rect.width(),
                false,
                (90.0, 110.0),
            );
        });
        output
            .shapes
            .iter()
            .filter(|shape| matches!(shape.shape, egui::Shape::Circle(_) | egui::Shape::Mesh(_)))
            .count()
    };
    assert!(
        ink(&view) > 0,
        "an eight-second-old print is inside ten seconds"
    );
    view.set_live_lane_window(LaneWindow::Fixed { ms: 5_000 });
    assert_eq!(
        frame.live_edge.unwrap().window_ms,
        10_000,
        "the cached frame has not changed"
    );
    assert_eq!(
        ink(&view),
        0,
        "the current five-second window excludes that old print"
    );
}

/// Real queue and mailbox, without a flush or an artificial zero-backlog
/// condition. CPU submission to egui is measured; feed and GPU latency are not.
#[test]
#[ignore = "measures worker scheduling; run with --ignored --nocapture"]
fn real_worker_tape_print_to_egui_latency() {
    use std::time::{Duration, Instant};
    const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);
    let mut view = OrderflowView::new("WINV26");
    let before = view.config.clone();
    view.config = HeatmapConfig::default();
    view.config.live_lane.enabled = true;
    view.config.live_lane.show_aggressions = true;
    view.config.live_lane.tape_only = true;
    view.config.volume_dots.enabled = true;
    view.commit_config_changes(before);
    view.set_live_lane_window(LaneWindow::Fixed { ms: 15_000 });
    let ctx = egui::Context::default();
    let viewport = Viewport::new();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 400.0));
    let mut partial = None::<Bar>;
    let mut expected = (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    let mut samples = Vec::new();
    let mut frames = Vec::new();
    for index in 0..20_u32 {
        let offset = [1, 17, 49, 83, 99][index as usize % 5];
        let trade = Trade {
            agg_id: u64::from(index) + 1,
            timestamp_ms: 1_000 + i64::from(index / 5) * 100 + offset,
            price: Decimal::from(100 + index % 2 * 5),
            quantity: Decimal::from(1 + index % 7),
            side: if index.is_multiple_of(2) {
                Side::Buy
            } else {
                Side::Sell
            },
        };
        expected.0 += trade.quantity;
        expected.1 += if trade.side == Side::Buy {
            trade.quantity
        } else {
            Decimal::ZERO
        };
        expected.2 += Decimal::from(trade.timestamp_ms) * trade.quantity;
        if let Some(partial) = partial.as_mut() {
            partial.extend(&trade);
        } else {
            partial = Some(Bar::opened_by(&trade));
        }
        let accepted = Instant::now();
        view.record_trade(&trade);
        // Hold the market clock inside this open 100 ms window throughout
        // observation: waiting for the window to close cannot pass this test.
        view.set_replay_clock_at(trade.timestamp_ms, Some(trade.timestamp_ms), None);
        let mut ui_frames = 0_u32;
        loop {
            let frame_started = Instant::now();
            let frame = view.project_visible(
                VisibleBarTimeline::new(u64::from(index), 0, &[], partial.as_ref()),
                true,
                true,
                Some(15_000),
                (90.0, 115.0),
                Some(PaneGeometry {
                    px_per_bar: 10.0,
                    lane_width_px: rect.width(),
                    lane_window_ms: 15_000,
                    height_px: rect.height(),
                    lane_bars: vec![(1_001, trade.timestamp_ms)],
                }),
            );
            if let Some(frame) = frame {
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    view.draw_aggressions(
                        &ctx.layer_painter(egui::LayerId::background()),
                        rect,
                        &viewport,
                        1,
                        &frame,
                        egui::Color32::BLACK,
                        rect.width(),
                        false,
                        (90.0, 115.0),
                    );
                });
                let live: Vec<_> = frame
                    .projection
                    .aggressions
                    .iter()
                    .filter(|mark| mark.live)
                    .collect();
                if let Some(mark) = live
                    .iter()
                    .find(|mark| mark.agg_ids.contains(&trade.agg_id))
                {
                    assert_eq!(mark.x, 1.0, "an open-window print reaches NOW");
                    assert_eq!(
                        live.iter().map(|mark| mark.quantity).sum::<Decimal>(),
                        expected.0
                    );
                    assert_eq!(
                        live.iter().map(|mark| mark.buy_quantity).sum::<Decimal>(),
                        expected.1
                    );
                    assert_eq!(
                        live.iter()
                            .map(|mark| mark.timestamp_quantity)
                            .sum::<Decimal>(),
                        expected.2
                    );
                    assert!(output.shapes.iter().any(|shape| matches!(
                        shape.shape,
                        egui::Shape::Circle(_) | egui::Shape::Mesh(_)
                    )));
                    samples.push(accepted.elapsed().as_secs_f64() * 1_000.0);
                    frames.push(ui_frames);
                    break;
                }
            }
            assert!(
                accepted.elapsed() < Duration::from_secs(5),
                "print {} never reached the painter",
                trade.agg_id
            );
            ui_frames += 1;
            std::thread::sleep(FRAME_INTERVAL.saturating_sub(frame_started.elapsed()));
        }
        std::thread::sleep(FRAME_INTERVAL.saturating_sub(accepted.elapsed()));
    }
    samples.sort_by(f64::total_cmp);
    frames.sort_unstable();
    println!(
        "TAPE_WORKER_LATENCY samples=20 frame_interval_ms=16.667 accepted_to_egui_ms[p50={:.3},p95={:.3},max={:.3}] ui_frames[p50={},p95={},max={}] same_frame={} over_100ms={} over_220ms={}; open market windows held fixed; no flush; excludes feed/GPU latency",
        samples[9],
        samples[18],
        samples[19],
        frames[9],
        frames[18],
        frames[19],
        frames.iter().filter(|&&count| count == 0).count(),
        samples.iter().filter(|&&ms| ms >= 100.0).count(),
        samples.iter().filter(|&&ms| ms >= 220.0).count(),
    );
}
