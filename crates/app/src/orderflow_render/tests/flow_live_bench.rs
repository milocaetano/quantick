//! Live FLOW frames as the app paints them, measured headless: a BTC-shaped
//! tape (the recorded Binance spot minutes, replayed as a random walk), the
//! chart's FLOW session with its worker run inline and timed apart, the
//! cached drawing with its labels, the caption's sphere check, and egui's
//! own tessellation of what was painted. Run by name:
//! `cargo test -p quantick-app --release --bin quantick-app live_flow_frames -- --ignored --nocapture`
//!
//! What it cannot see: the GPU driver (the upload is a plain copy into a warm
//! buffer, and a draw call per FLOW mesh costs nothing here), the rest of the
//! app frame (candles, footprint, the tape lane, the book and its depth
//! fold), the worker running on its own thread, and egui's window and input
//! handling.
use super::*;
use quantick_chart::flow_execution::{
    FlowChartView, FlowExecutionGeometry, place_flow_chrome, project_styled_flow_executions,
};
use quantick_chart::state::ChartState;
use quantick_chart::viewport::Viewport;
use quantick_engine::BarSpec;
use quantick_orderflow::projection::flow_tape::{
    FlowChunk, FlowRequest, FlowRunner, FlowSession, FlowWorkerCache,
};
use rust_decimal::prelude::ToPrimitive as _;
use std::cell::{Cell, RefCell};
use std::str::FromStr as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

const BUSY: &str = include_str!("../../../../stores/tests/fixtures/btc/spot-busy.csv");
const QUIET: &str = include_str!("../../../../stores/tests/fixtures/btc/spot-quiet.csv");
/// History as loaded: the A3 bench's 4.32 M prints, at about ten a second.
const HISTORY_PRINTS: usize = 4_320_000;
const HISTORY_PACE: f64 = 0.6;
/// The live tape the perf session saw: about 39 prints a second.
const LIVE_PACE: f64 = 0.155;
const FRAME_MS: f64 = 1000.0 / 60.0;
const PX_PER_BAR: f32 = 8.0;
/// The perf session's window: 2560 x 1369 points at 150 %, the candle pane
/// 930 x 1180 points.
const PIXELS_PER_POINT: f32 = 1.5;
const SCREEN: egui::Vec2 = egui::vec2(2560.0, 1369.0);
const HISTORY: egui::Rect = egui::Rect {
    min: egui::pos2(40.0, 120.0),
    max: egui::pos2(970.0, 1300.0),
};
/// The "Binance BTC Spot spheres" look.
const TICK: quantick_orderflow::TickBubbleStyle = quantick_orderflow::TickBubbleStyle {
    reference_quantity: 1.0,
    max_radius: 32.0,
    label_min_radius: 8.0,
    opacity: 0.9,
};
const WARMUP_FRAMES: usize = 240;
const MEASURED_FRAMES: usize = 900;

thread_local! {
    static WORKER: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

/// The pane's worker, run when the session collects its publication and
/// timed apart from the frame: on the app it runs on its own thread.
#[derive(Default)]
struct InlineWorker {
    cache: RefCell<FlowWorkerCache>,
    request: RefCell<Option<FlowRequest>>,
    queue: RefCell<Vec<FlowChunk>>,
}
impl FlowRunner for InlineWorker {
    fn request(&mut self, request: &FlowRequest, _: bool) -> bool {
        *self.request.get_mut() = Some(request.clone());
        false
    }
    fn submit(&self, chunk: FlowChunk) -> Result<(), Box<FlowChunk>> {
        self.queue.borrow_mut().push(chunk);
        Ok(())
    }
    fn finished(&self) -> Option<Arc<FlowTapeFrame>> {
        let started = Instant::now();
        let request = self.request.borrow().clone()?;
        let mut cache = self.cache.borrow_mut();
        cache.select_request(&request);
        for chunk in self.queue.borrow_mut().drain(..) {
            cache.append(&chunk);
        }
        let frame = cache.project_if_due(&request, true);
        WORKER.with(|time| time.set(time.get() + started.elapsed()));
        frame
    }
}

/// One recorded print as a step from the one before it.
struct Step {
    dt_ms: i64,
    dp: Decimal,
    quantity: Decimal,
    side: Side,
}

fn steps(csv: &str) -> Vec<Step> {
    let rows: Vec<(i64, Decimal, Decimal, Side)> = csv
        .lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<_> = line.split(',').collect();
            (
                fields[1].parse().unwrap(),
                Decimal::from_str(fields[2]).unwrap(),
                Decimal::from_str(fields[3]).unwrap(),
                if fields[4] == "buy" {
                    Side::Buy
                } else {
                    Side::Sell
                },
            )
        })
        .collect();
    rows.windows(2)
        .map(|pair| Step {
            dt_ms: pair[1].0 - pair[0].0,
            dp: pair[1].1 - pair[0].1,
            quantity: pair[1].2,
            side: pair[1].3,
        })
        .collect()
}

