//! Peak spheres: a zoom region divided at the valleys of its volume-by-price
//! profile, one sphere per peak, sized by rank among the masses in view.
use super::*;
use rust_decimal::prelude::ToPrimitive as _;

/// One print per `(price, quantity)` row, a second apart, all on `side`.
fn prints(rows: &[(i64, i64)], side: Side) -> Vec<Trade> {
    rows.iter()
        .enumerate()
        .map(|(n, &(price, quantity))| trade(n as i64 * 1_000, price, quantity, side))
        .collect()
}

/// A peak view of `candles` candles over `prices` drawn `px_per_price`
/// pixels a price unit tall, at a 32 px full radius.
fn peak_view(
    candles: usize,
    region: FlowRegionSpan,
    clip: (i64, i64),
    prices: (i64, i64),
    px_per_price: f32,
) -> FlowTapeView {
    FlowTapeView {
        first_slot: 0,
        end_slot: candles,
        clip_left: clip.0.into(),
        clip_right: clip.1.into(),
        width_px: 10.0 * candles as f32,
        height_px: px_per_price * (prices.1 - prices.0) as f32,
        prices: PriceWindow::new(prices.0.into(), prices.1.into()).unwrap(),
        reference: FlowReference::VisibleRegions,
        radius_limit: 32.0,
        merge_support_radius: 6.0,
        region,
        exclude_opening: false,
    }
}

/// `trades` in `view`, each on the candle `slot` names.
fn framed(trades: &[Trade], slot: impl Fn(usize) -> usize, view: FlowTapeView) -> FlowTapeFrame {
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: slot(ordinal),
                accepted_ordinal: ordinal,
                ticks_per_bar: Decimal::from(trades.len()),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        view,
    )
}

/// `trades`, `per_candle` to a candle, in a view of `prices` and `clip` fine
/// enough (40 px a price unit) that the profile is read on the native tick.
fn project(
    trades: &[Trade],
    per_candle: usize,
    region: FlowRegionSpan,
    clip: (i64, i64),
    prices: (i64, i64),
) -> FlowTapeFrame {
    let candles = trades.len().div_ceil(per_candle);
    let view = peak_view(candles, region, clip, prices, 40.0);
    framed(trades, |ordinal| ordinal / per_candle, view)
}

