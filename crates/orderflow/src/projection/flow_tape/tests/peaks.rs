//! Peak spheres: a zoom region divided at the valleys of its volume-by-price
//! profile, one sphere per peak, sized by rank among the masses in view.
use super::*;

/// One print per `(price, quantity)` row, a second apart, all on `side`.
fn prints(rows: &[(i64, i64)], side: Side) -> Vec<Trade> {
    rows.iter()
        .enumerate()
        .map(|(n, &(price, quantity))| trade(n as i64 * 1_000, price, quantity, side))
        .collect()
}

/// `trades`, `per_candle` to a candle, in a view of `prices` and `clip`.
fn project(
    trades: &[Trade],
    per_candle: usize,
    region: FlowRegionSpan,
    clip: (i64, i64),
    prices: (i64, i64),
) -> FlowTapeFrame {
    let candles = trades.len().div_ceil(per_candle);
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal / per_candle,
                accepted_ordinal: ordinal % per_candle,
                ticks_per_bar: Decimal::from(per_candle),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        FlowTapeView {
            first_slot: 0,
            end_slot: candles,
            clip_left: clip.0.into(),
            clip_right: clip.1.into(),
            width_px: 10.0 * candles as f32,
            height_px: 200.0,
            prices: PriceWindow::new(prices.0.into(), prices.1.into()).unwrap(),
            reference: FlowReference::VisibleRegions,
            radius_limit: 32.0,
            merge_support_radius: 6.0,
            region,
            exclude_opening: false,
        },
    )
}

/// Two candles divided at their peaks.
fn peaks(trades: &[Trade], prices: (i64, i64)) -> FlowTapeFrame {
    let per_candle = trades.len().div_ceil(2);
    project(trades, per_candle, FlowRegionSpan::Peaks(2), (0, 2), prices)
}

/// The dots from the lowest price up.
fn by_price(frame: &FlowTapeFrame) -> Vec<&FlowTapeDot> {
    let mut dots: Vec<_> = frame.dots.iter().collect();
    dots.sort_by_key(|dot| dot.mark.price);
    dots
}

fn members(dot: &FlowTapeDot) -> Vec<usize> {
    let mut ordinals: Vec<_> = dot.members.iter().map(|member| member.ordinal).collect();
    ordinals.sort_unstable();
    ordinals
}

/// Two plateaus of five rows, 100 a row, around a middle row of `middle`,
/// `step` apart from 1 000 up.
fn plateaus(middle: i64, step: i64) -> Vec<(i64, i64)> {
    (0..11)
        .map(|row| (1_000 + row * step, if row == 5 { middle } else { 100 }))
        .collect()
}

/// A single hill is one sphere, at its point of control, holding every print.
#[test]
fn one_hill_is_one_sphere_at_its_poc() {
    let rows = [
        (100, 1),
        (101, 3),
        (102, 8),
        (103, 20),
        (104, 40),
        (105, 20),
        (106, 8),
        (107, 3),
        (108, 1),
    ];
    let frame = peaks(&prints(&rows, Side::Buy), (90, 110));
    assert_eq!(frame.dots.len(), 1);
    let dot = &frame.dots[0];
    assert_eq!(dot.mark.quantity, Decimal::from(104));
    assert_eq!(dot.mark.price, Decimal::from(104));
    assert_eq!(members(dot), (0..9).collect::<Vec<_>>());
    assert_eq!(dot.radius, 32.0, "the only mass in view ranks first");
    // A hill whose top is the lowest price is still one hill.
    let edge = [(100, 40), (101, 20), (102, 8), (103, 3)];
    let frame = peaks(&prints(&edge, Side::Sell), (90, 110));
    assert_eq!(frame.dots.len(), 1);
    assert_eq!(frame.dots[0].mark.quantity, Decimal::from(71));
}

/// Two hills with a real valley between them are two spheres, cut at the
/// valley, whatever the instrument's tick: every print in exactly one.
#[test]
fn two_hills_with_a_real_valley_are_two_spheres() {
    for step in [1, 5] {
        let trades = prints(&plateaus(40, step), Side::Buy);
        let frame = peaks(&trades, (900, 1_100));
        let dots = by_price(&frame);
        assert_eq!(dots.len(), 2, "step {step}");
        assert_eq!(dots[0].mark.quantity, Decimal::from(500), "step {step}");
        assert_eq!(dots[1].mark.quantity, Decimal::from(540), "step {step}");
        assert_eq!(dots[0].mark.price, Decimal::from(1_000 + 2 * step));
        assert_eq!(members(dots[0]), (0..5).collect::<Vec<_>>());
        assert_eq!(members(dots[1]), (5..11).collect::<Vec<_>>());
    }
}

/// A dip shallower than the prominence floor (8% of the point of control,
/// after smoothing) leaves the hill whole.
#[test]
fn a_shallow_dip_stays_one_sphere() {
    let trades = prints(&plateaus(85, 1), Side::Buy);
    let frame = peaks(&trades, (900, 1_100));
    assert_eq!(frame.dots.len(), 1);
    assert_eq!(frame.dots[0].mark.quantity, Decimal::from(1_085));
}

