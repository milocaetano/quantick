use super::*;
use quantick_engine::{BarSpec, Side, Trade};
use quantick_orderflow::projection::flow_tape::{FlowTapeFrame, FlowWorkerCache};
use std::{cell::RefCell, sync::Arc};

#[derive(Default)]
struct Runner {
    cache: RefCell<FlowWorkerCache>,
    request: RefCell<Option<FlowRequest>>,
    output: RefCell<Option<Arc<FlowTapeFrame>>>,
}
impl FlowRunner for Runner {
    fn request(&mut self, request: &FlowRequest, _: bool) -> bool {
        self.cache.borrow_mut().select_request(request);
        *self.request.borrow_mut() = Some(request.clone());
        false
    }
    fn submit(&self, chunk: FlowChunk) -> Result<(), Box<FlowChunk>> {
        let mut cache = self.cache.borrow_mut();
        cache.append(&chunk);
        *self.output.borrow_mut() = Some(cache.project(self.request.borrow().as_ref().unwrap()));
        Ok(())
    }
    fn finished(&self) -> Option<Arc<FlowTapeFrame>> {
        self.output.borrow_mut().take()
    }
}
#[test]
fn capture_preserves_same_ms_membership_forming_bar_view_and_empty_reset() {
    let mut state = ChartState::new(BarSpec::Tick(2));
    for quantity in 1..=5 {
        state.ingest_live(&Trade {
            agg_id: 1,
            timestamp_ms: 1000,
            price: 100.into(),
            quantity: quantity.into(),
            side: Side::Buy,
        });
    }
    let mut session = FlowSession::<Runner>::default();
    session.set_ignore_opening(true);
    for _ in 0..2 {
        project_flow_executions(
            true,
            &state,
            &mut session,
            0..3,
            (300.0, 200.0),
            (90.0, 110.0),
            (0.0, 3.0),
        );
    }
    let frame = session.frame().unwrap();
    assert_eq!(frame.requested_ordinals, 0..5);
    assert_eq!(frame.view.width_px, 300.0);
    assert_eq!(frame.view.radius_limit, 12.0);
    assert_eq!(frame.view.reference, FlowReference::VisibleRegions);
    assert!(frame.view.exclude_opening);
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.quantity)
            .sum::<Decimal>(),
        15.into()
    );
    let mut members: Vec<_> = frame
        .dots
        .iter()
        .flat_map(|dot| dot.members.iter())
        .map(|member| {
            (
                member.ordinal,
                member.candle_slot,
                member.accepted_ordinal,
                member.source_id,
            )
        })
        .collect();
    members.sort();
    assert_eq!(
        members,
        [
            (0, 0, 0, 1),
            (1, 0, 1, 1),
            (2, 1, 0, 1),
            (3, 1, 1, 1),
            (4, 2, 0, 1)
        ]
    );
    project_flow_executions(
        true,
        &state,
        &mut session,
        0..0,
        (300.0, 200.0),
        (90.0, 110.0),
        (0.0, 3.0),
    );
    assert!(session.frame().is_none());
    assert_eq!(session.progress().requested_executions, 0);
    assert!(session.ignore_opening());
}
/// #10: a History install recuts the series from older prints. The spheres
/// on screen stay drawn at their own prints on the frame after the install,
/// before the new series has refilled anything, and the refilled frame puts
/// them at the same places.
#[test]
fn a_history_install_keeps_the_spheres_on_screen_at_their_prints() {
    let trade = |id: u64| Trade {
        agg_id: id,
        timestamp_ms: id as i64 * 1_000,
        price: (100 + id % 3).into(),
        quantity: 1.into(),
        side: Side::Buy,
    };
    let mut state = ChartState::new(BarSpec::Tick(2));
    state.ingest_backfill(&(10..20).map(trade).collect::<Vec<_>>());
    let mut session = FlowSession::<Runner>::default();
    let show =
        |state: &ChartState, session: &mut FlowSession<Runner>, slots: std::ops::Range<usize>| {
            let clip = (slots.start as f64, slots.end as f64);
            let width = slots.len() as f32 * 100.0;
            project_flow_executions(
                true,
                state,
                session,
                slots,
                (width, 200.0),
                (90.0, 110.0),
                clip,
            );
        };
    for _ in 0..2 {
        show(&state, &mut session, 0..5);
    }
    // Where each sphere stands, by the venue ids of its prints.
    let placed = |frame: &FlowTapeFrame| {
        let mut placed: Vec<(Vec<u64>, Decimal)> = frame
            .dots
            .iter()
            .map(|dot| {
                let mut ids: Vec<u64> = dot.members.iter().map(|m| m.source_id).collect();
                ids.sort_unstable();
                (ids, dot.candle_position)
            })
            .collect();
        placed.sort();
        placed
    };
    let before = placed(session.frame().unwrap());
    assert!(!before.is_empty());
    // Two older bars arrive: every print of the screen moves two candles on.
    state.prepend_history(&(6..10).map(trade).collect::<Vec<_>>());
    let shifted: Vec<_> = before
        .iter()
        .map(|(ids, position)| (ids.clone(), *position + Decimal::TWO))
        .collect();
    show(&state, &mut session, 2..7);
    let carried = session
        .frame()
        .expect("the spheres stay on screen across the install");
    assert_eq!(placed(carried), shifted);
    show(&state, &mut session, 2..7);
    assert_eq!(placed(session.frame().unwrap()), shifted);
}

