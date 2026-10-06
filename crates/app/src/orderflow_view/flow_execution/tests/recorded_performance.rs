//! Real worker/canonical capture timings; UI painting and GPU work are excluded.
use super::*;
use quantick_chart::state::ChartState;
use quantick_engine::{BarSpec, Trade};
use quantick_orderflow::projection::flow_tape::{FlowSession, FlowTapeFrame};
use quantick_replay::format::{ParseOptions, parse_file};
use std::{
    io::{BufRead as _, BufReader},
    time::{Duration, Instant},
};

const PRINTS: usize = 200_000;
const TICKS: usize = 2_000;
const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);

fn recording() -> Vec<Trade> {
    let path = std::env::var_os("TAPE_BENCH_CSV").expect("set TAPE_BENCH_CSV to the WIN recording");
    let mut text = String::new();
    let mut rows = 0;
    let mut header = false;
    for line in BufReader::new(std::fs::File::open(path).unwrap()).lines() {
        let line = line.unwrap();
        text.push_str(&line);
        text.push('\n');
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if header {
            rows += 1;
        } else {
            header = true;
        }
        if rows == PRINTS {
            break;
        }
    }
    let parsed = parse_file(&text, ParseOptions { keep_quotes: false }).unwrap();
    assert_eq!(parsed.trades.len(), PRINTS);
    assert_eq!(parsed.header.symbol.as_deref(), Some("WINV26"));
    parsed.trades
}

fn measure(
    state: &ChartState,
    prices: (f64, f64),
    legacy_packet_limit: bool,
) -> Arc<FlowTapeFrame> {
    let mut session = FlowSession::<FlowThread>::default();
    let slots = 0..PRINTS / TICKS;
    // Use the application's exact membership capture and worker; the legacy
    // control limits the captured packet to its former 2048 executions.
    let membership = state.tick_membership().unwrap();
    let view = quantick_orderflow::projection::flow_tape::FlowTapeView {
        first_slot: slots.start,
        end_slot: slots.end,
        clip_left: 0.into(),
        clip_right: slots.end.into(),
        width_px: 1200.0,
        height_px: 700.0,
        prices: quantick_orderflow::projection::PriceWindow::from_f64_range(prices).unwrap(),
        reference: quantick_orderflow::projection::flow_tape::FlowReference::VisibleRegions,
        radius_limit: 12.0,
        merge_support_radius: 12.0,
        exclude_opening: true,
    };
    let request = FlowRequest {
        epoch: state.series_revision(),
        layout_revision: 0,
        source_count: PRINTS,
        requested: 0..PRINTS,
        keep: quantick_orderflow::projection::flow_tape::FlowKeep {
            slots,
            ordinals: 0..PRINTS,
        },
        view,
        opening_windows: membership.opening_windows().to_vec(),
        opening_ordinals: membership.opening_ordinals().collect(),
    };
    let started = Instant::now();
    let mut first = None;
    let mut captures = Vec::new();
    let deadline = started + Duration::from_secs(30);
    loop {
        let frame_started = Instant::now();
        session.project(request.clone(), |mut range| {
            if legacy_packet_limit {
                range.end = range.end.min(range.start + 2048);
            }
            FlowChunk {
                epoch: request.epoch,
                ticks_per_bar: TICKS.into(),
                executions: state
                    .trades()
                    .range(range.clone())
                    .enumerate()
                    .map(|(offset, trade)| {
                        let ordinal = range.start + offset;
                        let (slot, accepted_ordinal) = membership.locate(ordinal).unwrap();
                        quantick_orderflow::projection::flow_tape::OwnedFlowExecution {
                            ordinal,
                            slot,
                            accepted_ordinal,
                            trade: trade.clone(),
                        }
                    })
                    .collect(),
            }
        });
        captures.push(frame_started.elapsed().as_secs_f64() * 1000.0);
        if session.frame().is_some_and(|frame| !frame.dots.is_empty()) && first.is_none() {
            first = Some(started.elapsed().as_secs_f64() * 1000.0);
        }
        if !session.progress().pending {
            break;
        }
        assert!(Instant::now() < deadline, "recorded worker completes");
        std::thread::sleep(FRAME_INTERVAL.saturating_sub(frame_started.elapsed()));
    }
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    captures.sort_by(f64::total_cmp);
    eprintln!(
        "RECORDED_FLOW_ASYNC legacy_packet_limit={legacy_packet_limit} prints={PRINTS} ticks={TICKS} first_useful_ms={:.3} full_ms={elapsed_ms:.3} frame_capture_p50_ms={:.3} frame_capture_p95_ms={:.3} frame_capture_max_ms={:.3}; real worker, 60fps capture pacing, excludes file load, chart ingest, paint/GPU",
        first.unwrap(),
        captures[captures.len() / 2],
        captures[captures.len() * 95 / 100],
        captures.last().unwrap()
    );
    Arc::clone(session.frame_handle().unwrap())
}

#[test]
#[ignore = "recorded real worker comparison; requires TAPE_BENCH_CSV, reports timings without thresholds"]
fn recorded_flow_actual_worker_cold_enable_comparison() {
    use rust_decimal::prelude::ToPrimitive as _;
    let trades = recording();
    let low = trades
        .iter()
        .map(|trade| trade.price)
        .min()
        .unwrap()
        .to_f64()
        .unwrap()
        - 5.0;
    let high = trades
        .iter()
        .map(|trade| trade.price)
        .max()
        .unwrap()
        .to_f64()
        .unwrap()
        + 5.0;
    let mut state = ChartState::new(BarSpec::Tick(TICKS as u64));
    state.ingest_backfill(&trades);
    let baseline = measure(&state, (low, high), true);
    let current = measure(&state, (low, high), false);
    assert_eq!(
        current, baseline,
        "cold packet scheduling preserves the entire completed frame"
    );
}