/// Every execution is in exactly one sphere: nothing lost, nothing counted twice.
fn conserved(frame: &FlowTapeFrame, trades: &[Trade]) {
    let drawn: Decimal = frame.dots.iter().map(|dot| dot.mark.quantity).sum();
    let printed: Decimal = trades.iter().map(|trade| trade.quantity).sum();
    assert_eq!(drawn, printed, "volume is conserved");
    let members: usize = frame.dots.iter().map(|dot| dot.members.len()).sum();
    assert_eq!(members, trades.len());
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
            .map(|(price, quantity)| (price + 200, quantity)),
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
    let first = peaks(&trades, (900, 1_300));
    let second = peaks(&trades, (900, 1_300));
    assert_eq!(first, second);
    assert_eq!(first.dots.len(), 4, "four hills apart");
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
        prices: PriceWindow::new(900.into(), 1_300.into()).unwrap(),
        region: FlowRegionSpan::Peaks(2),
        reference: FlowReference::VisibleRegionsAtLeast(1.into()),
        height_px: 16_000.0,
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

/// Size is area proportional to volume against the frame's reference, with
/// no quarter-of-the-reference floor: when two masses join, the joined sphere
/// has the area of both. Labels go to masses of at least half the reference;
/// colour strength follows the buy/sell imbalance.
#[test]
fn joined_masses_add_their_areas_and_colour_by_imbalance() {
    let trades = [
        trade(1_000, 1_000, 100, Side::Buy),
        trade(2_000, 1_000, 100, Side::Buy),
        trade(3_000, 2_000, 100, Side::Buy),
        trade(3_500, 2_000, 100, Side::Sell),
        trade(4_000, 6_000, 100, Side::Buy),
    ];
    let slot = |ordinal: usize| [0, 1, 2, 2, 3][ordinal];
    let at = |region| {
        framed(
            &trades,
            slot,
            peak_view(4, region, (0, 4), (0, 8_000), 0.25),
        )
    };
    let apart = at(FlowRegionSpan::Peaks(1));
    let joined = at(FlowRegionSpan::Peaks(2));
    conserved(&apart, &trades);
    conserved(&joined, &trades);
    assert_eq!(apart.dots.len(), 4);
    assert_eq!(joined.dots.len(), 3);
    let find = |frame: &FlowTapeFrame, price: i64, quantity: i64| {
        frame
            .dots
            .iter()
            .find(|dot| {
                dot.mark.price == Decimal::from(price) && dot.mark.quantity == quantity.into()
            })
            .cloned()
            .unwrap()
    };
    let part = find(&apart, 1_000, 100);
    let both = find(&joined, 1_000, 200);
    let single = find(&joined, 6_000, 100);
    let ratio = both.radius / part.radius;
    assert!((ratio - 2_f32.sqrt()).abs() < 1e-4, "joined {ratio}");
    let ratio = both.radius / single.radius;
    assert!(
        (ratio - 2_f32.sqrt()).abs() < 1e-4,
        "beside a single {ratio}"
    );
    assert_eq!(
        both.radius, 32.0,
        "the largest mass reaches the full radius"
    );
    assert_eq!(joined.large_region_threshold(), None, "no quarter floor");
    assert_eq!(joined.peak_label_floor(), Some(Decimal::from(100)));
    let balanced = find(&joined, 2_000, 200);
    assert_eq!(joined.peak_fill_strength(&balanced), PEAK_BALANCED_STRENGTH);
    assert_eq!(joined.peak_fill_strength(&both), 1.0);
    let off = at(FlowRegionSpan::Candles(2));
    assert_eq!(off.peak_label_floor(), None);
    assert_eq!(off.peak_fill_strength(&balanced), 1.0);
}

/// One spot walked through three zoom steps: as its candles pool, the
/// joined mass grows against a neighbour that never changes.
#[test]
fn a_joined_mass_grows_against_its_neighbour_through_three_zooms() {
    let mut trades: Vec<Trade> = (0..4)
        .map(|n| trade(n * 1_000, 1_000, 100, Side::Buy))
        .collect();
    trades.push(trade(5_000, 5_000, 400, Side::Sell));
    let mut grown = Vec::new();
    for candles in [1, 2, 4] {
        let view = peak_view(8, FlowRegionSpan::Peaks(candles), (0, 8), (0, 6_000), 0.25);
        let frame = framed(&trades, |ordinal| ordinal, view);
        conserved(&frame, &trades);
        let spot = frame
            .dots
            .iter()
            .filter(|dot| dot.mark.price == Decimal::from(1_000))
            .max_by_key(|dot| dot.mark.quantity)
            .unwrap();
        let neighbour = frame
            .dots
            .iter()
            .find(|dot| dot.mark.price == Decimal::from(5_000))
            .unwrap();
        assert_eq!(neighbour.radius, 32.0, "the neighbour never changes");
        let areas = (spot.radius / neighbour.radius).powi(2);
        let quantities = spot.mark.quantity / neighbour.mark.quantity;
        assert!(
            (f64::from(areas) - quantities.to_f64().unwrap()).abs() < 1e-4,
            "area follows volume at {candles}"
        );
        grown.push(spot.radius);
    }
    assert!(grown[0] < grown[1] && grown[1] < grown[2], "{grown:?}");
}

/// A mass of a hundredth of the reference draws at the minimum radius: small,
/// never hidden.
#[test]
fn a_hundredth_of_the_reference_draws_at_the_minimum_radius() {
    let trades = [
        trade(1_000, 1_000, 100, Side::Buy),
        trade(2_000, 2_000, 1, Side::Buy),
    ];
    let view = FlowTapeView {
        radius_limit: 18.0,
        ..peak_view(2, FlowRegionSpan::Peaks(1), (0, 2), (0, 4_000), 0.25)
    };
    let frame = framed(&trades, |ordinal| ordinal, view);
    let small = frame
        .dots
        .iter()
        .find(|dot| dot.mark.quantity == Decimal::ONE)
        .unwrap();
    assert_eq!(small.radius, PEAK_MIN_RADIUS_PX);
}

/// Six hills of one candle, 15 points apart on a 5-point tick, with a lot in
/// each valley so the tape names its tick.
fn six_hills() -> Vec<Trade> {
    let mut rows = Vec::new();
    for hill in 0..6 {
        rows.push((1_000 + hill * 15, 100));
        rows.push((1_005 + hill * 15, 1));
    }
    prints(&rows, Side::Buy)
}

/// Two peaks are two spheres only when the screen holds them further apart
/// than a sphere: the profile's rows are a power-of-two multiple of the tick
/// chosen from the pixels a price unit is drawn, so a coarse zoom reads the
/// six hills of one candle as one or two masses, a fine one as six, and no
/// two kept masses stand closer than a full radius.
#[test]
fn close_hills_are_one_mass_at_a_coarse_price_step() {
    let trades = six_hills();
    let mut counts = Vec::new();
    for px_per_price in [40.0, 2.0, 0.2] {
        let view = peak_view(
            1,
            FlowRegionSpan::Peaks(1),
            (0, 1),
            (990, 1_090),
            px_per_price,
        );
        let frame = framed(&trades, |_| 0, view);
        conserved(&frame, &trades);
        let mut prices: Vec<f64> = frame
            .dots
            .iter()
            .map(|dot| dot.mark.price.to_f64().unwrap())
            .collect();
        prices.sort_by(f64::total_cmp);
        for pair in prices.windows(2) {
            let apart = (pair[1] - pair[0]) * f64::from(px_per_price);
            assert!(apart >= 32.0, "{apart} px apart at {px_per_price}");
        }
        counts.push(frame.dots.len());
    }
    assert_eq!(counts[0], 6, "fine: one sphere per hill");
    assert!(counts[2] <= 2, "coarse: {counts:?}");
    assert!(
        counts[1] <= counts[0] && counts[2] <= counts[1],
        "{counts:?}"
    );
}

/// A test runner the session needs only for its type.
struct NoRunner;
impl FlowRunner for NoRunner {
    fn request(&mut self, _: &FlowRequest, _: bool) -> bool {
        false
    }
    fn submit(&self, _: FlowChunk) -> Result<(), Box<FlowChunk>> {
        Ok(())
    }
    fn finished(&self) -> Option<std::sync::Arc<FlowTapeFrame>> {
        None
    }
}
impl Default for NoRunner {
    fn default() -> Self {
        Self
    }
}

/// The price step follows the held price scale, not the axis: a vertical pan
/// keeps every mass, and a zoom wobbling across a step's boundary keeps the
/// step the session holds; only a zoom step past the hold band moves it.
#[test]
fn the_price_step_holds_under_pan_and_a_zoom_wobble() {
    let trades = six_hills();
    let facts = |frame: &FlowTapeFrame| {
        let mut facts: Vec<_> = frame
            .dots
            .iter()
            .map(|dot| (members(dot), dot.mark.quantity, dot.mark.price))
            .collect();
        facts.sort();
        facts
    };
    let at = |prices: (i64, i64), px_per_price: f64| {
        let view = peak_view(
            1,
            FlowRegionSpan::Peaks(1),
            (0, 1),
            prices,
            px_per_price as f32,
        );
        framed(&trades, |_| 0, view)
    };
    let held = facts(&at((990, 1_090), 0.81));
    assert_eq!(facts(&at((1_003, 1_103), 0.81)), held, "a vertical pan");
    // 32 px a sigma over 0.8 px a point is a 40-point row: 0.81 reads rows of
    // 40, 0.79 alone would read rows of 80. The session holds 0.81 through it.
    let mut session = FlowSession::<NoRunner>::default();
    assert_eq!(session.merge_px_per_price(0.81), 0.81);
    let wobble = session.merge_px_per_price(0.79);
    assert_eq!(wobble, 0.81, "inside the hold band");
    assert_eq!(facts(&at((990, 1_090), wobble)), held);
    assert_eq!(session.merge_px_per_price(0.4), 0.4, "a zoom step");
}

/// The daily opening burst stays out of the prominence reference: the
/// auction's spike does not swallow the ordinary hills of its region, and
/// with the opening ignored in the scale the ordinary masses are sized
/// against one another.
#[test]
fn the_opening_burst_does_not_swallow_its_region() {
    let mut trades = vec![trade(0, 1_000, 10_000, Side::Buy)];
    for (n, price) in [1_100, 1_200].into_iter().enumerate() {
        trades.push(trade(3_600_000 + n as i64 * 1_000, price, 100, Side::Sell));
    }
    let view = FlowTapeView {
        exclude_opening: true,
        ..peak_view(1, FlowRegionSpan::Peaks(1), (0, 1), (900, 1_300), 40.0)
    };
    let frame = project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: 0,
                accepted_ordinal: ordinal,
                ticks_per_bar: 3.into(),
                trade,
                opening: ordinal == 0,
            }),
        1,
        3,
        3,
        view,
    );
    conserved(&frame, &trades);
    assert_eq!(frame.dots.len(), 3, "the opening and both ordinary hills");
    assert!(frame.opening_exclusion_effective);
    assert_eq!(frame.effective_reference, Some(Decimal::from(100)));
    for dot in frame.dots.iter().filter(|dot| !dot.opening_anchor) {
        assert_eq!(dot.radius, 32.0);
    }
}