/// #23: tick spheres merge at a held price scale. A pan or an axis fit that
/// moves the live scale a little keeps every region as it was; only a zoom
/// step (the scale leaving the held band) regroups.
#[test]
fn tick_spheres_keep_their_regions_through_axis_drift_and_regroup_on_zoom() {
    let mut state = ChartState::new(BarSpec::Tick(50));
    for (id, time, price) in [(1, 1_000, 100), (2, 1_100, 101)] {
        state.ingest_live(&Trade {
            agg_id: id,
            timestamp_ms: time,
            price: price.into(),
            quantity: 1.into(),
            side: Side::Buy,
        });
    }
    let tick = quantick_orderflow::TickBubbleStyle {
        reference_quantity: 1.0,
        max_radius: 32.0,
        label_min_radius: 8.0,
        opacity: 0.9,
    };
    let mut session = FlowSession::<WorkerRunner>::default();
    let mut regions = |range: (f64, f64)| {
        for _ in 0..2 {
            project_styled_flow_executions(
                true,
                &state,
                &mut session,
                FlowChartView {
                    slots: 0..1,
                    pixels: (100.0, 200.0),
                    range,
                    clip: (0.0, 1.0),
                    tick_bubbles: Some(tick),
                },
            );
        }
        let mut regions: Vec<Vec<u64>> = session
            .frame()
            .unwrap()
            .dots
            .iter()
            .map(|dot| dot.members.iter().map(|member| member.source_id).collect())
            .collect();
        regions.sort();
        regions
    };
    // 10 px per price: the two prints, 10 px apart, are one region.
    assert_eq!(regions((90.0, 110.0)), [vec![1, 2]]);
    // About 13 px per price: an axis fit, not a zoom. Still one region.
    assert_eq!(regions((92.3, 107.7)), [vec![1, 2]]);
    // 33 px per price: a zoom step, which regroups.
    assert_eq!(regions((97.0, 103.0)), [vec![1], vec![2]]);
}

