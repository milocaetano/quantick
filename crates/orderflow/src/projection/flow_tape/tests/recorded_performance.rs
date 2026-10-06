//! Opt-in cold admission and worker costs over the unchanged recorded WIN prefix.
use super::*;
use crate::projection::flow_tape::{
    FlowChunk, FlowRequest, FlowRunner, FlowSession, FlowWorkerCache, OwnedFlowExecution,
};
use quantick_engine::Side;
use std::{
    cell::RefCell,
    io::{BufRead as _, BufReader},
    time::Instant,
};

const PRINTS: usize = 200_000;
const TICKS: usize = 2_000;

fn recording() -> Vec<Trade> {
    let path = std::env::var_os("TAPE_BENCH_CSV")
        .expect("set TAPE_BENCH_CSV to the 2026-09-22 WIN recording");
    BufReader::new(std::fs::File::open(path).unwrap())
        .lines()
        .map(Result::unwrap)
        .filter(|line| line.starts_with("2026-09-22,"))
        .take(PRINTS)
        .enumerate()
        .map(|(ordinal, line)| {
            let fields: Vec<_> = line.split(',').collect();
            let time: Vec<i64> = fields[1]
                .split([':', '.'])
                .map(|part| part.parse().unwrap())
                .collect();
            Trade {
                agg_id: ordinal as u64 + 1,
                timestamp_ms: 1_790_046_000_000
                    + ((time[0] * 60 + time[1]) * 60 + time[2]) * 1_000
                    + time[3],
                price: fields[2].parse().unwrap(),
                quantity: fields[5].parse().unwrap(),
                side: match fields[6] {
                    "B" => Side::Buy,
                    "S" => Side::Sell,
                    _ => panic!("recorded venue side"),
                },
            }
        })
        .collect()
}

#[derive(Default)]
struct Runner {
    request: Option<FlowRequest>,
    queue: RefCell<Vec<FlowChunk>>,
    output: RefCell<Option<Arc<FlowTapeFrame>>>,
    cache: FlowWorkerCache,
    worker_ms: f64,
    projection_ms: Vec<f64>,
}
impl FlowRunner for Runner {
    fn request(&mut self, request: &FlowRequest, _: bool) -> bool {
        self.request = Some(request.clone());
        false
    }
    fn submit(&self, chunk: FlowChunk) -> Result<(), Box<FlowChunk>> {
        self.queue.borrow_mut().push(chunk);
        Ok(())
    }
    fn finished(&self) -> Option<Arc<FlowTapeFrame>> {
        self.output.borrow_mut().take()
    }
}
impl Runner {
    fn pump(&mut self) {
        let request = self.request.as_ref().unwrap();
        let started = Instant::now();
        self.cache.select_request(request);
        for packet in self.queue.get_mut().drain(..) {
            self.cache.append(&packet);
        }
        if let Some(frame) = self.cache.project_if_due(request, false) {
            *self.output.get_mut() = Some(frame);
        }
        self.worker_ms += started.elapsed().as_secs_f64() * 1_000.0;
    }
}

#[test]
#[ignore = "recorded cold FLOW costs; requires TAPE_BENCH_CSV, reports timings without thresholds"]
fn recorded_flow_cold_enable_and_layout_costs() {
    let trades = recording();
    assert_eq!(trades.len(), PRINTS);
    let low = trades.iter().map(|trade| trade.price).min().unwrap() - Decimal::from(5);
    let high = trades.iter().map(|trade| trade.price).max().unwrap() + Decimal::from(5);
    let view = FlowTapeView {
        first_slot: 0,
        end_slot: PRINTS / TICKS,
        clip_left: Decimal::ZERO,
        clip_right: Decimal::from(PRINTS / TICKS),
        width_px: 1200.0,
        height_px: 700.0,
        prices: PriceWindow::new(low, high).unwrap(),
        reference: FlowReference::VisibleRegions,
        radius_limit: 7.0,
        merge_support_radius: 6.0,
        exclude_opening: true,
    };
    let request = FlowRequest {
        epoch: 1,
        layout_revision: 0,
        source_count: PRINTS,
        requested: 0..PRINTS,
        keep: super::super::FlowKeep {
            slots: 0..view.end_slot,
            ordinals: 0..PRINTS,
        },
        view,
        opening_windows: vec![trades[0].timestamp_ms.div_euclid(100) * 100],
        opening_ordinals: vec![0],
    };
    let mut session = FlowSession::<Runner>::default();
    let started = Instant::now();
    let mut first = None;
    let mut frames = 0;
    let mut ui = Vec::new();
    loop {
        let capture = Instant::now();
        session.project(request.clone(), |range| FlowChunk {
            epoch: 1,
            ticks_per_bar: TICKS.into(),
            executions: range
                .map(|ordinal| OwnedFlowExecution {
                    ordinal,
                    slot: ordinal / TICKS,
                    accepted_ordinal: ordinal % TICKS,
                    trade: trades[ordinal].clone(),
                })
                .collect(),
        });
        ui.push(capture.elapsed().as_secs_f64() * 1_000.0);
        frames += 1;
        if session.frame().is_some_and(|frame| !frame.dots.is_empty()) && first.is_none() {
            first = Some(frames);
        }
        if !session.progress().pending {
            break;
        }
        session.runner.pump();
    }
    let frame = session.frame().unwrap();
    assert_eq!(frame.loaded_executions, PRINTS);
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.quantity)
            .sum::<Decimal>(),
        trades.iter().map(|trade| trade.quantity).sum::<Decimal>()
    );
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.buy_quantity)
            .sum::<Decimal>(),
        trades
            .iter()
            .filter(|trade| trade.side == Side::Buy)
            .map(|trade| trade.quantity)
            .sum::<Decimal>()
    );
    let mut ordinals: Vec<_> = frame
        .dots
        .iter()
        .flat_map(|dot| dot.members.iter().map(|member| member.ordinal))
        .collect();
    ordinals.sort_unstable();
    assert_eq!(ordinals, (0..PRINTS).collect::<Vec<_>>());
    ui.sort_by(f64::total_cmp);
    eprintln!(
        "RECORDED_FLOW prints={PRINTS} ticks={TICKS} first_useful_frame={} full_frame={frames} paced_full_ms={:.3} cpu_total_ms={:.3} worker_ms={:.3} capture_p50_ms={:.3} capture_max_ms={:.3} dots={}",
        first.unwrap(),
        frames as f64 * 1000.0 / 60.0,
        started.elapsed().as_secs_f64() * 1000.0,
        session.runner.worker_ms,
        ui[ui.len() / 2],
        ui.last().unwrap(),
        frame.dots.len()
    );
    for width in [1200.0, 800.0, 1600.0] {
        let mut changed = request.clone();
        changed.view.width_px = width;
        let start = Instant::now();
        for _ in 0..10 {
            std::hint::black_box(session.runner.cache.project(&changed));
        }
        session
            .runner
            .projection_ms
            .push(start.elapsed().as_secs_f64() * 100.0);
    }
    eprintln!(
        "RECORDED_FLOW_LAYOUT widths=[1200,800,1600] mean_ms={:?}",
        session.runner.projection_ms
    );
}
