//! A Renko pane: no brick until its builder has read the price step, then
//! every brick the prints it held close, appended as history; one print
//! clearing several levels lands every brick it closed; and older history
//! that reshapes the first bricks keeps the marks and the view on the market
//! time they carry — while a tick pane's still shift by the bars it added.

use super::*;
use crate::viewport::Viewport;
use quantick_engine::bar_registry::BUILTIN_BARS;
use rust_decimal::Decimal;

fn print(agg_id: u64, price: i64) -> quantick_engine::Trade {
    quantick_engine::Trade {
        agg_id,
        timestamp_ms: 1_700_000_000_000 + agg_id as i64 * 1_000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: quantick_engine::Side::Buy,
    }
}

/// `prices` as consecutive prints, the first one numbered `first_id`.
fn prints(first_id: u64, prices: Vec<i64>) -> Vec<quantick_engine::Trade> {
    prices
        .into_iter()
        .enumerate()
        .map(|(i, price)| print(first_id + i as u64, price))
        .collect()
}

/// A flow pane cutting 3-tick bricks: on a one-point grid, two points tall.
fn renko_pane() -> ChartPane {
    let renko = BUILTIN_BARS.parse("renko:3").unwrap();
    ChartPane::flow(1, renko, "WINV26".to_owned())
}

#[test]
fn repeated_history_publications_preserve_fractional_pan_and_future_projection() {
    for projection in [false, true] {
        let mut pane = renko_pane();
        pane.ingest_backfill(&prints(1000, (1000..1400).collect()));
        for first in [800, 600, 400] {
            let slots = pane.slots();
            pane.model.viewport.snap_to_live();
            pane.model
                .viewport
                .pan_pixels(if projection { -25.5 } else { 45.5 }, slots);
            let before = pane.model.viewport.right_edge_bar(slots);
            let reference = (before.floor() as usize).min(slots - 1);
            let time = pane.slot_open_time(reference).unwrap();
            let offset = before - reference as f32;
            pane.receive_history(
                std::sync::Arc::new(prints(first, (first as i64..first as i64 + 200).collect())),
                true,
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !pane.prepare_history() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(pane.install_history());
            assert!(
                !pane.model.viewport.follows_live(),
                "publication must not reacquire follow"
            );
            let expected = pane.slot_at_time(time).unwrap() as f32 + offset;
            assert!(
                (pane.model.viewport.right_edge_bar(pane.slots()) - expected).abs() < 0.001,
                "fractional input survives every publication"
            );
        }
    }
}

#[test]
fn a_renko_pane_appends_every_brick_the_print_that_reads_the_step_cuts() {
    let mut pane = renko_pane();
    // A climb of one point a print: sixty-three distances read no step.
    let climb = prints(0, (100..=164).collect());
    for trade in &climb[..64] {
        pane.ingest_live_trade(trade);
    }
    assert_eq!(pane.closed_slots(), 0, "the prints are held");
    let appended = pane.pagination_revision();
    // The sixty-fourth distance reads it and cuts every brick the climb
    // closed; then 170 clears 164, 166 and 168 by a step in one print.
    pane.ingest_live_trade(&climb[64]);
    assert_eq!(pane.closed_slots(), 31);
    pane.ingest_live_trade(&print(65, 170));
    assert_eq!(pane.closed_slots(), 34);
    assert_eq!(
        pane.pagination_revision(),
        appended,
        "appended, never rewritten"
    );
    let closes: Vec<Decimal> = (31..34)
        .map(|slot| pane.closed_bar(slot).expect("a closed brick").close)
        .collect();
    assert_eq!(closes, [164, 166, 168].map(Decimal::from), "one per level");
}

/// Arm a strategy on a rectangle, so the pane queues it every live close.
fn arm_a_strategy(pane: &mut ChartPane) {
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == drawings::RECTANGLE_TOOL_ID)
        .expect("the rectangle tool is registered");
    pane.drawings.place(rectangle, ChartPoint::at(0.0, 100.0));
    pane.drawings.place(rectangle, ChartPoint::at(30.0, 110.0));
    let instance = crate::strategy_anchors::AnchoredInstance {
        drawing: pane.drawings.items()[0].id,
        preset: "BF".to_owned(),
        spec: crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Sell),
        armed: quantick_strategy::ArmedStrategy::new(
            quantick_strategy::StrategyParams {
                side: quantick_engine::Side::Sell,
                quantity: Decimal::ONE,
                tp_mult: Decimal::ONE,
                sl_mult: Decimal::ONE,
                rearm: quantick_strategy::Rearm::OneShot,
                on_break: quantick_strategy::BreakPolicy::Ignore,
                execution: quantick_strategy::Execution::Paper,
            },
            Box::new(quantick_strategy::ForceTrigger::new(
                quantick_strategy::ForceParams::default_band(),
            )),
        ),
        alarm: None,
        cue: crate::audio::Cue::default(),
        mark: crate::strategy_anchors::AlarmMark::Quiet,
    };
    assert!(pane.strategies.anchors.arm(instance).is_empty());
}

