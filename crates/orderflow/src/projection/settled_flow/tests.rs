use super::*;
use crate::config::BubbleStyle;
use crate::config::HeatmapConfig;
use crate::history::LiquidityHistory;
use crate::projection::tests::fixtures::*;
use crate::projection::*;
use quantick_engine::{Side, Trade};
use quantick_orderbook::BookDelta;
use rust_decimal::Decimal;

#[test]
fn clustering_partitions_seam_adjacent_events_without_reordering_or_matching() {
    let history = LiquidityHistory::new(config());
    let closed = [
        bar(0, 999),
        bar(1_000, 1_999),
        bar(2_000, 2_999),
        bar(3_000, 3_999),
    ];
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        None,
        Some(crate::LiveEdge {
            now_ms: 3_900,
            window_ms: 1_500,
            reference_ms: 1_500,
            on_newest_bar: true,
        }),
    );
    let grouping =
        EffectiveGrouping::resolve(crate::DisplayGrouping::Native, Decimal::ONE, dec("5"));
    let transitions: Vec<_> = [(1_999, 10, 8), (2_000, 8, 5), (2_001, 5, 1)]
        .into_iter()
        .map(|(timestamp_ms, before, after)| LiquidityTransition {
            generation: 1,
            side: crate::RestingSide::Bid,
            price_bucket: dec("100"),
            timestamp_ms,
            before: Decimal::from(before),
            after: Decimal::from(after),
        })
        .collect();
    let flow = cluster_settled(
        &history,
        &timeline,
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
        &[],
        grouping,
        &transitions,
    );
    assert_eq!(flow.live_from_ms, Some(2_000));
    assert_eq!(
        flow.clusters
            .events
            .iter()
            .map(|event| event.event_id)
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(
        flow.live_events
            .iter()
            .map(|event| event.event_id)
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(
        flow.clusters
            .events
            .iter()
            .chain(&flow.live_events)
            .map(|event| event.removed)
            .sum::<Decimal>(),
        dec("9")
    );
    assert!(
        flow.live_events
            .iter()
            .all(|event| event.matched_quantity == Decimal::ZERO)
    );
}

#[test]
fn finishing_associates_before_display_floor_and_event_visibility() {
    let history = reduction_history(HeatmapConfig {
        show_aligned_depletion: false,
        bubbles: BubbleStyle {
            min_quantity: 4.0,
            ..BubbleStyle::default()
        },
        ..event_config()
    });
    let timeline = BarTimeline::from_bars(0, &[bar(0, 999), bar(1_000, 1_999)], None, None);
    let prices = PriceWindow::new(dec("99"), dec("103")).unwrap();
    let crate::projection::window::WindowResolution::Ready(window) =
        crate::projection::window::resolve_window(&history, &timeline, prices)
    else {
        panic!("ready window");
    };
    let liquidity = crate::projection::window::sweep_window(&history, &window, prices);
    let flow = cluster_settled(
        &history,
        &timeline,
        prices,
        &liquidity.coverage,
        window.effective_grouping,
        &liquidity.grouped.transitions,
    );
    let marks = finish_settled(
        &history,
        &timeline,
        prices,
        window.effective_grouping,
        dec("10"),
        flow.clusters,
    );
    assert!(marks.aggressions.is_empty());
    assert_eq!(marks.floored_quantity, dec("3"));
    assert_eq!(marks.liquidity_events.len(), 1);
    assert_eq!(
        marks.liquidity_events[0].evidence,
        LiquidityEvidence::DepthOnly
    );
    assert_eq!(marks.dropped_liquidity_events, 0);
    assert_eq!(marks.aggression_reference, history.bubble_size_reference());
}

#[test]
fn a_hidden_print_still_explains_the_reduction_it_caused() {
    // The floor is applied after association, so a small print that ate a
    // wall keeps the reduction marked as aggression-aligned even though the
    // bubble itself is not drawn. Hiding noise must never rewrite evidence.
    let mut history = LiquidityHistory::new(HeatmapConfig {
        bubbles: BubbleStyle {
            min_quantity: 100.0,
            ..BubbleStyle::default()
        },
        ..config()
    });
    history.install_snapshot(100, 1, snapshot(10)).unwrap();
    history.record_aggression(&Trade {
        agg_id: 7,
        timestamp_ms: 400,
        price: dec("101"),
        quantity: dec("3"),
        side: Side::Buy,
    });
    // Ask 101: 4 -> 1, right after the print.
    history
        .apply_delta(
            450,
            &BookDelta::new(11, 11, vec![], vec![level("101", "1")]),
        )
        .unwrap();
    history
        .apply_delta(900, &BookDelta::new(12, 12, vec![], vec![]))
        .unwrap();

    let projection = project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("98"), dec("103")).unwrap(),
    );
    assert!(
        projection.aggressions.is_empty(),
        "a 3-lot print is under the 100 floor"
    );
    let aligned = projection
        .liquidity_events
        .iter()
        .find(|event| event.price_bucket == dec("101"))
        .expect("the reduction is still projected");
    assert_eq!(aligned.evidence, LiquidityEvidence::AggressionAligned);
    assert!(aligned.matched_quantity > Decimal::ZERO);
}

