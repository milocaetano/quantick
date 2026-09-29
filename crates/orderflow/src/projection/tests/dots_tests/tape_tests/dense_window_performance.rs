//! Opt-in synthetic density matching the long WIN tape QA workload.

use super::*;
use crate::projection::{TapeDotFrame, TapeDotMemory, TapeDotView, project_live, project_settled};
use std::hint::black_box;
use std::time::Instant;

const PRINTS: usize = 40_000;
const WINDOW_MS: i64 = 113_000;
const FIRST_MS: i64 = 1_000;
const NOW_MS: i64 = FIRST_MS + WINDOW_MS;

fn dense_window() -> (LiquidityHistory, BarTimeline, VolumeDots, Vec<Trade>) {
    let mut config = tape_config();
    config.price_grouping = dec("5");
    config.bubbles.max_radius = 18.0;
    let mut history = LiquidityHistory::new(config);
    // Correlated native-price movement: roughly six levels per 100 ms,
    // rather than 40,000 independently invented drawable primitives.
    let prints: Vec<_> = (0..PRINTS)
        .map(|index| Trade {
            agg_id: index as u64 + 1,
            timestamp_ms: FIRST_MS + index as i64 * WINDOW_MS / PRINTS as i64,
            price: Decimal::from(187_000 + 5 * ((index / 250 + index % 6) % 50)),
            quantity: Decimal::new(10 + (index % 190) as i64, 1),
            side: if index % 3 == 0 {
                Side::Sell
            } else {
                Side::Buy
            },
        })
        .collect();
    for trade in &prints {
        history.record_aggression(trade);
    }
    let bars: Vec<_> = prints
        .chunks(2_000)
        .map(|chunk| {
            let mut bar = Bar::opened_by(&chunk[0]);
            for trade in &chunk[1..] {
                bar.extend(trade);
            }
            bar
        })
        .collect();
    let timeline = BarTimeline::from_bars(
        0,
        &bars,
        None,
        Some(crate::LiveEdge {
            now_ms: NOW_MS,
            window_ms: WINDOW_MS,
            reference_ms: WINDOW_MS,
            on_newest_bar: true,
        }),
    )
    .with_full_lane_coverage();
    let dots = VolumeDots {
        bars: bars
            .iter()
            .map(|bar| (bar.open_time, bar.close_time))
            .collect(),
        ..tape_dots(100, 1)
    };
    (history, timeline, dots, prints)
}

fn timed<T>(label: &str, mut project: impl FnMut() -> T) -> T {
    const SAMPLES: usize = 30;
    for _ in 0..3 {
        black_box(project());
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    let mut last = None;
    for _ in 0..SAMPLES {
        let started = Instant::now();
        let result = black_box(project());
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
        last = Some(result);
    }
    let mean = samples.iter().sum::<f64>() / SAMPLES as f64;
    samples.sort_by(f64::total_cmp);
    let median = (samples[SAMPLES / 2 - 1] + samples[SAMPLES / 2]) / 2.0;
    eprintln!(
        "DENSE_TAPE_PERF synthetic_{label}: raw_input={PRINTS} window_ms={WINDOW_MS} mean_ms={mean:.3} median_ms={median:.3} samples={SAMPLES} warmups=3"
    );
    last.unwrap()
}

fn assert_conserved(marks: &[AggressionPrimitive], prints: &[Trade]) {
    assert_eq!(
        marks.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        prints.iter().map(|trade| trade.quantity).sum::<Decimal>()
    );
    assert_eq!(
        marks.iter().map(|mark| mark.buy_quantity).sum::<Decimal>(),
        prints
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
        prints
            .iter()
            .map(|trade| Decimal::from(trade.timestamp_ms) * trade.quantity)
            .sum::<Decimal>()
    );
    let mut ids: Vec<_> = marks
        .iter()
        .flat_map(|mark| mark.agg_ids.iter().copied())
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        (1..=PRINTS as u64).collect::<Vec<_>>(),
        "each retained execution appears exactly once"
    );
}

fn complete_fingerprint(frame: &TapeDotFrame) -> u64 {
    format!(
        "{:?}|{:?}|{:?}",
        frame.marks, frame.max_radius, frame.full_quantity
    )
    .bytes()
    .fold(14_695_981_039_346_656_037, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
    })
}