/// Bricks cut from the prints a Renko builder held while it read its step
/// are history by the time they exist: they land behind the backfill
/// boundary, reach the indicators as history rather than as live closes, and
/// no armed strategy is handed one. Only the bricks the print that read the
/// step closed itself are closes.
#[test]
fn bricks_cut_from_held_prints_reach_no_strategy_and_no_live_close() {
    use crate::indicator_worker::{IndicatorEvent, IndicatorSource};
    let mut pane = renko_pane();
    arm_a_strategy(&mut pane);
    pane.add_indicator(IndicatorSource::Native {
        id: "native.cvd".to_owned(),
        values: Vec::new(),
    });
    // Everything the worker published for the commands sent so far.
    let settled = |pane: &ChartPane| {
        pane.indicator_worker.flush();
        pane.indicator_worker.drain_events()
    };
    // One point a print through 163 closes 31 bricks while they are held;
    // 170, the sixty-fourth distance, reads the step and clears three more.
    let mut prices: Vec<i64> = (100..164).collect();
    prices.push(170);
    let tape = prints(0, prices);
    pane.ingest_backfill(&tape[..10]);
    for trade in &tape[10..64] {
        pane.ingest_live_trade(trade);
    }
    let _ = settled(&pane);
    pane.ingest_live_trade(&tape[64]);
    assert_eq!(pane.closed_slots(), 34);
    assert_eq!(pane.state.backfill_boundary(), Some(31));
    let events = settled(&pane);
    let rebuilt: Vec<usize> = events
        .iter()
        .filter_map(|event| match event {
            IndicatorEvent::Rebuilt { rows, .. } => Some(*rows),
            _ => None,
        })
        .collect();
    let appended = events
        .iter()
        .filter(|event| matches!(event, IndicatorEvent::Appended { .. }))
        .count();
    // The worker may take the print's own closes in the history's batch, so
    // one rebuild carries 31 to 34 rows; the 31 bricks of the held prints
    // never arrive as live closes, which only the print's own three can be.
    assert!(
        matches!(rebuilt[..], [rows] if rows >= 31 && rows + appended == 34),
        "{rebuilt:?} rebuilt, {appended} appended"
    );
    let queued: Vec<usize> = pane
        .strategies
        .pending
        .iter()
        .map(|(_, slot)| *slot)
        .collect();
    assert_eq!(queued, [31, 32, 33], "no strategy is handed history");
}

