//! Recorded-data CPU stages with persistent, incrementally warmed tape history.

use super::*;
use quantick_engine::{BarBuilder as _, TickBarBuilder};
use quantick_orderflow::engine::ProjectionRequest;
use quantick_orderflow::projection::DotZoom;
use quantick_replay::format::{ParseOptions, ParsedFile, parse_file};
use std::hint::black_box;
use std::io::{BufRead as _, BufReader};
use std::time::Instant;

const WINDOW_MS: i64 = 113_000;
const WARM_STEP_MS: i64 = 40;
const CHECKPOINTS: [usize; 3] = [38_000, 40_000, 42_000];

fn recorded_prefix() -> ParsedFile {
    let path = std::env::var_os("TAPE_BENCH_CSV")
        .expect("set TAPE_BENCH_CSV to the recorded Quantick replay CSV");
    let reader = BufReader::new(std::fs::File::open(path).expect("open recorded tape CSV"));
    let mut text = String::new();
    let mut header = false;
    let mut rows = 0;
    for line in reader.lines() {
        let line = line.expect("read recorded tape prefix");
        text.push_str(&line);
        text.push('\n');
        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if header {
            rows += 1;
        } else {
            header = true;
        }
        if rows == CHECKPOINTS[2] {
            break;
        }
    }
    assert_eq!(
        rows, CHECKPOINTS[2],
        "the recording needs at least 42000 prints"
    );
    let parsed = parse_file(&text, ParseOptions { keep_quotes: true })
        .expect("canonical replay parser accepts the prefix");
    assert_eq!(parsed.trades.len(), rows);
    assert!(
        parsed
            .header
            .symbol
            .as_deref()
            .is_some_and(|symbol| symbol.starts_with("WIN")),
        "this fixture measures the recorded WIN tape"
    );
    eprintln!(
        "TAPE_RECORDED_INPUT symbol={:?} timezone={} declared_timezone={} side_source={:?} rows={rows} quotes={} first_ms={} last_ms={}; only the requested prefix was read",
        parsed.header.symbol,
        parsed.header.timezone.label(),
        parsed.header.timezone_declared,
        parsed.header.side_source,
        parsed.quotes.len(),
        parsed.trades[0].timestamp_ms,
        parsed.trades[rows - 1].timestamp_ms
    );
    parsed
}

struct Chart {
    builder: TickBarBuilder,
    bars: Vec<Bar>,
    count: usize,
    now_ms: i64,
    last_price: f64,
    prices: (f64, f64),
    rect: egui::Rect,
}