/// The live app: a seed larger than one capture packet, then a print before
/// every frame. The newest-first fill must still reach the oldest seed
/// candle while prints keep arriving: the live tail never starves history.
#[test]
fn a_busy_tape_never_starves_the_seed_of_spheres() {
    let print = |id: u64| Trade {
        agg_id: id,
        timestamp_ms: 1_000 + id as i64 * 40,
        price: (100 + id % 7).into(),
        quantity: (1 + id % 3).into(),
        side: if id.is_multiple_of(2) {
            Side::Buy
        } else {
            Side::Sell
        },
    };
    let mut state = ChartState::new(BarSpec::Tick(50));
    state.ingest_backfill(&(0..6_000).map(print).collect::<Vec<_>>());
    let tick = quantick_orderflow::TickBubbleStyle {
        reference_quantity: 1.0,
        max_radius: 32.0,
        label_min_radius: 8.0,
        opacity: 0.9,
    };
    let mut session = FlowSession::<WorkerRunner>::default();
    let show = |state: &ChartState, session: &mut FlowSession<WorkerRunner>| {
        let total = state.bars().len() + usize::from(state.partial().is_some());
        project_styled_flow_executions(
            true,
            state,
            session,
            FlowChartView {
                slots: 0..total,
                pixels: (total as f32 * 8.0, 400.0),
                range: (95.0, 110.0),
                clip: (0.0, total as f64),
                tick_bubbles: Some(tick),
            },
        );
    };
    // A print arrives before every frame, as on a busy tape.
    for id in 6_000..6_040 {
        state.ingest_live(&print(id));
        show(&state, &mut session);
    }
    show(&state, &mut session);
    let frame = session.frame().unwrap();
    let mut slots: Vec<usize> = frame.dots.iter().map(|dot| dot.first_slot).collect();
    slots.sort_unstable();
    slots.dedup();
    assert_eq!(
        slots.first(),
        Some(&0),
        "the oldest seed candle has spheres"
    );
    assert_eq!(slots.len(), 121, "every candle has spheres");
}

/// The app's start: a seed of backfilled prints, then live prints one frame
/// at a time. Every candle on screen — seed and live — has its spheres, and
/// the newest-first fill leaves no older candle behind.
#[test]
fn seed_and_live_candles_all_get_spheres() {
    let print = |id: u64| Trade {
        agg_id: id,
        timestamp_ms: 1_000 + id as i64 * 40,
        price: (100 + id % 7).into(),
        quantity: (1 + id % 3).into(),
        side: if id.is_multiple_of(2) {
            Side::Buy
        } else {
            Side::Sell
        },
    };
    let mut state = ChartState::new(BarSpec::Tick(50));
    state.ingest_backfill(&(0..1_000).map(print).collect::<Vec<_>>());
    let tick = quantick_orderflow::TickBubbleStyle {
        reference_quantity: 1.0,
        max_radius: 32.0,
        label_min_radius: 8.0,
        opacity: 0.9,
    };
    let mut session = FlowSession::<WorkerRunner>::default();
    let show = |state: &ChartState, session: &mut FlowSession<WorkerRunner>| {
        let total = state.bars().len() + usize::from(state.partial().is_some());
        project_styled_flow_executions(
            true,
            state,
            session,
            FlowChartView {
                slots: 0..total,
                pixels: (total as f32 * 8.0, 400.0),
                range: (95.0, 110.0),
                clip: (0.0, total as f64),
                tick_bubbles: Some(tick),
            },
        );
    };
    for id in 1_000..2_000 {
        show(&state, &mut session);
        state.ingest_live(&print(id));
    }
    for _ in 0..4 {
        show(&state, &mut session);
    }
    let frame = session.frame().unwrap();
    assert_eq!(frame.omitted_executions, 0, "the fill completed");
    let mut slots: Vec<usize> = frame.dots.iter().map(|dot| dot.first_slot).collect();
    slots.sort_unstable();
    slots.dedup();
    assert_eq!(
        slots,
        (0..40).collect::<Vec<_>>(),
        "every candle has spheres"
    );
}

