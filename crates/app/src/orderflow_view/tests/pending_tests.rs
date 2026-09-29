//! The first visible tape frame must not depend on worker scheduling.
use super::*;
use crate::worker_progress::{
    Phase, WorkerProgress,
    test_support::{Gate, Hold},
};
use quantick_engine::Side;
use quantick_orderflow::engine::VisibleOrderflow;
use quantick_orderflow::projection::PaneGeometry;
use std::sync::Arc;

fn held_view(tape_only: bool) -> (OrderflowView, Arc<Gate>, Hold) {
    let gate = Gate::new();
    let hold = gate.hold(Phase::Applying);
    let (worker, run) =
        BookWorker::prepared_for_test("WINV26", WorkerProgress::with_clock(gate.clone()));
    let mut view = OrderflowView::new("WINV26");
    view.worker = worker;
    let before = view.config.clone();
    view.config = HeatmapConfig::default();
    view.config.live_lane.enabled = true;
    view.config.live_lane.show_aggressions = true;
    view.config.live_lane.tape_only = tape_only;
    view.config.volume_dots.enabled = true;
    view.commit_config_changes(before);
    view.set_live_lane_window(LaneWindow::Fixed { ms: 15_000 });
    std::thread::spawn(run);
    hold.reached();
    (view, gate, hold)
}

fn print(agg_id: u64, timestamp_ms: i64, price: i64, quantity: i64, side: Side) -> Trade {
    Trade {
        agg_id,
        timestamp_ms,
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side,
    }
}

fn frame(view: &mut OrderflowView, prints: &[Trade]) -> Option<Arc<VisibleOrderflow>> {
    let mut partial = Bar::opened_by(&prints[0]);
    for trade in &prints[1..] {
        partial.extend(trade);
    }
    let now = prints.iter().map(|trade| trade.timestamp_ms).max().unwrap();
    view.set_replay_clock_at(now, Some(now), None);
    view.project_visible(
        VisibleBarTimeline::new(prints.len() as u64, 0, &[], Some(&partial)),
        true,
        true,
        Some(15_000),
        (90.0, 210.0),
        Some(PaneGeometry {
            px_per_bar: 10.0,
            lane_width_px: 640.0,
            lane_window_ms: 15_000,
            height_px: 400.0,
            lane_bars: vec![(partial.open_time, partial.close_time)],
        }),
    )
}

fn tape(frame: &VisibleOrderflow) -> Vec<quantick_orderflow::AggressionPrimitive> {
    frame
        .projection
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .cloned()
        .collect()
}

#[test]
fn an_accepted_tape_print_is_drawable_on_the_same_frame_with_the_worker_held() {
    let (mut view, _, hold) = held_view(true);
    let trade = print(1, 1_001, 100, 3, Side::Buy);
    view.record_trade(&trade);
    let immediate = frame(&mut view, &[trade]);
    let fit = view.tape_price_range();
    let worker_has_no_frame = view.worker.published().frame.is_none();
    hold.release();
    assert!(
        worker_has_no_frame,
        "the test must not win a scheduling race"
    );
    let immediate = immediate.expect("the first print needs no worker projection");
    let marks = tape(&immediate);
    assert_eq!(marks.len(), 1);
    assert_eq!(
        (marks[0].x, marks[0].quantity, marks[0].price),
        (1.0, Decimal::from(3), Decimal::from(100))
    );
    assert_eq!(
        fit,
        Some((100.0, 100.0)),
        "pending prices feed the same-frame fit"
    );
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 400.0));
    let output = egui::Context::default().run(egui::RawInput::default(), |ctx| {
        view.draw_aggressions(
            &ctx.layer_painter(egui::LayerId::background()),
            rect,
            &Viewport::new(),
            1,
            &immediate,
            egui::Color32::BLACK,
            rect.width(),
            false,
            (90.0, 110.0),
        );
    });
    assert!(
        output
            .shapes
            .iter()
            .any(|shape| matches!(shape.shape, egui::Shape::Circle(_) | egui::Shape::Mesh(_)))
    );
}