/// The same prints give the same spheres, however their packets arrive and
/// whether the worker's cache or a whole projection builds them.
#[test]
fn peak_spheres_are_deterministic() {
    let mut rows = plateaus(40, 5);
    rows.extend(
        plateaus(60, 5)
            .into_iter()
            .map(|(price, quantity)| (price + 20, quantity)),
    );
    let trades: Vec<Trade> = prints(&rows, Side::Buy)
        .into_iter()
        .enumerate()
        .map(|(n, mut trade)| {
            if n % 3 == 0 {
                trade.side = Side::Sell;
            }
            trade
        })
        .collect();
    let first = peaks(&trades, (900, 1_200));
    let second = peaks(&trades, (900, 1_200));
    assert_eq!(first, second);
    assert!(first.dots.len() > 1);
    let execution = |ordinal: usize| FlowExecution {
        ordinal,
        slot: ordinal / 11,
        accepted_ordinal: ordinal % 11,
        ticks_per_bar: 11.into(),
        trade: &trades[ordinal],
        opening: false,
    };
    let view = FlowTapeView {
        end_slot: 2,
        clip_right: 2.into(),
        prices: PriceWindow::new(900.into(), 1_200.into()).unwrap(),
        region: FlowRegionSpan::Peaks(2),
        reference: FlowReference::VisibleRegionsAtLeast(1.into()),
        ..view(20.0)
    };
    let openings = FlowOpeningSelection::default();
    let mut forward = FlowTapeSource::default();
    forward.append((0..trades.len()).map(execution));
    let mut backward = FlowTapeSource::default();
    backward.append((0..trades.len()).rev().map(execution));
    let whole = forward.project(1, 0, 22, 0..22, view, openings);
    assert_eq!(
        backward.project_cached(1, 0, 22, 0..22, view, openings),
        whole
    );
    assert_eq!(
        forward.project_cached(1, 0, 22, 0..22, view, openings),
        whole
    );
}

/// A pan never regroups: the clip moving through a region, or the price
/// axis leaving one of its masses, keeps every other mass as it was.
#[test]
fn a_pan_never_regroups_peak_spheres() {
    // Four regions of two candles of six prints: two plateaus and one more
    // print in the valley between them.
    let mut rows = Vec::new();
    for _ in 0..4 {
        rows.extend(plateaus(20, 1));
        rows.push((1_005, 1));
    }
    let trades = prints(&rows, Side::Buy);
    let (region, per_candle) = (FlowRegionSpan::Peaks(2), 6);
    let whole = project(&trades, per_candle, region, (0, 8), (900, 1_100));
    assert_eq!(whole.dots.len(), 8);
    let panned = project(&trades, per_candle, region, (1, 8), (900, 1_100));
    let facts = |dot: &FlowTapeDot| {
        (
            members(dot),
            dot.mark.quantity,
            dot.mark.price,
            dot.candle_position,
        )
    };
    let held: Vec<_> = whole.dots.iter().map(facts).collect();
    assert!(!panned.dots.is_empty());
    for dot in &panned.dots {
        assert!(held.contains(&facts(dot)), "a pan regrouped {dot:?}");
    }
    // The axis leaves the upper hill of every region: the lower spheres are
    // the ones the whole axis shows, the upper ones leave whole.
    let low = project(&trades, per_candle, region, (0, 8), (990, 1_004));
    assert_eq!(low.dots.len(), 4);
    for dot in &low.dots {
        assert!(held.contains(&facts(dot)), "the axis regrouped {dot:?}");
    }
    let upper: usize = whole
        .dots
        .iter()
        .filter(|dot| dot.mark.price > Decimal::from(1_004))
        .map(|dot| dot.members.len())
        .sum();
    assert_eq!(low.off_axis_executions, upper);
}