/// The trader's mark and view sit on market time. Older history that
/// reshapes the bricks a series opened on must not move them onto a brick
/// that ended before that time — which is where a shift by the count of
/// bricks the history added would put them.
#[test]
fn older_history_that_reshapes_bricks_keeps_marks_and_view_on_their_market_time() {
    // Older: seventy prints at 120/121 read the step, then a fall to 99
    // cuts eleven bricks down to 100.
    let mut older: Vec<i64> = (0..70).map(|i| 120 + i % 2).collect();
    older.extend((99..=119).rev());
    let older = prints(0, older);
    // Held: a tape opening fresh at 100 — seventy prints at 100/101 read the
    // step, then a climb cuts 100->102 at 103 and 102->104 at 105.
    let mut held: Vec<i64> = (0..70).map(|i| 100 + i % 2).collect();
    held.extend(102..=105);
    let held = prints(older.len() as u64, held);

    let mut pane = renko_pane();
    pane.ingest_backfill(&held);
    assert_eq!(pane.closed_slots(), 2);
    let first = pane.closed_bar(0).expect("a brick").clone();
    assert_eq!(
        (first.open, first.close),
        (Decimal::from(100), Decimal::from(102))
    );
    let mark_time = first.open_time;
    let line = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .expect("the horizontal line is registered");
    assert!(
        pane.drawings
            .place(line, ChartPoint::at_time(0.0, 101.0, Some(mark_time)))
    );
    // The view parked on that brick, off the live edge.
    let slots = pane.slots();
    pane.model.viewport.pan_pixels(1.0e6, slots);
    assert_eq!(pane.right_edge_time(), Some(mark_time));

    let added = pane.prepend_history(&older);
    // From the older tape the series reaches 100 already falling, so the
    // climb only reverses at 105: 100->102 is gone, and 102->104 opens on
    // the fall's last print and holds the mark's instant.
    assert_eq!((pane.closed_slots(), added), (12, 10));
    let holder = 11;
    let brick = pane.closed_bar(holder).expect("a brick");
    assert_eq!(
        (brick.open, brick.close),
        (Decimal::from(102), Decimal::from(104))
    );
    assert!(brick.open_time <= mark_time && mark_time <= brick.close_time);
    let mark = pane.drawings.items()[0].points[0];
    assert_eq!(Viewport::slot_of(mark.bar), Some(holder), "the mark");
    #[allow(clippy::cast_precision_loss)]
    let holder_bar = holder as f32;
    assert_eq!(
        pane.model.viewport.right_edge_bar(pane.slots()),
        holder_bar,
        "and the view stay on their market time"
    );
    assert_ne!(added, holder, "a shift by the count lands a brick early");
}

/// Older history only shifts a tick pane's bars: a count partitions them
/// from the first print, so the bars it added are the whole move. The view
/// and the marks shift by that count and keep a pan's fraction of a bar;
/// re-placed by market time instead, they would land on the first of the
/// bars sharing a millisecond and lose the fraction.
#[test]
fn older_history_shifts_a_tick_panes_view_and_marks_by_the_bars_it_added() {
    use crate::state::BarSpec;
    let at = |agg_id: u64, timestamp_ms: i64| quantick_engine::Trade {
        timestamp_ms,
        ..print(agg_id, 100 + (agg_id % 3) as i64)
    };
    // Forty prints in one millisecond cut twenty two-print bars sharing it.
    let shared_ms = 1_700_000_100_000;
    let newer: Vec<_> = (20..60).map(|id| at(id, shared_ms)).collect();
    let older: Vec<_> = (0..20)
        .map(|id| at(id, 1_700_000_000_000 + id as i64))
        .collect();
    let mut pane = ChartPane::flow(1, BarSpec::Tick(2), "WINV26".to_owned());
    pane.ingest_backfill(&newer);
    assert_eq!(pane.closed_slots(), 20);
    let line = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .expect("the horizontal line is registered");
    assert!(
        pane.drawings
            .place(line, ChartPoint::at_time(7.25, 101.0, Some(shared_ms)))
    );
    let px = pane.model.viewport.px_per_bar();
    pane.model.viewport.pan_pixels(px * 10.5, pane.slots());
    let edge = pane.model.viewport.right_edge_bar(pane.slots());
    assert!((edge - 8.5).abs() < 1e-3, "parked half a bar off bar 8");

    let added = pane.prepend_history(&older);
    assert_eq!(added, 10);
    assert_eq!(
        pane.model.viewport.right_edge_bar(pane.slots()),
        edge + 10.0,
        "the view, its half bar kept"
    );
    assert_eq!(
        pane.drawings.items()[0].points[0].bar,
        17.25,
        "and the mark"
    );
}