impl Chart {
    fn new(first: &Trade) -> Self {
        let price = first.price.to_f64().unwrap();
        Self {
            builder: TickBarBuilder::new(2_000),
            bars: Vec::new(),
            count: 0,
            now_ms: first.timestamp_ms,
            last_price: price,
            prices: (price - 5.0, price + 5.0),
            // The observed right-hand Flow canvas, rather than the old
            // synthetic full-screen 1399 by 1135 lane.
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(825.0, 646.0)),
        }
    }

    fn admit(&mut self, view: &mut OrderflowView, trade: &Trade) {
        view.record_trade(trade);
        if let Some(bar) = self.builder.push(trade) {
            self.bars.push(bar);
        }
        self.count += 1;
        self.now_ms = self.now_ms.max(trade.timestamp_ms);
        self.last_price = trade.price.to_f64().unwrap();
    }

    fn clock(&mut self, view: &mut OrderflowView, now_ms: i64) {
        self.now_ms = self.now_ms.max(now_ms);
        view.set_replay_clock_at(self.now_ms, Some(self.now_ms), None);
    }

    fn fit(&self, view: &OrderflowView) -> (f64, f64) {
        quantick_chart::geometry::tape_price_window(
            view.tape_price_range(),
            Some(self.last_price),
            Some(self.prices),
            self.rect.top(),
            self.rect.bottom(),
            view.config.bubbles.max_radius,
        )
        .expect("recorded tape has a price window")
        .range()
    }

    fn geometry(&self) -> PaneGeometry {
        PaneGeometry {
            px_per_bar: 10.0,
            lane_width_px: self.rect.width(),
            lane_window_ms: WINDOW_MS,
            height_px: self.rect.height(),
            lane_bars: self
                .bars
                .iter()
                .chain(self.builder.partial())
                .map(|bar| (bar.open_time, bar.close_time))
                .collect(),
        }
    }

    fn project(&self, view: &mut OrderflowView) -> Arc<VisibleOrderflow> {
        view.project_visible(
            VisibleBarTimeline::new(self.count as u64, 0, &self.bars, self.builder.partial()),
            true,
            true,
            Some(WINDOW_MS),
            self.prices,
            Some(self.geometry()),
        )
        .expect("recorded trades project on the same frame")
    }

    fn request(&self) -> ProjectionRequest {
        ProjectionRequest {
            timeline_revision: self.count as u64,
            first_bar_index: 0,
            closed: self.bars.clone(),
            partial: self.builder.partial().cloned(),
            lane: true,
            on_newest_bar: true,
            lane_reference_ms: Some(WINDOW_MS),
            lane_now_ms: Some(self.now_ms),
            price_range: self.prices,
            dot_zoom: Some(DotZoom {
                tape_only: true,
                tape_window_ms: 100,
                tape_level_ticks: 1,
                candle_level_ticks: 1,
                lane_bars: self.geometry().lane_bars,
            }),
        }
    }

    fn draw(
        &self,
        view: &OrderflowView,
        frame: &VisibleOrderflow,
        ctx: &egui::Context,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(self.rect),
                ..Default::default()
            },
            |ctx| {
                view.draw_aggressions(
                    &ctx.layer_painter(egui::LayerId::background()),
                    self.rect,
                    &Viewport::new(),
                    self.bars.len() + usize::from(self.builder.partial().is_some()),
                    frame,
                    egui::Color32::BLACK,
                    self.rect.width(),
                    false,
                    self.prices,
                );
            },
        )
    }
}

fn once<T>(target: usize, opening: bool, stage: &str, run: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let value = black_box(run());
    eprintln!(
        "TAPE_RECORDED_FIRST target={target} opening_exclusion={opening} stage={stage} ms={:.3}",
        start.elapsed().as_secs_f64() * 1_000.0
    );
    value
}