/// Size is rank among the masses in view, not area against the largest: a
/// mass under a quarter of the largest still draws, as a small dot. Labels
/// are for the top fifth; colour strength follows the buy/sell imbalance.
#[test]
fn spheres_are_sized_by_rank_and_coloured_by_imbalance() {
    // Ten candles, one mass each, 10 to 100, far apart in price.
    let mut trades: Vec<Trade> = (1..=10)
        .map(|n| trade(n * 1_000, 100 + n * 50, n * 10, Side::Buy))
        .collect();
    // The second mass is balanced: half of it sold, in its own candle.
    trades[1].quantity = 10.into();
    trades.insert(2, trade(2_500, 200, 10, Side::Sell));
    let execution = |ordinal: usize, trade| FlowExecution {
        ordinal,
        slot: if ordinal < 2 { ordinal } else { ordinal - 1 },
        accepted_ordinal: usize::from(ordinal == 2),
        ticks_per_bar: 2.into(),
        trade,
        opening: false,
    };
    let framing = |region| FlowTapeView {
        end_slot: 10,
        clip_right: 10.into(),
        width_px: 100.0,
        prices: PriceWindow::new(0.into(), 1_000.into()).unwrap(),
        reference: FlowReference::VisibleRegions,
        radius_limit: 32.0,
        region,
        ..view(100.0)
    };
    let projected = |region| {
        project_flow_tape(
            trades
                .iter()
                .enumerate()
                .map(|(ordinal, trade)| execution(ordinal, trade)),
            1,
            11,
            11,
            framing(region),
        )
    };
    let frame = projected(FlowRegionSpan::Peaks(1));
    assert_eq!(frame.dots.len(), 10);
    let rank_radius = |rank: f32| {
        let stops = PEAK_RANK_STOPS;
        let shares = PEAK_RADIUS_SHARES;
        let at = stops.windows(2).position(|pair| rank <= pair[1]).unwrap();
        let t = (rank - stops[at]) / (stops[at + 1] - stops[at]);
        32.0 * (shares[at] + t * (shares[at + 1] - shares[at]))
    };
    let mut dots = frame.dots.clone();
    dots.sort_by_key(|dot| dot.mark.quantity);
    for (n, dot) in dots.iter().enumerate() {
        let expected = rank_radius((n + 1) as f32 / 10.0);
        assert!(
            (dot.radius - expected).abs() < 1e-3,
            "{}: {} against {expected}",
            dot.mark.quantity,
            dot.radius
        );
    }
    assert_eq!(dots[9].radius, 32.0);
    assert_eq!(frame.peak_label_floor(), Some(Decimal::from(80)));
    let balanced = frame
        .dots
        .iter()
        .find(|dot| dot.mark.quantity == Decimal::from(20))
        .unwrap();
    assert_eq!(frame.peak_fill_strength(balanced), PEAK_BALANCED_STRENGTH);
    assert_eq!(frame.peak_fill_strength(&dots[9]), 1.0);
    // Off, a frame keeps its earned areas, labels by size and full strength.
    let off = projected(FlowRegionSpan::Candles(1));
    assert_eq!(off.peak_label_floor(), None);
    assert_eq!(off.peak_fill_strength(balanced), 1.0);
}

/// The smoothing kernel is the Gaussian its sigma names, in exact integers.
#[test]
fn the_kernel_is_the_named_gaussian() {
    for (distance, weight) in PEAK_SMOOTHING_KERNEL.iter().enumerate() {
        let d = distance as f64 / PEAK_SMOOTHING_SIGMA_TICKS;
        let expected = (1e6 * (-d * d / 2.0).exp()).round() as u32;
        assert_eq!(*weight, expected, "distance {distance}");
    }
}

/// FNV-1a of a frame's full debug text: every field, every bit.
fn digest(frame: &FlowTapeFrame) -> u64 {
    format!("{frame:?}")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// With peak spheres off a frame is the one this crate built before them:
/// the digests were recorded on the code before the mode existed.
#[test]
fn off_is_the_frame_from_before_peak_spheres() {
    let trades: Vec<Trade> = (0..400_i64)
        .map(|n| {
            let side = if n % 3 == 0 { Side::Sell } else { Side::Buy };
            trade(1_000 + n * 37, 95 + n / 5 % 11, 1 + n * 13 % 17, side)
        })
        .collect();
    let mut digests = Vec::new();
    for reference in [
        FlowReference::VisibleRegions,
        FlowReference::Typed(900.into()),
        FlowReference::VisibleRegionsAtLeast(90.into()),
    ] {
        for region in [
            FlowRegionSpan::Time,
            FlowRegionSpan::Candles(1),
            FlowRegionSpan::Candles(8),
        ] {
            let frame = project_flow_tape(
                trades
                    .iter()
                    .enumerate()
                    .map(|(ordinal, trade)| FlowExecution {
                        ordinal,
                        slot: ordinal / 4,
                        accepted_ordinal: ordinal % 4,
                        ticks_per_bar: 4.into(),
                        trade,
                        opening: ordinal < 3,
                    }),
                1,
                400,
                400,
                FlowTapeView {
                    end_slot: 100,
                    clip_left: 3.into(),
                    clip_right: 97.into(),
                    width_px: 2_000.0,
                    reference,
                    region,
                    ..view(2_000.0)
                },
            );
            assert!(!frame.dots.is_empty());
            digests.push(digest(&frame));
        }
    }
    assert_eq!(digests, GOLDEN_OFF_DIGESTS);
}

/// Recorded at `dd7c8e68`, before peak spheres.
const GOLDEN_OFF_DIGESTS: [u64; 9] = [
    6_621_146_585_317_302_784,
    16_283_649_034_519_367_430,
    593_744_703_204_269_686,
    4_953_434_344_659_677_610,
    12_806_478_368_885_026_861,
    1_504_679_874_952_691_980,
    13_539_935_223_157_163_082,
    4_489_037_191_021_631_944,
    3_515_181_521_242_157_339,
];
