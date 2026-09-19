use quantick_engine::{Bar, Side, Trade, bar_timeline::BarTimeline};

fn bar(ms: i64) -> Bar {
    Bar::opened_by(&Trade {
        agg_id: 0,
        timestamp_ms: ms,
        price: 100.into(),
        quantity: 1.into(),
        side: Side::Buy,
    })
}

#[test]
fn empty_and_partial_only_series_keep_their_honest_bounds() {
    let empty = BarTimeline::new(&[], None);
    assert_eq!(empty.slot_at_time(100), None);
    assert_eq!(empty.slot_open_time(0), None);
    let partial = bar(100);
    let timeline = BarTimeline::new(&[], Some(&partial));
    assert_eq!(timeline.slot_at_time(0), Some(0));
    assert_eq!(timeline.slot_at_time(200), Some(0));
    assert_eq!(timeline.slot_open_time(0), Some(100));
    assert_eq!(timeline.slot_open_time(1), None);
}

#[test]
fn equal_open_times_resolve_to_the_last_slot_and_partial_is_the_next_slot() {
    let closed = [bar(100), bar(100), bar(200)];
    let partial = bar(200);
    let timeline = BarTimeline::new(&closed, Some(&partial));
    for (time, slot) in [(0, 0), (100, 1), (150, 1), (200, 3), (500, 3)] {
        assert_eq!(timeline.slot_at_time(time), Some(slot));
    }
    assert_eq!(timeline.slot_open_time(2), Some(200));
    assert_eq!(timeline.slot_open_time(3), Some(200));
    assert_eq!(timeline.slot_open_time(4), None);
    assert_eq!(BarTimeline::new(&closed, None).slot_at_time(500), Some(2));
}