#[test]
#[ignore = "opt-in 40k-print worker and warmed-memory measurement; no timing threshold"]
fn dense_113_second_tape_worker_and_warmed_memory_cost() {
    let (history, timeline, dots, prints) = dense_window();
    let prices = prices("186900", "187400");
    assert!(
        prints
            .iter()
            .all(|trade| (NOW_MS - WINDOW_MS..NOW_MS).contains(&trade.timestamp_ms))
    );
    assert_eq!(history.aggressions_since(FIRST_MS).count(), PRINTS);
    let settled = project_settled(&history, &timeline, prices, Some(&dots));
    // The worker's live timer includes raw clustering, native folding and
    // placement, but excludes settled construction and the UI pending bridge.
    let live = timed("project_live", || {
        project_live(&history, &timeline, prices, &settled, Some(&dots))
    });
    let native: Vec<_> = live
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .cloned()
        .collect();
    let hidden = live.aggressions.len() - native.len();
    assert!(
        (5_000..9_000).contains(&native.len()),
        "the fixture exercises thousands of genuine native keys"
    );
    assert_conserved(&native, &prints);
    eprintln!(
        "DENSE_TAPE_COUNTS native={} hidden_live_slots={hidden} settled_slots={}",
        native.len(),
        settled.aggressions.len()
    );
    let left_x = timeline
        .locate_in_lane_clamped(FIRST_MS)
        .unwrap()
        .normalized;
    let view = TapeDotView {
        now_ms: NOW_MS,
        window_ms: WINDOW_MS,
        dot_window_ms: 100,
        evicted_through_ms: None,
        prices,
        geometry: TapeDotGeometry {
            left_x,
            right_x: 1.0,
            width_px: 1_399.0,
            height_px: 1_135.0,
        },
    };
    let sizing = DotSizing {
        native_tape: history.config().native_tape(),
        tape_column_px: 1.0,
        candle_column_px: 8.0,
        px_per_price: 2.27,
        typed_full: None,
    };
    // Hold the same genuine native input for both paths. At NOW_MS - 1 the
    // latest executions have arrived but their 100 ms window is still open.
    for (phase, now_ms) in [("closed", NOW_MS), ("forming", NOW_MS - 1)] {
        let current = TapeDotView { now_ms, ..view };
        let mut without_opening = None;
        for (policy, openings) in [
            ("none", &[][..]),
            ("expired", &[FIRST_MS - 100][..]),
            ("present", &[FIRST_MS][..]),
        ] {
            let mut memory = TapeDotMemory::default();
            let label = format!("warmed_memory_with_input_clone_{phase}_opening_{policy}");
            let frame = timed(&label, || {
                let shown = black_box(native.clone());
                memory.project(
                    &shown,
                    current,
                    sizing,
                    &history.config().bubbles,
                    &history.config().live_lane,
                    openings,
                )
            });
            assert_conserved(&frame.marks, &prints);
            assert_eq!(
                frame.marks.iter().any(|mark| mark.x == 1.0),
                phase == "forming"
            );
            let fingerprint = complete_fingerprint(&frame);
            let expected = match (phase, policy) {
                ("closed", "none" | "expired") => 0xcc0d_2472_d8fb_773a,
                ("closed", "present") => 0xf89a_0aa3_7b2c_663e,
                ("forming", "none" | "expired") => 0x7dae_4a0e_115d_5d73,
                ("forming", "present") => 0x49ae_a2b6_e727_1d18,
                _ => unreachable!("the measurement matrix is fixed"),
            };
            assert_eq!(
                fingerprint, expected,
                "complete baseline for {phase}/{policy}"
            );
            eprintln!(
                "DENSE_TAPE_MEMORY phase={phase} opening={policy} native_input={} rendered_output={} max_radius={:.3} full_quantity={} fingerprint={fingerprint:#018x}",
                native.len(),
                frame.marks.len(),
                frame.max_radius,
                frame.full_quantity
            );
            if phase == "forming" && policy == "expired" {
                let mut style = crate::config::theme::OrderflowRenderStyle::from_config(
                    history.config(),
                    [0, 0, 0, 255],
                );
                style.dot_sizing = Some(sizing);
                memory.measure_native_stages(&native, current, &frame, &style, openings);
                let after = memory.project(
                    &native,
                    current,
                    sizing,
                    &style.bubbles,
                    &style.live_lane,
                    openings,
                );
                assert_eq!(
                    complete_fingerprint(&after),
                    expected,
                    "stage measurement preserves retained facts"
                );
            }
            if policy == "expired" {
                let expected: &TapeDotFrame = without_opening.as_ref().unwrap();
                assert_eq!(frame.marks, expected.marks);
                assert_eq!(frame.max_radius, expected.max_radius);
                assert_eq!(frame.full_quantity, expected.full_quantity);
            }
            if policy == "none" {
                without_opening = Some(frame);
            }
        }
    }
}