#[test]
fn projects_partial_and_full_reductions_with_conserved_aggression_evidence() {
    let projection = project_reductions(event_config());
    assert_eq!(projection.liquidity_events.len(), 2);
    let partial = projection
        .liquidity_events
        .iter()
        .find(|event| event.timestamp_ms == 500)
        .unwrap();
    assert_eq!(partial.before, dec("10"));
    assert_eq!(partial.after, dec("6"));
    assert_eq!(partial.removed, dec("4"));
    assert!(!partial.full_removal);
    assert_eq!(partial.matched_quantity, dec("3"));
    assert_eq!(partial.matched_fraction, 0.75);
    assert_eq!(partial.evidence, LiquidityEvidence::AggressionAligned);

    let full = projection
        .liquidity_events
        .iter()
        .find(|event| event.timestamp_ms == 800)
        .unwrap();
    assert_eq!(full.removed, dec("6"));
    assert!(full.full_removal);
    assert_eq!(full.evidence, LiquidityEvidence::DepthOnly);

    let bubble = projection.aggressions.first().unwrap();
    assert_eq!(bubble.trade_count, 1);
    assert_eq!(bubble.agg_ids, [77]);
    assert_eq!(bubble.matched_quantity, dec("3"));
    assert_eq!(bubble.matched_fraction, 1.0);
    assert_eq!(bubble.liquidity_event_ids, [partial.event_id]);
    let total_event_match: Decimal = projection
        .liquidity_events
        .iter()
        .map(|event| event.matched_quantity)
        .sum();
    assert_eq!(total_event_match, bubble.matched_quantity);
}

#[test]
fn evidence_toggles_hide_their_markers_but_keep_bubble_evidence() {
    let no_aligned = project_reductions(HeatmapConfig {
        show_aligned_depletion: false,
        ..event_config()
    });
    assert_eq!(no_aligned.liquidity_events.len(), 1);
    assert_eq!(
        no_aligned.liquidity_events[0].evidence,
        LiquidityEvidence::DepthOnly
    );
    // The hidden marker's evidence still reaches the bubble: association
    // ran, only the marker itself is off screen.
    assert_eq!(no_aligned.aggressions[0].matched_quantity, dec("3"));

    let no_unattributed = project_reductions(HeatmapConfig {
        show_unattributed_reductions: false,
        ..event_config()
    });
    assert_eq!(no_unattributed.liquidity_events.len(), 1);
    assert_eq!(
        no_unattributed.liquidity_events[0].evidence,
        LiquidityEvidence::AggressionAligned
    );

    // With both depletion layers off no association runs at all, and the
    // bubble honestly loses its consumption marks: there is no factual
    // reduction on screen (or off it) for the mark to point at.
    let neither = project_reductions(HeatmapConfig {
        show_aligned_depletion: false,
        show_unattributed_reductions: false,
        ..event_config()
    });
    assert!(neither.liquidity_events.is_empty());
    assert_eq!(neither.aggressions[0].matched_quantity, Decimal::ZERO);
}