/// Recorded busy and quiet minutes in a seeded order, each replayed upward
/// or downward: the price walks like BTC's and the quantities keep their
/// recorded tail.
struct BtcTape {
    minutes: [Vec<Step>; 2],
    seed: u64,
    minute: usize,
    step: usize,
    downward: bool,
    time_ms: f64,
    price: Decimal,
    next_id: u64,
}
impl BtcTape {
    fn new() -> Self {
        Self {
            minutes: [steps(BUSY), steps(QUIET)],
            seed: 0x9E37_79B9_7F4A_7C15,
            minute: 0,
            step: 0,
            downward: false,
            time_ms: 1_790_000_000_000.0,
            price: Decimal::from(80_000),
            next_id: 1,
        }
    }
    fn next(&mut self, pace: f64) -> Trade {
        if self.step == self.minutes[self.minute].len() {
            self.seed = self
                .seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.minute = usize::from(self.seed >> 63 == 1);
            self.downward = (self.seed >> 62) & 1 == 1;
            self.step = 0;
        }
        let step = &self.minutes[self.minute][self.step];
        self.step += 1;
        self.time_ms += step.dt_ms as f64 * pace;
        self.price += if self.downward { -step.dp } else { step.dp };
        self.next_id += 1;
        Trade {
            agg_id: self.next_id,
            timestamp_ms: self.time_ms as i64,
            price: self.price,
            quantity: step.quantity,
            side: step.side,
        }
    }
}

