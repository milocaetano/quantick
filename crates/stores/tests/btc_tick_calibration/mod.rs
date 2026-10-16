//! Actual LEFT tick-chart projection, including native facts at different zooms.
use super::*;
use quantick_orderflow::projection::flow_tape::{
    FlowExecution, FlowReference, FlowTapeView, project_flow_tape,
};
use rust_decimal::prelude::ToPrimitive as _;

#[test]
fn btc_tick_spheres_preserve_exact_sides_membership_and_area_at_each_zoom() {
    for (name, csv, total) in recordings() {
        let prints = trades(csv);
        let preset = if name.starts_with("spot") {
            "Binance BTC Spot spheres"
        } else {
            "Binance BTC perpetual spheres"
        };
        let style = bubble_presets::embedded()
            .get(preset)
            .unwrap()
            .tick_bubbles
            .unwrap();
        let first_price = prints.iter().map(|trade| trade.price).min().unwrap() - Decimal::ONE;
        let last_price = prints.iter().map(|trade| trade.price).max().unwrap() + Decimal::ONE;
        let slots = prints.len().div_ceil(50);
        for width in [400.0, 800.0, 1600.0] {
            let frame = project_flow_tape(
                prints
                    .iter()
                    .enumerate()
                    .map(|(ordinal, trade)| FlowExecution {
                        ordinal,
                        slot: ordinal / 50,
                        accepted_ordinal: ordinal % 50,
                        ticks_per_bar: 50.into(),
                        trade,
                        opening: false,
                    }),
                1,
                prints.len(),
                prints.len(),
                FlowTapeView {
                    first_slot: 0,
                    end_slot: slots,
                    clip_left: Decimal::ZERO,
                    clip_right: Decimal::from(slots),
                    width_px: width,
                    height_px: 500.0,
                    prices: PriceWindow::new(first_price, last_price).unwrap(),
                    reference: FlowReference::from(style),
                    radius_limit: style.max_radius,
                    merge_support_radius: 6.0,
                    exclude_opening: false,
                },
            );
            assert_eq!(
                frame
                    .dots
                    .iter()
                    .map(|dot| dot.mark.quantity)
                    .sum::<Decimal>(),
                dec(total)
            );
            assert_eq!(
                frame
                    .dots
                    .iter()
                    .map(|dot| dot.mark.buy_quantity)
                    .sum::<Decimal>(),
                prints
                    .iter()
                    .filter(|trade| trade.side == Side::Buy)
                    .map(|trade| trade.quantity)
                    .sum::<Decimal>()
            );
            let mut members: Vec<_> = frame
                .dots
                .iter()
                .flat_map(|dot| dot.members.iter().map(|member| member.ordinal))
                .collect();
            members.sort_unstable();
            assert_eq!(members, (0..prints.len()).collect::<Vec<_>>());
            let expected_reference = frame
                .dots
                .iter()
                .map(|dot| dot.mark.quantity)
                .max()
                .unwrap()
                .max(style.reference());
            assert_eq!(
                frame.effective_reference,
                Some(expected_reference),
                "{name}: the visible maximum respects the configured floor"
            );
            let reference = expected_reference.to_f64().unwrap();
            let area_per_btc = f64::from(style.max_radius).powi(2) / reference;
            let mut eligible = 0;
            for dot in &frame.dots {
                let quantity = dot.mark.quantity.to_f64().unwrap();
                let expected = quantity * area_per_btc;
                assert!(
                    (f64::from(dot.radius).powi(2) - expected).abs() <= expected * 0.00001,
                    "{name}: all ordinary areas remain proportional"
                );
                let (buy, sell) = dot.side_shares();
                assert!((buy + sell - 1.0).abs() < 1e-10);
                eligible += usize::from(dot.radius >= style.label_min_radius);
            }
            assert!(
                eligible > 0,
                "{name}: meaningful quantities must be eligible in quiet and busy views"
            );
            eprintln!(
                "BTC_LEFT {name} width={width} raw={} regions={} exact={} scale={} largest_radius={} labels_eligible={eligible}",
                prints.len(),
                frame.dots.len(),
                total,
                frame.effective_reference.unwrap(),
                frame
                    .dots
                    .iter()
                    .map(|dot| dot.radius)
                    .fold(0.0_f32, f32::max)
            );
        }
    }
}