/// #23: a pan by whole candles at the same zoom keeps every tick sphere
/// that stays on screen: the same prints, at the same place on their
/// candles, of the same size.
#[test]
fn a_pan_keeps_every_tick_sphere_on_screen() {
    // Four prints a candle, each candle inside one region window, so no
    // sphere spans two candles and every candle on screen is whole.
    let mut state = ChartState::new(BarSpec::Tick(4));
    for id in 0..40_u64 {
        state.ingest_live(&Trade {
            agg_id: id,
            timestamp_ms: 1_000 * (id as i64 / 4 + 1) + id as i64 % 4 * 100,
            price: (100 + id % 5).into(),
            quantity: (1 + id % 3).into(),
            side: if id.is_multiple_of(2) {
                Side::Buy
            } else {
                Side::Sell
            },
        });
    }
    let tick = quantick_orderflow::TickBubbleStyle {
        reference_quantity: 4.0,
        max_radius: 32.0,
        label_min_radius: 8.0,
        opacity: 0.9,
    };
    let mut session = FlowSession::<WorkerRunner>::default();
    let mut spheres = |slots: std::ops::Range<usize>| {
        let clip = (slots.start as f64, slots.end as f64);
        let width = slots.len() as f32 * 40.0;
        for _ in 0..2 {
            project_styled_flow_executions(
                true,
                &state,
                &mut session,
                FlowChartView {
                    slots: slots.clone(),
                    pixels: (width, 300.0),
                    range: (98.0, 106.0),
                    clip,
                    tick_bubbles: Some(tick),
                },
            );
        }
        let frame = session.frame().unwrap();
        let mut spheres: Vec<(Vec<u64>, Decimal, u32)> = frame
            .dots
            .iter()
            .filter(|dot| dot.first_slot >= 3 && dot.end_slot <= 7)
            .inspect(|dot| assert_eq!(dot.end_slot, dot.first_slot + 1))
            .map(|dot| {
                let mut ids: Vec<u64> = dot.members.iter().map(|m| m.source_id).collect();
                ids.sort_unstable();
                (ids, dot.candle_position, dot.radius.to_bits())
            })
            .collect();
        spheres.sort();
        spheres
    };
    let before = spheres(2..8);
    assert!(!before.is_empty());
    assert_eq!(spheres(1..7), before, "one candle left");
    assert_eq!(spheres(3..9), before, "one candle right");
}

#[test]
fn retained_flow_prices_follow_the_current_axis_while_layout_is_pending() {
    let old = PriceWindow::new(90.into(), 110.into()).unwrap();
    let current = PriceWindow::new(90.into(), 130.into()).unwrap();
    assert_eq!(flow_price_y(old, 100.into(), 10.0, 200.0, false), 110.0);
    assert_eq!(flow_price_y(current, 100.into(), 10.0, 200.0, false), 160.0);
    assert_eq!(flow_price_y(current, 100.into(), 10.0, 200.0, true), 60.0);
}

std::thread_local! {
    /// Time the bench runner's worker half spent, apart from the frame.
    static WORKER_TIME: std::cell::Cell<std::time::Duration> =
        const { std::cell::Cell::new(std::time::Duration::ZERO) };
}

/// A runner that does its worker's turn when the session collects the
/// publication, and accounts that time apart from the frame's.
#[derive(Default)]
struct WorkerRunner {
    cache: RefCell<FlowWorkerCache>,
    request: RefCell<Option<FlowRequest>>,
    queue: RefCell<Vec<FlowChunk>>,
}
impl FlowRunner for WorkerRunner {
    fn request(&mut self, request: &FlowRequest, _: bool) -> bool {
        *self.request.get_mut() = Some(request.clone());
        false
    }
    fn submit(&self, chunk: FlowChunk) -> Result<(), Box<FlowChunk>> {
        self.queue.borrow_mut().push(chunk);
        Ok(())
    }
    fn finished(&self) -> Option<Arc<FlowTapeFrame>> {
        let started = std::time::Instant::now();
        let request = self.request.borrow().clone()?;
        let mut cache = self.cache.borrow_mut();
        cache.select_request(&request);
        for chunk in self.queue.borrow_mut().drain(..) {
            cache.append(&chunk);
        }
        let frame = cache.project_if_due(&request, true);
        WORKER_TIME.with(|time| time.set(time.get() + started.elapsed()));
        frame
    }
}