/// Mean, median and 95th percentile of `samples`, in microseconds.
fn summary(samples: &[Duration]) -> String {
    if samples.is_empty() {
        return "null".into();
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let total: Duration = sorted.iter().sum();
    let us = |duration: Duration| duration.as_secs_f64() * 1e6;
    format!(
        "{{\"n\":{},\"mean\":{:.1},\"p50\":{:.1},\"p95\":{:.1},\"max\":{:.1}}}",
        sorted.len(),
        us(total) / sorted.len() as f64,
        us(sorted[sorted.len() / 2]),
        us(sorted[(sorted.len() * 95).div_ceil(100) - 1]),
        us(sorted[sorted.len() - 1]),
    )
}

/// The visible bars' price range, held while they stay inside it.
fn fit(state: &ChartState, slots: std::ops::Range<usize>, held: Option<(f64, f64)>) -> (f64, f64) {
    let bars = state.bars().iter().chain(state.partial());
    let (low, high) =
        bars.skip(slots.start)
            .take(slots.len())
            .fold((f64::MAX, f64::MIN), |(low, high), bar| {
                (
                    low.min(bar.low.to_f64().unwrap()),
                    high.max(bar.high.to_f64().unwrap()),
                )
            });
    match held {
        Some(range) if range.0 <= low && high <= range.1 => range,
        _ => {
            let pad = (high - low).max(1.0) * 0.04;
            (low - pad, high + pad)
        }
    }
}

#[derive(Default)]
struct Samples {
    session: Vec<Duration>,
    worker: Vec<Duration>,
    paint_built: Vec<Duration>,
    paint_reused: Vec<Duration>,
    tessellate: Vec<Duration>,
    upload: Vec<Duration>,
    total: Vec<Duration>,
}

#[test]
#[ignore = "measurement; run by name with --release"]
fn live_flow_frames() {
    for ticks in [1000_u64, 50] {
        live_flow_frames_at(ticks);
    }
}

fn live_flow_frames_at(ticks: u64) {
    let mut tape = BtcTape::new();
    let history: Vec<Trade> = (0..HISTORY_PRINTS)
        .map(|_| tape.next(HISTORY_PACE))
        .collect();
    let mut state = ChartState::new(BarSpec::Tick(ticks));
    state.ingest_backfill(&history);
    drop(history);
    let mut session = FlowSession::<InlineWorker>::default();
    let mut viewport = Viewport::new();
    viewport.set_px_per_bar(PX_PER_BAR);
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(PIXELS_PER_POINT);
    let look = (TICK, quantick_orderflow::config::HeatmapTheme::default());
    let mut range = None;
    let mut clock_ms = tape.time_ms;
    let mut pending = Some(tape.next(LIVE_PACE));
    let mut samples = Samples::default();
    let mut last: Option<Arc<FlowDrawing>> = None;
    let (mut publications, mut builds, mut range_changes, mut new_bars) = (0, 0, 0, 0);
    let mut last_frame: Option<Arc<FlowTapeFrame>> = None;
    let mut last_geometry = None;
    let (mut vertices, mut indices) = (0, 0);
    let mut upload_vertices: Vec<egui::epaint::Vertex> = Vec::new();
    let mut upload_indices: Vec<u32> = Vec::new();
    for frame_index in 0..WARMUP_FRAMES + MEASURED_FRAMES {
        let measured = frame_index >= WARMUP_FRAMES;
        // The prints whose exchange time this frame has reached.
        clock_ms += if measured { FRAME_MS } else { 0.0 };
        let bars_before = state.bars().len();
        while measured
            && pending
                .as_ref()
                .is_some_and(|print| print.timestamp_ms as f64 <= clock_ms)
        {
            state.ingest_live(&pending.take().unwrap());
            pending = Some(tape.next(LIVE_PACE));
        }
        if state.bars().len() != bars_before {
            new_bars += 1;
        }
        let total = state.bars().len() + usize::from(state.partial().is_some());
        let visible = (HISTORY.width() / PX_PER_BAR).ceil() as usize + 1;
        let slots = total.saturating_sub(visible)..total;
        let fitted = fit(&state, slots.clone(), range);
        if range.is_some_and(|held| held != fitted) {
            range_changes += usize::from(measured);
        }
        range = Some(fitted);
        let right = f64::from(viewport.right_edge_bar(total)) + 1.0;
        let left = right - f64::from(HISTORY.width() / PX_PER_BAR);
        let started = Instant::now();
        let worker_before = WORKER.with(Cell::get);
        project_styled_flow_executions(
            true,
            &state,
            &mut session,
            FlowChartView {
                slots: slots.clone(),
                pixels: (slots.len() as f32 * PX_PER_BAR, HISTORY.height()),
                range: fitted,
                clip: (left, right),
                tick_bubbles: Some(TICK),
            },
        );
        let worker = WORKER.with(Cell::get) - worker_before;
        let session_time = started.elapsed() - worker;
        let Some(frame) = session.frame_handle().cloned() else {
            continue;
        };
        if last_frame
            .as_ref()
            .is_none_or(|last| !Arc::ptr_eq(last, &frame))
        {
            publications += usize::from(measured);
        }
        last_frame = Some(Arc::clone(&frame));
        let geometry = FlowExecutionGeometry::new(
            viewport,
            total,
            0,
            fitted,
            [
                HISTORY.left(),
                HISTORY.top(),
                HISTORY.right(),
                HISTORY.bottom(),
            ],
            false,
        )
        .unwrap();
        last_geometry = Some(geometry);
        let paint_started = Instant::now();
        let mut drawn = None;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
                ..Default::default()
            },
            |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                // The candle clear and the candles share the pane's clip, as
                // on the app: egui merges FLOW into their mesh.
                let candles = painter.with_clip_rect(HISTORY);
                candles.rect_filled(HISTORY, 0.0, crate::theme::CANVAS);
                let drawing = crate::orderflow_render::cached_flow(
                    &painter,
                    HISTORY,
                    &frame,
                    geometry,
                    crate::theme::CANVAS,
                    None,
                    Some(look),
                );
                drawing.paint(&painter, false);
                candles.rect_filled(HISTORY.shrink(400.0), 0.0, egui::Color32::GRAY);
                drawing.paint(&painter, true);
                candles.rect_filled(HISTORY.shrink(450.0), 0.0, egui::Color32::GRAY);
                crate::orderflow_render::draw_flow_perimeters(
                    &painter,
                    HISTORY,
                    &frame,
                    &drawing.plan,
                );
                drawing.paint_labels(&painter);
                let caption = [
                    [HISTORY.left() + 8.0, HISTORY.top() + 28.0],
                    [HISTORY.left() + 328.0, HISTORY.top() + 72.0],
                ];
                std::hint::black_box(place_flow_chrome(
                    [HISTORY.min.into(), HISTORY.max.into()],
                    caption,
                    &drawing.plan.regions,
                ));
                drawn = Some(drawing);
            },
        );
        let paint = paint_started.elapsed();
        let drawn = drawn.unwrap();
        let built = last.as_ref().is_none_or(|last| !Arc::ptr_eq(last, &drawn));
        last = Some(drawn);
        let tessellate_started = Instant::now();
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let tessellate = tessellate_started.elapsed();
        let upload_started = Instant::now();
        (vertices, indices) = (0, 0);
        for primitive in &primitives {
            if let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive {
                upload_vertices.clear();
                upload_vertices.extend_from_slice(&mesh.vertices);
                upload_indices.clear();
                upload_indices.extend_from_slice(&mesh.indices);
                vertices += mesh.vertices.len();
                indices += mesh.indices.len();
            }
        }
        std::hint::black_box((&upload_vertices, &upload_indices));
        let upload = upload_started.elapsed();
        if measured {
            builds += usize::from(built);
            samples.session.push(session_time);
            samples.worker.push(worker);
            if built {
                samples.paint_built.push(paint);
            } else {
                samples.paint_reused.push(paint);
            }
            samples.tessellate.push(tessellate);
            samples.upload.push(upload);
            samples
                .total
                .push(session_time + paint + tessellate + upload);
        }
        drop(primitives);
    }
    assert!((ctx.pixels_per_point() - PIXELS_PER_POINT).abs() < 1e-6);
    let frame = last_frame.expect("a FLOW frame");
    let drawing = last.expect("a drawing");
    let regions = drawing.plan.regions.len();
    let rims = drawing
        .plan
        .regions
        .iter()
        .filter(|region| region.disc.radius >= TICK.bubbles().detail_min_radius)
        .count();
    let meshes = drawing.meshes();
    let mesh_vertices: usize = meshes.iter().map(|mesh| mesh.vertices.len()).sum();
    let mesh_indices: usize = meshes.iter().map(|mesh| mesh.indices.len()).sum();
    eprintln!(
        "FLOW_LIVE {{\"spec\":\"tick:{ticks}\",\"marks\":{},\"regions\":{regions},\"rims\":{rims},\"labels\":{},\"trades\":{},\"mesh_vertices\":{mesh_vertices},\"mesh_indices\":{mesh_indices},\"tessellated_vertices\":{vertices},\"tessellated_indices\":{indices},\"frames\":{MEASURED_FRAMES},\"publications\":{publications},\"drawing_builds\":{builds},\"range_changes\":{range_changes},\"new_bars\":{new_bars}}}",
        frame.dots.len(),
        drawing.labels.len(),
        frame.loaded_executions,
    );
    eprintln!(
        "FLOW_LIVE_US spec=tick:{ticks} session={} worker={} paint_built={} paint_reused={} tessellate={} upload={} total_ui={}",
        summary(&samples.session),
        summary(&samples.worker),
        summary(&samples.paint_built),
        summary(&samples.paint_reused),
        summary(&samples.tessellate),
        summary(&samples.upload),
        summary(&samples.total),
    );
    stage_breakdown(&ctx, &frame, last_geometry.expect("a geometry"), ticks);
    floor_experiments(&ctx, &drawing, ticks);
}

