//! Opt-in audit over a private canonical trade fixture, never a bundled old trace.
use quantick_engine::{Side, Trade, fixture::parse_trades};
use quantick_orderflow::PriceWindow;
use quantick_orderflow::projection::flow_tape::{
    FlowBurstView, FlowExecution, FlowReference, FlowTapeFrame, FlowTapeView, project_flow_tape,
};
use quantick_stores::bubble_presets;
use rust_decimal::{Decimal, prelude::ToPrimitive as _};

#[path = "win_tick_calibration/continuity.rs"]
mod continuity;
#[path = "win_tick_calibration/native.rs"]
mod native;
#[path = "win_tick_calibration/visual.rs"]
mod visual;

fn project(trades: &[Trade], width: f32, window: i64) -> FlowTapeFrame {
    let style = bubble_presets::embedded()
        .get("B3 WIN tick spheres")
        .unwrap()
        .tick_bubbles
        .unwrap();
    let low = trades.iter().map(|trade| trade.price).min().unwrap() - Decimal::from(30);
    let high = trades.iter().map(|trade| trade.price).max().unwrap() + Decimal::from(30);
    let slots = trades.len().div_ceil(2_000);
    project_flow_tape(
        trades
            .iter()
            .enumerate()
            .map(|(ordinal, trade)| FlowExecution {
                ordinal,
                slot: ordinal / 2_000,
                accepted_ordinal: ordinal % 2_000,
                ticks_per_bar: Decimal::from(2_000),
                trade,
                opening: false,
            }),
        1,
        trades.len(),
        trades.len(),
        FlowTapeView {
            tape_bursts: (window == style.region_window_ms).then(|| {
                let calibration = style.tape_bursts.expect("causal WIN calibration");
                FlowBurstView {
                    calibration,
                    settled_through_ms: i64::MAX,
                    from_ms: trades[0].timestamp_ms.div_euclid(calibration.window_ms)
                        * calibration.window_ms,
                    until_ms: (trades
                        .last()
                        .unwrap()
                        .timestamp_ms
                        .div_euclid(calibration.window_ms)
                        + 1)
                        * calibration.window_ms,
                }
            }),
            region_window_ms: window,
            first_slot: 0,
            end_slot: slots,
            clip_left: Decimal::ZERO,
            clip_right: Decimal::from(slots),
            width_px: width,
            height_px: 800.0,
            prices: PriceWindow::new(low, high).unwrap(),
            reference: FlowReference::Typed(
                Decimal::from_f64_retain(style.reference_quantity).unwrap(),
            ),
            radius_limit: style.max_radius,
            merge_support_radius: 6.0,
            exclude_opening: false,
        },
    )
}

fn reconcile(frame: &FlowTapeFrame, trades: &[Trade]) {
    let mut members = vec![0_u8; trades.len()];
    for dot in &frame.dots {
        let mut total = Decimal::ZERO;
        let mut buy = Decimal::ZERO;
        for member in dot.members.iter() {
            let trade = &trades[member.ordinal];
            members[member.ordinal] += 1;
            assert_eq!(member.source_id, trade.agg_id);
            assert_eq!(member.candle_slot, member.ordinal / 2_000);
            assert_eq!(member.accepted_ordinal, member.ordinal % 2_000);
            total += trade.quantity;
            if trade.side == Side::Buy {
                buy += trade.quantity;
            }
        }
        assert_eq!(dot.mark.quantity, total);
        assert_eq!(dot.mark.buy_quantity, buy);
        assert!(dot.radius > 0.0);
    }
    assert!(members.iter().all(|count| *count == 1));
    assert_eq!(
        frame.offscreen_executions
            + frame.off_axis_executions
            + frame.ineligible_executions
            + frame.omitted_executions,
        0
    );
}

fn density(frame: &FlowTapeFrame, trades: &[Trade]) -> (f64, f32) {
    let slots = Decimal::from(trades.len().div_ceil(2_000));
    let xy: Vec<_> = frame
        .dots
        .iter()
        .map(|dot| {
            (
                (dot.candle_position / slots).to_f64().unwrap() * f64::from(frame.view.width_px),
                frame.view.prices.y_unclamped(dot.mark.price).unwrap()
                    * f64::from(frame.view.height_px),
                f64::from(dot.radius),
            )
        })
        .collect();
    let overlap = xy
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            xy.iter()
                .enumerate()
                .any(|(j, b)| *i != j && (a.0 - b.0).hypot(a.1 - b.1) < a.2.max(b.2))
        })
        .count();
    let mut radii: Vec<_> = frame.dots.iter().map(|dot| dot.radius).collect();
    radii.sort_by(f32::total_cmp);
    (
        overlap as f64 / radii.len() as f64,
        radii[radii.len() * 9 / 10] / radii[radii.len() / 10],
    )
}

