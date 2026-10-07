//! Opt-in synthetic UI-stage measurements; these are not recorded-session timings.

use super::*;
use quantick_orderflow::engine::ProjectionRequest;
use quantick_orderflow::projection::DotZoom;
use std::hint::black_box;
use std::time::Instant;

const PRINTS: usize = 40_000;
const WINDOW_MS: i64 = 113_000;
const NOW_MS: i64 = 114_050;
const PRICES: (f64, f64) = (186_900.0, 187_400.0);

fn timed<T>(phase: &str, ignore_opening: bool, mut run: impl FnMut() -> T) -> T {
    const SAMPLES: usize = 30;
    for _ in 0..3 {
        black_box(run());
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    let mut last = None;
    for _ in 0..SAMPLES {
        let start = Instant::now();
        let result = black_box(run());
        samples.push(start.elapsed().as_secs_f64() * 1_000.0);
        last = Some(result);
    }
    let mean = samples.iter().sum::<f64>() / SAMPLES as f64;
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "TAPE_UI_PERF synthetic_{phase} opening_exclusion={ignore_opening} samples={SAMPLES} warmups=3 mean_ms={mean:.3} p50_ms={:.3} p95_ms={:.3} max_ms={:.3}",
        samples[14], samples[28], samples[29]
    );
    last.unwrap()
}

fn geometry(bars: &[Bar], partial: &Bar, rect: egui::Rect) -> PaneGeometry {
    PaneGeometry {
        px_per_bar: 10.0,
        lane_width_px: rect.width(),
        lane_window_ms: WINDOW_MS,
        height_px: rect.height(),
        lane_bars: bars
            .iter()
            .chain(std::iter::once(partial))
            .map(|bar| (bar.open_time, bar.close_time))
            .collect(),
    }
}

fn project(
    view: &mut OrderflowView,
    bars: &[Bar],
    partial: &Bar,
    rect: egui::Rect,
) -> Arc<VisibleOrderflow> {
    view.project_visible(
        VisibleBarTimeline::new(1, 0, bars, Some(partial)),
        true,
        true,
        Some(WINDOW_MS),
        PRICES,
        Some(geometry(bars, partial, rect)),
    )
    .expect("the synthetic prefix and suffix project")
}

fn draw(
    view: &OrderflowView,
    frame: &VisibleOrderflow,
    ctx: &egui::Context,
    rect: egui::Rect,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ctx| {
            view.draw_aggressions(
                &ctx.layer_painter(egui::LayerId::background()),
                rect,
                &Viewport::new(),
                21,
                frame,
                egui::Color32::BLACK,
                rect.width(),
                false,
                PRICES,
            );
        },
    )
}

