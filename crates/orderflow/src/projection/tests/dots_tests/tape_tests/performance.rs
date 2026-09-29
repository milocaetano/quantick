//! Opt-in measurement of the work the tape painter performs every frame.

use std::hint::black_box;
use std::time::Instant;

use super::*;

const NOW_MS: i64 = 1_790_102_415_000;
const WINDOW_MS: i64 = 15_000;

/// A dense native-tick path with unequal integer contract quantities and
/// exact moments of two executions per source dot, spread over fifteen seconds.
fn win_marks(count: usize) -> Vec<AggressionPrimitive> {
    let (_, _, base) = fixture(3_400);
    let mut random = Lcg(53);
    let prices = prices("131475", "131825");
    (0..count)
        .map(|index| {
            let offset = index as i64 * (WINDOW_MS - 1) / (count - 1) as i64;
            let first_ms = NOW_MS - WINDOW_MS + 1 + offset;
            let last_ms = first_ms.saturating_add(2).min(NOW_MS);
            let first_quantity = Decimal::from(1 + random.next(20));
            let last_quantity = Decimal::from(1 + random.next(80));
            let quantity = first_quantity + last_quantity;
            let trend = (index * 60 / count) as i64;
            let jitter = random.next(7) as i64 - 3;
            let price = Decimal::from(131_500 + (trend + jitter) * 5);
            let mut mark = base[0].clone();
            mark.agg_id = index as u64 * 2 + 1;
            mark.agg_ids = vec![mark.agg_id, mark.agg_id + 1];
            mark.quantity = quantity;
            mark.buy_quantity = if index.is_multiple_of(2) {
                first_quantity
            } else {
                last_quantity
            };
            mark.buy_share = (mark.buy_quantity / quantity).to_string().parse().unwrap();
            mark.side = if mark.buy_quantity >= quantity / Decimal::TWO {
                Side::Buy
            } else {
                Side::Sell
            };
            mark.consumed_side = if mark.side == Side::Buy {
                BookSide::Ask
            } else {
                BookSide::Bid
            };
            mark.price = price;
            mark.price_bucket = price;
            mark.price_span = Decimal::from(5);
            mark.trade_count = 2;
            mark.first_timestamp_ms = first_ms;
            mark.last_timestamp_ms = last_ms;
            mark.timestamp_quantity =
                Decimal::from(first_ms) * first_quantity + Decimal::from(last_ms) * last_quantity;
            mark.y = prices
                .y(price)
                .expect("the native price is inside the fixture axis");
            mark
        })
        .collect()
}

fn tape_frame_parameters() -> (HeatmapConfig, DotSizing, TapeDotGeometry) {
    let config = tape_config();
    let sizing = DotSizing {
        tape_column_px: 1.0,
        candle_column_px: 10.0,
        px_per_price: 600.0 / 350.0,
        typed_full: None,
    };
    let horizontal = TapeHorizontalGeometry::resolve(900.0, &config.bubbles);
    let geometry = TapeDotGeometry {
        left_x: 0.0,
        right_x: 1.0,
        width_px: horizontal.span_px,
        height_px: 600.0,
    };
    (config, sizing, geometry)
}

/// Fingerprint the complete Debug record, including every exact Decimal,
/// primitive coordinate, side, execution id and evidence id. The fixed FNV
/// constants make this independent of process hash seeds.
fn complete_fingerprint(marks: &[AggressionPrimitive]) -> u64 {
    const OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
    const PRIME: u64 = 1_099_511_628_211;
    format!("{marks:?}").bytes().fold(OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

#[test]
fn tape_merge_preserves_complete_fixture_outputs() {
    let (config, sizing, geometry) = tape_frame_parameters();
    let outputs = [600, 6_000].map(|count| {
        let mut marks = win_marks(count);
        position_tape_at(&mut marks, NOW_MS, WINDOW_MS, 0.0, 100);
        let merged = merge_tape_dots(
            &marks,
            sizing,
            &config.bubbles,
            &config.live_lane,
            geometry,
        );
        let fingerprint = complete_fingerprint(&merged);
        eprintln!("tape baseline: {count} input, {} output, fingerprint {fingerprint:#018x}", merged.len());
        (count, merged.len(), fingerprint)
    });
    assert_eq!(
        outputs,
        [(600, 271, 0xfe3f_a4e6_88f5_c2dc), (6_000, 587, 0xb16d_5bc3_764b_b568)]
    );
}

#[test]
#[ignore = "opt-in measurement; timings are reported, never used as a pass threshold"]
fn per_frame_tape_positioning_and_merge_cost() {
    let (config, sizing, geometry) = tape_frame_parameters();
    for (count, iterations) in [(600, 100_u32), (6_000, 10)] {
        let marks = win_marks(count);
        let quantity: Decimal = marks.iter().map(|mark| mark.quantity).sum();
        let bought: Decimal = marks.iter().map(|mark| mark.buy_quantity).sum();
        let moment: Decimal = marks.iter().map(|mark| mark.timestamp_quantity).sum();
        assert!(marks.iter().all(|mark| {
            mark.first_timestamp_ms >= NOW_MS - WINDOW_MS && mark.last_timestamp_ms <= NOW_MS
        }));
        let mut positioned = marks.clone();
        position_tape_at(&mut positioned, NOW_MS, WINDOW_MS, 0.0, 100);
        assert!(positioned.iter().all(|mark| (0.0..=1.0).contains(&mark.x)));
        let frame = || {
            let mut positioned = black_box(marks.clone());
            position_tape_at(&mut positioned, NOW_MS, WINDOW_MS, 0.0, 100);
            black_box(merge_tape_dots(
                &positioned,
                sizing,
                &config.bubbles,
                &config.live_lane,
                geometry,
            ))
        };
        for _ in 0..3 {
            black_box(frame());
        }
        let started = Instant::now();
        let mut result = Vec::new();
        for _ in 0..iterations {
            result = frame();
        }
        let ms_per_frame = started.elapsed().as_secs_f64() * 1_000.0 / f64::from(iterations);
        eprintln!(
            "tape positioning + merge: {count} input dots -> {} output dots; \
             {ms_per_frame:.3} ms/frame over {iterations} measured frames (3 warm-ups)",
            result.len()
        );
        assert_eq!(
            result.iter().map(|mark| mark.quantity).sum::<Decimal>(),
            quantity
        );
        assert_eq!(
            result.iter().map(|mark| mark.buy_quantity).sum::<Decimal>(),
            bought
        );
        assert_eq!(
            result
                .iter()
                .map(|mark| mark.timestamp_quantity)
                .sum::<Decimal>(),
            moment
        );
        assert!(
            result.len() < count,
            "the dense fixture exercises collision merges"
        );
        black_box(result);
    }
}