#[test]
fn pending_native_windows_handoff_to_the_published_frame_without_double_counting() {
    let (mut view, gate, initial) = held_view(true);
    let mut prints = vec![print(1, 1_001, 100, 2, Side::Buy)];
    view.record_trade(&prints[0]);
    let _ = frame(&mut view, &prints);
    initial.release();
    view.flush_for_test();
    let held = gate.hold(Phase::Applying);
    for trade in [
        print(2, 1_017, 100, 3, Side::Sell),
        print(3, 1_099, 100, 5, Side::Buy),
    ] {
        view.record_trade(&trade);
        prints.push(trade);
    }
    held.reached();
    let before = frame(&mut view, &prints).expect("the published prefix plus pending suffix");
    let pending_marks = tape(&before);
    held.release();
    view.flush_for_test();
    let after = frame(&mut view, &prints).expect("the worker now owns every print");
    let after_marks = tape(&after);
    assert_eq!(
        pending_marks, after_marks,
        "publishing must not resize, move, split or duplicate a dot"
    );
    assert_eq!(
        after_marks.len(),
        1,
        "all prints share one native 100 ms window and price"
    );
    assert_eq!(after_marks[0].quantity, Decimal::from(10));
    assert_eq!(after_marks[0].buy_quantity, Decimal::from(7));
    assert_eq!(after_marks[0].timestamp_quantity, Decimal::from(10_548));
    assert_eq!(after_marks[0].agg_ids, vec![1, 2, 3]);
}

#[test]
fn a_pending_new_price_extreme_is_in_the_fit_before_any_worker_publication() {
    let (mut view, gate, initial) = held_view(true);
    let first = print(1, 1_001, 100, 1, Side::Buy);
    view.record_trade(&first);
    let _ = frame(&mut view, std::slice::from_ref(&first));
    initial.release();
    view.flush_for_test();
    let held = gate.hold(Phase::Applying);
    let next = print(2, 1_101, 200, 2, Side::Sell);
    view.record_trade(&next);
    held.reached();
    view.set_replay_clock_at(next.timestamp_ms, Some(next.timestamp_ms), None);
    let fit_before_projection = view.tape_price_range();
    let shown = frame(&mut view, &[first, next]);
    held.release();
    assert_eq!(fit_before_projection, Some((100.0, 200.0)));
    assert_eq!(
        tape(&shown.unwrap())
            .iter()
            .map(|mark| mark.quantity)
            .sum::<Decimal>(),
        Decimal::from(3)
    );
}

#[test]
fn ordinary_btc_style_views_keep_the_existing_worker_publication_contract() {
    let (mut view, _, hold) = held_view(false);
    let trade = print(1, 1_001, 100, 1, Side::Buy);
    view.record_trade(&trade);
    let immediate = frame(&mut view, &[trade]);
    hold.release();
    assert!(
        immediate.is_none(),
        "the synchronous bridge is opt-in tape behavior only"
    );
}

#[test]
fn a_source_or_replay_reset_discards_pending_prints_even_when_ids_restart() {
    let (mut view, _, hold) = held_view(true);
    view.record_trade(&print(1, 1_001, 100, 11, Side::Buy));
    let _ = frame(&mut view, &[print(1, 1_001, 100, 11, Side::Buy)]);
    view.reset_for_symbol("WINV26");
    let fresh = print(1, 101, 200, 2, Side::Sell);
    view.record_trade(&fresh);
    let shown = frame(&mut view, &[fresh]);
    let fit = view.tape_price_range();
    hold.release();
    let marks = tape(&shown.expect("the new source has a same-frame print"));
    assert_eq!(marks.len(), 1);
    assert_eq!(
        (marks[0].quantity, marks[0].price),
        (Decimal::from(2), Decimal::from(200))
    );
    assert_eq!(fit, Some((200.0, 200.0)));
}

