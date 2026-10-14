//! Member-level comparison against the unchanged tape kernel on a real WIN burst.
use quantick_engine::{Side, Trade, fixture::parse_trades};
use quantick_orderflow::{
    HeatmapConfig,
    projection::{
        DotSizing, PriceWindow, TapeDotGeometry, TapeDotMemory, TapeDotView,
        flow_tape::{
            FlowBurstView, FlowExecution, FlowReference, FlowTapeFrame, FlowTapeView,
            project_flow_tape,
        },
    },
};
use quantick_stores::bubble_presets;
use rust_decimal::{
    Decimal,
    prelude::{FromPrimitive as _, ToPrimitive as _},
};

#[test]
fn fixed_win_width_retains_the_measured_calibration() {
    let style = bubble_presets::embedded()
        .get("B3 WIN tick spheres")
        .unwrap()
        .tick_bubbles
        .unwrap();
    let calibration = style.tape_bursts.unwrap();
    assert_eq!(calibration.width_px.to_bits(), 1_152_262_407);
    assert_eq!(calibration.window_ms, 72_000);
    assert_eq!(calibration.price_units_per_pixel, 0.8);
    assert_eq!(calibration.max_radius, 18.0);
    assert_eq!(style.reference_quantity, 20_000.0);
    assert_eq!(style.max_radius, 18.0);
}

fn recorded_burst(width: f32) -> (FlowTapeFrame, Vec<Trade>) {
    let path = std::env::var_os("WIN_TICK_FIXTURE").expect("private canonical recording");
    let all = parse_trades(&std::fs::read_to_string(path).unwrap()).unwrap();
    let presets = bubble_presets::embedded();
    let style = presets
        .get("B3 WIN tick spheres")
        .unwrap()
        .tick_bubbles
        .unwrap();
    let mut calibration = style.tape_bursts.unwrap();
    calibration.width_px = width;
    let target = all
        .iter()
        .rfind(|trade| trade.quantity == Decimal::from(1291))
        .unwrap();
    assert_eq!(
        target.timestamp_ms, 1_791_992_767_257,
        "the inspected source event"
    );
    let start = target.timestamp_ms.div_euclid(calibration.window_ms) * calibration.window_ms;
    let end = start + calibration.window_ms;
    let trades = &all[all.partition_point(|trade| trade.timestamp_ms < start)
        ..all.partition_point(|trade| trade.timestamp_ms < end)];
    let unit = Decimal::from_f64(calibration.price_units_per_pixel).unwrap();
    let pad = unit * Decimal::from_f32(calibration.max_radius + 1.0).unwrap();
    let prices = PriceWindow::new(
        trades.iter().map(|trade| trade.price).min().unwrap() - pad,
        trades.iter().map(|trade| trade.price).max().unwrap() + pad,
    )
    .unwrap();
    let height = ((prices.high - prices.low) / unit).to_f32().unwrap();
    let project = |native_cells| {
        let ticks = if native_cells { trades.len() } else { 2_000 };
        let slots = trades.len().div_ceil(ticks);
        project_flow_tape(
            trades
                .iter()
                .enumerate()
                .map(|(ordinal, trade)| FlowExecution {
                    ordinal,
                    slot: ordinal / ticks,
                    accepted_ordinal: ordinal % ticks,
                    ticks_per_bar: Decimal::from(ticks),
                    trade,
                    opening: false,
                }),
            1,
            trades.len(),
            trades.len(),
            FlowTapeView {
                tape_bursts: (!native_cells).then_some(FlowBurstView {
                    calibration,
                    from_ms: start,
                    until_ms: end,
                    settled_through_ms: end,
                }),
                region_window_ms: 100,
                first_slot: 0,
                end_slot: slots,
                clip_left: Decimal::ZERO,
                clip_right: Decimal::from(slots),
                width_px: 840.0,
                height_px: height,
                prices,
                reference: FlowReference::Typed(20_000.into()),
                radius_limit: style.max_radius,
                merge_support_radius: 0.000_001,
                exclude_opening: false,
            },
        )
    };
    let cells = project(true);
    let marks: Vec<_> = cells
        .dots
        .iter()
        .map(|dot| {
            let mut mark = dot.mark.clone();
            mark.live = true;
            mark.agg_ids = dot.members.iter().map(|member| member.source_id).collect();
            mark
        })
        .collect();
    let mut original = HeatmapConfig::default();
    presets
        .get("mini index regions")
        .unwrap()
        .apply_to(&mut original);
    let tape = TapeDotMemory::default().project(
        &marks,
        TapeDotView {
            now_ms: end,
            window_ms: calibration.window_ms,
            dot_window_ms: 100,
            evicted_through_ms: None,
            prices,
            geometry: TapeDotGeometry {
                left_x: 0.0,
                right_x: 1.0,
                width_px: calibration.width_px,
                height_px: height,
            },
        },
        DotSizing {
            native_tape: true,
            tape_column_px: 0.0,
            candle_column_px: 0.0,
            px_per_price: 0.0,
            typed_full: None,
        },
        &original.bubbles,
        &original.live_lane,
        &[],
    );
    let chart = project(false);
    assert_eq!(chart.dots.len(), tape.marks.len());
    for dot in &chart.dots {
        let mut ids: Vec<_> = dot.members.iter().map(|member| member.source_id).collect();
        ids.sort_unstable();
        let mark = tape
            .marks
            .iter()
            .find(|mark| mark.agg_ids == ids)
            .expect("identical native tape members");
        assert_eq!(dot.mark.quantity, mark.quantity);
        assert_eq!(dot.mark.buy_quantity, mark.buy_quantity);
        assert_eq!(dot.mark.price, mark.price);
        let position_quantity: Decimal = dot
            .members
            .iter()
            .map(|member| {
                (Decimal::from(member.candle_slot)
                    + (Decimal::from(member.accepted_ordinal) + Decimal::new(5, 1))
                        / Decimal::from(2_000))
                    * trades[member.ordinal].quantity
            })
            .sum();
        assert_eq!(dot.candle_position, position_quantity / dot.mark.quantity);
    }
    let peak = chart
        .dots
        .iter()
        .max_by_key(|dot| dot.mark.quantity)
        .unwrap();
    eprintln!(
        "WIN_BURST_GOLDEN source_count={} native_cells={} groups={} quantity={} buy={} executions={} start={} end={} radius={}",
        trades.len(),
        cells.dots.len(),
        chart.dots.len(),
        peak.mark.quantity,
        peak.mark.buy_quantity,
        peak.members.len(),
        peak.mark.first_timestamp_ms,
        peak.mark.last_timestamp_ms,
        peak.radius
    );
    let mut occurrences = vec![0; trades.len()];
    for member in chart.dots.iter().flat_map(|dot| dot.members.iter()) {
        occurrences[member.ordinal] += 1;
    }
    assert!(occurrences.iter().all(|count| *count == 1));
    (chart, trades.to_vec())
}