/// Prices at the edge of what a decimal holds never panic the worker.
#[test]
fn prices_with_many_decimals_are_divided_safely() {
    let price = |n: i128| Decimal::from_i128_with_scale(10_i128.pow(25) + n * 7, 25);
    let trades: Vec<Trade> = (0..12)
        .map(|n| Trade {
            agg_id: n as u64,
            timestamp_ms: n * 1_000,
            price: price(i128::from(n % 4)),
            quantity: 3.into(),
            side: Side::Buy,
        })
        .collect();
    for px_per_price in [1e20_f32, 1.0, 1e-9] {
        let view = FlowTapeView {
            prices: PriceWindow::new(Decimal::ZERO, 2.into()).unwrap(),
            height_px: px_per_price * 2.0,
            ..peak_view(1, FlowRegionSpan::Peaks(1), (0, 1), (0, 2), 1.0)
        };
        let frame = framed(&trades, |_| 0, view);
        conserved(&frame, &trades);
    }
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

/// The prototype's measurement on a recorded WIN session: 60 candles of
/// 2 000 prints, 70 from the end, at the mini index's 24 px peak radius, in
/// regions of two candles (28 px a candle) and of one (60 px a candle, where
/// the worst overlap between two spheres is reported). Reads the
/// `quantick-replay 1` CSV named by `PEAK_SPHERES_REPLAY_CSV`; run by name
/// with `--ignored --nocapture`.
#[test]
#[ignore = "reads a local replay recording; run by name"]
fn a_recorded_win_session_matches_the_prototype() {
    const TICKS: usize = 2_000;
    const HEIGHT_PX: f32 = 660.0;
    let path = std::env::var("PEAK_SPHERES_REPLAY_CSV").expect("PEAK_SPHERES_REPLAY_CSV");
    let text = std::fs::read_to_string(path).expect("a readable recording");
    let all: Vec<Trade> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("Date"))
        .enumerate()
        .map(|(n, line)| {
            let fields: Vec<&str> = line.split(',').collect();
            let side = if fields[6] == "B" {
                Side::Buy
            } else {
                Side::Sell
            };
            Trade {
                agg_id: n as u64,
                timestamp_ms: n as i64,
                price: fields[2].parse().unwrap(),
                quantity: fields[5].parse().unwrap(),
                side,
            }
        })
        .collect();
    let first = (all.len() / TICKS - 70) * TICKS;
    let trades = &all[first..first + 60 * TICKS];
    let low = trades.iter().map(|trade| trade.price).min().unwrap() - Decimal::from(40);
    let high = trades.iter().map(|trade| trade.price).max().unwrap() + Decimal::from(40);
    let px_per_price = f64::from(HEIGHT_PX) / (high - low).to_f64().unwrap();
    for (candles, px_per_candle) in [(2, 28.0), (1, 60.0)] {
        let started = std::time::Instant::now();
        let frame = project_flow_tape(
            trades
                .iter()
                .enumerate()
                .map(|(ordinal, trade)| FlowExecution {
                    ordinal,
                    slot: ordinal / TICKS,
                    accepted_ordinal: ordinal % TICKS,
                    ticks_per_bar: TICKS.into(),
                    trade,
                    opening: false,
                }),
            1,
            trades.len(),
            trades.len(),
            FlowTapeView {
                first_slot: 0,
                end_slot: 60,
                clip_left: 0.into(),
                clip_right: 60.into(),
                width_px: 60.0 * px_per_candle,
                height_px: HEIGHT_PX,
                prices: PriceWindow::new(low, high).unwrap(),
                reference: FlowReference::VisibleRegionsAtLeast(Decimal::ONE),
                radius_limit: 24.0,
                merge_support_radius: 6.0,
                region: FlowRegionSpan::Peaks(candles),
                exclude_opening: false,
            },
        );
        let elapsed = started.elapsed();
        conserved(&frame, trades);
        let mut radii: Vec<f32> = frame.dots.iter().map(|dot| dot.radius).collect();
        radii.sort_by(f32::total_cmp);
        let floor = frame.peak_label_floor().unwrap();
        let labelled = frame.dots.iter().filter(|dot| dot.mark.quantity >= floor);
        let centre = |dot: &FlowTapeDot| {
            (
                dot.candle_position.to_f64().unwrap() * f64::from(px_per_candle),
                dot.mark.price.to_f64().unwrap() * px_per_price,
            )
        };
        let (mut worst, mut worst_same) = (0.0_f64, 0.0_f64);
        for (n, a) in frame.dots.iter().enumerate() {
            for b in &frame.dots[n + 1..] {
                let ((ax, ay), (bx, by)) = (centre(a), centre(b));
                let reach = f64::from(a.radius + b.radius);
                let overlap = (reach - (ax - bx).hypot(ay - by)).max(0.0)
                    / f64::from(2.0 * a.radius.min(b.radius));
                worst = worst.max(overlap);
                if a.first_slot / candles == b.first_slot / candles {
                    worst_same = worst_same.max(overlap);
                }
            }
        }
        println!(
            "k = {candles}: {} spheres over 60 candles; radius min / median / max \
             {:.1} / {:.1} / {:.1}; {} labelled; worst overlap {:.0}% of the smaller \
             diameter ({:.0}% inside one region); whole cold projection {elapsed:?}",
            radii.len(),
            radii[0],
            radii[radii.len() / 2],
            radii[radii.len() - 1],
            labelled.count(),
            worst * 100.0,
            worst_same * 100.0,
        );
    }
}