fn assert_conserved(frame: &VisibleOrderflow, prints: &[Trade]) {
    let live: Vec<_> = frame
        .tape_projection()
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .collect();
    assert_eq!(
        live.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        prints.iter().map(|trade| trade.quantity).sum::<Decimal>()
    );
    assert_eq!(
        live.iter().map(|mark| mark.buy_quantity).sum::<Decimal>(),
        prints
            .iter()
            .filter(|trade| trade.side == Side::Buy)
            .map(|trade| trade.quantity)
            .sum::<Decimal>()
    );
    assert_eq!(
        live.iter()
            .map(|mark| mark.timestamp_quantity)
            .sum::<Decimal>(),
        prints
            .iter()
            .map(|trade| Decimal::from(trade.timestamp_ms) * trade.quantity)
            .sum::<Decimal>()
    );
    let mut actual: Vec<_> = live
        .iter()
        .flat_map(|mark| mark.agg_ids.iter().copied())
        .collect();
    actual.sort_unstable();
    assert_eq!(
        actual,
        prints.iter().map(|trade| trade.agg_id).collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "synthetic 40k-trade/20-pending UI stage benchmark; no timing threshold"]
fn dense_113_second_tape_same_frame_ui_stages() {
    let prefix: Vec<_> = (0..PRINTS)
        .map(|index| Trade {
            agg_id: index as u64 + 2,
            timestamp_ms: 1_100 + index as i64 * 112_900 / PRINTS as i64,
            price: Decimal::from(187_000 + 5 * ((index / 250 + index % 6) % 50)),
            quantity: Decimal::new(10 + (index % 190) as i64, 1),
            side: if index.is_multiple_of(3) {
                Side::Sell
            } else {
                Side::Buy
            },
        })
        .collect();
    let suffix: Vec<_> = (0..20_u64)
        .map(|index| {
            print(
                PRINTS as u64 + index + 2,
                114_001 + index as i64,
                187_000 + (index % 6) as i64 * 5,
                (index + 1) as i64,
                if index.is_multiple_of(2) {
                    Side::Buy
                } else {
                    Side::Sell
                },
            )
        })
        .collect();
    let bars: Vec<_> = prefix
        .chunks(2_000)
        .map(|chunk| {
            let mut bar = Bar::opened_by(&chunk[0]);
            for trade in &chunk[1..] {
                bar.extend(trade);
            }
            bar
        })
        .collect();
    let mut partial = Bar::opened_by(&suffix[0]);
    for trade in &suffix[1..] {
        partial.extend(trade);
    }
    let expected: Vec<_> = prefix.iter().chain(&suffix).cloned().collect();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_399.0, 1_135.0));
    for ignore_opening in [false, true] {
        let (mut view, gate, initial) = held_view(true);
        let before = view.config.clone();
        view.config.price_grouping = Decimal::from(5);
        view.config.max_aggressions = 100_000;
        view.config.bubbles.max_radius = 18.0;
        view.config.volume_dots.ignore_opening_burst_in_scale = ignore_opening;
        view.config.live_lane.window = LaneWindow::Fixed { ms: WINDOW_MS };
        view.commit_config_changes(before);
        // Real captures retain the day's opening metadata after that burst
        // leaves the lane. Its old quantity must not enter today's visible sum.
        view.record_trade(&print(1, 1, 187_225, 74_365, Side::Buy));
        for trade in &prefix {
            view.record_trade(trade);
        }
        view.set_replay_clock_at(NOW_MS, Some(NOW_MS), None);
        let _ = project(&mut view, &bars, &partial, rect);
        initial.release();
        view.flush_for_test();
        assert!(view.pane_tape.pending().is_empty());
        let published = view
            .published
            .frame
            .clone()
            .expect("worker published the prefix");
        assert_conserved(&published, &prefix);
        let native = published
            .projection
            .aggressions
            .iter()
            .filter(|mark| mark.live)
            .count();
        assert!(
            (5_000..9_000).contains(&native),
            "fixture native count={native}"
        );
        assert_eq!(
            published
                .projection
                .tape_facts
                .as_ref()
                .unwrap()
                .opening_bursts,
            [0]
        );

        let held = gate.hold(Phase::Applying);
        for trade in &suffix {
            view.record_trade(trade);
        }
        held.reached();
        view.set_replay_clock_at(NOW_MS, Some(NOW_MS), None);
        assert_eq!(view.pane_tape.pending().len(), 20);
        let request = ProjectionRequest {
            timeline_revision: 1,
            first_bar_index: 0,
            closed: bars.clone(),
            partial: Some(partial.clone()),
            lane: true,
            on_newest_bar: true,
            lane_reference_ms: Some(WINDOW_MS),
            lane_now_ms: Some(NOW_MS),
            price_range: PRICES,
            dot_zoom: Some(DotZoom {
                native_tape: true,
                tape_window_ms: 100,
                tape_level_ticks: 1,
                candle_level_ticks: 1,
                lane_bars: geometry(&bars, &partial, rect).lane_bars,
            }),
        };
        let pending = timed("complete_pending_frame", ignore_opening, || {
            view.complete_pending_frame(&request).unwrap()
        });
        assert_conserved(&pending, &expected);
        let frame = timed("project_visible", ignore_opening, || {
            project(&mut view, &bars, &partial, rect)
        });
        assert_conserved(&frame, &expected);
        assert!(
            frame.tape_projection().aggressions.iter().any(|mark| {
                mark.live && mark.agg_ids.contains(&suffix[19].agg_id) && mark.x == 1.0
            }),
            "the fixed suffix really exercises an open native cell"
        );

        let ctx = egui::Context::default();
        let output = timed("draw_aggressions_and_egui", ignore_opening, || {
            draw(&view, &frame, &ctx, rect)
        });
        assert!(!output.shapes.is_empty());
        let fitted = timed("price_range_and_fit", ignore_opening, || {
            quantick_chart::geometry::tape_price_window(
                view.tape_price_range(),
                partial.close.to_f64(),
                Some(PRICES),
                rect.top(),
                rect.bottom(),
                18.0,
            )
        })
        .unwrap();
        assert!(fitted.range().0 <= 187_000.0 && fitted.range().1 >= 187_245.0);
        let meshes = timed("tessellate_with_shape_clone", ignore_opening, || {
            ctx.tessellate(output.shapes.clone(), output.pixels_per_point)
        });
        let vertices: usize = meshes
            .iter()
            .map(|primitive| match &primitive.primitive {
                egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                egui::epaint::Primitive::Callback(_) => 0,
            })
            .sum();
        let combined = timed("combined_tape_ui", ignore_opening, || {
            let fit = quantick_chart::geometry::tape_price_window(
                view.tape_price_range(),
                partial.close.to_f64(),
                Some(PRICES),
                rect.top(),
                rect.bottom(),
                18.0,
            );
            black_box(fit);
            let frame = project(&mut view, &bars, &partial, rect);
            let output = draw(&view, &frame, &ctx, rect);
            let meshes = ctx.tessellate(output.shapes, output.pixels_per_point);
            black_box(meshes);
            frame
        });
        assert_conserved(&combined, &expected);
        assert_eq!(
            view.pane_tape.pending().len(),
            20,
            "the held worker cannot acknowledge the suffix"
        );
        eprintln!(
            "TAPE_UI_COUNTS raw_prefix={PRINTS} published_native={native} pending=20 retained_groups={} shapes={} vertices={vertices} window_ms={WINDOW_MS} now_ms={NOW_MS} opening_exclusion={ignore_opening}; fixed open-cell suffix, warmed memory; CPU only, excludes ingestion/setup/worker/GPU; isolated tessellation includes input shape clone",
            view.tape_dots.borrow().retained_group_count(),
            output.shapes.len()
        );
        held.release();
    }
}
