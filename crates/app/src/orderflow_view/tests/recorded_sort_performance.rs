//! Isolate ordering and owned-buffer costs without painting historical frames.

use super::*;
use quantick_orderflow::AggressionPrimitive;

#[test]
#[ignore = "recorded WIN sorting and allocation costs; requires TAPE_BENCH_CSV"]
fn recorded_native_sort_and_buffer_costs_at_40000_prints() {
    const TARGET: usize = 40_000;
    let parsed = recorded_prefix();
    let trades = &parsed.trades[..TARGET];
    let (mut view, gate, initial) = held_view(true);
    view.reset_for_symbol(parsed.header.symbol.as_ref().unwrap());
    let before = view.config.clone();
    assert!(view.apply_preset("mini index regions"));
    view.config.price_grouping = Decimal::from(5);
    view.config.max_aggressions = 100_000;
    view.config.volume_dots.ignore_opening_burst_in_scale = true;
    view.config.live_lane.window = LaneWindow::Fixed { ms: WINDOW_MS };
    view.commit_config_changes(before);
    initial.release();
    let mut chart = Chart::new(&trades[0]);
    for trade in &trades[..TARGET - 20] {
        chart.admit(&mut view, trade);
    }
    chart.clock(&mut view, chart.now_ms);
    chart.prices = chart.fit(&view);
    let _ = chart.project(&mut view);
    view.flush_for_test();
    assert!(view.pending_tape.is_empty());
    let held = gate.hold(Phase::Applying);
    for trade in &trades[TARGET - 20..] {
        chart.admit(&mut view, trade);
    }
    held.reached();
    chart.clock(&mut view, trades[TARGET - 1].timestamp_ms + 40);
    chart.prices = chart.fit(&view);
    let request = chart.request();
    let frame = view.complete_pending_frame(&request).unwrap();
    conserved(&frame, trades, chart.now_ms);
    let published = view.published.frame.as_ref().unwrap();
    let facts = published.projection.tape_facts.as_ref().unwrap();
    let zoom = request.dot_zoom.as_ref().unwrap();
    let widths_match = facts
        .clusters
        .iter()
        .all(|cluster| cluster.price_span == view.config.price_grouping);
    let eligible =
        zoom.tape_only && zoom.tape_window_ms == 100 && zoom.tape_level_ticks == 1 && widths_match;
    assert!(
        eligible,
        "the recorded prefix exercises touched-key refolding"
    );
    assert_eq!(view.pending_tape.len(), 20);
    assert!(frame.projection.aggressions.iter().all(|mark| mark.live));
    eprintln!(
        "TAPE_RECORDED_ALLOCATION target={TARGET} native={} output_capacity={} primitive_bytes={} cluster_bytes={} published_native={} native_width={} tape_only={} tape_window_ms={} tape_level_ticks={} widths_match={widths_match} touched_key_eligible={eligible}; one published prefix and held suffix, no historical paint warmup",
        frame.projection.aggressions.len(),
        frame.projection.aggressions.capacity(),
        std::mem::size_of::<AggressionPrimitive>(),
        std::mem::size_of::<quantick_orderflow::interaction::AggressionCluster>(),
        facts.clusters.len(),
        view.config.price_grouping,
        zoom.tape_only,
        zoom.tape_window_ms,
        zoom.tape_level_ticks,
    );

    // Restore the comparator used by interaction::sort_clusters, before the
    // production quantity sort. Sorting an already quantity-sorted frame would
    // hide the cost paid on every accepted suffix.
    let mut canonical = frame.projection.aggressions.clone();
    canonical.sort_by(|a, b| {
        let side = |side| match side {
            Side::Buy => 0,
            Side::Sell => 1,
        };
        a.first_timestamp_ms
            .cmp(&b.first_timestamp_ms)
            .then_with(|| a.last_timestamp_ms.cmp(&b.last_timestamp_ms))
            .then_with(|| side(a.side).cmp(&side(b.side)))
            .then_with(|| a.price_bucket.cmp(&b.price_bucket))
            .then_with(|| a.agg_id.cmp(&b.agg_id))
    });
    assert!(
        canonical
            .windows(2)
            .any(|pair| pair[0].quantity > pair[1].quantity)
    );
    let plain = repeated(
        TARGET,
        true,
        "canonical_clone_and_stable_quantity_sort",
        || {
            let mut marks = canonical.clone();
            marks.sort_by_key(|mark| mark.quantity);
            marks
        },
    );
    let cached = repeated(
        TARGET,
        true,
        "canonical_clone_and_cached_quantity_sort",
        || {
            let mut marks = canonical.clone();
            marks.sort_by_cached_key(|mark| mark.quantity);
            marks
        },
    );
    assert_eq!(plain, frame.projection.aggressions);
    assert_eq!(
        cached, plain,
        "cached keys retain exact fields and tie order"
    );

    let moved = repeated(TARGET, true, "clone_and_move_primitive_buffer", || {
        canonical.clone()
    });
    let extended = repeated(
        TARGET,
        true,
        "clone_and_empty_extend_primitive_buffer",
        || {
            let source = black_box(canonical.clone());
            let mut destination = Vec::new();
            destination.extend(source);
            destination
        },
    );
    assert_eq!(extended, moved);

    // Allocation-only proxy for the private native helper: same output type,
    // slice/filter lower bound zero, and owned per-cell ID copies. No placement
    // math is replaced, and these figures are not full tier-placement timings.
    let collected = repeated(TARGET, true, "primitive_filter_map_collect_proxy", || {
        canonical
            .iter()
            .filter_map(|mark| black_box(Some(mark.clone())))
            .collect::<Vec<_>>()
    });
    let reserved = repeated(TARGET, true, "primitive_lazy_reserved_push_proxy", || {
        let mut marks = Vec::new();
        for mark in &canonical {
            let mark = black_box(Some(mark.clone()));
            if let Some(mark) = mark {
                if marks.is_empty() {
                    marks.reserve(canonical.len());
                }
                marks.push(mark);
            }
        }
        marks
    });
    assert_eq!(collected, moved);
    assert_eq!(reserved, moved);
    eprintln!(
        "TAPE_RECORDED_BUFFER_CAPACITY count={} moved={} empty_extend={} filter_map_proxy={} lazy_reserved_proxy={}; all paired full vectors equal, no timing assertions",
        canonical.len(),
        moved.capacity(),
        extended.capacity(),
        collected.capacity(),
        reserved.capacity()
    );
    held.release();
    view.flush_for_test();
}