#[test]
fn pending_print_storage_obeys_the_existing_retention_capacity() {
    let (mut view, _, hold) = held_view(true);
    let before = view.config.clone();
    view.config.max_aggressions = 2;
    view.commit_config_changes(before);
    let prints = [
        print(1, 1_001, 100, 1, Side::Buy),
        print(2, 1_101, 105, 2, Side::Buy),
        print(3, 1_201, 110, 3, Side::Sell),
    ];
    for trade in &prints {
        view.record_trade(trade);
    }
    let shown = frame(&mut view, &prints);
    let pending_count = view.pending_tape.len();
    hold.release();
    assert_eq!(
        pending_count, 2,
        "a stalled worker cannot grow the pending store past its configured cap"
    );
    let marks = tape(&shown.unwrap());
    assert_eq!(
        marks.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        Decimal::from(5)
    );
    assert!(marks.iter().all(|mark| !mark.agg_ids.contains(&1)));
}

#[test]
fn reentering_tape_mode_never_uses_a_legacy_dot_window_for_the_pending_print() {
    let (mut view, gate, initial) = held_view(false);
    let first = print(1, 1_001, 100, 1, Side::Buy);
    view.record_trade(&first);
    let _ = frame(&mut view, std::slice::from_ref(&first));
    initial.release();
    view.flush_for_test();
    let held = gate.hold(Phase::Applying);
    let before = view.config.clone();
    view.config.live_lane.tape_only = true;
    view.commit_config_changes(before);
    held.reached();
    let next = print(2, 1_101, 105, 2, Side::Sell);
    view.record_trade(&next);
    let shown = frame(&mut view, &[first, next]);
    held.release();
    let shown = shown.expect("enabling tape can draw before its config is published");
    assert_eq!(shown.volume_dots.as_ref().unwrap().tape_window_ms, 100);
    assert!(
        tape(&shown)
            .iter()
            .any(|mark| mark.agg_ids.contains(&2) && mark.x == 1.0)
    );
}

#[test]
fn pending_storage_retires_the_existing_time_retention_prefix() {
    let (mut view, _, hold) = held_view(true);
    let before = view.config.clone();
    view.config.retention_ms = 1_000;
    view.commit_config_changes(before);
    let prints = [
        print(1, 1_001, 100, 1, Side::Buy),
        print(2, 2_101, 105, 2, Side::Sell),
    ];
    for trade in &prints {
        view.record_trade(trade);
    }
    let shown = frame(&mut view, &prints);
    let pending_count = view.pending_tape.len();
    hold.release();
    assert_eq!(pending_count, 1);
    let marks = tape(&shown.unwrap());
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].agg_ids, vec![2]);
    assert_eq!(marks[0].quantity, Decimal::from(2));
}

#[test]
fn metatrader_reconnect_ids_cannot_acknowledge_an_unpublished_new_print() {
    let (mut view, gate, initial) = held_view(true);
    let first = print(100_000, 1_001, 100, 2, Side::Buy);
    view.record_trade(&first);
    let _ = frame(&mut view, std::slice::from_ref(&first));
    initial.release();
    view.flush_for_test();
    // MT5 bridge reconnection preserves the chart but restarts trade IDs.
    let held = gate.hold(Phase::Applying);
    let reconnected = print(1, 1_101, 105, 3, Side::Sell);
    view.record_trade(&reconnected);
    held.reached();
    let prints = [first, reconnected];
    let immediate = frame(&mut view, &prints);
    held.release();
    let immediate = tape(&immediate.unwrap());
    assert_eq!(
        immediate.iter().map(|mark| mark.quantity).sum::<Decimal>(),
        Decimal::from(5)
    );
    assert!(
        immediate
            .iter()
            .any(|mark| mark.agg_ids == [1] && mark.quantity == Decimal::from(3))
    );
    view.flush_for_test();
    let settled = tape(&frame(&mut view, &prints).unwrap());
    assert_eq!(
        immediate, settled,
        "the receipt is an owner ordinal, never a venue ID"
    );
    assert!(view.pending_tape.is_empty());
}