/// What a frame that reuses the drawing pays, piece by piece: the copies egui
/// is handed (one whole mesh per pass, or the runs as painted), its checks,
/// and its tessellation with FLOW merged into the neighbours' mesh or apart.
fn floor_experiments(ctx: &egui::Context, drawing: &FlowDrawing, ticks: u64) {
    const RUNS: u32 = 20;
    let clip = |shape| egui::epaint::ClippedShape {
        clip_rect: HISTORY,
        shape,
    };
    let whole = drawing.meshes();
    let parts: Vec<&egui::Mesh> = drawing
        .runs
        .iter()
        .flat_map(|(_, run)| run.iter())
        .collect();
    let (mut whole_time, mut runs_time) = (Duration::ZERO, Duration::ZERO);
    let mut merged_time = Duration::ZERO;
    let mut isolated_time = Duration::ZERO;
    let mut validate_time = Duration::ZERO;
    for _ in 0..RUNS {
        let started = Instant::now();
        let meshes = whole.clone();
        whole_time += started.elapsed();
        let started = Instant::now();
        let copies: Vec<egui::Mesh> = parts.iter().map(|mesh| (*mesh).clone()).collect();
        runs_time += started.elapsed();
        let started = Instant::now();
        std::hint::black_box(copies.iter().all(egui::Mesh::is_valid));
        std::hint::black_box(copies.iter().map(egui::Mesh::calc_bounds).count());
        validate_time += started.elapsed();
        let neighbour = || clip(egui::Shape::rect_filled(HISTORY, 0.0, crate::theme::CANVAS));
        let [context, peaks] = meshes;
        let merged = vec![
            neighbour(),
            clip(egui::Shape::mesh(context)),
            neighbour(),
            clip(egui::Shape::mesh(peaks)),
            neighbour(),
        ];
        let started = Instant::now();
        std::hint::black_box(ctx.tessellate(merged, PIXELS_PER_POINT));
        merged_time += started.elapsed();
        let mut isolated = vec![neighbour()];
        for (index, mut mesh) in copies.into_iter().enumerate() {
            mesh.texture_id = egui::TextureId::User(7 + index as u64 % 2);
            isolated.push(clip(egui::Shape::mesh(mesh)));
        }
        isolated.push(neighbour());
        let started = Instant::now();
        std::hint::black_box(ctx.tessellate(isolated, PIXELS_PER_POINT));
        isolated_time += started.elapsed();
    }
    let ms = |time: Duration| time.as_secs_f64() * 1e3 / f64::from(RUNS);
    eprintln!(
        "FLOW_FLOOR spec=tick:{ticks} runs={} clone_whole_ms={:.2} clone_runs_ms={:.2} validate_and_bounds_ms={:.2} tessellate_whole_merged_ms={:.2} tessellate_runs_apart_ms={:.2}",
        parts.len(),
        ms(whole_time),
        ms(runs_time),
        ms(validate_time),
        ms(merged_time),
        ms(isolated_time),
    );
}