#[test]
#[ignore = "requires WIN_TICK_FIXTURE containing the private canonical recording"]
fn recorded_fixed_width_segments_the_old_burst_without_losing_members() {
    let (baseline, trades) = recorded_burst(420.0);
    let old = baseline
        .dots
        .iter()
        .max_by_key(|dot| dot.mark.quantity)
        .unwrap();
    assert_eq!(old.mark.quantity, Decimal::from(6113));
    assert_eq!(old.mark.buy_quantity, Decimal::from(611));
    assert_eq!(old.members.len(), 250);
    let width = bubble_presets::embedded()
        .get("B3 WIN tick spheres")
        .unwrap()
        .tick_bubbles
        .unwrap()
        .tape_bursts
        .unwrap()
        .width_px;
    let (candidate, same_trades) = recorded_burst(width);
    assert_eq!(trades, same_trades);
    assert_eq!(candidate.dots.len(), 623);
    let largest = candidate
        .dots
        .iter()
        .max_by_key(|dot| dot.mark.quantity)
        .unwrap();
    assert_eq!(largest.mark.quantity, Decimal::from(4716));
    assert_eq!(largest.mark.buy_quantity, Decimal::from(343));
    assert_eq!(largest.members.len(), 88);
    let old_members: std::collections::BTreeSet<_> =
        old.members.iter().map(|member| member.ordinal).collect();
    let touched: Vec<_> = candidate
        .dots
        .iter()
        .filter(|dot| {
            dot.members
                .iter()
                .any(|member| old_members.contains(&member.ordinal))
        })
        .collect();
    assert_eq!(
        touched.len(),
        38,
        "the old event is now explicitly segmented"
    );
    let mut original_count = 0;
    let mut original_quantity = Decimal::ZERO;
    let mut original_buy = Decimal::ZERO;
    let mut extra_count = 0;
    let mut extra_quantity = Decimal::ZERO;
    for member in touched.iter().flat_map(|dot| dot.members.iter()) {
        let trade = &trades[member.ordinal];
        if old_members.contains(&member.ordinal) {
            original_count += 1;
            original_quantity += trade.quantity;
            if trade.side == Side::Buy {
                original_buy += trade.quantity;
            }
        } else {
            extra_count += 1;
            extra_quantity += trade.quantity;
        }
    }
    assert_eq!(original_count, 250);
    assert_eq!(original_quantity, Decimal::from(6113));
    assert_eq!(original_buy, Decimal::from(611));
    assert_eq!(
        extra_count, 13,
        "neighbors are not attributed to the old event"
    );
    assert_eq!(extra_quantity, Decimal::from(75));
}