/// Five days of BTCUSDT-like prints on tick:50, 200 candles on screen at
/// the live edge, one new print per frame: what FLOW costs the painting
/// thread per frame and its worker per frame. Neither may grow with the
/// history loaded. Run by name:
/// `cargo test -p quantick-chart --release --lib bench_flow_frames_over_five_days -- --ignored --nocapture`
#[test]
#[ignore = "measurement; run by name"]
fn bench_flow_frames_over_five_days() {
    const PRINTS_PER_SECOND: usize = 10;
    const PRINTS: usize = 5 * 24 * 3_600 * PRINTS_PER_SECOND;
    const VISIBLE_SLOTS: usize = 200;
    const FRAMES: usize = 600;
    let print = |index: usize| Trade {
        agg_id: index as u64,
        timestamp_ms: 1_700_000_000_000 + (index * 1_000 / PRINTS_PER_SECOND) as i64,
        price: Decimal::new(6_000_000 + (index as i64 * 7_919 % 400) - 200, 2),
        quantity: Decimal::new(1 + index as i64 * 31 % 900, 3),
        side: if index.is_multiple_of(3) {
            Side::Sell
        } else {
            Side::Buy
        },
    };
    let mut state = ChartState::new(BarSpec::Tick(50));
    let history: Vec<Trade> = (0..PRINTS).map(print).collect();
    state.ingest_backfill(&history);
    drop(history);
    let tick = quantick_orderflow::TickBubbleStyle {
        reference_quantity: 1.0,
        max_radius: 32.0,
        label_min_radius: 8.0,
        opacity: 0.9,
    };
    let mut session = FlowSession::<WorkerRunner>::default();
    // One painted frame: its total time and the worker's share of it.
    let frame = |state: &ChartState, session: &mut FlowSession<WorkerRunner>| {
        let total = state.bars().len() + usize::from(state.partial().is_some());
        let slots = total - VISIBLE_SLOTS..total;
        let clip = (slots.start as f64, slots.end as f64);
        let worker_before = WORKER_TIME.with(std::cell::Cell::get);
        let started = std::time::Instant::now();
        project_styled_flow_executions(
            true,
            state,
            session,
            FlowChartView {
                slots,
                pixels: (VISIBLE_SLOTS as f32 * 6.0, 600.0),
                range: (59_990.0, 60_010.0),
                clip,
                tick_bubbles: Some(tick),
            },
        );
        let elapsed = started.elapsed();
        let worker = WORKER_TIME.with(std::cell::Cell::get) - worker_before;
        (elapsed - worker, worker)
    };
    // The cold fill, then the steady live edge.
    for _ in 0..64 {
        let _ = frame(&state, &mut session);
    }
    let mut painting = Vec::with_capacity(FRAMES);
    let mut worker = Vec::with_capacity(FRAMES);
    for index in PRINTS..PRINTS + FRAMES {
        state.ingest_live(&print(index));
        let (paint, work) = frame(&state, &mut session);
        painting.push(paint);
        worker.push(work);
    }
    let summary = |name: &str, times: &mut Vec<std::time::Duration>| {
        times.sort_unstable();
        let total: std::time::Duration = times.iter().sum();
        eprintln!(
            "FLOW_BENCH {name} prints={PRINTS} frames={FRAMES} mean_us={:.1} p50_us={:.1} p99_us={:.1} max_us={:.1}",
            total.as_secs_f64() * 1e6 / times.len() as f64,
            times[times.len() / 2].as_secs_f64() * 1e6,
            times[times.len() * 99 / 100].as_secs_f64() * 1e6,
            times[times.len() - 1].as_secs_f64() * 1e6,
        );
    };
    summary("painting", &mut painting);
    summary("worker", &mut worker);
    assert!(session.frame().is_some_and(|frame| !frame.dots.is_empty()));
}