/// One build of the last frame's drawing, stage by stage.
fn stage_breakdown(
    ctx: &egui::Context,
    frame: &Arc<FlowTapeFrame>,
    geometry: FlowExecutionGeometry,
    ticks: u64,
) {
    let mut plan_time = Duration::ZERO;
    let mut drawing_time = Duration::ZERO;
    let mut kept_time = Duration::ZERO;
    let mut labels_time = Duration::ZERO;
    const RUNS: u32 = 5;
    for _ in 0..RUNS {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
                ..Default::default()
            },
            |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                let started = Instant::now();
                let plan = FlowPresentation::styled(
                    frame,
                    [HISTORY.min.into(), HISTORY.max.into()],
                    Some(TICK),
                    |dot| geometry.point(dot).map(|(x, y)| [x, y]),
                );
                plan_time += started.elapsed();
                let started = Instant::now();
                let (canvas, look) = (
                    crate::theme::CANVAS,
                    Some((TICK, quantick_orderflow::config::HeatmapTheme::default())),
                );
                let build = |plan, previous| {
                    FlowDrawing::styled(
                        (frame.as_ref(), plan),
                        canvas,
                        None,
                        look,
                        &painter,
                        HISTORY,
                        previous,
                    )
                };
                let built = build(plan.clone(), None);
                drawing_time += started.elapsed();
                let started = Instant::now();
                let kept = build(plan, Some(&built));
                kept_time += started.elapsed();
                assert!(kept.meshes() == built.meshes());
                let started = Instant::now();
                let labels = built.plan.quantity_labels(frame, Some(TICK));
                let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
                let measure =
                    |label: &FlowQuantityLabel| quantity_galley(&painter, label).size().into();
                let area = [HISTORY.min.into(), HISTORY.max.into()];
                std::hint::black_box(flow::place_quantity_labels(labels, area, fit, measure));
                labels_time += started.elapsed();
            },
        );
    }
    let us = |time: Duration| time.as_secs_f64() * 1e6 / f64::from(RUNS);
    eprintln!(
        "FLOW_STAGES spec=tick:{ticks} plan_us={:.1} drawing_us={:.1} kept_drawing_us={:.1} of_which_labels_us={:.1}",
        us(plan_time),
        us(drawing_time),
        us(kept_time),
        us(labels_time),
    );
}
