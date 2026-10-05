//! The freeze behind a zoom, measured. A zoom starts the tape memory over,
//! so the next frame merges every native window of the new tape window
//! again. That pass grew with the square of the window — on the trader's WIN
//! recording, 29 s for 30 minutes of tape — and it ran inside a frame. This
//! bench rereads a synthetic WIN-like tape, about ten thousand prints a
//! second at the 50x the trader replays at, at windows from 30 s to 30
//! minutes, prints what each pass costs, and fails while the widest takes
//! 2.5 s or more. Run it in release:
//!
//! `cargo test --release -p quantick-orderflow a_wide_tape_rereads -- --ignored --nocapture`
use super::*;

/// Prints per second of market time: WIN's busy hour, ten thousand a
/// second of wall clock at 50x.
const WIN_PRINTS_PER_SECOND: i64 = 200;

/// `minutes` of a WIN-like tape: bursts of prints on a five-point grid near
/// 190,000, walking a tick or two at a time, both sides, mostly small lots.
fn win_like_tape(minutes: i64) -> Vec<Trade> {
    let mut state: u64 = 0x5eed;
    let mut random = move |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let start_ms = 1_790_000_000_000;
    let mut tick: i64 = 0;
    let mut buying = true;
    (0..minutes * 60 * WIN_PRINTS_PER_SECOND)
        .map(|index| {
            if random(20) == 0 {
                tick += [-2, -1, -1, 1, 1, 2][random(6) as usize];
            }
            if random(8) == 0 {
                buying = !buying;
            }
            let lot = if random(30) == 0 {
                50 + random(150)
            } else {
                1 + random(20)
            };
            Trade {
                agg_id: index as u64 + 1,
                timestamp_ms: start_ms + index * 1_000 / WIN_PRINTS_PER_SECOND + random(3) as i64,
                price: Decimal::from(190_000 + 5 * tick),
                quantity: Decimal::from(lot),
                side: if buying { Side::Buy } else { Side::Sell },
            }
        })
        .collect()
}

#[test]
#[ignore = "a release-mode bench: it replays 32 minutes of a synthetic WIN tape"]
fn a_wide_tape_rereads_in_about_linear_time() {
    let prints = win_like_tape(32);
    let mut config = config(1_800_000, 5_000_000);
    config.retention_ms = 3_600_000;
    let mut worker = BookEngine::new("WINV26");
    worker.apply_grouping_now(Decimal::from(5));
    worker.apply_visual_config(config.clone());
    let mut bars = TickBarBuilder::new(2_000);
    let mut closed: Vec<Bar> = Vec::new();
    for trade in &prints {
        worker.record_trade(trade);
        if let Some(bar) = bars.push(trade) {
            closed.push(bar);
        }
    }
    let now_ms = prints.last().expect("prints").timestamp_ms + 1;
    let recent = prints
        .iter()
        .filter(|trade| trade.timestamp_ms >= now_ms - 1_800_000)
        .map(|trade| trade.price.to_f64().unwrap_or_default());
    let (low, high) = recent.fold((f64::MAX, f64::MIN), |(low, high), price| {
        (low.min(price), high.max(price))
    });
    let request = ProjectionRequest {
        timeline_revision: prints.len() as u64,
        first_bar_index: closed.len().saturating_sub(40),
        closed: closed[closed.len().saturating_sub(40)..].to_vec(),
        partial: bars.partial().cloned(),
        lane: true,
        on_newest_bar: true,
        lane_reference_ms: Some(6_000),
        lane_now_ms: Some(now_ms),
        price_range: (low - 10.0, high + 10.0),
        dot_zoom: Some(DotZoom {
            native_tape: true,
            tape_window_ms: 100,
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            lane_bars: Vec::new(),
        }),
    };
    let frame = worker
        .project_at(&request, std::time::Instant::now())
        .expect("the tape projected");
    let mut style = OrderflowRenderStyle::from_config(&config, [0, 0, 0, 255]);
    style.dot_sizing = Some(DotSizing {
        native_tape: true,
        tape_column_px: 2.0,
        candle_column_px: 12.0,
        px_per_price: 1.0,
        typed_full: None,
    });
    let prices = PriceWindow::new(
        Decimal::from_f64(low - 10.0).expect("a price"),
        Decimal::from_f64(high + 10.0).expect("a price"),
    );
    let geometry = TapeDotGeometry {
        left_x: 1.0 - 1.0 / frame.slot_count.max(1) as f64,
        right_x: 1.0,
        width_px: 350.0,
        height_px: 600.0,
    };
    let facts = frame.projection.tape_facts.as_deref().expect("tape facts");
    let mut costs = Vec::new();
    for window_ms in [30_000, 120_000, 480_000, 1_800_000] {
        let mut edge = frame.live_edge.expect("a live edge");
        edge.now_ms = now_ms;
        edge.window_ms = window_ms;
        let cells = facts
            .clusters
            .iter()
            .filter(|cell| cell.first_timestamp_ms >= now_ms - window_ms)
            .count();
        let mut memory = TapeDotMemory::default();
        let started = std::time::Instant::now();
        let drawn_frame = project_tape_frame_with_overlay(
            drawn(&style, &frame.projection),
            Some(&mut memory),
            &style,
            geometry,
            Some((edge, 100)),
            prices,
            Some(facts),
            None,
        )
        .expect("the tape draws");
        let ms = started.elapsed().as_secs_f64() * 1_000.0;
        eprintln!(
            "TAPE_REREAD window_s={} cells={cells} groups={} dots={} ms={ms:.1}",
            window_ms / 1_000,
            memory.retained_group_count(),
            drawn_frame.marks.len()
        );
        costs.push((cells, ms));
    }
    // A pass merges each window against the frontier, which grows with the
    // window until it fills the pixels a dot can reach: past that, a wider
    // window costs its extra windows and no more.
    let (wide_cells, wide_ms) = costs[3];
    assert!(
        wide_ms < 2_500.0,
        "a 30-minute reread of {wide_cells} cells took {wide_ms:.0} ms"
    );
}
