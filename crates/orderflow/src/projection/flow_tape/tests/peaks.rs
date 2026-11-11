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

/// The radius rank `rank` gives at a 32 px full radius: the rank map's share
/// of the range from the minimum radius up.
fn ranked_radius(rank: f32) -> f32 {
    let (stops, shares) = (PEAK_RANK_STOPS, PEAK_RADIUS_SHARES);
    let at = stops.windows(2).position(|pair| rank <= pair[1]).unwrap();
    let t = (rank - stops[at]) / (stops[at + 1] - stops[at]);
    let share = shares[at] + t * (shares[at + 1] - shares[at]);
    32.0 - (1.0 - share) * (32.0 - PEAK_MIN_RADIUS_PX)
}

/// One mass a candle, far apart in price, `quantities` in order.
fn masses(quantities: &[i64]) -> FlowTapeFrame {
    let trades: Vec<Trade> = quantities
        .iter()
        .enumerate()
        .map(|(n, &quantity)| {
            trade(
                n as i64 * 1_000,
                1_000 + n as i64 * 1_000,
                quantity,
                Side::Buy,
            )
        })
        .collect();
    let candles = trades.len();
    let top = 1_000 * (candles as i64 + 1);
    let view = peak_view(
        candles,
        FlowRegionSpan::Peaks(1),
        (0, candles as i64),
        (0, top),
        0.25,
    );
    framed(&trades, |ordinal| ordinal, view)
}

/// Size is rank among the masses in view: more volume is never a smaller
/// sphere, equal volume is an equal sphere, and the scale says so. Labels go
/// to the top fifth by rank.
#[test]
fn spheres_are_ranked_monotonically_and_ties_share_a_rank() {
    let frame = masses(&[40, 10, 80, 20, 80, 20]);
    let mut dots = frame.dots.clone();
    dots.sort_by_key(|dot| dot.mark.quantity);
    for pair in dots.windows(2) {
        assert!(pair[0].radius <= pair[1].radius, "monotonic");
        if pair[0].mark.quantity == pair[1].mark.quantity {
            assert_eq!(pair[0].radius, pair[1].radius, "ties share a rank");
        }
    }
    for dot in &dots {
        let at_or_below = dots
            .iter()
            .filter(|other| other.mark.quantity <= dot.mark.quantity)
            .count();
        let expected = ranked_radius(at_or_below as f32 / 6.0);
        assert!(
            (dot.radius - expected).abs() < 1e-3,
            "{} {}",
            dot.radius,
            expected
        );
    }
    assert_eq!(dots[5].radius, 32.0);
    assert_eq!(
        frame.scale_basis,
        FlowScaleBasis::RankedVisibleMasses {
            masses: 6,
            labelled_from: 80.into(),
        }
    );
    assert_eq!(frame.effective_reference, Some(Decimal::from(80)));
    assert_eq!(frame.peak_label_floor(), Some(Decimal::from(80)));
    assert_eq!(frame.large_region_threshold(), None, "no quarter floor");
    assert!(
        flow_notes(Some(&frame), FlowProgress::default())
            .contains(&"Peak spheres ranked among 6 visible masses; labels from 80".to_owned())
    );
}

/// With one mass in view it is the largest; with two unequal ones the
/// smaller sits at the median share; two equal ones are both the largest.
#[test]
fn one_or_two_masses_are_ranked_stably() {
    assert_eq!(masses(&[50]).dots[0].radius, 32.0);
    let two = masses(&[50, 100]);
    assert_eq!(two.dots[0].radius, ranked_radius(0.5));
    assert_eq!(two.dots[1].radius, 32.0);
    let tie = masses(&[70, 70]);
    assert!(tie.dots.iter().all(|dot| dot.radius == 32.0));
    assert_eq!(masses(&[50, 100]), two, "the same masses, the same sizes");
}

/// A mass that joins two neighbours outranks an unchanged neighbour either
/// part tied: the joined sphere grows, the neighbour falls back. Colour
/// strength follows the buy/sell imbalance.
#[test]
fn a_join_outranks_the_neighbour_its_parts_tied() {
    let trades = [
        trade(1_000, 1_000, 100, Side::Buy),
        trade(2_000, 1_000, 100, Side::Buy),
        trade(3_000, 2_000, 50, Side::Buy),
        trade(3_500, 2_000, 50, Side::Sell),
        trade(4_000, 3_000, 150, Side::Buy),
    ];
    let slot = |ordinal: usize| [0, 1, 2, 2, 3][ordinal];
    let at = |region| {
        framed(
            &trades,
            slot,
            peak_view(4, region, (0, 4), (0, 4_000), 0.25),
        )
    };
    let find = |frame: &FlowTapeFrame, price: i64| {
        frame
            .dots
            .iter()
            .filter(|dot| dot.mark.price == Decimal::from(price))
            .max_by_key(|dot| dot.mark.quantity)
            .cloned()
            .unwrap()
    };
    let apart = at(FlowRegionSpan::Peaks(1));
    let joined = at(FlowRegionSpan::Peaks(2));
    conserved(&apart, &trades);
    conserved(&joined, &trades);
    let (part, before) = (find(&apart, 1_000), find(&apart, 2_000));
    assert_eq!(part.radius, before.radius, "the part ties the neighbour");
    let (both, after) = (find(&joined, 1_000), find(&joined, 2_000));
    assert_eq!(both.mark.quantity, Decimal::from(200));
    assert!(both.radius > part.radius, "the join grows");
    assert!(both.radius > after.radius, "and outranks the neighbour");
    assert!(after.radius < before.radius, "which falls back");
    assert_eq!(joined.peak_fill_strength(&after), PEAK_BALANCED_STRENGTH);
    assert_eq!(joined.peak_fill_strength(&both), 1.0);
    let off = at(FlowRegionSpan::Candles(2));
    assert_eq!(off.peak_label_floor(), None);
    assert_eq!(off.peak_fill_strength(&after), 1.0);
}