fn repeated<T>(target: usize, opening: bool, stage: &str, mut run: impl FnMut() -> T) -> T {
    const SAMPLES: usize = 20;
    for _ in 0..2 {
        black_box(run());
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    let mut last = None;
    for _ in 0..SAMPLES {
        let start = Instant::now();
        last = Some(black_box(run()));
        samples.push(start.elapsed().as_secs_f64() * 1_000.0);
    }
    let mean = samples.iter().sum::<f64>() / SAMPLES as f64;
    samples.sort_by(f64::total_cmp);
    eprintln!(
        "TAPE_RECORDED_REPEAT target={target} opening_exclusion={opening} stage={stage} samples={SAMPLES} mean_ms={mean:.3} p50_ms={:.3} p95_ms={:.3} max_ms={:.3}",
        samples[9], samples[18], samples[19]
    );
    last.unwrap()
}

fn conserved(frame: &VisibleOrderflow, prints: &[Trade], now_ms: i64) {
    let facts = frame.projection.tape_facts.as_ref().unwrap();
    let expected: Vec<_> = prints
        .iter()
        .filter(|trade| {
            let window = trade.timestamp_ms.div_euclid(100) * 100;
            window >= now_ms - WINDOW_MS
                && facts
                    .evicted_through_ms
                    .is_none_or(|evicted| window > evicted)
        })
        .collect();
    let marks: Vec<_> = frame
        .projection
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .collect();
    assert_eq!(
        marks.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        expected.iter().map(|trade| trade.quantity).sum::<Decimal>()
    );
    assert_eq!(
        marks.iter().map(|mark| mark.buy_quantity).sum::<Decimal>(),
        expected
            .iter()
            .filter(|trade| trade.side == Side::Buy)
            .map(|trade| trade.quantity)
            .sum::<Decimal>()
    );
    assert_eq!(
        marks
            .iter()
            .map(|mark| mark.timestamp_quantity)
            .sum::<Decimal>(),
        expected
            .iter()
            .map(|trade| Decimal::from(trade.timestamp_ms) * trade.quantity)
            .sum::<Decimal>()
    );
    let mut actual_ids: Vec<_> = marks
        .iter()
        .flat_map(|mark| mark.agg_ids.iter().copied())
        .collect();
    let mut expected_ids: Vec<_> = expected.iter().map(|trade| trade.agg_id).collect();
    actual_ids.sort_unstable();
    expected_ids.sort_unstable();
    assert_eq!(actual_ids, expected_ids);
}

fn flat_comparison(
    view: &mut OrderflowView,
    frame: &VisibleOrderflow,
    chart: &Chart,
    ctx: &egui::Context,
    target: usize,
    opening: bool,
) {
    let mode = view.config.bubbles.render_mode;
    let groups = view.tape_dots.borrow().retained_group_count();
    // Paint-only comparison: do not commit settings, reset the display epoch,
    // change the projection, or reconstruct historical group membership.
    view.config.bubbles.render_mode = quantick_orderflow::BubbleRenderMode::Flat;
    let output = repeated(target, opening, "flat_warm_draw_and_egui", || {
        chart.draw(view, frame, ctx)
    });
    let meshes = repeated(
        target,
        opening,
        "flat_tessellation_with_shape_clone",
        || ctx.tessellate(output.shapes.clone(), output.pixels_per_point),
    );
    repeated(target, opening, "flat_paint_and_tessellation", || {
        let output = chart.draw(view, frame, ctx);
        black_box(ctx.tessellate(output.shapes, output.pixels_per_point));
    });
    let vertices: usize = meshes
        .iter()
        .map(|primitive| match &primitive.primitive {
            egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
            egui::epaint::Primitive::Callback(_) => 0,
        })
        .sum();
    assert_eq!(view.tape_dots.borrow().retained_group_count(), groups);
    view.config.bubbles.render_mode = mode;
    eprintln!(
        "TAPE_RECORDED_FLAT target={target} opening_exclusion={opening} groups={groups} shapes={} vertices={vertices}; same projection, retained history, price fit, geometry, labels and radii; only render_mode differs",
        output.shapes.len()
    );
}

#[test]
#[ignore = "recorded WIN prefix CPU stages; requires TAPE_BENCH_CSV and has no timing threshold"]
fn recorded_113_second_tape_ui_stages_around_40000_prints() {
    let parsed = recorded_prefix();
    let trades = &parsed.trades;
    for opening in [false, true] {
        let (mut view, gate, initial) = held_view(true);
        view.reset_for_symbol(parsed.header.symbol.as_ref().unwrap());
        let before = view.config.clone();
        assert!(view.apply_preset("mini index regions"));
        view.config.price_grouping = Decimal::from(5);
        view.config.max_aggressions = 100_000;
        view.config.volume_dots.ignore_opening_burst_in_scale = opening;
        view.config.live_lane.window = LaneWindow::Fixed { ms: WINDOW_MS };
        view.commit_config_changes(before);
        initial.release();
        let mut chart = Chart::new(&trades[0]);
        let ctx = egui::Context::default();
        let mut cursor = 0;
        let mut last_painted_ms = chart.now_ms - WARM_STEP_MS;
        let mut warm_frames = 0;
        for target in CHECKPOINTS {
            while cursor < target - 20 {
                chart.admit(&mut view, &trades[cursor]);
                cursor += 1;
                if chart.now_ms - last_painted_ms >= WARM_STEP_MS {
                    chart.clock(&mut view, chart.now_ms);
                    chart.prices = chart.fit(&view);
                    let frame = chart.project(&mut view);
                    black_box(chart.draw(&view, &frame, &ctx));
                    last_painted_ms = chart.now_ms;
                    warm_frames += 1;
                }
            }
            chart.clock(&mut view, chart.now_ms);
            chart.prices = chart.fit(&view);
            let _ = chart.project(&mut view);
            view.flush_for_test();
            assert!(view.pending_tape.is_empty());
            let published_native = view
                .published
                .frame
                .as_ref()
                .unwrap()
                .projection
                .aggressions
                .iter()
                .filter(|mark| mark.live)
                .count();
            let before_groups = view.tape_dots.borrow().retained_group_count();
            let held = gate.hold(Phase::Applying);
            while cursor < target {
                chart.admit(&mut view, &trades[cursor]);
                cursor += 1;
            }
            held.reached();
            chart.clock(&mut view, trades[target - 1].timestamp_ms + 40);
            chart.prices = chart.fit(&view);
            let request = chart.request();
            let first = once(target, opening, "complete_pending_frame", || {
                view.complete_pending_frame(&request).unwrap()
            });
            conserved(&first, &trades[..target], chart.now_ms);
            let first_output = once(target, opening, "changed_draw_and_egui", || {
                chart.draw(&view, &first, &ctx)
            });
            let after_groups = view.tape_dots.borrow().retained_group_count();
            black_box(once(target, opening, "changed_tessellation", || {
                ctx.tessellate(first_output.shapes, first_output.pixels_per_point)
            }));

            repeated(target, opening, "complete_pending_frame", || {
                view.complete_pending_frame(&request).unwrap()
            });
            let frame = repeated(target, opening, "project_visible", || {
                chart.project(&mut view)
            });
            let output = repeated(target, opening, "warm_draw_and_egui", || {
                chart.draw(&view, &frame, &ctx)
            });
            repeated(target, opening, "price_range_and_fit", || chart.fit(&view));
            let meshes = repeated(target, opening, "tessellation_with_shape_clone", || {
                ctx.tessellate(output.shapes.clone(), output.pixels_per_point)
            });
            let vertices: usize = meshes
                .iter()
                .map(|primitive| match &primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                    egui::epaint::Primitive::Callback(_) => 0,
                })
                .sum();
            let combined = repeated(target, opening, "combined_tape_ui", || {
                black_box(chart.fit(&view));
                let frame = chart.project(&mut view);
                let output = chart.draw(&view, &frame, &ctx);
                black_box(ctx.tessellate(output.shapes, output.pixels_per_point));
                frame
            });
            conserved(&combined, &trades[..target], chart.now_ms);
            assert_eq!(view.pending_tape.len(), 20);
            let expired = trades[..target]
                .iter()
                .filter(|trade| trade.timestamp_ms.div_euclid(100) * 100 < chart.now_ms - WINDOW_MS)
                .count();
            eprintln!(
                "TAPE_RECORDED_COUNTS target={target} opening_exclusion={opening} published_prefix={} suffix=20 native_before={published_native} native_now={} groups_before={before_groups} groups_after={after_groups} warm_frames={warm_frames} warm_step_ms={WARM_STEP_MS} now_ms={} elapsed_market_ms={} expired_raw={expired} price_low={} price_high={} canvas={}x{} shapes={} vertices={vertices}; CPU only, excludes file loading/ingestion/worker/GPU, first changed frame precedes fixed-frame repetitions",
                target - 20,
                combined
                    .projection
                    .aggressions
                    .iter()
                    .filter(|mark| mark.live)
                    .count(),
                chart.now_ms,
                chart.now_ms - trades[0].timestamp_ms,
                chart.prices.0,
                chart.prices.1,
                chart.rect.width(),
                chart.rect.height(),
                output.shapes.len()
            );
            flat_comparison(&mut view, &combined, &chart, &ctx, target, opening);
            held.release();
            view.flush_for_test();
            last_painted_ms = chart.now_ms;
        }
    }
}