#[test]
fn win_tick_sizes_keep_small_contracts_small_and_large_bursts_distinct() {
    let trades: Vec<_> = [4, 100, 1_000, 5_850]
        .into_iter()
        .enumerate()
        .map(|(index, quantity)| Trade {
            agg_id: index as u64 + 1,
            timestamp_ms: index as i64 * 20_000 + 1_000,
            price: Decimal::from(209_365),
            quantity: Decimal::from(quantity),
            side: if index % 2 == 0 {
                Side::Buy
            } else {
                Side::Sell
            },
        })
        .collect();
    let style = bubble_presets::embedded()
        .get("B3 WIN tick spheres")
        .unwrap()
        .tick_bubbles
        .unwrap();
    let frame = project(&trades, 840.0, style.region_window_ms);
    reconcile(&frame, &trades);
    assert_eq!(
        frame.dots.len(),
        4,
        "separate native bursts retain their sizes"
    );
    let radius = |quantity| {
        frame
            .dots
            .iter()
            .find(|dot| dot.mark.quantity == Decimal::from(quantity))
            .unwrap()
            .radius
    };
    assert!(
        radius(4) > 0.0 && radius(4) < 1.0,
        "no readability floor inflates dust"
    );
    assert!((1.0..1.5).contains(&radius(100)));
    assert!((4.0..4.5).contains(&radius(1_000)));
    assert!((9.0..11.0).contains(&radius(5_850)));
    assert!((radius(5_850).powi(2) / radius(100).powi(2) - 58.5).abs() < 0.001);
}

#[test]
fn win_volume_emphasis_keeps_peaks_bright_and_legacy_styles_unchanged() {
    let presets = bubble_presets::embedded();
    let win = presets
        .get("B3 WIN tick spheres")
        .unwrap()
        .tick_bubbles
        .unwrap();
    for (quantity, opacity) in [
        (0, 0.225),
        (100, 0.2385),
        (1_000, 0.36),
        (5_000, 0.9),
        (50_000, 0.9),
    ] {
        assert!((win.fill_opacity(quantity.into()) - opacity).abs() < 0.00001);
    }
    let btc = presets
        .get("Binance BTC Spot spheres")
        .unwrap()
        .tick_bubbles
        .unwrap();
    assert!(btc.volume_opacity_floor.is_none());
    for quantity in [0, 1, 100, 50_000] {
        assert_eq!(btc.fill_opacity(quantity.into()), btc.opacity);
    }
    assert_eq!(win.quantity_label(5_850.into()), "5.8K");
    assert_eq!(win.quantity_label(6_590.into()), "6.6K");
    assert_eq!(btc.quantity_label(5_850.into()), "5.85K");
    assert_eq!(win.light_offset(1.0), 0.0);
    assert!(win.light_offset(4.0) > 0.0);
    assert!(btc.light_offset(1.0) > 0.0);
    let mut uniform = win;
    uniform.volume_opacity_floor = None;
    assert_ne!(win.fingerprint(), uniform.fingerprint());
    assert_eq!(
        win.bubbles(),
        uniform.bubbles(),
        "the emphasis never edits sphere geometry or tape style"
    );
}

#[test]
#[ignore = "requires WIN_TICK_FIXTURE containing the private canonical recording"]
fn recorded_win_members_quantities_and_size_hierarchy() {
    let path = std::env::var_os("WIN_TICK_FIXTURE").expect("private canonical trade fixture");
    let trades = parse_trades(&std::fs::read_to_string(path).unwrap()).unwrap();
    let region_ms = bubble_presets::embedded()
        .get("B3 WIN tick spheres")
        .unwrap()
        .tick_bubbles
        .unwrap()
        .region_window_ms;
    for sample in trades.chunks(200_000) {
        for width in [500.0, 840.0] {
            reconcile(&project(sample, width, region_ms), sample);
        }
    }
    eprintln!(
        "WIN_COVERAGE records={} total={} buy={} widths=500,840 missing=0 duplicated=0 ineligible=0 off_axis=0 offscreen=0 omitted=0",
        trades.len(),
        trades.iter().map(|t| t.quantity).sum::<Decimal>(),
        trades
            .iter()
            .filter(|t| t.side == Side::Buy)
            .map(|t| t.quantity)
            .sum::<Decimal>()
    );
    let mut density_failures = Vec::new();
    for offset in [0, 60_000, 300_000, 700_000, 1_100_000] {
        let sample = &trades[offset..offset + 40_000];
        for width in [500.0, 840.0] {
            let base = project(sample, width, 1_000);
            let win = project(sample, width, region_ms);
            reconcile(&base, sample);
            reconcile(&win, sample);
            let (base_overlap, _) = density(&base, sample);
            let (overlap, radius_ratio) = density(&win, sample);
            eprintln!(
                "WIN_AUDIT offset={offset} width={width} records={} base_dots={} dots={} base_overlap={base_overlap:.6} overlap={overlap:.6} reduction={:.6} radius_p90_p10={radius_ratio:.6} reference={} total={} buy={}",
                sample.len(),
                base.dots.len(),
                win.dots.len(),
                1.0 - overlap / base_overlap,
                win.effective_reference.unwrap(),
                sample.iter().map(|t| t.quantity).sum::<Decimal>(),
                sample
                    .iter()
                    .filter(|t| t.side == Side::Buy)
                    .map(|t| t.quantity)
                    .sum::<Decimal>()
            );
            if 1.0 - overlap / base_overlap < 0.4 {
                density_failures.push((offset, width));
            }
            assert!(
                radius_ratio >= 3.0,
                "size hierarchy gate at {offset}/{width}"
            );
        }
    }
    eprintln!(
        "LEGACY_DENSITY_DIAGNOSTIC retired neighbour-reduction failures={density_failures:?}; membership and size hierarchy remain asserted"
    );
}