/// One spot walked through three zoom steps: as its candles pool, the
/// joined mass grows against a neighbour that never changes.
#[test]
fn a_joined_mass_grows_against_its_neighbour_through_three_zooms() {
    let mut trades: Vec<Trade> = (0..4)
        .map(|n| trade(n * 1_000, 1_000, 100, Side::Buy))
        .collect();
    trades.push(trade(5_000, 5_000, 400, Side::Sell));
    trades.push(trade(6_000, 3_000, 50, Side::Sell));
    trades.push(trade(7_000, 4_000, 200, Side::Buy));
    let mut shares = Vec::new();
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
        shares.push(spot.radius / neighbour.radius);
    }
    assert!(shares[0] < shares[1] && shares[1] < shares[2], "{shares:?}");
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
/// two kept masses stand closer than two smoothing sigmas.
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
            let sigma_px = PEAK_SIGMA_RADIUS_SHARE as f32 * 32.0;
            assert!(
                apart >= 2.0 * f64::from(sigma_px),
                "{apart} px apart at {px_per_price}"
            );
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
    let held = facts(&at((990, 1_090), 0.65));
    assert_eq!(facts(&at((1_003, 1_103), 0.65)), held, "a vertical pan");
    // A 6.4 px sigma over 0.64 px a point is a 10-point row: 0.65 reads rows
    // of 10, 0.63 alone would read rows of 20. The session holds 0.65.
    let mut session = FlowSession::<NoRunner>::default();
    assert_eq!(session.merge_px_per_price(0.65), 0.65);
    let wobble = session.merge_px_per_price(0.63);
    assert_eq!(wobble, 0.65, "inside the hold band");
    assert_eq!(facts(&at((990, 1_090), wobble)), held);
    assert_eq!(session.merge_px_per_price(0.4), 0.4, "a zoom step");
}

/// The daily opening burst stays out of the prominence reference: the
/// auction's spike does not swallow the ordinary hills of its region. With
/// the opening ignored it leaves the ranking and keeps its own treatment;
/// with it counted it ranks like any mass.
#[test]
fn the_opening_burst_does_not_swallow_its_region() {
    let mut trades = vec![trade(0, 1_000, 10_000, Side::Buy)];
    for (n, price) in [1_100, 1_200].into_iter().enumerate() {
        trades.push(trade(3_600_000 + n as i64 * 1_000, price, 100, Side::Sell));
    }
    let project = |exclude_opening| {
        let view = FlowTapeView {
            exclude_opening,
            ..peak_view(1, FlowRegionSpan::Peaks(1), (0, 1), (900, 1_300), 40.0)
        };
        project_flow_tape(
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
        )
    };
    let counted = project(false);
    let burst = counted.dots.iter().find(|dot| dot.opening_anchor).unwrap();
    assert_eq!(burst.radius, 32.0, "counted, the burst ranks first");
    assert!(
        counted
            .dots
            .iter()
            .all(|dot| dot.opening_anchor || dot.radius < 32.0)
    );
    let frame = project(true);
    conserved(&frame, &trades);
    assert_eq!(frame.dots.len(), 3, "the opening and both ordinary hills");
    assert!(frame.opening_exclusion_effective);
    assert_eq!(frame.effective_reference, Some(Decimal::from(100)));
    assert_eq!(
        frame.scale_basis,
        FlowScaleBasis::RankedVisibleMasses {
            masses: 2,
            labelled_from: 100.into(),
        },
        "the burst left the ranking"
    );
    for dot in frame.dots.iter().filter(|dot| !dot.opening_anchor) {
        assert_eq!(dot.radius, 32.0);
    }
    let burst = frame.dots.iter().find(|dot| dot.opening_anchor).unwrap();
    assert!(burst.opening_oversized, "the existing opening treatment");
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
/// 2 000 prints, 70 from the end, at the mini index's 18 px radius, in
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
                radius_limit: 18.0,
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
